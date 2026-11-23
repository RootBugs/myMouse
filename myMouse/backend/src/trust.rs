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

fn sanitize_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect()
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
        self.devices.contains_key(&sanitize_id(id))
    }

    pub fn grant(&mut self, id: &str, capabilities: Capabilities) {
        self.devices.insert(
            sanitize_id(id),
            TrustedDevice {
                capabilities,
                fingerprint: None,
            },
        );
    }

    pub fn revoke(&mut self, id: &str) {
        self.devices.remove(&sanitize_id(id));
    }

    pub fn capabilities(&self, id: &str) -> Option<&Capabilities> {
        self.devices.get(&sanitize_id(id)).map(|d| &d.capabilities)
    }

    pub fn set_fingerprint(&mut self, id: &str, fingerprint: String) {
        if let Some(device) = self.devices.get_mut(&sanitize_id(id)) {
            device.fingerprint = Some(fingerprint);
        }
    }

    pub fn fingerprint(&self, id: &str) -> Option<&str> {
        self.devices
            .get(&sanitize_id(id))
            .and_then(|d| d.fingerprint.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_store_stays_inside_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::default();
        store.grant("pc/../evil", Capabilities::default());
        store.save(dir.path().join("trust.json")).unwrap();
        assert!(dir.path().join("trust.json").exists());
        assert_eq!(dir.path().read_dir().unwrap().count(), 1);
    }
}
