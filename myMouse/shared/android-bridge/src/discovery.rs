/// Discovery primitive models for Android.
///
/// The Windows backend uses `mdns-sd` directly. On Android, mDNS/NSD requires
/// platform APIs (NsdManager) or a platform-specific Rust implementation with
/// appropriate network permissions. This module provides the shared data model
/// and parsing helpers so Kotlin and Rust agree on peer identity.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub fingerprint_hint: Option<String>,
}

pub const SERVICE_TYPE: &str = "_mymouse._tcp.";
pub const DEFAULT_PORT: u16 = 45552;

/// Parse a service instance name in the form `name@id` (same as backend).
pub fn parse_service_name(service: &str) -> Option<(String, String)> {
    let (name, id) = service.split_once('@')?;
    if name.is_empty() || id.is_empty() {
        return None;
    }
    Some((name.to_string(), id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_service_name() {
        let (name, id) = parse_service_name("MyPC@abc123").unwrap();
        assert_eq!(name, "MyPC");
        assert_eq!(id, "abc123");
    }
}
