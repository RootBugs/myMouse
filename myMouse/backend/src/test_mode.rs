//! Deterministic two-instance test mode.
//!
//! Production always uses mDNS discovery, explicit pairing and per-pair pinned
//! certificates. Test mode replaces discovery with a static peer list, uses a
//! single fixed test certificate for both instances, and replaces OS input
//! hooks with synthetic crossing commands driven over stdio JSON lines.
//! Clipboard and file messages travel through the same peer runtime and the
//! same pinned-TLS transport; OS clipboard writes and key injection stay off.

use crate::input::{CapturedEvent, InputBridge};
use crate::layout::{Edge, Layout, Node, Side};
use crate::network::{start as start_network, PeerRuntimeHandle};
use crate::transport::{Client, Server};
use crate::{
    apply_clipboard_settings, apply_file_transfer_settings, clipboard_message_from_params, AppState,
};
use crate::trust::{is_valid_id, Capabilities};
use mymouse_protocol::{Message, PROTOCOL_VERSION};
use serde_json::{json, Value};
use std::error::Error;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

// TEST MODE ONLY. Shared certificate/pinning material lets two local test
// instances complete a pinned TLS handshake without an interactive pairing
// step. Never used outside `--test-mode`.
const TEST_CERT_HEX: &str = "3082016230820108a00302010202141c687cd2239b7f510e15c3e2a7c8b97ae68824b1300a06082a8648ce3d0403023021311f301d06035504030c16726367656e2073656c66207369676e656420636572743020170d3735303130313030303030305a180f34303936303130313030303030305a3021311f301d06035504030c16726367656e2073656c66207369676e656420636572743059301306072a8648ce3d020106082a8648ce3d030107034200042a86d1797c8c6d3327f8899bc472bcbe9ea917146482366bf054968bd71e0b360c4b36199e3e5c9bef60594e6a99b80cd2f6c04cc3d258258cc30ee9e501c418a31c301a30180603551d110411300f820d6d796d6f7573652e6c6f63616c300a06082a8648ce3d0403020348003045022100842777d4726d47597eaed3217c57443aacf730cc16b574059ad1ec9d06ef4093022002e8bf8328dd2664d6d0e6164c9100d7333079f848a7c5e30615f0dc25d5918c";
const TEST_KEY_HEX: &str = "308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b020101042022faefb9d31181e7ce624952c422f3f8b504ee0beba4945112c80a4fa7206453a144034200042a86d1797c8c6d3327f8899bc472bcbe9ea917146482366bf054968bd71e0b360c4b36199e3e5c9bef60594e6a99b80cd2f6c04cc3d258258cc30ee9e501c418";
const TEST_FINGERPRINT: &str = "877469d7b10f9bf5c552e68494d3402dc3f116c41f8788eb9633e6de02120e7d";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestConfig {
    pub device_id: String,
    pub peer_id: String,
    pub port: u16,
    pub peer: Option<SocketAddr>,
    /// Optional startup crossing for the script-driven smoke test: (target, x, y).
    pub auto_cross: Option<(String, u32, u32)>,
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

pub fn parse_args(args: &[String]) -> Option<TestConfig> {
    if !args.iter().any(|arg| arg == "--test-mode") {
        return None;
    }
    let device_id = value_after(args, "--device-id").unwrap_or_else(|| "pc-a".into());
    let peer_id = value_after(args, "--peer-id").unwrap_or_else(|| "pc-b".into());
    let port = value_after(args, "--port")?
        .parse::<u16>()
        .expect("--port must be a u16");
    let peer = value_after(args, "--peer")
        .map(|address| address.parse::<SocketAddr>().expect("--peer must be host:port"));
    let auto_cross = value_after(args, "--cross-to").map(|target| {
        let x = value_after(args, "--x")
            .map(|value| value.parse::<u32>().expect("--x must be u32"))
            .unwrap_or(100);
        let y = value_after(args, "--y")
            .map(|value| value.parse::<u32>().expect("--y must be u32"))
            .unwrap_or(200);
        (target, x, y)
    });
    Some(TestConfig {
        device_id,
        peer_id,
        port,
        peer,
        auto_cross,
    })
}

/// Test mode must never read or write the production config root. An explicit
/// harness override is honoured; otherwise state is kept in the OS temp tree,
/// keyed by the synthetic device ID so two test instances cannot collide.
pub(crate) fn test_config_dir(device_id: &str) -> PathBuf {
    if let Ok(dir) = std::env::var("MYMOUSE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let name = if is_valid_id(device_id) {
        device_id
    } else {
        "test"
    };
    std::env::temp_dir().join("myMouse-test").join(name)
}

fn hex_decode(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).expect("valid test hex"))
        .collect()
}

fn emit(value: Value) {
    println!("{value}");
    let _ = io::stdout().flush();
}

fn rpc_response(id: u64, result: Value) {
    emit(json!({ "id": id, "result": result }));
}

fn rpc_err(id: u64, message: &str) {
    rpc_response(id, json!({ "ok": false, "error": message }));
}

fn build_layout(device_id: &str, peer_id: &str) -> Layout {
    // Small synthetic screens make (100, 200) sit on the configured right edge.
    let mut layout = Layout::new();
    layout.add_node(Node::monitor(device_id, 0, 101, 201));
    layout.add_node(Node::monitor(peer_id, 0, 101, 201));
    layout.set_edge(Edge {
        from: device_id.to_string(),
        to: peer_id.to_string(),
        side: Side::Right,
    });
    layout
}

/// Drive one synthetic cursor crossing through the real layout + input bridge
/// and forward the resulting `InputEvent` through the peer runtime. Shared by
/// the stdio RPC handler and the startup `--cross-to` flag.
async fn send_crossing(
    layout: &Layout,
    device_id: &str,
    network: &PeerRuntimeHandle,
    target: &str,
    x: u32,
    y: u32,
) -> Result<Value, String> {
    let crossing = layout
        .cross(device_id, x, y)
        .ok_or_else(|| "no configured edge at position".to_string())?;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut bridge = InputBridge::with_sender(target, tx);
    bridge.forwarding = true;
    bridge.handle(CapturedEvent::MouseMove {
        x: crossing.x as i32,
        y: crossing.y as i32,
    });
    let Some(Message::InputEvent { target, payload }) = rx.recv().await else {
        return Err("input bridge did not emit event".into());
    };
    network
        .send(Message::InputEvent { target, payload })
        .await
        .map_err(|error| error.to_string())?;
    emit(json!({
        "notification": "forwarded",
        "deviceId": device_id,
        "target": crossing.target
    }));
    Ok(json!({
        "ok": true,
        "target": crossing.target,
        "x": crossing.x,
        "y": crossing.y
    }))
}

async fn send_messages(network: &PeerRuntimeHandle, messages: Vec<Message>) -> Result<usize, String> {
    let count = messages.len();
    for message in messages {
        network.send(message).await?;
    }
    Ok(count)
}

pub async fn run(config: TestConfig) -> Result<(), Box<dyn Error + Send + Sync>> {
    let device_id = config.device_id.clone();
    let peer_id = config.peer_id.clone();
    let state = Arc::new(Mutex::new(AppState::with_device_id(
        test_config_dir(&device_id),
        device_id.clone(),
    )?));
    // Test mode has no interactive pairing; grant the statically configured
    // peer full capabilities so the same dispatch gates can be exercised.
    {
        let mut st = state.lock().unwrap();
        st.trust.grant(&peer_id, Capabilities::default());
        let _ = std::fs::create_dir_all(&st.config_dir);
        let trust_file = st.config_dir.join("trust.json");
        let _ = st.trust.save(trust_file);
    }
    let network = start_network(
        state.clone(),
        device_id.clone(),
        peer_id.clone(),
        false,
        Arc::new(AtomicBool::new(true)),
    );

    let server = Server::bind(
        &format!("127.0.0.1:{}", config.port),
        hex_decode(TEST_CERT_HEX),
        hex_decode(TEST_KEY_HEX),
    )
    .await?;
    emit(json!({
        "notification": "ready",
        "deviceId": device_id,
        "port": config.port
    }));

    let accept_network = network.clone();
    let accept_state = state.clone();
    tokio::spawn(async move {
        loop {
            match server.accept().await {
                Ok(mut stream) => {
                    emit(json!({ "notification": "peer-stream" }));
                    // Test mode uses a fixed test certificate, so it does not
                    // repeat the production trust gate. It still reads the
                    // Hello, however, so the runtime labels the connection
                    // with the peer's real announced ID instead of the local
                    // static configuration.
                    let hello = tokio::time::timeout(Duration::from_secs(5), stream.recv())
                        .await
                        .ok()
                        .and_then(|result| result.ok());
                    let Some(Message::Hello {
                        version: PROTOCOL_VERSION,
                        device_id: peer_id,
                        ..
                    }) = hello
                    else {
                        emit(json!({
                            "notification": "inboundRejected",
                            "reason": "missing test Hello"
                        }));
                        continue;
                    };
                    {
                        let mut st = accept_state.lock().unwrap();
                        st.trust.grant(&peer_id, Capabilities::default());
                    }
                    if accept_network.attach_inbound(stream, peer_id).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    if let Some(address) = config.peer {
        let mut last_error = String::from("no attempt made");
        let mut connected = false;
        for _ in 0..100 {
            match Client::connect(address, &[TEST_FINGERPRINT.to_string()]).await {
                Ok(stream) => {
                    match network
                        .attach_outbound(stream, peer_id.clone())
                        .await
                    {
                        Ok(()) => {
                            // Mirror the production connect handshake so the
                            // listener can bind this connection to our real
                            // device ID.
                            if let Err(error) = network
                                .send(Message::Hello {
                                    version: PROTOCOL_VERSION,
                                    device_id: device_id.clone(),
                                    name: device_id.clone(),
                                })
                                .await
                            {
                                last_error = error;
                                continue;
                            }
                            connected = true;
                            break;
                        }
                        Err(error) => last_error = error,
                    }
                }
                Err(error) => {
                    last_error = error.to_string();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
        if !connected {
            return Err(format!(
                "test peer {address} never accepted a pinned TLS connection: {last_error}"
            )
            .into());
        }
        emit(json!({
            "notification": "connected",
            "deviceId": device_id,
            "peer": address.to_string()
        }));
    }

    let layout = build_layout(&device_id, &peer_id);
    if let Some((target, x, y)) = config.auto_cross.clone() {
        match send_crossing(&layout, &device_id, &network, &target, x, y).await {
            Ok(result) => emit(json!({ "notification": "auto-cross", "result": result })),
            Err(error) => {
                emit(json!({ "notification": "auto-cross-error", "error": error }));
                return Err(error.into());
            }
        }
    }

    let (rpc_tx, mut rpc_rx) = mpsc::unbounded_channel::<(u64, Value)>();
    let stdin = tokio::io::stdin();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdin).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                let id = value.get("id").and_then(Value::as_u64).unwrap_or(0);
                let _ = rpc_tx.send((id, value));
            }
        }
    });

    // Keep serving after stdin EOF: script-driven instances have no RPC input,
    // but must stay alive to accept peers and emit delivery notifications.
    'rpc: loop {
        let Some((id, request)) = rpc_rx.recv().await else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue 'rpc;
        };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request
            .get("params")
            .cloned()
            .unwrap_or_else(|| json!({}));
        match method {
            "status" => {
                let connected = network.connected().await;
                rpc_response(
                    id,
                    json!({
                        "ok": true,
                        "connected": connected,
                        "deviceId": device_id,
                    }),
                );
            }
            "crossTo" => {
                let target = params
                    .get("target")
                    .and_then(Value::as_str)
                    .unwrap_or(&peer_id)
                    .to_string();
                let x = params
                    .get("x")
                    .and_then(Value::as_u64)
                    .unwrap_or(100) as u32;
                let y = params
                    .get("y")
                    .and_then(Value::as_u64)
                    .unwrap_or(200) as u32;

                match send_crossing(&layout, &device_id, &network, &target, x, y).await {
                    Ok(result) => rpc_response(id, result),
                    Err(error) => rpc_response(id, json!({ "ok": false, "error": error })),
                }
            }
            "setClipboardSettings" => {
                let mut st = state.lock().unwrap();
                apply_clipboard_settings(&mut st, &params);
                rpc_response(
                    id,
                    json!({ "ok": true, "settings": st.settings.clipboard }),
                );
            }
            "setFileTransferSettings" => {
                let mut st = state.lock().unwrap();
                apply_file_transfer_settings(&mut st, &params);
                rpc_response(
                    id,
                    json!({ "ok": true, "settings": st.settings.file_transfer }),
                );
            }
            "listTransfers" => {
                let st = state.lock().unwrap();
                rpc_response(id, json!({ "ok": true, "transfers": st.transfers.history() }));
            }
            "acceptTransfer" => {
                let transfer_id = params
                    .get("transferId")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let messages = {
                    let mut st = state.lock().unwrap();
                    st.transfers.accept(transfer_id)
                };
                match messages {
                    Ok(messages) => match send_messages(&network, messages).await {
                        Ok(count) => rpc_response(id, json!({ "ok": true, "messages": count })),
                        Err(error) => rpc_err(id, &error),
                    },
                    Err(error) => rpc_err(id, &error.to_string()),
                }
            }
            "rejectTransfer" => {
                let transfer_id = params
                    .get("transferId")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let reason = params
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let messages = {
                    let mut st = state.lock().unwrap();
                    st.transfers.reject(transfer_id, reason)
                };
                match send_messages(&network, messages).await {
                    Ok(_) => rpc_response(id, json!({ "ok": true })),
                    Err(error) => rpc_err(id, &error),
                }
            }
            "offerFile" => {
                let path = params.get("path").and_then(Value::as_str).unwrap_or("");
                let peer = params
                    .get("peerDeviceId")
                    .and_then(Value::as_str)
                    .unwrap_or(&peer_id);
                let offer = {
                    let mut st = state.lock().unwrap();
                    st.transfers
                        .create_offer(std::path::Path::new(path), peer)
                        .map(|(offer, _)| offer)
                };
                match offer {
                    Ok(offer) => {
                        match network
                            .send(Message::FileTransferOffer(offer.clone()))
                            .await
                        {
                            Ok(()) => rpc_response(id, json!({ "ok": true, "offer": offer })),
                            Err(error) => rpc_err(id, &error),
                        }
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
                        Ok(()) => rpc_response(id, json!({ "ok": true })),
                        Err(error) => rpc_err(id, &error),
                    },
                    None => rpc_err(id, "clipboard disabled, duplicate or oversized"),
                }
            }
            "quit" => {
                network.shutdown();
                rpc_response(id, json!({ "ok": true }));
                break 'rpc;
            }
            _ => rpc_response(id, json!({ "ok": false, "error": "unknown method" })),
        }
    }
    Ok(())
}
