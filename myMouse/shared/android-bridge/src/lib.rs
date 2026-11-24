//! Android bridge for myMouse.
//!
//! This crate exposes protocol, pairing, trust, discovery, and transport
//! primitives in an FFI-friendly shape. It is intended to be consumed by the
//! Android app via UniFFI-generated Kotlin bindings when cross-compiled with the
//! Android NDK. In this scaffold environment, the crate compiles and tests on
//! the host; actual `.so` packaging and UniFFI binding generation are documented
//! in `README.md`.
//!
//! The bridge intentionally reuses `mymouse-protocol` for wire format so Android
//! and Windows speak the same protocol.

pub mod discovery;
pub mod pairing;
pub mod protocol;
pub mod transport;
pub mod trust;

pub use discovery::{parse_service_name, DiscoveredPeer, DEFAULT_PORT, SERVICE_TYPE};
pub use pairing::{generate_pairing_code, pairing_uri, parse_pairing_uri, PairingSession, ParsedPairingUri};
pub use protocol::{decode_message, encode_message, protocol_version, BridgeMessage, BridgeProtocolError};
pub use transport::{decode_frame_payload, encode_frame, frame_length, TransportConfig, MAX_FRAME_LEN};
pub use trust::{Capabilities, TrustStoreModel, TrustedDeviceRecord};

/// Simple version string for diagnostics from Kotlin.
pub fn bridge_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Build a default capabilities object for newly trusted devices.
pub fn default_capabilities() -> Capabilities {
    Capabilities::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_version_present() {
        assert!(!bridge_version().is_empty());
    }
}
