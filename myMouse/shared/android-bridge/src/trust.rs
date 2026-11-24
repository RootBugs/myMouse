use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Capability toggles stored per trusted device. Mirrors backend trust model.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedDeviceRecord {
    pub device_id: String,
    pub name: String,
    pub capabilities: Capabilities,
    pub fingerprint: Option<String>,
    pub paired_at_unix_ms: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TrustStoreModel {
    devices: HashMap<String, TrustedDeviceRecord>,
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

impl TrustStoreModel {
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        let temp = path.with_extension("tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&temp, json)?;
        std::fs::rename(&temp, path)
    }

    pub fn upsert(&mut self, record: TrustedDeviceRecord) {
        let id = sanitize_id(&record.device_id);
        self.devices.insert(id, record);
    }

    pub fn revoke(&mut self, device_id: &str) -> bool {
        self.devices.remove(&sanitize_id(device_id)).is_some()
    }

    pub fn get(&self, device_id: &str) -> Option<&TrustedDeviceRecord> {
        self.devices.get(&sanitize_id(device_id))
    }

    pub fn list(&self) -> Vec<TrustedDeviceRecord> {
        let mut values: Vec<_> = self.devices.values().cloned().collect();
        values.sort_by(|a, b| a.name.cmp(&b.name));
        values
    }

    pub fn is_trusted(&self, device_id: &str) -> bool {
        self.devices.contains_key(&sanitize_id(device_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_list_revoke() {
        let mut store = TrustStoreModel::default();
        store.upsert(TrustedDeviceRecord {
            device_id: "pc-1".into(),
            name: "PC".into(),
            capabilities: Capabilities::default(),
            fingerprint: Some("abc".into()),
            paired_at_unix_ms: 1,
        });
        assert!(store.is_trusted("pc-1"));
        assert_eq!(store.list().len(), 1);
        assert!(store.revoke("pc-1"));
        assert!(!store.is_trusted("pc-1"));
    }
}
