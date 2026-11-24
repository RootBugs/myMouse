mod clipboard;
mod discovery;
mod file_transfer;
mod input;
mod layout;
mod pairing;
mod test_mode;
mod transport;
mod trust;

use clipboard::{ClipboardManager, ClipboardSettings};
use file_transfer::{FileTransferManager, FileTransferSettings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use trust::TrustStore;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppSettings {
    #[serde(default)]
    clipboard: ClipboardSettings,
    #[serde(default)]
    file_transfer: FileTransferSettings,
    #[serde(default = "default_true")]
    auto_reconnect: bool,
    #[serde(default)]
    launch_at_start: bool,
    #[serde(default = "default_true")]
    quick_drop_enabled: bool,
}

fn default_true() -> bool {
    true
}

struct AppState {
    device_id: String,
    settings: AppSettings,
    trust: TrustStore,
    clipboard: ClipboardManager,
    transfers: FileTransferManager,
    config_dir: PathBuf,
}

impl AppState {
    fn new(config_dir: PathBuf) -> Self {
        let device_id = "local-pc".to_string();
        let settings_path = config_dir.join("settings.json");
        let settings: AppSettings = std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let trust = TrustStore::load(config_dir.join("trust.json"));
        let clipboard = ClipboardManager::new(device_id.clone(), settings.clipboard.clone());
        let transfers = FileTransferManager::new(device_id.clone(), settings.file_transfer.clone());
        Self {
            device_id,
            settings,
            trust,
            clipboard,
            transfers,
            config_dir,
        }
    }

    fn save_settings(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        let path = self.config_dir.join("settings.json");
        let tmp = path.with_extension("tmp");
        let json = serde_json::to_string_pretty(&self.settings)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, path)
    }
}

fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MYMOUSE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("myMouse");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".mymouse");
    }
    std::env::temp_dir().join("myMouse")
}

fn emit(value: Value) {
    println!("{value}");
    let _ = io::stdout().flush();
}

fn rpc_ok(id: u64, result: Value) {
    emit(json!({ "id": id, "result": result }));
}

fn rpc_err(id: u64, message: &str) {
    emit(json!({ "id": id, "result": { "ok": false, "error": message } }));
}

fn handle_request(state: &Arc<Mutex<AppState>>, id: u64, request: &Value) {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "status" => {
            let st = state.lock().unwrap();
            rpc_ok(id, json!({ "ok": true, "connected": false, "deviceId": st.device_id }));
        }
        "devices" => {
            let st = state.lock().unwrap();
            let devices: Vec<Value> = st
                .trust
                .capabilities_map()
                .into_iter()
                .map(|(id, caps)| json!({ "id": id, "name": id, "capabilities": caps }))
                .collect();
            rpc_ok(id, json!(devices));
        }
        "pairingUri" => {
            let session = pairing::PairSession::new("local-pc");
            rpc_ok(id, json!(session.pairing_uri()));
        }
        "approvePairing" => {
            let code = params.get("code").and_then(Value::as_str).unwrap_or("");
            let ok = code.len() == 6 && code.chars().all(|c| c.is_ascii_digit());
            rpc_ok(id, json!({ "ok": ok }));
        }
        "setLayout" => {
            rpc_ok(id, json!({ "ok": true }));
        }
        "getSettings" => {
            let st = state.lock().unwrap();
            rpc_ok(id, json!({ "ok": true, "settings": st.settings }));
        }
        "setClipboardSettings" => {
            let mut st = state.lock().unwrap();
            if let Some(enabled) = params.get("enabled").and_then(Value::as_bool) {
                st.settings.clipboard.enabled = enabled;
            }
            if let Some(mode) = params.get("mode").and_then(Value::as_str) {
                st.settings.clipboard.mode = match mode {
                    "cursor_scoped" | "cursorScoped" => clipboard::ClipboardMode::CursorScoped,
                    _ => clipboard::ClipboardMode::Global,
                };
            }
            let s = st.settings.clipboard.clone();
            st.clipboard.set_settings(s);
            let _ = st.save_settings();
            rpc_ok(id, json!({ "ok": true, "settings": st.settings.clipboard }));
        }
        "setFileTransferSettings" => {
            let mut st = state.lock().unwrap();
            if let Some(auto_accept) = params.get("autoAccept").and_then(Value::as_bool) {
                st.settings.file_transfer.auto_accept = auto_accept;
            }
            if let Some(save_dir) = params.get("saveDir").and_then(Value::as_str) {
                if !save_dir.trim().is_empty() {
                    st.settings.file_transfer.save_dir = save_dir.to_string();
                }
            }
            if let Some(max) = params.get("maxTransferBytes").and_then(Value::as_u64) {
                st.settings.file_transfer.max_transfer_bytes = max.max(1024);
            }
            let s = st.settings.file_transfer.clone();
            st.transfers.set_settings(s);
            let _ = st.save_settings();
            rpc_ok(id, json!({ "ok": true, "settings": st.settings.file_transfer }));
        }
        "listTransfers" => {
            let st = state.lock().unwrap();
            rpc_ok(id, json!({ "ok": true, "transfers": st.transfers.history() }));
        }
        "acceptTransfer" => {
            let transfer_id = params.get("transferId").and_then(Value::as_str).unwrap_or("");
            let mut st = state.lock().unwrap();
            match st.transfers.accept(transfer_id) {
                Ok(msgs) => rpc_ok(id, json!({ "ok": true, "messages": msgs.len() })),
                Err(e) => rpc_err(id, &e.to_string()),
            }
        }
        "rejectTransfer" => {
            let transfer_id = params.get("transferId").and_then(Value::as_str).unwrap_or("");
            let reason = params.get("reason").and_then(Value::as_str).map(|s| s.to_string());
            let mut st = state.lock().unwrap();
            st.transfers.reject(transfer_id, reason);
            rpc_ok(id, json!({ "ok": true }));
        }
        "offerFile" => {
            let path = params.get("path").and_then(Value::as_str).unwrap_or("");
            let peer = params.get("peerDeviceId").and_then(Value::as_str).unwrap_or("peer");
            let mut st = state.lock().unwrap();
            match st.transfers.create_offer(Path::new(path), peer) {
                Ok((offer, _msgs)) => rpc_ok(id, json!({ "ok": true, "offer": offer })),
                Err(e) => rpc_err(id, &e.to_string()),
            }
        }
        "clipboardRead" => {
            let st = state.lock().unwrap();
            if !st.settings.clipboard.enabled {
                return rpc_err(id, "clipboard disabled");
            }
            #[cfg(windows)]
            {
                let snapshot = clipboard::os::read();
                rpc_ok(id, json!({ "ok": true, "snapshot": snapshot.map(|s| json!({"kind": format!("{:?}", s.kind).to_lowercase(), "text": s.text, "dataBase64": s.data_base64, "mime": s.mime})) }));
            }
            #[cfg(not(windows))]
            {
                rpc_ok(id, json!({ "ok": true, "snapshot": null }));
            }
        }
        "quit" => {
            rpc_ok(id, json!({ "ok": true }));
            std::process::exit(0);
        }
        _ => rpc_err(id, "unknown method"),
    }
}

fn run_ipc() {
    let state = Arc::new(Mutex::new(AppState::new(config_dir())));
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = request.get("id").and_then(Value::as_u64).unwrap_or(0);
        handle_request(&state, id, &request);
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(config) = test_mode::parse_args(&args) {
        if let Err(error) = test_mode::run(config).await {
            eprintln!("mymouse test mode failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    if args.iter().any(|a| a == "--ipc" || a == "stdio") {
        run_ipc();
        return;
    }
    // Default: run IPC loop (Electron spawns with --ipc stdio; also safe default).
    run_ipc();
}

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mymouse-backend");
    }
}