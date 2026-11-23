use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL_VERSION: u16 = 1;
pub const PROTOCOL_HEADER_LEN: usize = 2;
const MAX_BODY_LEN: usize = 1024 * 1024;

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
            ProtocolError::TooLarge => "message body exceeds 1 MiB",
            ProtocolError::InvalidPayload => "invalid message payload",
        };
        f.write_str(text)
    }
}

impl std::error::Error for ProtocolError {}

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
    let message: Message = bincode::deserialize(body).map_err(|_| ProtocolError::InvalidPayload)?;
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
    fn hello_round_trip() {
        let m = Message::Hello { version: 1, device_id: "pc-a".into(), name: "PC A".into() };
        assert_eq!(decode(&encode(&m)).unwrap(), m);
    }

    #[test]
    fn rejects_wrong_version() {
        let bytes = encode(&Message::Hello { version: 99, device_id: "x".into(), name: "X".into() });
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn rejects_truncated_and_garbage_without_panicking() {
        assert!(decode(&[1]).is_err());
        assert!(decode(&[1, 0, 0xff, 0xff, 0x00]).is_err());
    }
}
