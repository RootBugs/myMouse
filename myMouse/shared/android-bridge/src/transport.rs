/// Transport primitives exposed for Android bridge.
///
/// Full TLS/pinned transport in the Windows backend uses tokio-rustls.
/// Android integration can either:
/// 1. Use this crate compiled for Android targets via cargo-ndk + UniFFI, or
/// 2. Use a Kotlin TLS implementation (OkHttp/Conscrypt/Android TLS) that speaks
///    the same length-prefixed myMouse protocol frames and pins the peer cert.
///
/// This module provides frame helpers and config models so both paths stay
/// compatible.

use crate::protocol::{decode_message, encode_message, BridgeMessage, BridgeProtocolError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportConfig {
    pub host: String,
    pub port: u16,
    /// Hex SHA-256 fingerprints of pinned peer certs.
    pub pinned_fingerprints: Vec<String>,
    pub device_id: String,
    pub device_name: String,
}

pub const MAX_FRAME_LEN: usize = 1024 * 1024;

/// Encode a protocol message into a length-prefixed frame (u32 big-endian length + payload).
pub fn encode_frame(message: BridgeMessage) -> Vec<u8> {
    let payload = encode_message(message);
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    frame
}

/// Decode a frame payload (without the 4-byte length prefix).
pub fn decode_frame_payload(payload: Vec<u8>) -> Result<BridgeMessage, BridgeProtocolError> {
    if payload.is_empty() || payload.len() > MAX_FRAME_LEN {
        return Err(BridgeProtocolError::InvalidPayload);
    }
    decode_message(payload)
}

/// Return the frame length from a 4-byte big-endian header.
pub fn frame_length(header: [u8; 4]) -> usize {
    u32::from_be_bytes(header) as usize
}

/// True when a declared frame length can be read safely.
pub fn is_valid_frame_length(len: usize) -> bool {
    len > 0 && len <= MAX_FRAME_LEN
}

/// Decode one complete frame (4-byte big-endian header + payload).
/// Rejects short, oversized, truncated and trailing-byte inputs.
pub fn decode_frame(frame: &[u8]) -> Result<BridgeMessage, BridgeProtocolError> {
    if frame.len() < 4 {
        return Err(BridgeProtocolError::InvalidPayload);
    }
    let declared = frame_length([frame[0], frame[1], frame[2], frame[3]]);
    if !is_valid_frame_length(declared) || frame.len() != 4 + declared {
        return Err(BridgeProtocolError::InvalidPayload);
    }
    decode_frame_payload(frame[4..].to_vec())
}

/// Normalize a displayed SHA-256 fingerprint (optionally colon/space separated,
/// optionally uppercase) to the 64 lowercase hex characters used for pinning.
/// Returns `None` for anything that is not exactly 32 bytes of hex.
pub fn normalize_fingerprint(input: &str) -> Option<String> {
    let digits: String = input
        .chars()
        .filter(|c| !matches!(c, ':' | ' ' | '-' | '\t' | '\n' | '\r'))
        .collect();
    if digits.len() != 64 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(digits.to_ascii_lowercase())
}

/// SHA-256 of a DER certificate, lowercase hex. Matches the backend's pin
/// computation over the TLS end-entity certificate (no chain, no hostname check).
pub fn certificate_fingerprint(cert_der: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(cert_der)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BridgeMessage;

    #[test]
    fn frame_round_trip() {
        let msg = BridgeMessage::PairCode { code: "123456".into() };
        let frame = encode_frame(msg.clone());
        let len = frame_length([frame[0], frame[1], frame[2], frame[3]]);
        assert_eq!(len, frame.len() - 4);
        let decoded = decode_frame_payload(frame[4..].to_vec()).unwrap();
        assert_eq!(decoded, msg);
    }

    /// Golden bytes shared with the Kotlin codec in apps/android. Any change here
    /// is a wire-format break and must be mirrored in `WireProtocol.kt`.
    #[test]
    fn input_event_frame_matches_kotlin_wire_layout() {
        let frame = encode_frame(BridgeMessage::InputEvent {
            target: "pc".into(),
            payload: vec![0xaa, 0xbb, 0xcc],
        });
        let expected = vec![
            0, 0, 0, 27, // u32 BE frame length
            2, 0, // u16 LE protocol version
            4, 0, 0, 0, // u32 LE Message::InputEvent variant index
            2, 0, 0, 0, 0, 0, 0, 0, b'p', b'c', // String length u64 LE + UTF-8
            3, 0, 0, 0, 0, 0, 0, 0, 0xaa, 0xbb, 0xcc, // Vec length u64 LE + bytes
        ];
        assert_eq!(frame, expected);
    }

    #[test]
    fn decode_frame_rejects_bad_lengths_and_trailing_bytes() {
        let good = encode_frame(BridgeMessage::PairCode { code: "123456".into() });
        assert!(decode_frame(&good).is_ok());
        assert_eq!(
            decode_frame(&good[..good.len() - 1]).unwrap_err(),
            BridgeProtocolError::InvalidPayload
        );
        let mut trailing = good.clone();
        trailing.push(0);
        assert_eq!(
            decode_frame(&trailing).unwrap_err(),
            BridgeProtocolError::InvalidPayload
        );
        assert_eq!(
            decode_frame(&[0, 0, 0, 0]).unwrap_err(),
            BridgeProtocolError::InvalidPayload
        );
        assert_eq!(
            decode_frame(&[0xff, 0xff, 0xff, 0xff]).unwrap_err(),
            BridgeProtocolError::InvalidPayload
        );
    }

    #[test]
    fn fingerprint_parser_normalizes_hex_and_rejects_garbage() {
        let hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let coloned = hex
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(":");
        assert_eq!(normalize_fingerprint(hex).as_deref(), Some(hex));
        assert_eq!(normalize_fingerprint(&coloned).as_deref(), Some(hex));
        assert_eq!(normalize_fingerprint(&hex.to_uppercase()).as_deref(), Some(hex));
        assert_eq!(normalize_fingerprint("zz"), None);
        assert_eq!(normalize_fingerprint(&hex[..62]), None);
    }

    #[test]
    fn certificate_fingerprint_matches_backend_lowercase_hex() {
        assert_eq!(
            certificate_fingerprint(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
