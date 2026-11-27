mod clipboard;
mod device;
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
use device::DeviceInfo;
use file_transfer::{FileTransferManager, FileTransferSettings};
use identity::Identity;
use input::InputBridge;
use layout::Layout;
use mymouse_protocol::{ClipboardKind, Message, PROTOCOL_VERSION};
use network::PeerRuntimeHandle;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use transport::{ClientPinMap, Server};
use trust::{is_valid_id, Capabilities, TrustStore};

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
    device_name: String,
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
    /// Latest mDNS peers, keyed by device ID.
    discovered: Arc<RwLock<HashMap<String, discovery::Peer>>>,
    /// Short-lived pairing sessions, keyed by the displayed one-time code.
    pairing_sessions: Mutex<HashMap<String, pairing::PairSession>>,
    /// Shared capture bridge so RPCs can stop/resume local forwarding.
    input_bridge: Arc<Mutex<InputBridge>>,
    /// Shared topology so the UI/RPCs and the OS input hook see the same state.
    layout: Arc<RwLock<Layout>>,
}

impl AppState {
    pub(crate) fn new(config_dir: PathBuf) -> Result<Self, String> {
        let device = DeviceInfo::load_or_create(&config_dir)
            .map_err(|error| format!("could not load device identity: {error}"))?;
        Self::with_device_info(config_dir, device)
    }

    fn with_device_id(config_dir: PathBuf, device_id: String) -> Result<Self, String> {
        Self::with_device_info(
            config_dir,
            DeviceInfo {
                id: device_id.clone(),
                name: device_id,
            },
        )
    }

    fn with_device_info(config_dir: PathBuf, device: DeviceInfo) -> Result<Self, String> {
        let settings_path = config_dir.join("settings.json");
        let settings: AppSettings = std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let trust = TrustStore::load(config_dir.join("trust.json"));
        let layout = Layout::load(config_dir.join("layout.json"))?;
        let clipboard = ClipboardManager::new(device.id.clone(), settings.clipboard.clone());
        let transfers = FileTransferManager::new(device.id.clone(), settings.file_transfer.clone());
        Ok(Self {
            device_id: device.id,
            device_name: device.name,
            settings,
            trust,
            clipboard,
            transfers,
            config_dir,
            identity_fingerprint: String::new(),
            listen_port: 0,
            client_pins: Arc::new(RwLock::new(HashMap::new())),
            discovered: Arc::new(RwLock::new(HashMap::new())),
            pairing_sessions: Mutex::new(HashMap::new()),
            input_bridge: Arc::new(Mutex::new(InputBridge::new())),
            layout: Arc::new(RwLock::new(layout)),
        })
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

/// Validate, persist, then apply a topology. Persistence happens before the
/// live layout changes so a failed write cannot leave hook state ahead of disk.
fn apply_layout_params(state: &mut AppState, params: &Value) -> Result<Layout, String> {
    let raw = params
        .get("layout")
        .ok_or_else(|| "missing layout".to_string())?;
    let layout: Layout = serde_json::from_value(raw.clone())
        .map_err(|error| format!("invalid layout: {error}"))?;
    layout.validate()?;
    layout.save(state.config_dir.join("layout.json"))?;
    state.layout.write().unwrap().replace(layout.clone());
    Ok(layout)
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

/// Resolve the local monitor from the persisted topology. Layout validation
/// already bounds each index; with several local monitors the lowest index is
/// the deterministic local primary, and an unconfigured device keeps index 0.
fn local_monitor_index(layout: &Layout, local_device_id: &str) -> usize {
    layout
        .nodes()
        .iter()
        .filter(|node| node.device_id == local_device_id)
        .map(|node| node.monitor_index as usize)
        .min()
        .unwrap_or(0)
}

/// Production peers announce themselves on every outbound connection so the
/// remote listener's trust gate can identify them before dispatching data.
fn hello_message(state: &Arc<Mutex<AppState>>) -> Message {
    let st = state.lock().unwrap();
    Message::Hello {
        version: PROTOCOL_VERSION,
        device_id: st.device_id.clone(),
        name: st.device_name.clone(),
    }
}

/// Every production exit path releases remote-injected input before stopping
/// the peer runtime. `release_modifiers` also drains the injected tracker on
/// Windows and unconditionally lifts modifiers from older code paths.
fn shutdown_runtime(network: &PeerRuntimeHandle, release_input: impl FnOnce()) {
    release_input();
    network.shutdown();
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
                    "name": st.device_name,
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
        "discoveredPeers" => {
            let st = state.lock().unwrap();
            let peers: Vec<Value> = match st.discovered.read() {
                Ok(map) => map
                    .values()
                    .map(|peer| {
                        json!({
                            "id": peer.id,
                            "name": peer.name,
                            "address": peer.address,
                            "port": peer.port,
                            "fingerprint": peer.fingerprint,
                        })
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            rpc_ok(id, json!({ "ok": true, "peers": peers }));
        }
        "pairDiscovered" => {
            let peer_id = params.get("peerId").and_then(Value::as_str).unwrap_or("");
            let verified = params.get("verified").and_then(Value::as_bool).unwrap_or(false);
            let supplied_fp = params
                .get("fingerprint")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !verified {
                return rpc_err(id, "verify the full fingerprint out of band before pairing");
            }
            if !is_valid_id(peer_id) {
                return rpc_err(id, "invalid peer id");
            }
            if !is_hex_fingerprint(supplied_fp) {
                return rpc_err(id, "fingerprint must be 64 hex characters");
            }
            let fingerprint = supplied_fp.to_ascii_lowercase();
            let (address, port, advertised_fp) = {
                let st = state.lock().unwrap();
                match st.discovered.read().ok().and_then(|map| map.get(peer_id).cloned()) {
                    Some(peer) => (peer.address, peer.port, peer.fingerprint),
                    None => {
                        return rpc_err(id, "peer was not discovered");
                    }
                }
            };
            // mDNS TXT is unauthenticated. The full fingerprint must be
            // confirmed by the user from the peer's local UI.
            if !advertised_fp.eq_ignore_ascii_case(&fingerprint) {
                return rpc_err(id, "fingerprint does not match the discovered peer");
            }
            let Ok(socket_addr) = format!("{address}:{port}").parse::<SocketAddr>() else {
                return rpc_err(id, "discovered peer has no valid address");
            };
            {
                let st = state.lock().unwrap();
                if let Some(pinned) = st.trust.fingerprint(peer_id) {
                    if !pinned.eq_ignore_ascii_case(&fingerprint) {
                        return rpc_err(id, "peer fingerprint changed; revoke the old device first");
                    }
                }
            }
            // Connect first. Trust and pin are persisted only after the pinned
            // TLS handshake succeeds, so a failed pairing leaves no attacker
            // pin behind.
            match network
                .connect(socket_addr, fingerprint.clone(), peer_id.to_string())
                .await
            {
                Ok(ticket) => {
                    // The remote listener is gated: it will not dispatch any
                    // application data until it sees our trusted Hello. The
                    // ticket binds that Hello to this exact connection.
                    if let Err(error) =
                        network.send_to_connection(hello_message(state), ticket).await
                    {
                        return rpc_err(id, &format!("peer rejected handshake: {error}"));
                    }
                    {
                        let mut st = state.lock().unwrap();
                        st.trust.grant_if_absent(
                            peer_id,
                            Capabilities {
                                input: true,
                                clipboard: true,
                                files: true,
                                screen: false,
                            },
                        );
                        st.trust.set_fingerprint(peer_id, fingerprint.clone());
                        let trust_path = st.config_dir.join("trust.json");
                        let _ = st.trust.save(trust_path);
                        if let Ok(mut pins) = st.client_pins.write() {
                            pins.insert(fingerprint.clone(), peer_id.to_string());
                        };
                    }
                    rpc_ok(id, json!({ "ok": true }))
                }
                Err(error) => rpc_err(id, &error),
            }
        }
        "connectPeer" => {
            let address = params.get("address").and_then(Value::as_str).unwrap_or("");
            let peer_id = params
                .get("peerDeviceId")
                .and_then(Value::as_str)
                .unwrap_or("peer")
                .to_string();
            if !is_valid_id(&peer_id) {
                return rpc_err(id, "invalid peer device id");
            }
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
                Ok(ticket) => {
                    // Pinning authenticates the TLS peer; the Hello carries our
                    // device identity through the remote trust gate and is bound
                    // to the connection that this RPC just created.
                    if let Err(error) =
                        network.send_to_connection(hello_message(state), ticket).await
                    {
                        return rpc_err(id, &format!("peer rejected handshake: {error}"));
                    }
                    {
                        let mut st = state.lock().unwrap();
                        st.trust.grant_if_absent(
                            &peer_id,
                            Capabilities {
                                input: true,
                                clipboard: true,
                                files: true,
                                screen: false,
                            },
                        );
                        st.trust.set_fingerprint(&peer_id, pinned.clone());
                        if let Ok(mut pins) = st.client_pins.write() {
                            pins.insert(pinned, peer_id.clone());
                        };
                        let trust_path = st.config_dir.join("trust.json");
                        let _ = st.trust.save(trust_path);
                    }
                    rpc_ok(id, json!({ "ok": true }))
                }
                Err(error) => rpc_err(id, &error),
            }
        }
        "pairingUri" => {
            let peer_id = params.get("peerId").and_then(Value::as_str).unwrap_or("");
            let fingerprint = params
                .get("fingerprint")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !is_valid_id(peer_id) {
                return rpc_err(id, "invalid joining peer device id");
            }
            if !is_hex_fingerprint(fingerprint) {
                return rpc_err(id, "fingerprint must be 64 hex characters");
            }
            let st = state.lock().unwrap();
            let mut session = pairing::PairSession::new(&st.device_id);
            if !session.bind_peer(peer_id, fingerprint) {
                return rpc_err(id, "could not bind the pairing session to the joining peer");
            }
            let uri = session.pairing_uri();
            let code = session.code().to_string();
            if let Ok(mut sessions) = st.pairing_sessions.lock() {
                sessions.insert(code, session);
            } else {
                return rpc_err(id, "pairing store unavailable");
            }
            drop(st);
            rpc_ok(id, json!(uri));
        }
        "approvePairing" => {
            let code = params.get("code").and_then(Value::as_str).unwrap_or("");
            let peer_id = params.get("peerId").and_then(Value::as_str).unwrap_or("");
            let fingerprint = params
                .get("fingerprint")
                .and_then(Value::as_str)
                .unwrap_or("");
            let verified = params.get("verified").and_then(Value::as_bool).unwrap_or(false);
            if !verified {
                return rpc_err(id, "verify the full fingerprint out of band before approving");
            }
            if !is_valid_id(peer_id) {
                return rpc_err(id, "invalid peer device id");
            }
            if !is_hex_fingerprint(fingerprint) {
                return rpc_err(id, "fingerprint must be 64 hex characters");
            }
            {
                let st = state.lock().unwrap();
                let Ok(mut sessions) = st.pairing_sessions.lock() else {
                    return rpc_err(id, "pairing store unavailable");
                };
                let Some(session) = sessions.get_mut(code) else {
                    return rpc_err(id, "unknown or expired pairing code");
                };
                if !session.matches(code) {
                    return rpc_err(id, "invalid or already used pairing code");
                }
                if session.bound_peer_id() != Some(peer_id)
                    || !session
                        .bound_fingerprint()
                        .is_some_and(|bound| bound.eq_ignore_ascii_case(fingerprint))
                {
                    return rpc_err(id, "peer does not match the pairing session");
                }
                if let Some(pinned) = st.trust.fingerprint(peer_id) {
                    if !pinned.eq_ignore_ascii_case(fingerprint) {
                        return rpc_err(id, "peer fingerprint changed; revoke the old device first");
                    }
                }
                // Keep the approved session until the bound peer presents the
                // code; PairCode consumption is the one-time acknowledgement.
                if !session.approve() {
                    return rpc_err(id, "invalid or already used pairing code");
                }
            }
            let fingerprint = fingerprint.to_ascii_lowercase();
            {
                let mut st = state.lock().unwrap();
                st.trust.grant_if_absent(
                    peer_id,
                    Capabilities {
                        input: true,
                        clipboard: true,
                        files: true,
                        screen: false,
                    },
                );
                st.trust.set_fingerprint(peer_id, fingerprint.clone());
                let trust_path = st.config_dir.join("trust.json");
                let _ = st.trust.save(trust_path);
                if let Ok(mut pins) = st.client_pins.write() {
                    pins.insert(fingerprint, peer_id.to_string());
                };
            }
            rpc_ok(id, json!({ "ok": true }));
        }
        "stopInput" | "emergencyStop" => {
            network.set_input_enabled(false);
            if let Ok(mut bridge) = state.lock().unwrap().input_bridge.lock() {
                // Pause keeps the OS hooks installed but passes all input
                // through. Stopping the hooks here made resume impossible.
                bridge.pause();
            };
            InputBridge::release_modifiers();
            emit(json!({
                "notification": "inputState",
                "enabled": false,
                "message": "remote input stopped",
            }));
            rpc_ok(id, json!({ "ok": true, "enabled": false }));
        }
        "resumeInput" => {
            network.set_input_enabled(true);
            if let Ok(mut bridge) = state.lock().unwrap().input_bridge.lock() {
                bridge.resume();
            };
            emit(json!({
                "notification": "inputState",
                "enabled": true,
                "message": "remote input resumed",
            }));
            rpc_ok(id, json!({ "ok": true, "enabled": true }));
        }
        "revokeDevice" | "revokePairing" => {
            let peer_id = params.get("peerId").and_then(Value::as_str).unwrap_or("");
            if !is_valid_id(peer_id) {
                return rpc_err(id, "invalid peer device id");
            }
            // Capture this before disconnect clears the active peer slot.
            let was_active = {
                let active = network.active_peer_id();
                let current = active.lock().unwrap().clone();
                let matches = current.as_deref() == Some(peer_id);
                matches
            };
            {
                let mut st = state.lock().unwrap();
                st.trust.revoke(peer_id);
                if let Ok(mut pins) = st.client_pins.write() {
                    pins.retain(|_, owner| owner != peer_id);
                };
                let trust_path = st.config_dir.join("trust.json");
                let _ = st.trust.save(trust_path);
            }
            // Revoking a trusted peer must drop its live TLS session too.
            // Otherwise an already-armed handoff could keep forwarding to a
            // peer that is no longer trusted.
            network.disconnect_peer(peer_id).await;
            if was_active {
                network.set_input_enabled(false);
                if let Ok(mut bridge) = state.lock().unwrap().input_bridge.lock() {
                    bridge.pause();
                }
                InputBridge::release_modifiers();
                emit(json!({
                    "notification": "inputState",
                    "enabled": false,
                    "message": "revoked peer disconnected; input returned to this PC",
                }));
            }
            rpc_ok(id, json!({ "ok": true }));
        }
        "setDeviceCapabilities" => {
            let peer_id = params.get("peerId").and_then(Value::as_str).unwrap_or("");
            if !is_valid_id(peer_id) {
                return rpc_err(id, "invalid peer device id");
            }
            let caps = Capabilities {
                input: params.get("input").and_then(Value::as_bool).unwrap_or(false),
                clipboard: params.get("clipboard").and_then(Value::as_bool).unwrap_or(false),
                files: params.get("files").and_then(Value::as_bool).unwrap_or(false),
                screen: params.get("screen").and_then(Value::as_bool).unwrap_or(false),
            };
            let updated = {
                let mut st = state.lock().unwrap();
                let updated = st.trust.set_capabilities(peer_id, caps);
                if updated {
                    let trust_path = st.config_dir.join("trust.json");
                    let _ = st.trust.save(trust_path);
                }
                updated
            };
            if updated {
                rpc_ok(id, json!({ "ok": true }))
            } else {
                rpc_err(id, "unknown device")
            }
        }
        "setLayout" => {
            let layout = {
                let mut st = state.lock().unwrap();
                match apply_layout_params(&mut st, &params) {
                    Ok(layout) => layout,
                    Err(error) => return rpc_err(id, &error),
                }
            };
            match serde_json::to_value(&layout) {
                Ok(value) => emit(json!({ "notification": "layoutChanged", "layout": value })),
                Err(error) => return rpc_err(id, &format!("could not encode layout: {error}")),
            }
            rpc_ok(id, json!({ "ok": true }));
        }
        "getLayout" => {
            let layout = state.lock().unwrap().layout.read().unwrap().clone();
            match serde_json::to_value(&layout) {
                Ok(value) => rpc_ok(id, json!({ "ok": true, "layout": value })),
                Err(error) => rpc_err(id, &format!("could not encode layout: {error}")),
            }
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
            let peer = params
                .get("peerDeviceId")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| network.active_peer_id().lock().unwrap().clone());
            let peer = match peer {
                Some(peer) if !peer.is_empty() => peer,
                _ => return rpc_err(id, "no connected peer"),
            };
            let offer = {
                let mut st = state.lock().unwrap();
                st.transfers
                    .create_offer(std::path::Path::new(path), &peer)
                    .map(|(offer, _)| offer)
            };
            match offer {
                Ok(offer) => {
                    // The record's peer and the frame destination must come
                    // from the same resolved value or the accept-side gate
                    // rejects every UI file transfer.
                    if let Err(error) = network
                        .send_to(Message::FileTransferOffer(offer.clone()), Some(peer.clone()))
                        .await
                    {
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
            shutdown_runtime(network, || {
                InputBridge::release_modifiers();
            });
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
    let usable = st.trust.allows_peer(device_id);
    let certificate_matches = st
        .trust
        .fingerprint(device_id)
        .zip(peer_fingerprint)
        .map(|(pinned, presented)| pinned.eq_ignore_ascii_case(presented))
        .unwrap_or(false);
    (usable && certificate_matches).then(|| device_id.clone())
}

/// Handshake gate for a single production inbound stream. TLS is already
/// pinned by the transport; this reads the peer's first frame, requires a v2
/// Hello, and checks that the named device is trusted, usable and owns the
/// client certificate that was presented. On any failure the stream is dropped
/// here, before it can reach the message dispatcher.
async fn gate_inbound_stream(
    state: &Arc<Mutex<AppState>>,
    stream: &mut transport::PeerStream,
) -> Result<String, String> {
    let presented = stream.peer_fingerprint().map(str::to_string);
    let message = timeout(Duration::from_secs(5), stream.recv())
        .await
        .map_err(|_| "handshake timed out".to_string())?
        .map_err(|error| format!("handshake failed: {error}"))?;
    authorize_inbound_hello(state, &message, presented.as_deref())
        .ok_or_else(|| "untrusted peer".to_string())
}

/// Accept pinned inbound peers without blocking the accept loop on a slow or
/// hostile handshake. Each gate has a five-second deadline.
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
        let gate_state = state.clone();
        let gate_network = network.clone();
        tokio::spawn(async move {
            let peer_id = match gate_inbound_stream(&gate_state, &mut stream).await {
                Ok(peer_id) => peer_id,
                Err(reason) => {
                    emit(json!({ "notification": "inboundRejected", "reason": reason }));
                    return;
                }
            };
            emit(json!({
                "notification": "inboundPeer",
                "deviceId": peer_id,
                "message": "trusted peer attached",
            }));
            if let Err(reason) = gate_network.attach_inbound(stream, peer_id).await {
                emit(json!({
                    "notification": "listener-error",
                    "error": reason,
                }));
            }
        });
    }
}

async fn run_ipc() -> Result<(), String> {
    let config = config_dir();
    let state = Arc::new(Mutex::new(AppState::new(config.clone())?));
    let device_id = state.lock().unwrap().device_id.clone();
    let input_gate = Arc::new(AtomicBool::new(true));
    let network = network::start(
        state.clone(),
        device_id.clone(),
        String::new(),
        true,
        input_gate,
    );

    // Production input capture: local events are forwarded to exactly one
    // peer, while remote events are injected only through the runtime gate.
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Message>();
    let forward_network = network.clone();
    tokio::spawn(async move {
        while let Some(message) = input_rx.recv().await {
            // Handoff frames carry the exact peer chosen by the topology
            // crossing. Route by that ID so a second accepted connection can
            // never receive input stamped for a different device.
            let target = match &message {
                Message::InputEvent { target, .. } => Some(target.clone()),
                _ => None,
            };
            if forward_network.send_to(message, target).await.is_err() {
                break;
            }
        }
    });
    let gate_network = network.clone();
    let active_peer_id = network.active_peer_id();
    let (layout, local_device_id) = {
        let st = state.lock().unwrap();
        (st.layout.clone(), st.device_id.clone())
    };
    let monitor_index = local_monitor_index(&layout.read().unwrap(), &local_device_id);
    let bridge = Arc::new(Mutex::new(InputBridge::with_topology(
        layout,
        &local_device_id,
        monitor_index as u32,
        input_tx,
        Arc::new(move || gate_network.connected_now()),
        Arc::new(move || active_peer_id.lock().unwrap().clone()),
    )));
    // The topology bridge starts armed but with no handoff target. Local input
    // remains on this PC until a real MouseMove crosses a configured edge and
    // the crossing target matches the single connected peer.
    {
        let mut st = state.lock().unwrap();
        st.input_bridge = bridge.clone();
    }
    let stop_network = network.clone();
    input::set_stop_callback(Arc::new(move || {
        stop_network.set_input_enabled(false);
        emit(json!({
            "notification": "inputState",
            "enabled": false,
            "message": "input returned to this PC",
        }));
    }));
    let _hook_task = InputBridge::start(bridge);

    // Production identity is generated once and reused; its fingerprint is the
    // public pairing token. The private key never leaves the config directory.
    let identity = Identity::load_or_create(&config)
        .map_err(|error| format!("could not load TLS identity: {error}"))?;
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
    let server = Server::bind_pinned(
        &format!("0.0.0.0:{port}"),
        identity.cert_der.clone(),
        identity.key_der.clone(),
        pins,
    )
    .await
    .map_err(|error| format!("could not bind pinned TLS listener on port {port}: {error}"))?;
    let listen_port = server.local_addr().port();
    {
        let mut st = state.lock().unwrap();
        st.identity_fingerprint = identity.fingerprint().to_string();
        st.listen_port = listen_port;
    }
    let listener_state = state.clone();
    let listener_network = network.clone();
    tokio::spawn(accept_inbound_loop(server, listener_state, listener_network));

    // Advertising the real listener and fingerprint over mDNS gives the UI a
    // peer to pair with; the fingerprint still requires out-of-band approval.
    let device_name = state.lock().unwrap().device_name.clone();
    if let Ok(mut discovered) =
        discovery::Discovery::new(&device_id, &device_name).start(listen_port, identity.fingerprint())
    {
        let discovery_state = state.clone();
        tokio::spawn(async move {
            while let Some(peer) = discovered.recv().await {
                {
                    let st = discovery_state.lock().unwrap();
                    if let Ok(mut map) = st.discovered.write() {
                        map.insert(peer.id.clone(), peer.clone());
                    };
                }
                emit(json!({
                    "notification": "discoveredPeer",
                    "id": peer.id,
                    "name": peer.name,
                    "address": peer.address,
                    "port": peer.port,
                    "fingerprint": peer.fingerprint,
                }));
            }
        });
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
    shutdown_runtime(&network, || {
        InputBridge::release_modifiers();
    });
    Ok(())
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
        if let Err(error) = run_ipc().await {
            eprintln!("mymouse IPC failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    // Default: run IPC loop (Electron spawns with --ipc stdio; also safe default).
    if let Err(error) = run_ipc().await {
        eprintln!("mymouse IPC failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layout::Node;
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

    #[tokio::test]
    async fn shutdown_helper_releases_input_before_stopping_network() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(Mutex::new(AppState::new(dir.path().to_path_buf()).unwrap()));
        let device_id = state.lock().unwrap().device_id.clone();
        let handle = network::start(
            state,
            device_id,
            String::new(),
            false,
            Arc::new(AtomicBool::new(true)),
        );

        let released = Arc::new(AtomicBool::new(false));
        let marker = released.clone();
        shutdown_runtime(&handle, || {
            marker.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        assert!(
            released.load(std::sync::atomic::Ordering::SeqCst),
            "every shutdown path must run input cleanup before stopping the runtime"
        );
    }

    #[tokio::test]
    async fn production_offer_file_defaults_to_the_active_peer() {
        // Regression: the UI calls offerFile(path) with no peer id. The
        // production handler defaulted the history record to the placeholder
        // "peer" while the frame was routed to the active peer, so the
        // accept-side peer check failed every normal UI file transfer.
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("offer.bin");
        std::fs::write(&source, vec![1u8; 32]).unwrap();

        let state = Arc::new(Mutex::new(AppState::new(dir.path().to_path_buf()).unwrap()));
        let device_id = state.lock().unwrap().device_id.clone();
        let network = network::start(
            state.clone(),
            device_id,
            String::new(),
            false,
            Arc::new(AtomicBool::new(true)),
        );
        *network.active_peer_id().lock().unwrap() = Some("pc-b".to_string());

        handle_request(
            &state,
            &network,
            1,
            &json!({
                "method": "offerFile",
                "params": { "path": source.to_string_lossy() }
            }),
        )
        .await;

        let record = {
            let st = state.lock().unwrap();
            st.transfers
                .history()
                .into_iter()
                .find(|entry| entry.name == "offer.bin")
                .cloned()
                .expect("offer must be recorded in history")
        };
        assert_eq!(
            record.peer_device_id, "pc-b",
            "a UI offer without a peer id must target the connected peer, not a placeholder"
        );

        shutdown_runtime(&network, || {});
    }

    #[tokio::test]
    async fn production_offer_file_sends_to_recorded_peer() {
        // Regression: offerFile recorded the active peer ID but sent the frame
        // through the legacy outbound-first path, so accept-side ownership
        // checks rejected every UI file transfer when both peers were attached.
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("offer.bin");
        std::fs::write(&source, vec![1u8; 32]).unwrap();

        let mut app = AppState::new(dir.path().to_path_buf()).unwrap();
        app.trust.grant("pc-c", Capabilities::default());
        let state = Arc::new(Mutex::new(app));
        let device_id = state.lock().unwrap().device_id.clone();
        let network = network::start(
            state.clone(),
            device_id,
            String::new(),
            false,
            Arc::new(AtomicBool::new(true)),
        );

        let (out_client, out_server) = tokio::io::duplex(4096);
        network
            .attach_outbound(
                crate::transport::PeerStream::from_test_duplex(out_client),
                "pc-b".into(),
            )
            .await
            .unwrap();
        let (in_client, in_server) = tokio::io::duplex(4096);
        network
            .attach_inbound(
                crate::transport::PeerStream::from_test_duplex(in_client),
                "pc-c".into(),
            )
            .await
            .unwrap();
        *network.active_peer_id().lock().unwrap() = Some("pc-c".into());

        handle_request(
            &state,
            &network,
            1,
            &json!({
                "method": "offerFile",
                "params": { "path": source.to_string_lossy() }
            }),
        )
        .await;

        let expected_id = {
            let st = state.lock().unwrap();
            st.transfers
                .history()
                .iter()
                .find(|entry| entry.name == "offer.bin")
                .unwrap()
                .transfer_id
                .clone()
        };
        let mut active_peer = crate::transport::PeerStream::from_test_duplex(in_server);
        let offer = tokio::time::timeout(std::time::Duration::from_millis(500), async {
            loop {
                match active_peer.recv().await {
                    Ok(Message::FileTransferOffer(offer)) => break offer,
                    // Heartbeats may precede the offer; they are not the frame
                    // under test.
                    Ok(_) => continue,
                    Err(error) => panic!("the active peer's frame must decode: {error}"),
                }
            }
        })
        .await
        .expect("the offer must reach the recorded active peer");
        assert_eq!(offer.transfer_id, expected_id);

        let mut other_peer = crate::transport::PeerStream::from_test_duplex(out_server);
        let leaked = tokio::time::timeout(std::time::Duration::from_millis(100), async {
            loop {
                match other_peer.recv().await {
                    Ok(Message::FileTransferOffer(offer)) => break Some(offer),
                    Ok(_) => continue,
                    Err(_) => break None,
                }
            }
        })
        .await;
        assert!(
            !matches!(leaked, Ok(Some(_))),
            "the offer must not leak to the outbound peer that was not selected"
        );

        shutdown_runtime(&network, || {});
    }

    #[test]
    fn production_device_id_is_unique_and_persisted() {
        // Regression: every install used the shared ID "local-pc", so mDNS
        // self-filtered peers and trust/pinning collapsed onto one identity.
        let dir = tempfile::tempdir().unwrap();
        let first = AppState::new(dir.path().to_path_buf()).unwrap();
        assert!(is_valid_id(&first.device_id));
        assert_ne!(first.device_id, "local-pc");
        let saved = std::fs::read_to_string(dir.path().join("device.json")).unwrap();
        assert!(saved.contains(&first.device_id));
        let second = AppState::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(second.device_id, first.device_id);
    }

    #[test]
    fn app_state_loads_persisted_layout_and_rejects_malformed_layout() {
        let dir = tempfile::tempdir().unwrap();
        let missing = AppState::new(dir.path().to_path_buf()).unwrap();
        assert!(missing.layout.read().unwrap().nodes().is_empty());

        let raw = r#"{
            "nodes": [
                { "deviceId": "pc-a", "monitorIndex": 0, "width": 101, "height": 201 },
                { "deviceId": "pc-b", "monitorIndex": 0, "width": 101, "height": 201 }
            ],
            "edges": [{ "from": "pc-a", "to": "pc-b", "side": "right" }]
        }"#;
        std::fs::write(dir.path().join("layout.json"), raw).unwrap();
        let loaded = AppState::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(loaded.layout.read().unwrap().nodes().len(), 2);
        assert_eq!(loaded.layout.read().unwrap().edges().len(), 1);

        std::fs::write(dir.path().join("layout.json"), "not-json").unwrap();
        assert!(AppState::new(dir.path().to_path_buf()).is_err());
    }

    #[test]
    fn set_layout_params_persist_and_apply_only_when_valid() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new(dir.path().to_path_buf()).unwrap();
        let params = json!({
            "layout": {
                "nodes": [
                    { "deviceId": "pc-a", "monitorIndex": 0, "width": 101, "height": 201 },
                    { "deviceId": "pc-b", "monitorIndex": 0, "width": 101, "height": 201 }
                ],
                "edges": [{ "from": "pc-a", "to": "pc-b", "side": "right" }]
            }
        });

        let applied = apply_layout_params(&mut state, &params).unwrap();
        let disk = std::fs::read_to_string(dir.path().join("layout.json")).unwrap();
        assert!(disk.contains("\"deviceId\""), "{disk}");
        assert_eq!(state.layout.read().unwrap().clone(), applied);
        assert_eq!(applied.edges().len(), 1);

        let rejected = json!({
            "layout": {
                "nodes": [
                    { "deviceId": "pc-a", "monitorIndex": 0, "width": 101, "height": 201 }
                ],
                "edges": [{ "from": "pc-a", "to": "pc-a", "side": "right" }]
            }
        });
        assert!(apply_layout_params(&mut state, &rejected).is_err());
        assert_eq!(state.layout.read().unwrap().clone(), applied);
        let after_reject = std::fs::read_to_string(dir.path().join("layout.json")).unwrap();
        assert_eq!(after_reject, disk);
    }

    #[test]
    fn local_monitor_index_uses_persisted_local_node() {
        let mut layout = Layout::new();
        layout.add_node(Node::monitor("pc-b", 0, 101, 201));
        layout.add_node(Node::monitor("pc-a", 2, 101, 201));

        assert_eq!(
            local_monitor_index(&layout, "pc-a"),
            2,
            "the local monitor index must come from the local node, not the peer or a hardcoded 0"
        );
    }

    #[test]
    fn local_monitor_index_uses_lowest_local_node_and_falls_back_to_zero() {
        let mut layout = Layout::new();
        layout.add_node(Node::monitor("pc-a", 3, 101, 201));
        layout.add_node(Node::monitor("pc-a", 1, 101, 201));
        assert_eq!(local_monitor_index(&layout, "pc-a"), 1);
        assert_eq!(
            local_monitor_index(&layout, "missing"),
            0,
            "an unconfigured local device must keep the validated legacy default"
        );
    }

    #[test]
    fn inbound_hello_requires_trust_and_supported_version() {
        let mut state =
            AppState::with_device_id(std::env::temp_dir(), "local-pc".to_string()).unwrap();
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

    /// Production-style end-to-end gate check with real generated identities:
    /// a pinned client whose Hello names a trusted device is admitted, while
    /// an unknown device ID or clipboard data before Hello is rejected.
    #[tokio::test]
    async fn production_gate_accepts_only_pinned_trusted_hello() {
        use crate::transport::Client;

        let server_dir = tempfile::tempdir().unwrap();
        let client_dir = tempfile::tempdir().unwrap();
        let state_dir = tempfile::tempdir().unwrap();
        let server_identity = Identity::load_or_create(server_dir.path()).unwrap();
        let client_identity = Identity::load_or_create(client_dir.path()).unwrap();
        let client_fp = client_identity.fingerprint().to_string();
        let server_fp = server_identity.fingerprint().to_string();

        let pins: ClientPinMap = Arc::new(RwLock::new(HashMap::from([(
            client_fp.clone(),
            "pc-b".to_string(),
        )])));
        let server = Server::bind_pinned(
            "127.0.0.1:0",
            server_identity.cert_der.clone(),
            server_identity.key_der.clone(),
            pins,
        )
        .await
        .unwrap();

        let mut app =
            AppState::with_device_id(state_dir.path().to_path_buf(), "local-pc".into()).unwrap();
        app.trust.grant("pc-b", Capabilities::default());
        app.trust.set_fingerprint("pc-b", client_fp.clone());
        let state = Arc::new(Mutex::new(app));

        let mut trusted = Client::connect_with_identity(
            server.local_addr(),
            &[server_fp.clone()],
            Some((
                client_identity.cert_der.clone(),
                client_identity.key_der.clone(),
            )),
        )
        .await
        .unwrap();
        trusted.send(&hello(PROTOCOL_VERSION, "pc-b")).await.unwrap();
        let mut accepted = server.accept().await.unwrap();
        assert_eq!(
            gate_inbound_stream(&state, &mut accepted).await,
            Ok("pc-b".to_string())
        );

        let mut untrusted = Client::connect_with_identity(
            server.local_addr(),
            &[server_fp.clone()],
            Some((
                client_identity.cert_der.clone(),
                client_identity.key_der.clone(),
            )),
        )
        .await
        .unwrap();
        untrusted
            .send(&hello(PROTOCOL_VERSION, "stranger"))
            .await
            .unwrap();
        let mut rejected = server.accept().await.unwrap();
        assert!(gate_inbound_stream(&state, &mut rejected).await.is_err());

        let mut early_data = Client::connect_with_identity(
            server.local_addr(),
            &[server_fp],
            Some((
                client_identity.cert_der.clone(),
                client_identity.key_der.clone(),
            )),
        )
        .await
        .unwrap();
        early_data
            .send(&Message::ClipboardUpdate(mymouse_protocol::ClipboardPayload {
                device_id: "pc-b".to_string(),
                kind: mymouse_protocol::ClipboardKind::Text,
                text: Some("early clipboard data".to_string()),
                data_base64: None,
                mime: None,
                timestamp_ms: 1,
                seq: 1,
            }))
            .await
            .unwrap();
        let mut rejected_data = server.accept().await.unwrap();
        assert!(gate_inbound_stream(&state, &mut rejected_data).await.is_err());
    }

    #[tokio::test]
    async fn approve_pairing_rejects_peer_id_outside_session() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(Mutex::new(AppState::new(dir.path().to_path_buf()).unwrap()));
        let bound_fingerprint = "a".repeat(64);
        let other_fingerprint = "b".repeat(64);
        let code = {
            let st = state.lock().unwrap();
            let mut session = pairing::PairSession::new(&st.device_id);
            let _ = session.bind_peer("pc-b", &bound_fingerprint);
            let code = session.code().to_string();
            st.pairing_sessions.lock().unwrap().insert(code.clone(), session);
            code
        };
        let device_id = state.lock().unwrap().device_id.clone();
        let network = network::start(
            state.clone(),
            device_id,
            String::new(),
            false,
            Arc::new(AtomicBool::new(true)),
        );

        handle_request(
            &state,
            &network,
            17,
            &json!({
                "method": "approvePairing",
                "params": {
                    "code": code,
                    "peerId": "pc-c",
                    "fingerprint": other_fingerprint,
                    "verified": true,
                }
            }),
        )
        .await;

        {
            let st = state.lock().unwrap();
            assert!(
                !st.trust.is_trusted("pc-c"),
                "a peer outside the session must not gain trust"
            );
            let sessions = st.pairing_sessions.lock().unwrap();
            let session = sessions
                .get(&code)
                .expect("a rejected approval must not consume the session");
            assert!(!session.approved(), "a mismatched peer must not approve the session");
        }
        shutdown_runtime(&network, || {});
    }

    #[tokio::test]
    async fn approve_pairing_accepts_the_bound_peer() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(Mutex::new(AppState::new(dir.path().to_path_buf()).unwrap()));
        let fingerprint = "A".repeat(64);
        let code = {
            let st = state.lock().unwrap();
            let mut session = pairing::PairSession::new(&st.device_id);
            let _ = session.bind_peer("pc-b", &fingerprint);
            let code = session.code().to_string();
            st.pairing_sessions.lock().unwrap().insert(code.clone(), session);
            code
        };
        let device_id = state.lock().unwrap().device_id.clone();
        let network = network::start(
            state.clone(),
            device_id,
            String::new(),
            false,
            Arc::new(AtomicBool::new(true)),
        );

        handle_request(
            &state,
            &network,
            18,
            &json!({
                "method": "approvePairing",
                "params": {
                    "code": code,
                    "peerId": "pc-b",
                    "fingerprint": fingerprint,
                    "verified": true,
                }
            }),
        )
        .await;

        {
            let st = state.lock().unwrap();
            assert!(st.trust.is_trusted("pc-b"), "the bound peer must gain trust");
            assert_eq!(
                st.trust.fingerprint("pc-b"),
                Some(fingerprint.to_ascii_lowercase().as_str()),
                "the verified fingerprint must be pinned"
            );
            let sessions = st.pairing_sessions.lock().unwrap();
            assert!(
                sessions.get(&code).is_some_and(|session| session.approved()),
                "the approved session must remain available for the PairCode acknowledgement"
            );
        }
        shutdown_runtime(&network, || {});
    }
}
