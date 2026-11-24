use base64::Engine;
use mymouse_protocol::{ClipboardKind, ClipboardPayload, Message};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CLIPBOARD_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardMode {
    Global,
    CursorScoped,
}

impl Default for ClipboardMode {
    fn default() -> Self {
        Self::Global
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardSettings {
    pub enabled: bool,
    pub mode: ClipboardMode,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: ClipboardMode::Global,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    pub kind: ClipboardKind,
    pub text: Option<String>,
    pub data_base64: Option<String>,
    pub mime: Option<String>,
}

pub struct ClipboardManager {
    device_id: String,
    settings: ClipboardSettings,
    last_timestamp_ms: AtomicU64,
    last_seq: AtomicU64,
    local_seq: AtomicU64,
    last_fingerprint: std::sync::Mutex<Option<String>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn fingerprint(snapshot: &ClipboardSnapshot) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(format!("{:?}|", snapshot.kind).as_bytes());
    if let Some(text) = &snapshot.text {
        hasher.update(text.as_bytes());
    }
    if let Some(data) = &snapshot.data_base64 {
        hasher.update(data.as_bytes());
    }
    if let Some(mime) = &snapshot.mime {
        hasher.update(mime.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

impl ClipboardManager {
    pub fn new(device_id: impl Into<String>, settings: ClipboardSettings) -> Self {
        Self {
            device_id: device_id.into(),
            settings,
            last_timestamp_ms: AtomicU64::new(0),
            last_seq: AtomicU64::new(0),
            local_seq: AtomicU64::new(0),
            last_fingerprint: std::sync::Mutex::new(None),
        }
    }

    pub fn settings(&self) -> &ClipboardSettings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: ClipboardSettings) {
        self.settings = settings;
    }

    pub fn build_update(&self, snapshot: ClipboardSnapshot) -> Option<Message> {
        if !self.settings.enabled {
            return None;
        }
        let fp = fingerprint(&snapshot);
        {
            let mut last = self.last_fingerprint.lock().unwrap();
            if last.as_deref() == Some(&fp) {
                return None;
            }
            *last = Some(fp);
        }

        let text = snapshot.text;
        let data_base64 = snapshot.data_base64;
        if let Some(t) = &text {
            if t.len() > MAX_CLIPBOARD_BYTES {
                return None;
            }
        }
        if let Some(d) = &data_base64 {
            if d.len() > MAX_CLIPBOARD_BYTES * 2 {
                return None;
            }
        }

        let seq = self.local_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let timestamp_ms = now_ms();
        self.last_timestamp_ms.store(timestamp_ms, Ordering::SeqCst);
        self.last_seq.store(seq, Ordering::SeqCst);

        Some(Message::ClipboardUpdate(ClipboardPayload {
            device_id: self.device_id.clone(),
            kind: snapshot.kind,
            text,
            data_base64,
            mime: snapshot.mime,
            timestamp_ms,
            seq,
        }))
    }

    /// Apply a remote clipboard update if it wins last-write-wins arbitration.
    /// Returns a snapshot to write to the OS clipboard when accepted.
    pub fn accept_remote(&self, payload: &ClipboardPayload) -> Option<ClipboardSnapshot> {
        if !self.settings.enabled {
            return None;
        }
        if payload.device_id == self.device_id {
            return None;
        }

        let incoming_ts = payload.timestamp_ms;
        let incoming_seq = payload.seq;
        let current_ts = self.last_timestamp_ms.load(Ordering::SeqCst);
        let current_seq = self.last_seq.load(Ordering::SeqCst);
        let wins = incoming_ts > current_ts
            || (incoming_ts == current_ts && incoming_seq > current_seq)
            || (incoming_ts == current_ts && incoming_seq == current_seq && payload.device_id > self.device_id);
        if !wins {
            return None;
        }

        let snapshot = ClipboardSnapshot {
            kind: payload.kind.clone(),
            text: payload.text.clone(),
            data_base64: payload.data_base64.clone(),
            mime: payload.mime.clone(),
        };
        let fp = fingerprint(&snapshot);
        {
            let mut last = self.last_fingerprint.lock().unwrap();
            *last = Some(fp);
        }
        self.last_timestamp_ms.store(incoming_ts, Ordering::SeqCst);
        self.last_seq.store(incoming_seq, Ordering::SeqCst);
        Some(snapshot)
    }

    /// Encode image bytes for protocol transport.
    pub fn encode_image(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    pub fn decode_image(data_base64: &str) -> Option<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(data_base64)
            .ok()
    }
}

#[cfg(windows)]
pub mod os {
    use super::ClipboardSnapshot;
    use base64::Engine;
    use mymouse_protocol::ClipboardKind;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

    const CF_UNICODETEXT: u32 = 13;
    const CF_DIB: u32 = 8;

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn read() -> Option<ClipboardSnapshot> {
        unsafe {
            if OpenClipboard(None).is_err() {
                return None;
            }
            let result = (|| {
                if IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok() {
                    if let Ok(handle) = GetClipboardData(CF_UNICODETEXT) {
                        let hglobal = HGLOBAL(handle.0);
                        let ptr = GlobalLock(hglobal) as *const u16;
                        if !ptr.is_null() {
                            let mut len = 0usize;
                            while *ptr.add(len) != 0 {
                                len += 1;
                                if len > 262144 { break; }
                            }
                            let slice = std::slice::from_raw_parts(ptr, len);
                            let text = String::from_utf16_lossy(slice);
                            let _ = GlobalUnlock(hglobal);
                            return Some(ClipboardSnapshot {
                                kind: ClipboardKind::Text,
                                text: Some(text),
                                data_base64: None,
                                mime: Some("text/plain".into()),
                            });
                        }
                    }
                }
                // PNG format fallback
                let png_fmt = RegisterClipboardFormatW(windows::core::PCWSTR(wide("PNG").as_ptr()));
                if png_fmt != 0 && IsClipboardFormatAvailable(png_fmt).is_ok() {
                    if let Ok(handle) = GetClipboardData(png_fmt) {
                        let hglobal = HGLOBAL(handle.0);
                        let ptr = GlobalLock(hglobal) as *const u8;
                        let size = GlobalSize(hglobal);
                        if !ptr.is_null() && size > 0 && size < 1024 * 1024 {
                            let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
                            let _ = GlobalUnlock(hglobal);
                            let data_base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                            return Some(ClipboardSnapshot {
                                kind: ClipboardKind::Image,
                                text: None,
                                data_base64: Some(data_base64),
                                mime: Some("image/png".into()),
                            });
                        }
                    }
                }
                let _ = CF_DIB;
                None
            })();
            let _ = CloseClipboard();
            result
        }
    }

    pub fn write(snapshot: &ClipboardSnapshot) -> bool {
        unsafe {
            if OpenClipboard(None).is_err() {
                return false;
            }
            let ok = (|| {
                if EmptyClipboard().is_err() {
                    return false;
                }
                match snapshot.kind {
                    ClipboardKind::Text => {
                        let text = snapshot.text.as_deref().unwrap_or("");
                        let wide: Vec<u16> = OsStr::new(text)
                            .encode_wide()
                            .chain(std::iter::once(0))
                            .collect();
                        let bytes = wide.len() * 2;
                        let hmem = match GlobalAlloc(GMEM_MOVEABLE, bytes) {
                            Ok(h) => h,
                            Err(_) => return false,
                        };
                        let ptr = GlobalLock(hmem) as *mut u16;
                        if ptr.is_null() {
                            return false;
                        }
                        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                        let _ = GlobalUnlock(hmem);
                        let handle = HANDLE(hmem.0);
                        SetClipboardData(CF_UNICODETEXT, Some(handle)).is_ok()
                    }
                    ClipboardKind::Image => {
                        let Some(data_b64) = &snapshot.data_base64 else { return false; };
                        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data_b64) else {
                            return false;
                        };
                        let png_fmt = RegisterClipboardFormatW(windows::core::PCWSTR(wide("PNG").as_ptr()));
                        if png_fmt == 0 { return false; }
                        let hmem = match GlobalAlloc(GMEM_MOVEABLE, bytes.len()) {
                            Ok(h) => h,
                            Err(_) => return false,
                        };
                        let ptr = GlobalLock(hmem) as *mut u8;
                        if ptr.is_null() { return false; }
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
                        let _ = GlobalUnlock(hmem);
                        let handle = HANDLE(hmem.0);
                        SetClipboardData(png_fmt, Some(handle)).is_ok()
                    }
                }
            })();
            let _ = CloseClipboard();
            ok
        }
    }
}

#[cfg(not(windows))]
pub mod os {
    use super::ClipboardSnapshot;

    pub fn read() -> Option<ClipboardSnapshot> {
        None
    }

    pub fn write(_snapshot: &ClipboardSnapshot) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_snapshot(text: &str) -> ClipboardSnapshot {
        ClipboardSnapshot {
            kind: ClipboardKind::Text,
            text: Some(text.to_string()),
            data_base64: None,
            mime: Some("text/plain".into()),
        }
    }

    #[test]
    fn build_update_emits_message_and_dedups() {
        let mgr = ClipboardManager::new("pc-a", ClipboardSettings::default());
        let msg = mgr.build_update(text_snapshot("hello")).expect("first update");
        match msg {
            Message::ClipboardUpdate(payload) => {
                assert_eq!(payload.device_id, "pc-a");
                assert_eq!(payload.text.as_deref(), Some("hello"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(mgr.build_update(text_snapshot("hello")).is_none(), "duplicate should be suppressed");
    }

    #[test]
    fn last_write_wins_accepts_newer_remote() {
        let mgr = ClipboardManager::new("pc-a", ClipboardSettings::default());
        let _ = mgr.build_update(text_snapshot("old"));
        let remote = ClipboardPayload {
            device_id: "pc-b".into(),
            kind: ClipboardKind::Text,
            text: Some("new".into()),
            data_base64: None,
            mime: Some("text/plain".into()),
            timestamp_ms: now_ms() + 10_000,
            seq: 1,
        };
        let accepted = mgr.accept_remote(&remote).expect("newer remote should win");
        assert_eq!(accepted.text.as_deref(), Some("new"));
    }

    #[test]
    fn ignores_older_remote() {
        let mgr = ClipboardManager::new("pc-a", ClipboardSettings::default());
        mgr.last_timestamp_ms.store(now_ms() + 10_000, Ordering::SeqCst);
        let remote = ClipboardPayload {
            device_id: "pc-b".into(),
            kind: ClipboardKind::Text,
            text: Some("old".into()),
            data_base64: None,
            mime: Some("text/plain".into()),
            timestamp_ms: 1,
            seq: 1,
        };
        assert!(mgr.accept_remote(&remote).is_none());
    }

    #[test]
    fn disabled_settings_block_updates() {
        let mgr = ClipboardManager::new("pc-a", ClipboardSettings { enabled: false, mode: ClipboardMode::Global });
        assert!(mgr.build_update(text_snapshot("x")).is_none());
    }
}