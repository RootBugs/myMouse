mod clipboard;
mod discovery;
mod file_transfer;
mod identity;
mod input;
mod layout;
mod network;
mod pairing;
mod test_mode;
mod transport;
mod trust;

use clipboard::{ClipboardManager, ClipboardMode, ClipboardSettings, ClipboardSnapshot};
use file_transfer::{FileTransferManager, FileTransferSettings};
use identity::Identity;
use mymouse_protocol::{ClipboardKind, Message, PROTOCOL_VERSION};
use network::PeerRuntimeHandle;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{timeout, Duration};
use transport::{ClientPinMap, Server};
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
    /// Pairing token for this install, filled in when production starts.
    identity_fingerprint: String,
    listen_port: u16,
    /// Fingerprint → device ID map used by the mutual-TLS listener.
    client_pins: ClientPinMap,
}

impl AppState {
    fn new(config_dir: PathBuf) -> Self {
        Self::with_device_id(config_dir, "local-pc".to_string())
    }

    fn with_device_id(config_dir: PathBuf, device_id: String) -> Self {
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
            identity_fingerprint: String::new(),
            listen_port: 0,
            client_pins: Arc::new(RwLock::new(HashMap::new())),
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

/// Shared by production and test-mode IPC so both expose identical settings
/// and clipboard-send behaviour.
pub(crate) fn apply_clipboard_settings(state: &mut AppState, params: &Value) {
    if let Some(enabled) = params.get("enabled").and_then(Value::as_bool) {
        state.settings.clipboard.enabled = enabled;
    }
    if let Some(mode) = params.get("mode").and_then(Value::as_str) {
        state.settings.clipboard.mode = match mode {
            "cursor_scoped" | "cursorScoped" => ClipboardMode::CursorScoped,
            _ => ClipboardMode::Global,
        };
    }
    let settings = state.settings.clipboard.clone();
    state.clipboard.set_settings(settings);
    let _ = state.save_settings();
}

pub(crate) fn apply_file_transfer_settings(state: &mut AppState, params: &Value) {
    if let Some(auto_accept) = params.get("autoAccept").and_then(Value::as_bool) {
        state.settings.file_transfer.auto_accept = auto_accept;
    }
    if let Some(save_dir) = params.get("saveDir").and_then(Value::as_str) {
        if !save_dir.trim().is_empty() {
            state.settings.file_transfer.save_dir = save_dir.to_string();
        }
    }
    if let Some(max) = params.get("maxTransferBytes").and_then(Value::as_u64) {
        state.settings.file_transfer.max_transfer_bytes = max.max(1024);
    }
    let settings = state.settings.file_transfer.clone();
    state.transfers.set_settings(settings);
    let _ = state.save_settings();
}

pub(crate) fn clipboard_message_from_params(
    manager: &ClipboardManager,
    params: &Value,
) -> Option<Message> {
    let kind = match params.get("kind").and_then(Value::as_str).unwrap_or("text") {
        "image" => ClipboardKind::Image,
        _ => ClipboardKind::Text,
    };
    let text = params.get("text").and_then(Value::as_str).map(str::to_string);
    let data_base64 = params
        .get("dataBase64")
        .and_then(Value::as_str)
        .or_else(|| params.get("data_base64").and_then(Value::as_str))
        .map(str::to_string);
    let mime = params.get("mime").and_then(Value::as_str).map(str::to_string);
    manager.build_update(ClipboardSnapshot {
        kind,
        text,
        data_base64,
        mime,
    })
}

fn is_hex_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

async fn handle_request(
    state: &Arc<Mutex<AppState>>,
    network: &PeerRuntimeHandle,
    id: u64,
    request: &Value,
) {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "status" => {
            let connected = network.connected().await;
            let device_id = state.lock().unwrap().device_id.clone();
            rpc_ok(
                id,
                json!({ "ok": true, "connected": connected, "deviceId": device_id }),
            );
        }
        "localIdentity" => {
            let st = state.lock().unwrap();
            rpc_ok(
                id,
                json!({
                    "ok": true,
                    "deviceId": st.device_id,
                    "fingerprint": st.identity_fingerprint,
                    "listenPort": st.listen_port,
                }),
            );
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
        "connectPeer" => {
            let address = params.get("address").and_then(Value::as_str).unwrap_or("");
            let peer_id = params
                .get("peerDeviceId")
                .and_then(Value::as_str)
                .unwrap_or("peer")
                .to_string();
            let fingerprint = params
                .get("fingerprint")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !is_hex_fingerprint(fingerprint) {
                return rpc_err(id, "fingerprint must be 64 hex characters");
            }
            if let Some(pinned) = state.lock().unwrap().trust.fingerprint(&peer_id) {
                if !pinned.eq_ignore_ascii_case(fingerprint) {
                    return rpc_err(id, "fingerprint does not match pinned trust entry");
                }
            }
            let Ok(address) = address.parse::<SocketAddr>() else {
                return rpc_err(id, "address must be host:port");
            };
            let pinned = fingerprint.to_ascii_lowercase();
            match network.connect(address, pinned.clone(), peer_id.clone()).await {
                Ok(()) => {
                    {
                        let mut st = state.lock().unwrap();
                        st.trust.set_fingerprint(&peer_id, pinned.clone());
                        if let Ok(mut pins) = st.client_pins.write() {
                            pins.insert(pinned, peer_id.clone());
                        }
                        let trust_path = st.config_dir.join("trust.json");
                        let _ = st.trust.save(trust_path);
                    }
                    rpc_ok(id, json!({ "ok": true }))
                }
                Err(error) => rpc_err(id, &error),
            }
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
            apply_clipboard_settings(&mut st, &params);
            rpc_ok(id, json!({ "ok": true, "settings": st.settings.clipboard }));
        }
        "setFileTransferSettings" => {
            let mut st = state.lock().unwrap();
            apply_file_transfer_settings(&mut st, &params);
            rpc_ok(id, json!({ "ok": true, "settings": st.settings.file_transfer }));
        }
        "listTransfers" => {
            let st = state.lock().unwrap();
            rpc_ok(id, json!({ "ok": true, "transfers": st.transfers.history() }));
        }
        "acceptTransfer" => {
            let transfer_id = params.get("transferId").and_then(Value::as_str).unwrap_or("");
            let messages = {
                let mut st = state.lock().unwrap();
                st.transfers.accept(transfer_id)
            };
            match messages {
                Ok(messages) => {
                    let count = messages.len();
                    for message in messages {
                        if let Err(error) = network.send(message).await {
                            return rpc_err(id, &error);
                        }
                    }
                    rpc_ok(id, json!({ "ok": true, "messages": count }));
                }
                Err(error) => rpc_err(id, &error.to_string()),
            }
        }
        "rejectTransfer" => {
            let transfer_id = params.get("transferId").and_then(Value::as_str).unwrap_or("");
            let reason = params.get("reason").and_then(Value::as_str).map(|s| s.to_string());
            let messages = {
                let mut st = state.lock().unwrap();
                st.transfers.reject(transfer_id, reason)
            };
            for message in messages {
                if let Err(error) = network.send(message).await {
                    return rpc_err(id, &error);
                }
            }
            rpc_ok(id, json!({ "ok": true }));
        }
        "offerFile" => {
            let path = params.get("path").and_then(Value::as_str).unwrap_or("");
            let peer = params.get("peerDeviceId").and_then(Value::as_str).unwrap_or("peer");
            let offer = {
                let mut st = state.lock().unwrap();
                st.transfers
                    .create_offer(std::path::Path::new(path), peer)
                    .map(|(offer, _)| offer)
            };
            match offer {
                Ok(offer) => {
                    if let Err(error) = network.send(Message::FileTransferOffer(offer.clone())).await {
                        return rpc_err(id, &error);
                    }
                    rpc_ok(id, json!({ "ok": true, "offer": offer }));
                }
                Err(error) => rpc_err(id, &error.to_string()),
            }
        }
        "sendClipboard" | "clipboardWrite" => {
            let message = {
                let st = state.lock().unwrap();
                clipboard_message_from_params(&st.clipboard, &params)
            };
            match message {
                Some(message) => match network.send(message).await {
                    Ok(()) => rpc_ok(id, json!({ "ok": true })),
                    Err(error) => rpc_err(id, &error),
                },
                None => rpc_err(id, "clipboard disabled, duplicate or oversized"),
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
            network.shutdown();
            rpc_ok(id, json!({ "ok": true }));
            std::process::exit(0);
        }
        _ => rpc_err(id, "unknown method"),
    }
}

/// A peer must announce itself with a v2 `Hello` naming a device already in
/// the trust store with at least one usable capability, and must present the
/// client certificate pinned for that exact device ID. Anything else is
/// dropped before the stream reaches the message dispatcher.
fn authorize_inbound_hello(
    state: &Arc<Mutex<AppState>>,
    message: &Message,
    peer_fingerprint: Option<&str>,
) -> Option<String> {
    let Message::Hello {
        version,
        device_id,
        name: _,
    } = message
    else {
        return None;
    };
    if *version != PROTOCOL_VERSION {
        return None;
    }
    let st = state.lock().unwrap();
    if !st.trust.is_trusted(device_id) {
        return None;
    }
    let usable = st
        .trust
        .capabilities(device_id)
        .map(|cap| cap.input || cap.clipboard || cap.files)
        .unwrap_or(false);
    let certificate_matches = st
        .trust
        .fingerprint(device_id)
        .zip(peer_fingerprint)
        .map(|(pinned, presented)| pinned.eq_ignore_ascii_case(presented))
        .unwrap_or(false);
    (usable && certificate_matches).then(|| device_id.clone())
}

/// Handshake gate for production inbound peers. TLS is already pinned by the
/// transport; this adds an identity/capability check before any application
/// message (clipboard, files, input) is dispatched.
async fn accept_inbound_loop(
    server: Server,
    state: Arc<Mutex<AppState>>,
    network: PeerRuntimeHandle,
) {
    loop {
        let mut stream = match server.accept().await {
            Ok(stream) => stream,
            Err(error) => {
                emit(json!({ "notification": "listener-error", "error": error.to_string() }));
                break;
            }
        };
        let presented = stream.peer_fingerprint().map(str::to_string);
        let peer_id = match timeout(Duration::from_secs(5), stream.recv()).await {
            Ok(Ok(message)) => authorize_inbound_hello(&state, &message, presented.as_deref()),
            Ok(Err(error)) => {
                emit(json!({
                    "notification": "inboundRejected",
                    "reason": format!("handshake failed: {error}"),
                }));
                None
            }
            Err(_) => {
                emit(json!({
                    "notification": "inboundRejected",
                    "reason": "handshake timed out",
                }));
                None
            }
        };
        let Some(peer_id) = peer_id else {
            emit(json!({
                "notification": "inboundRejected",
                "reason": "untrusted peer",
            }));
            continue;
        };
        emit(json!({
            "notification": "inboundPeer",
            "deviceId": peer_id,
            "message": "trusted peer attached",
        }));
        if network.attach_inbound(stream, peer_id).await.is_err() {
            break;
        }
    }
}

async fn run_ipc() {
    let config = config_dir();
    let state = Arc::new(Mutex::new(AppState::new(config.clone())));
    let device_id = state.lock().unwrap().device_id.clone();
    let network = network::start(state.clone(), device_id, String::new(), true);

    // Production identity is generated once and reused; its fingerprint is the
    // public pairing token. The private key never leaves the config directory.
    let identity = match Identity::load_or_create(&config) {
        Ok(identity) => identity,
        Err(error) => {
            eprintln!("mymouse: could not load TLS identity: {error}");
            return;
        }
    };
    network.set_identity(Arc::new(identity.clone()));
    let pins: ClientPinMap = Arc::new(RwLock::new(
        state
            .lock()
            .unwrap()
            .trust
            .fingerprint_map()
            .into_iter()
            .collect(),
    ));
    {
        let mut st = state.lock().unwrap();
        st.client_pins = pins.clone();
    }
    let port = std::env::var("MYMOUSE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(24800);
    match Server::bind_pinned(
        &format!("0.0.0.0:{port}"),
        identity.cert_der.clone(),
        identity.key_der.clone(),
        pins,
    )
    .await
    {
        Ok(server) => {
            {
                let mut st = state.lock().unwrap();
                st.identity_fingerprint = identity.fingerprint().to_string();
                st.listen_port = server.local_addr().port();
            }
            let listener_state = state.clone();
            let listener_network = network.clone();
            tokio::spawn(accept_inbound_loop(server, listener_state, listener_network));
        }
        Err(error) => {
            eprintln!("mymouse: could not bind pinned TLS listener: {error}");
            let mut st = state.lock().unwrap();
            st.identity_fingerprint = identity.fingerprint().to_string();
        }
    }

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = request.get("id").and_then(Value::as_u64).unwrap_or(0);
        handle_request(&state, &network, id, &request).await;
    }
    network.shutdown();
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
        run_ipc().await;
        return;
    }
    // Default: run IPC loop (Electron spawns with --ipc stdio; also safe default).
    run_ipc().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use trust::Capabilities;

    fn hello(version: u16, device_id: &str) -> Message {
        Message::Hello {
            version,
            device_id: device_id.to_string(),
            name: "Peer".to_string(),
        }
    }

    #[test]
    fn scaffold_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mymouse-backend");
    }

    #[test]
    fn inbound_hello_requires_trust_and_supported_version() {
        let mut state = AppState::with_device_id(std::env::temp_dir(), "local-pc".to_string());
        state.trust.grant("pc-b", Capabilities::default());
        let pin = "c".repeat(64);
        state.trust.set_fingerprint("pc-b", pin.clone());
        let state = Arc::new(Mutex::new(state));

        assert_eq!(
            authorize_inbound_hello(
                &state,
                &hello(PROTOCOL_VERSION, "pc-b"),
                Some(pin.as_str())
            ),
            Some("pc-b".to_string())
        );
        assert_eq!(
            authorize_inbound_hello(&state, &hello(999, "pc-b"), Some(pin.as_str())),
            None
        );
        assert_eq!(
            authorize_inbound_hello(
                &state,
                &hello(PROTOCOL_VERSION, "pc-b"),
                Some(&"d".repeat(64))
            ),
            None
        );
        assert_eq!(
            authorize_inbound_hello(&state, &hello(PROTOCOL_VERSION, "pc-b"), None),
            None
        );
        assert_eq!(
            authorize_inbound_hello(&state, &hello(PROTOCOL_VERSION, "stranger"), Some(&pin)),
            None
        );
        assert_eq!(
            authorize_inbound_hello(
                &state,
                &Message::ClipboardUpdate(mymouse_protocol::ClipboardPayload {
                    device_id: "pc-b".to_string(),
                    kind: mymouse_protocol::ClipboardKind::Text,
                    text: Some("skip".to_string()),
                    data_base64: None,
                    mime: None,
                    timestamp_ms: 1,
                    seq: 1,
                }),
                Some(pin.as_str())
            ),
            None
        );
    }
}
