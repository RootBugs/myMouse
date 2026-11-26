//! Persistent per-installation device identity.
//!
//! The identity is created once and reused. Both the device ID and the TLS
//! fingerprint are public pairing tokens; the TLS private key stays separate
//! in `identity.rs`.

use crate::trust::is_valid_id;
use serde::{Deserialize, Serialize};
use std::io::{self, ErrorKind};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}

impl DeviceInfo {
    pub fn load_or_create(config_dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(config_dir)?;
        let path = config_dir.join("device.json");
        if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            let info: Self = serde_json::from_str(&raw)
                .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
            if !is_valid_id(&info.id) || info.name.trim().is_empty() {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "device.json contains an invalid device ID or name",
                ));
            }
            return Ok(info);
        }

        let info = Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: system_device_name(),
        };
        info.save(config_dir)?;
        Ok(info)
    }

    fn save(&self, config_dir: &Path) -> io::Result<()> {
        let path = config_dir.join("device.json");
        let tmp = config_dir.join("device.json.tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, path)
    }
}

fn system_device_name() -> String {
    let raw = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "myMouse PC".to_string());
    let cleaned: String = raw.chars().filter(|c| !c.is_control()).take(64).collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        "myMouse PC".to_string()
    } else {
        cleaned
    }
}
