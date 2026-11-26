use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub input: bool,
    pub clipboard: bool,
    pub files: bool,
    pub screen: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            input: true,
            clipboard: true,
            files: true,
            screen: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrustedDevice {
    capabilities: Capabilities,
    fingerprint: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TrustStore {
    devices: HashMap<String, TrustedDevice>,
}

/// Device IDs are used as exact trust-store and pin keys. Sanitising them
/// many-to-one allowed a crafted ID to overwrite an unrelated trusted device,
/// so invalid IDs are rejected outright instead.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

impl TrustStore {
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| {
                eprintln!("warning: corrupt trust store at {}, using empty store", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        let temp = path.with_extension("tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        std::fs::write(&temp, json)?;
        std::fs::rename(&temp, path)
    }

    pub fn is_trusted(&self, id: &str) -> bool {
        is_valid_id(id) && self.devices.contains_key(id)
    }

    pub fn grant(&mut self, id: &str, capabilities: Capabilities) {
        if is_valid_id(id) {
            self.devices.insert(
                id.to_string(),
                TrustedDevice {
                    capabilities,
                    fingerprint: None,
                },
            );
        }
    }

    /// Add a device only if it is not already trusted, so pairing a new peer
    /// cannot silently replace another device's capabilities or pin.
    pub fn grant_if_absent(&mut self, id: &str, capabilities: Capabilities) -> bool {
        if !is_valid_id(id) || self.devices.contains_key(id) {
            return false;
        }
        self.devices.insert(
            id.to_string(),
            TrustedDevice {
                capabilities,
                fingerprint: None,
            },
        );
        true
    }

    pub fn revoke(&mut self, id: &str) {
        if is_valid_id(id) {
            self.devices.remove(id);
        }
    }

    pub fn capabilities(&self, id: &str) -> Option<&Capabilities> {
        if !is_valid_id(id) {
            return None;
        }
        self.devices.get(id).map(|d| &d.capabilities)
    }

    /// A peer may open an application session only if it is explicitly trusted
    /// for at least one capability this runtime dispatches. A `screen`-only
    /// grant is intentionally not enough.
    pub fn allows_peer(&self, id: &str) -> bool {
        self.capabilities(id)
            .map(|caps| caps.input || caps.clipboard || caps.files)
            .unwrap_or(false)
    }

    pub fn set_capabilities(&mut self, id: &str, capabilities: Capabilities) -> bool {
        let Some(device) = self.devices.get_mut(id) else {
            return false;
        };
        device.capabilities = capabilities;
        true
    }

    pub fn capabilities_map(&self) -> Vec<(String, Capabilities)> {
        self.devices
            .iter()
            .map(|(id, device)| (id.clone(), device.capabilities.clone()))
            .collect()
    }

    pub fn set_fingerprint(&mut self, id: &str, fingerprint: String) {
        if !is_valid_id(id) {
            return;
        }
        if let Some(device) = self.devices.get_mut(id) {
            device.fingerprint = Some(fingerprint);
        }
    }

    pub fn fingerprint(&self, id: &str) -> Option<&str> {
        if !is_valid_id(id) {
            return None;
        }
        self.devices.get(id).and_then(|d| d.fingerprint.as_deref())
    }

    /// All pinned device/fingerprint pairs, for configuring mutual TLS.
    pub fn fingerprint_map(&self) -> Vec<(String, String)> {
        self.devices
            .iter()
            .filter_map(|(id, device)| device.fingerprint.as_ref().map(|fp| (id.clone(), fp.clone())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_peer_ids_are_never_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::default();
        for id in ["", "pc/../evil", "pc!b", " pc", "a/b", "a.b"] {
            store.grant(id, Capabilities::default());
            assert!(!store.is_trusted(id), "invalid id was trusted: {id:?}");
        }
        assert!(store.capabilities_map().is_empty());
        store.save(dir.path().join("trust.json")).unwrap();
        assert!(dir.path().join("trust.json").exists());
        assert_eq!(dir.path().read_dir().unwrap().count(), 1);
    }

    #[test]
    fn allows_peer_requires_an_explicit_usable_capability() {
        let mut store = TrustStore::default();
        assert!(!store.allows_peer("unknown"));

        store.grant(
            "screen-only",
            Capabilities {
                input: false,
                clipboard: false,
                files: false,
                screen: true,
            },
        );
        assert!(store.is_trusted("screen-only"));
        assert!(!store.allows_peer("screen-only"));

        for (id, capabilities) in [
            (
                "input-only",
                Capabilities {
                    input: true,
                    clipboard: false,
                    files: false,
                    screen: false,
                },
            ),
            (
                "clipboard-only",
                Capabilities {
                    input: false,
                    clipboard: true,
                    files: false,
                    screen: false,
                },
            ),
            (
                "files-only",
                Capabilities {
                    input: false,
                    clipboard: false,
                    files: true,
                    screen: false,
                },
            ),
        ] {
            store.grant(id, capabilities);
            assert!(store.allows_peer(id), "{id} should be allowed");
        }
    }

    #[test]
    fn trust_ids_are_exact_and_revocation_removes_the_pin() {
        let mut store = TrustStore::default();
        store.grant("pc-b", Capabilities::default());
        store.set_fingerprint("pc-b", "a".repeat(64));
        assert!(store.is_trusted("pc-b"));
        assert!(!store.is_trusted("pc!b"), "sanitized collisions must not grant trust");
        assert!(store.fingerprint("pc!b").is_none());
        store.revoke("pc-b");
        assert!(!store.is_trusted("pc-b"));
        assert!(store.fingerprint_map().is_empty());
    }
}
