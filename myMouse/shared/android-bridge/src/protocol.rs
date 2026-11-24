use mymouse_protocol::{
    decode as proto_decode, encode as proto_encode, ClipboardPayload, FileChunk, FileOffer, Message,
    ProtocolError,
};

/// FFI-friendly protocol error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeProtocolError {
    TooShort,
    VersionMismatch,
    TooLarge,
    InvalidPayload,
}

impl From<ProtocolError> for BridgeProtocolError {
    fn from(value: ProtocolError) -> Self {
        match value {
            ProtocolError::TooShort => Self::TooShort,
            ProtocolError::VersionMismatch => Self::VersionMismatch,
            ProtocolError::TooLarge => Self::TooLarge,
            ProtocolError::InvalidPayload => Self::InvalidPayload,
        }
    }
}

/// FFI-friendly message envelope. The Android UI uses this instead of
/// binding directly to the Rust enum's serde/bincode representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeMessage {
    Hello { version: u16, device_id: String, name: String },
    PairCode { code: String },
    PairApprove,
    PairReject,
    InputEvent { target: String, payload: Vec<u8> },
    Status { connected: bool, devices: Vec<String> },
    Error { message: String },

    // Phase 2 messages, exposed to the Android app through the same bridge.
    ClipboardUpdate(ClipboardPayload),
    FileTransferOffer(FileOffer),
    FileTransferAccept { transfer_id: String },
    FileTransferReject { transfer_id: String, reason: Option<String> },
    FileTransferChunk(FileChunk),
    FileTransferComplete { transfer_id: String },
    FileTransferCancel { transfer_id: String, reason: Option<String> },
}

impl From<Message> for BridgeMessage {
    fn from(value: Message) -> Self {
        match value {
            Message::Hello { version, device_id, name } => Self::Hello { version, device_id, name },
            Message::PairCode { code } => Self::PairCode { code },
            Message::PairApprove => Self::PairApprove,
            Message::PairReject => Self::PairReject,
            Message::InputEvent { target, payload } => Self::InputEvent { target, payload },
            Message::Status { connected, devices } => Self::Status { connected, devices },
            Message::Error { message } => Self::Error { message },
            Message::ClipboardUpdate(payload) => Self::ClipboardUpdate(payload),
            Message::FileTransferOffer(offer) => Self::FileTransferOffer(offer),
            Message::FileTransferAccept { transfer_id } => Self::FileTransferAccept { transfer_id },
            Message::FileTransferReject { transfer_id, reason } => Self::FileTransferReject { transfer_id, reason },
            Message::FileTransferChunk(chunk) => Self::FileTransferChunk(chunk),
            Message::FileTransferComplete { transfer_id } => Self::FileTransferComplete { transfer_id },
            Message::FileTransferCancel { transfer_id, reason } => Self::FileTransferCancel { transfer_id, reason },
        }
    }
}

impl From<BridgeMessage> for Message {
    fn from(value: BridgeMessage) -> Self {
        match value {
            BridgeMessage::Hello { version, device_id, name } => Self::Hello { version, device_id, name },
            BridgeMessage::PairCode { code } => Self::PairCode { code },
            BridgeMessage::PairApprove => Self::PairApprove,
            BridgeMessage::PairReject => Self::PairReject,
            BridgeMessage::InputEvent { target, payload } => Self::InputEvent { target, payload },
            BridgeMessage::Status { connected, devices } => Self::Status { connected, devices },
            BridgeMessage::Error { message } => Self::Error { message },
            BridgeMessage::ClipboardUpdate(payload) => Self::ClipboardUpdate(payload),
            BridgeMessage::FileTransferOffer(offer) => Self::FileTransferOffer(offer),
            BridgeMessage::FileTransferAccept { transfer_id } => Self::FileTransferAccept { transfer_id },
            BridgeMessage::FileTransferReject { transfer_id, reason } => Self::FileTransferReject { transfer_id, reason },
            BridgeMessage::FileTransferChunk(chunk) => Self::FileTransferChunk(chunk),
            BridgeMessage::FileTransferComplete { transfer_id } => Self::FileTransferComplete { transfer_id },
            BridgeMessage::FileTransferCancel { transfer_id, reason } => Self::FileTransferCancel { transfer_id, reason },
        }
    }
}

pub fn encode_message(message: BridgeMessage) -> Vec<u8> {
    proto_encode(&Message::from(message))
}

pub fn decode_message(bytes: Vec<u8>) -> Result<BridgeMessage, BridgeProtocolError> {
    proto_decode(&bytes)
        .map(BridgeMessage::from)
        .map_err(BridgeProtocolError::from)
}

pub fn protocol_version() -> u16 {
    mymouse_protocol::PROTOCOL_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use mymouse_protocol::ClipboardKind;

    #[test]
    fn phase_two_messages_round_trip_through_bridge() {
        let clipboard = BridgeMessage::ClipboardUpdate(ClipboardPayload {
            device_id: "pc-a".into(),
            kind: ClipboardKind::Text,
            text: Some("hello".into()),
            data_base64: None,
            mime: None,
            timestamp_ms: 1,
            seq: 2,
        });
        let decoded = decode_message(encode_message(clipboard.clone())).unwrap();
        assert_eq!(decoded, clipboard);

        let chunk = BridgeMessage::FileTransferChunk(FileChunk {
            transfer_id: "t1".into(),
            offset: 0,
            data: vec![1, 2, 3],
            last: true,
        });
        let decoded = decode_message(encode_message(chunk.clone())).unwrap();
        assert_eq!(decoded, chunk);
    }
}
