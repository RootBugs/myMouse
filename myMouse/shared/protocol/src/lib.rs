use bincode::Options;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL_VERSION: u16 = 2;
pub const PROTOCOL_HEADER_LEN: usize = 2;
/// Encoded-frame cap shared with the transport. The transport length prefix is
/// separate from these bytes, so both layers use the same encoded payload cap.
pub const MAX_ENCODED_FRAME_LEN: usize = 1024 * 1024;
const MAX_BODY_LEN: usize = MAX_ENCODED_FRAME_LEN - PROTOCOL_HEADER_LEN;

pub type DeviceId = String;
pub type TransferId = String;

pub type ProtocolResult<T> = Result<T, ProtocolError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    TooShort,
    VersionMismatch,
    TooLarge,
    InvalidPayload,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            ProtocolError::TooShort => "message shorter than protocol header",
            ProtocolError::VersionMismatch => "unsupported protocol version",
            ProtocolError::TooLarge => "message frame exceeds 1 MiB",
            ProtocolError::InvalidPayload => "invalid message payload",
        };
        f.write_str(text)
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardKind {
    Text,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardPayload {
    pub device_id: DeviceId,
    pub kind: ClipboardKind,
    /// UTF-8 text for text clipboard updates.
    pub text: Option<String>,
    /// Base64-encoded image bytes (PNG) for image clipboard updates.
    pub data_base64: Option<String>,
    /// MIME type, e.g. "image/png".
    pub mime: Option<String>,
    /// Milliseconds since UNIX epoch; used for last-write-wins.
    pub timestamp_ms: u64,
    /// Monotonic per-device sequence for tie-breaking.
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOffer {
    pub transfer_id: TransferId,
    pub from_device_id: DeviceId,
    pub name: String,
    /// Size in bytes.
    pub size: u64,
    /// SHA-256 hex of the full file content.
    pub sha256: String,
    /// MIME type hint.
    pub mime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChunk {
    pub transfer_id: TransferId,
    /// Byte offset of this chunk.
    pub offset: u64,
    /// Raw chunk bytes.
    pub data: Vec<u8>,
    /// Whether this is the final chunk.
    pub last: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatus {
    Offered,
    Accepted,
    Rejected,
    InProgress,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message {
    Hello {
        version: u16,
        device_id: String,
        name: String,
    },
    PairCode {
        code: String,
    },
    PairApprove,
    PairReject,
    InputEvent {
        target: String,
        payload: Vec<u8>,
    },
    Status {
        connected: bool,
        devices: Vec<String>,
    },
    Error {
        message: String,
    },

    // Phase 2: clipboard sync
    ClipboardUpdate(ClipboardPayload),

    // Phase 2: file transfer
    FileTransferOffer(FileOffer),
    FileTransferAccept {
        transfer_id: TransferId,
    },
    FileTransferReject {
        transfer_id: TransferId,
        reason: Option<String>,
    },
    FileTransferChunk(FileChunk),
    FileTransferComplete {
        transfer_id: TransferId,
    },
    FileTransferCancel {
        transfer_id: TransferId,
        reason: Option<String>,
    },
}

/// Kotlin mirrors these fields as signed `Long`. Values above `i64::MAX`
/// decode in Rust but wrap in the Android client, so the wire format rejects
/// them at decode time instead of letting peers disagree.
fn has_unsigned_value_above_i64_max(message: &Message) -> bool {
    const LIMIT: u64 = i64::MAX as u64;
    match message {
        Message::ClipboardUpdate(payload) => {
            payload.timestamp_ms > LIMIT || payload.seq > LIMIT
        }
        Message::FileTransferOffer(offer) => offer.size > LIMIT,
        Message::FileTransferChunk(chunk) => chunk.offset > LIMIT,
        _ => false,
    }
}

pub fn encode(message: &Message) -> Vec<u8> {
    let body = bincode::serialize(message).expect("Message serialization cannot fail");
    let mut bytes = Vec::with_capacity(PROTOCOL_HEADER_LEN + body.len());
    bytes.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

pub fn decode(bytes: &[u8]) -> ProtocolResult<Message> {
    if bytes.len() < PROTOCOL_HEADER_LEN {
        return Err(ProtocolError::TooShort);
    }
    let version = u16::from_le_bytes([bytes[0], bytes[1]]);
    if version != PROTOCOL_VERSION {
        return Err(ProtocolError::VersionMismatch);
    }
    let body = &bytes[PROTOCOL_HEADER_LEN..];
    if body.len() > MAX_BODY_LEN {
        return Err(ProtocolError::TooLarge);
    }
    // Bound bincode's internal allocations by the bytes actually received.
    // bincode 1.3's slice `deserialize` discards `with_limit`, so decode from
    // a Cursor instead; that path honours the configured byte budget.
    let mut cursor = std::io::Cursor::new(body);
    let message: Message = bincode::options()
        .with_fixint_encoding()
        .with_limit(body.len() as u64)
        .deserialize_from(&mut cursor)
        .map_err(|error| match *error {
            bincode::ErrorKind::SizeLimit => ProtocolError::TooLarge,
            _ => ProtocolError::InvalidPayload,
        })?;
    // A frame is one message. Rejecting trailing bytes prevents peer-dependent
    // interpretation of the same authenticated frame.
    if cursor.position() as usize != body.len() {
        return Err(ProtocolError::InvalidPayload);
    }
    if has_unsigned_value_above_i64_max(&message) {
        return Err(ProtocolError::InvalidPayload);
    }
    if let Message::Hello { version, .. } = &message {
        if *version != PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch);
        }
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_declared_length_larger_than_body() {
        let mut bytes = encode(&Message::FileTransferChunk(FileChunk {
            transfer_id: "t".into(),
            offset: 0,
            data: vec![0; 4],
            last: true,
        }));
        // Body layout: variant (4) + id length (8) + id (1) + offset (8),
        // then the 8-byte length prefix for `data`.
        let prefix_at = PROTOCOL_HEADER_LEN + 4 + 8 + 1 + 8;
        bytes[prefix_at..prefix_at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        // The frame itself is small; only the internal length is bogus. The
        // decoder must reject it without trusting the declared allocation.
        assert_eq!(decode(&bytes), Err(ProtocolError::TooLarge));
    }

    #[test]
    fn decode_rejects_huge_string_length_without_allocating() {
        let mut bytes = encode(&Message::Error {
            message: "x".into(),
        });
        // Body: variant (4), then the String length prefix (8).
        let prefix_at = PROTOCOL_HEADER_LEN + 4;
        bytes[prefix_at..prefix_at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(decode(&bytes), Err(ProtocolError::TooLarge));
    }

    #[test]
    fn decode_rejects_trailing_bytes() {
        let mut bytes = encode(&Message::Hello {
            version: PROTOCOL_VERSION,
            device_id: "pc-a".into(),
            name: "A".into(),
        });
        bytes.extend_from_slice(&[0u8, 0u8]);
        assert_eq!(decode(&bytes), Err(ProtocolError::InvalidPayload));
    }

    #[test]
    fn decode_rejects_unsigned_values_above_i64_max() {
        let above = i64::MAX as u64 + 1;
        let clipboard_timestamp = Message::ClipboardUpdate(ClipboardPayload {
            device_id: "pc-a".into(),
            kind: ClipboardKind::Text,
            text: Some("x".into()),
            data_base64: None,
            mime: None,
            timestamp_ms: above,
            seq: 1,
        });
        assert_eq!(
            decode(&encode(&clipboard_timestamp)),
            Err(ProtocolError::InvalidPayload)
        );

        let clipboard_seq = Message::ClipboardUpdate(ClipboardPayload {
            device_id: "pc-a".into(),
            kind: ClipboardKind::Text,
            text: Some("x".into()),
            data_base64: None,
            mime: None,
            timestamp_ms: 1,
            seq: above,
        });
        assert_eq!(
            decode(&encode(&clipboard_seq)),
            Err(ProtocolError::InvalidPayload)
        );

        let offer = Message::FileTransferOffer(FileOffer {
            transfer_id: "t".into(),
            from_device_id: "pc-a".into(),
            name: "big.bin".into(),
            size: above,
            sha256: "x".into(),
            mime: None,
        });
        assert_eq!(decode(&encode(&offer)), Err(ProtocolError::InvalidPayload));

        let chunk = Message::FileTransferChunk(FileChunk {
            transfer_id: "t".into(),
            offset: above,
            data: vec![0; 1],
            last: true,
        });
        assert_eq!(decode(&encode(&chunk)), Err(ProtocolError::InvalidPayload));
    }

    #[test]
    fn hello_round_trip() {
        let m = Message::Hello {
            version: PROTOCOL_VERSION,
            device_id: "pc-a".into(),
            name: "PC A".into(),
        };
        assert_eq!(decode(&encode(&m)).unwrap(), m);
    }

    #[test]
    fn rejects_wrong_version_header() {
        let mut bytes = encode(&Message::Hello {
            version: PROTOCOL_VERSION,
            device_id: "x".into(),
            name: "X".into(),
        });
        bytes[0] = 99;
        bytes[1] = 0;
        assert_eq!(decode(&bytes).unwrap_err(), ProtocolError::VersionMismatch);
    }

    #[test]
    fn rejects_wrong_hello_version_field() {
        let bytes = encode(&Message::Hello {
            version: 99,
            device_id: "x".into(),
            name: "X".into(),
        });
        assert_eq!(decode(&bytes).unwrap_err(), ProtocolError::VersionMismatch);
    }

    #[test]
    fn rejects_truncated_and_garbage_without_panicking() {
        assert_eq!(decode(&[1]).unwrap_err(), ProtocolError::TooShort);
        // A truncated/invalid frame must be rejected cleanly; the exact
        // error depends on whether the byte budget or the reader hits EOF.
        assert!(matches!(
            decode(&[PROTOCOL_VERSION as u8, 0, 0xff, 0xff, 0x00]),
            Err(ProtocolError::InvalidPayload | ProtocolError::TooLarge)
        ));
    }

    #[test]
    fn rejects_oversized_body() {
        let mut bytes = PROTOCOL_VERSION.to_le_bytes().to_vec();
        bytes.extend(std::iter::repeat(0u8).take(MAX_BODY_LEN + 1));
        assert_eq!(decode(&bytes).unwrap_err(), ProtocolError::TooLarge);
    }

    #[test]
    fn clipboard_update_round_trip() {
        let m = Message::ClipboardUpdate(ClipboardPayload {
            device_id: "pc-a".into(),
            kind: ClipboardKind::Text,
            text: Some("hello".into()),
            data_base64: None,
            mime: Some("text/plain".into()),
            timestamp_ms: 123,
            seq: 4,
        });
        assert_eq!(decode(&encode(&m)).unwrap(), m);
    }

    #[test]
    fn file_transfer_round_trip() {
        let offer = Message::FileTransferOffer(FileOffer {
            transfer_id: "t1".into(),
            from_device_id: "pc-a".into(),
            name: "note.txt".into(),
            size: 5,
            sha256: "abcd".into(),
            mime: Some("text/plain".into()),
        });
        assert_eq!(decode(&encode(&offer)).unwrap(), offer);

        let chunk = Message::FileTransferChunk(FileChunk {
            transfer_id: "t1".into(),
            offset: 0,
            data: b"hello".to_vec(),
            last: true,
        });
        assert_eq!(decode(&encode(&chunk)).unwrap(), chunk);

        let accept = Message::FileTransferAccept {
            transfer_id: "t1".into(),
        };
        assert_eq!(decode(&encode(&accept)).unwrap(), accept);
        let complete = Message::FileTransferComplete {
            transfer_id: "t1".into(),
        };
        assert_eq!(decode(&encode(&complete)).unwrap(), complete);
        let cancel = Message::FileTransferCancel {
            transfer_id: "t1".into(),
            reason: Some("user".into()),
        };
        assert_eq!(decode(&encode(&cancel)).unwrap(), cancel);
    }
}
