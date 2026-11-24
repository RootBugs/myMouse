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
}
