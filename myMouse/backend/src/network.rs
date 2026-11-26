//! Peer session runtime.
//!
//! Owns the outbound pinned-TLS connection and every accepted inbound
//! connection, dispatches protocol messages to the existing clipboard/file
//! managers, and gives synchronous/async RPC handlers a safe channel-based
//! way to send outbound messages.

use crate::clipboard;
use crate::file_transfer::TransferRecord;
use crate::identity::Identity;
use crate::input::{CapturedEvent, InputBridge};
use crate::transport::{Client, PeerStream};
use crate::trust::Capabilities;
use crate::{emit, AppState};
use mymouse_protocol::{ClipboardKind, Message};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const MAX_INPUT_EVENTS_PER_SECOND: usize = 250;

/// Capability flags are enforced for every inbound message class, not just
/// during the Hello handshake. Control messages remain allowed so a peer can
/// accept/reject an offer it did not originate.
fn message_allowed_by_capabilities(capabilities: Option<&Capabilities>, message: &Message) -> bool {
    let Some(capabilities) = capabilities else {
        return !matches!(
            message,
            Message::InputEvent { .. }
                | Message::ClipboardUpdate(_)
                | Message::FileTransferOffer(_)
                | Message::FileTransferChunk(_)
                | Message::FileTransferComplete { .. }
        );
    };
    match message {
        Message::InputEvent { .. } => capabilities.input,
        Message::ClipboardUpdate(_) => capabilities.clipboard,
        Message::FileTransferOffer(_)
        | Message::FileTransferChunk(_)
        | Message::FileTransferComplete { .. } => capabilities.files,
        _ => true,
    }
}

/// Final gate before an event reaches the OS. `enabled` is the user-controlled
/// emergency stop; `target` must be this machine; hotkeys are local-only.
fn can_inject(
    capabilities: Option<&Capabilities>,
    target: &str,
    local_device_id: &str,
    enabled: bool,
    event: &CapturedEvent,
) -> bool {
    enabled
        && target == local_device_id
        && !matches!(event, CapturedEvent::Hotkey(_))
        && capabilities.map(|caps| caps.input).unwrap_or(false)
}

/// Decode an input payload with a byte budget equal to the frame size. A
/// hostile length prefix can never make bincode reserve gigabytes.
fn decode_captured_event(payload: &[u8]) -> Option<CapturedEvent> {
    use bincode::Options;
    bincode::options()
        .with_fixint_encoding()
        .with_limit(payload.len() as u64)
        .deserialize_from(std::io::Cursor::new(payload))
        .ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PeerKey {
    Outbound,
    Accepted(u64),
}

struct OutgoingFrame {
    message: Message,
    ack: Option<oneshot::Sender<Result<(), String>>>,
}

struct InboundFrame {
    key: PeerKey,
    generation: u64,
    peer_device_id: String,
    message: Message,
}

struct Connection {
    tx: mpsc::UnboundedSender<OutgoingFrame>,
    tasks: Vec<JoinHandle<()>>,
    generation: u64,
}

enum RuntimeCommand {
    Send {
        message: Message,
        ack: oneshot::Sender<Result<(), String>>,
    },
    AttachOutbound {
        stream: PeerStream,
        peer_device_id: String,
        ack: oneshot::Sender<()>,
    },
    AttachInbound {
        stream: PeerStream,
        peer_device_id: String,
        ack: oneshot::Sender<()>,
    },
    ConnectionClosed { key: PeerKey, generation: u64 },
    Status {
        ack: oneshot::Sender<bool>,
    },
    Shutdown,
}

/// Cloneable handle used by RPC handlers and test-mode crossing code.
#[derive(Clone)]
pub struct PeerRuntimeHandle {
    commands: mpsc::UnboundedSender<RuntimeCommand>,
    /// Cloneable slot for this install's TLS identity, used by mutual-TLS
    /// outbound connections. Test mode never sets it.
    identity: Arc<Mutex<Option<Arc<Identity>>>>,
    /// Emergency stop shared with the RPC layer. Injection is blocked while
    /// false even if a trusted peer is still attached.
    input_gate: Arc<AtomicBool>,
}

impl PeerRuntimeHandle {
    /// Install this install's TLS identity for outbound mutual-TLS handshakes.
    pub fn set_identity(&self, identity: Arc<Identity>) {
        *self.identity.lock().unwrap() = Some(identity);
    }

    /// Emergency stop: blocks every inbound input event without dropping the
    /// TLS connection, so clipboard/file transfers stay usable.
    pub fn set_input_enabled(&self, enabled: bool) {
        self.input_gate.store(enabled, Ordering::SeqCst);
    }

    pub fn input_enabled(&self) -> bool {
        self.input_gate.load(Ordering::SeqCst)
    }

    /// Queue a message for the connected peer. The ack resolves after the
    /// framed write completes, so callers keep the same error semantics as a
    /// direct `PeerStream::send`.
    pub async fn send(&self, message: Message) -> Result<(), String> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.commands
            .send(RuntimeCommand::Send {
                message,
                ack: ack_tx,
            })
            .map_err(|_| "network runtime stopped".to_string())?;
        ack_rx
            .await
            .map_err(|_| "network runtime stopped".to_string())?
    }

    pub async fn connected(&self) -> bool {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self.commands.send(RuntimeCommand::Status { ack: ack_tx }).is_err() {
            return false;
        }
        ack_rx.await.unwrap_or(false)
    }

    pub async fn attach_outbound(
        &self,
        stream: PeerStream,
        peer_device_id: String,
    ) -> Result<(), String> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.commands
            .send(RuntimeCommand::AttachOutbound {
                stream,
                peer_device_id,
                ack: ack_tx,
            })
            .map_err(|_| "network runtime stopped".to_string())?;
        ack_rx
            .await
            .map_err(|_| "network runtime stopped".to_string())
    }

    pub async fn attach_inbound(
        &self,
        stream: PeerStream,
        peer_device_id: String,
    ) -> Result<(), String> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.commands
            .send(RuntimeCommand::AttachInbound {
                stream,
                peer_device_id,
                ack: ack_tx,
            })
            .map_err(|_| "network runtime stopped".to_string())?;
        ack_rx
            .await
            .map_err(|_| "network runtime stopped".to_string())
    }

    /// Connect with an explicit certificate fingerprint. The transport's
    /// pinned verifier rejects any peer whose certificate does not match.
    pub async fn connect(
        &self,
        address: SocketAddr,
        fingerprint: String,
        peer_device_id: String,
    ) -> Result<(), String> {
        let identity = self
            .identity
            .lock()
            .unwrap()
            .as_ref()
            .map(|id| (id.cert_der.clone(), id.key_der.clone()));
        let stream = Client::connect_with_identity(address, &[fingerprint], identity)
            .await
            .map_err(|error| error.to_string())?;
        self.attach_outbound(stream, peer_device_id).await
    }

    pub fn shutdown(&self) {
        let _ = self.commands.send(RuntimeCommand::Shutdown);
    }
}

struct PeerRuntime {
    state: Arc<Mutex<AppState>>,
    local_device_id: String,
    peer_device_id: String,
    /// Test mode keeps OS input hooks and the OS clipboard untouched.
    os_integration: bool,
    commands: mpsc::UnboundedReceiver<RuntimeCommand>,
    command_sender: mpsc::UnboundedSender<RuntimeCommand>,
    inbound_rx: mpsc::UnboundedReceiver<InboundFrame>,
    inbound_tx: mpsc::UnboundedSender<InboundFrame>,
    connections: HashMap<PeerKey, Connection>,
    next_accepted: u64,
    next_generation: u64,
    active_outbound_generation: u64,
    input_gate: Arc<AtomicBool>,
    last_input_window: Instant,
    input_events_in_window: usize,
}

impl PeerRuntime {
    fn start(
        state: Arc<Mutex<AppState>>,
        local_device_id: String,
        peer_device_id: String,
        os_integration: bool,
        input_gate: Arc<AtomicBool>,
    ) -> PeerRuntimeHandle {
        let (command_sender, commands) = mpsc::unbounded_channel();
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let handle = PeerRuntimeHandle {
            commands: command_sender.clone(),
            identity: Arc::new(Mutex::new(None)),
            input_gate: input_gate.clone(),
        };
        tokio::spawn(async move {
            let mut runtime = Self {
                state,
                local_device_id,
                peer_device_id,
                os_integration,
                commands,
                command_sender,
                inbound_rx,
                inbound_tx,
                connections: HashMap::new(),
                next_accepted: 0,
                next_generation: 0,
                active_outbound_generation: 0,
                input_gate,
                last_input_window: Instant::now(),
                input_events_in_window: 0,
            };
            runtime.run().await;
        });
        handle
    }

    async fn run(&mut self) {
        loop {
            tokio::select! {
                command = self.commands.recv() => {
                    match command {
                        Some(RuntimeCommand::Shutdown) | None => break,
                        Some(command) => self.handle_command(command),
                    }
                }
                inbound = self.inbound_rx.recv() => {
                    if let Some(frame) = inbound {
                        self.dispatch(frame);
                    }
                }
            }
        }
        self.connections.clear();
    }

    fn handle_command(&mut self, command: RuntimeCommand) {
        match command {
            RuntimeCommand::Send { message, ack } => {
                let target = if self.connections.contains_key(&PeerKey::Outbound) {
                    Some(PeerKey::Outbound)
                } else {
                    self.connections.keys().copied().next()
                };
                let Some(target) = target else {
                    let _ = ack.send(Err("no peer connected".into()));
                    return;
                };
                // Send to exactly one peer: broadcasting a file offer or a
                // clipboard update to every accepted connection would leak
                // data to peers that did not request it.
                if let Some(connection) = self.connections.get(&target) {
                    let _ = connection.tx.send(OutgoingFrame {
                        message,
                        ack: Some(ack),
                    });
                } else {
                    let _ = ack.send(Err("no peer connected".into()));
                }
            }
            RuntimeCommand::AttachOutbound {
                stream,
                peer_device_id,
                ack,
            } => {
                // Exactly one outbound peer is active. Abort the previous
                // tasks so a replaced peer cannot keep injecting input or
                // re-route `Send` when it eventually disconnects.
                if let Some(old) = self.connections.remove(&PeerKey::Outbound) {
                    for task in old.tasks {
                        task.abort();
                    }
                }
                self.next_generation += 1;
                let generation = self.next_generation;
                self.active_outbound_generation = generation;
                let (tx, tasks) = spawn_connection(
                    stream,
                    PeerKey::Outbound,
                    generation,
                    peer_device_id.clone(),
                    self.inbound_tx.clone(),
                    self.command_sender.clone(),
                );
                self.connections.insert(
                    PeerKey::Outbound,
                    Connection {
                        tx,
                        tasks,
                        generation,
                    },
                );
                self.peer_device_id = peer_device_id;
                let _ = ack.send(());
            }
            RuntimeCommand::AttachInbound {
                stream,
                peer_device_id,
                ack,
            } => {
                let key = PeerKey::Accepted(self.next_accepted);
                self.next_accepted += 1;
                self.next_generation += 1;
                let generation = self.next_generation;
                let (tx, tasks) = spawn_connection(
                    stream,
                    key,
                    generation,
                    peer_device_id,
                    self.inbound_tx.clone(),
                    self.command_sender.clone(),
                );
                self.connections.insert(
                    key,
                    Connection {
                        tx,
                        tasks,
                        generation,
                    },
                );
                let _ = ack.send(());
            }
            RuntimeCommand::ConnectionClosed { key, generation } => {
                // A stale reader must never remove the replacement connection.
                let stale_outbound = matches!(key, PeerKey::Outbound)
                    && generation != self.active_outbound_generation;
                if !stale_outbound {
                    if let Some(connection) = self.connections.remove(&key) {
                        for task in connection.tasks {
                            task.abort();
                        }
                    }
                    // The last peer disappearing means no further chunks can
                    // arrive for its in-flight files. Remove the `.part` files
                    // instead of leaving them on disk indefinitely.
                    if self.connections.is_empty() {
                        let mut state = self.state.lock().unwrap();
                        state.transfers.cleanup_disconnected_peer();
                    }
                }
            }
            RuntimeCommand::Status { ack } => {
                let _ = ack.send(!self.connections.is_empty());
            }
            RuntimeCommand::Shutdown => {}
        }
    }

    fn send_to(&mut self, key: PeerKey, message: Message) {
        if let Some(connection) = self.connections.get(&key) {
            let _ = connection.tx.send(OutgoingFrame {
                message,
                ack: None,
            });
        }
    }

    fn dispatch(&mut self, frame: InboundFrame) {
        // Frames from a replaced outbound connection are discarded even if
        // its reader was still draining buffered data.
        if frame.key == PeerKey::Outbound
            && frame.generation != self.active_outbound_generation
        {
            return;
        }
        let capabilities = {
            let state = self.state.lock().unwrap();
            state
                .trust
                .capabilities(&frame.peer_device_id)
                .cloned()
        };
        if !message_allowed_by_capabilities(capabilities.as_ref(), &frame.message) {
            emit(json!({
                "notification": "messageRejected",
                "deviceId": frame.peer_device_id,
                "reason": "capability not granted",
            }));
            return;
        }
        match frame.message {
            Message::ClipboardUpdate(payload) => {
                let snapshot = {
                    let state = self.state.lock().unwrap();
                    state.clipboard.accept_remote(&payload)
                };
                if let Some(snapshot) = &snapshot {
                    if self.os_integration {
                        let _ = clipboard::os::write(snapshot);
                    }
                }
                let kind = match payload.kind {
                    ClipboardKind::Text => "text",
                    ClipboardKind::Image => "image",
                };
                emit(json!({
                    "notification": "clipboardUpdate",
                    "deviceId": payload.device_id,
                    "kind": kind,
                    "text": payload.text,
                    "dataBase64": payload.data_base64,
                    "mime": payload.mime,
                    "timestampMs": payload.timestamp_ms,
                    "seq": payload.seq,
                    "applied": snapshot.is_some(),
                }));
            }
            Message::FileTransferOffer(offer) => {
                let transfer_id = offer.transfer_id.clone();
                let (replies, record) = {
                    let mut state = self.state.lock().unwrap();
                    let replies = state.transfers.handle_offer(offer, &frame.peer_device_id);
                    let record = state
                        .transfers
                        .history()
                        .iter()
                        .rev()
                        .find(|entry| entry.transfer_id == transfer_id)
                        .cloned();
                    (replies, record)
                };
                if let Some(record) = record {
                    emit_transfer_record(&record);
                }
                for reply in replies {
                    self.send_to(frame.key, reply);
                }
            }
            Message::FileTransferAccept { transfer_id } => {
                let result = {
                    let mut state = self.state.lock().unwrap();
                    state.transfers.handle_accept(&transfer_id)
                };
                match result {
                    Ok(messages) => {
                        let record = {
                            let state = self.state.lock().unwrap();
                            state
                                .transfers
                                .history()
                                .iter()
                                .rev()
                                .find(|entry| entry.transfer_id == transfer_id)
                                .cloned()
                        };
                        if let Some(record) = record {
                            emit_transfer_record(&record);
                        }
                        let completed = messages.iter().any(|m| {
                            matches!(m, Message::FileTransferComplete { .. })
                        });
                        for message in messages {
                            self.send_to(frame.key, message);
                        }
                        if completed {
                            let record = {
                                let mut state = self.state.lock().unwrap();
                                state.transfers.mark_outgoing_complete(&transfer_id)
                            };
                            if let Some(record) = record {
                                emit_transfer_record(&record);
                            }
                        }
                    }
                    Err(error) => emit_transfer_failure(&transfer_id, &error.to_string()),
                }
            }
            Message::FileTransferReject { transfer_id, reason } => {
                let record = {
                    let mut state = self.state.lock().unwrap();
                    state.transfers.handle_reject(&transfer_id, reason)
                };
                match record {
                    Some(record) => emit_transfer_record(&record),
                    None => emit_transfer_failure(&transfer_id, "unknown transfer"),
                }
            }
            Message::FileTransferChunk(chunk) => {
                let transfer_id = chunk.transfer_id.clone();
                let result = {
                    let mut state = self.state.lock().unwrap();
                    state.transfers.handle_chunk(chunk).map(|final_record| {
                        let record = final_record.or_else(|| {
                            state
                                .transfers
                                .history()
                                .iter()
                                .rev()
                                .find(|entry| entry.transfer_id == transfer_id)
                                .cloned()
                        });
                        record
                    })
                };
                match result {
                    Ok(Some(record)) => emit_transfer_record(&record),
                    Ok(None) => emit_transfer_failure(&transfer_id, "chunk had no transfer record"),
                    Err(error) => emit_transfer_failure(&transfer_id, &error.to_string()),
                }
            }
            Message::FileTransferComplete { transfer_id } => {
                let result = {
                    let mut state = self.state.lock().unwrap();
                    state.transfers.handle_complete(&transfer_id)
                };
                match result {
                    Ok(record) => emit_transfer_record(&record),
                    Err(error) => emit_transfer_failure(&transfer_id, &error.to_string()),
                }
            }
            Message::FileTransferCancel { transfer_id, reason } => {
                let record = {
                    let mut state = self.state.lock().unwrap();
                    state.transfers.handle_cancel(&transfer_id, reason)
                };
                match record {
                    Some(record) => emit_transfer_record(&record),
                    None => emit_transfer_failure(&transfer_id, "unknown transfer"),
                }
            }
            Message::InputEvent { target, payload } => {
                let payload_bytes = payload.len();
                let Some(event) = decode_captured_event(&payload) else {
                    emit(json!({
                        "notification": "inputRejected",
                        "reason": "invalid event payload",
                        "payloadBytes": payload_bytes,
                    }));
                    return;
                };
                let gate_open = self.input_gate.load(Ordering::SeqCst);
                if !can_inject(
                    capabilities.as_ref(),
                    &target,
                    &self.local_device_id,
                    gate_open,
                    &event,
                ) {
                    emit(json!({
                        "notification": "inputRejected",
                        "deviceId": frame.peer_device_id,
                        "reason": "input disabled or wrong target",
                    }));
                    return;
                }
                // Bound how much a compromised peer can move/type per second.
                if self.last_input_window.elapsed() >= Duration::from_secs(1) {
                    self.last_input_window = Instant::now();
                    self.input_events_in_window = 0;
                }
                if self.input_events_in_window >= MAX_INPUT_EVENTS_PER_SECOND {
                    emit(json!({
                        "notification": "inputRejected",
                        "reason": "rate limit exceeded",
                    }));
                    return;
                }
                self.input_events_in_window += 1;
                // Test mode still emits delivery notifications, but never
                // touches the OS input stack.
                if self.os_integration {
                    InputBridge::inject(&event);
                }
                // Never dump raw event bytes (keystrokes/clipboard) to stdout.
                emit(json!({
                    "notification": "input",
                    "deviceId": frame.peer_device_id,
                    "kind": match &event {
                        CapturedEvent::Key { .. } => "key",
                        CapturedEvent::MouseMove { .. } => "mouse",
                        CapturedEvent::Hotkey(_) => "hotkey",
                    },
                    "payloadBytes": payload_bytes,
                }));
            }
            other => {
                emit(json!({
                    "notification": "message",
                    "debug": format!("{other:?}"),
                }));
            }
        }
    }
}

/// Start the runtime. `os_integration` is disabled in deterministic test mode
/// so injected input and remote clipboard data never touch the host OS.
pub fn start(
    state: Arc<Mutex<AppState>>,
    local_device_id: String,
    peer_device_id: String,
    os_integration: bool,
    input_gate: Arc<AtomicBool>,
) -> PeerRuntimeHandle {
    PeerRuntime::start(state, local_device_id, peer_device_id, os_integration, input_gate)
}

fn spawn_connection(
    stream: PeerStream,
    key: PeerKey,
    generation: u64,
    peer_device_id: String,
    inbound_tx: mpsc::UnboundedSender<InboundFrame>,
    command_tx: mpsc::UnboundedSender<RuntimeCommand>,
) -> (mpsc::UnboundedSender<OutgoingFrame>, Vec<JoinHandle<()>>) {
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<OutgoingFrame>();
    let (mut reader, mut writer) = stream.into_split();

    let read_tx = inbound_tx.clone();
    let read_commands = command_tx.clone();
    let read_peer_id = peer_device_id;
    let reader_task = tokio::spawn(async move {
        loop {
            match reader.recv().await {
                Ok(message) => {
                    if read_tx
                        .send(InboundFrame {
                            key,
                            generation,
                            peer_device_id: read_peer_id.clone(),
                            message,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    emit(json!({
                        "notification": "stream-error",
                        "error": error.to_string(),
                    }));
                    break;
                }
            }
        }
        let _ = read_commands.send(RuntimeCommand::ConnectionClosed { key, generation });
    });

    let writer_task = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            let result = writer.send(&frame.message).await.map_err(|e| e.to_string());
            if let Some(ack) = frame.ack {
                let _ = ack.send(result);
            }
        }
    });

    (out_tx, vec![reader_task, writer_task])
}

fn emit_transfer_record(record: &TransferRecord) {
    emit(json!({
        "notification": "transferUpdate",
        "transferId": record.transfer_id,
        "direction": format!("{:?}", record.direction).to_lowercase(),
        "state": serde_json::to_value(&record.state).unwrap_or(json!("unknown")),
        "peerDeviceId": record.peer_device_id,
        "name": record.name,
        "size": record.size,
        "bytesTransferred": record.bytes_transferred,
        "path": record.path,
        "message": record.message,
    }));
}

fn emit_transfer_failure(transfer_id: &str, message: &str) {
    emit(json!({
        "notification": "transferUpdate",
        "transferId": transfer_id,
        "state": "failed",
        "message": message,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::Capabilities;

    fn caps() -> Capabilities {
        Capabilities::default()
    }

    #[test]
    fn injection_requires_gate_capability_and_matching_target() {
        let event = CapturedEvent::Key {
            key: "A".into(),
            ctrl: false,
            alt: false,
            shift: false,
        };
        assert!(can_inject(
            Some(&caps()),
            "local-pc",
            "local-pc",
            true,
            &event
        ));
        assert!(!can_inject(
            Some(&caps()),
            "local-pc",
            "local-pc",
            false,
            &event
        ));
        assert!(!can_inject(
            Some(&caps()),
            "other-pc",
            "local-pc",
            true,
            &event
        ));
        assert!(!can_inject(None, "local-pc", "local-pc", true, &event));
        let no_input = Capabilities {
            input: false,
            clipboard: true,
            files: true,
            screen: false,
        };
        assert!(!can_inject(
            Some(&no_input),
            "local-pc",
            "local-pc",
            true,
            &event
        ));
        assert!(!can_inject(
            Some(&caps()),
            "local-pc",
            "local-pc",
            true,
            &CapturedEvent::Hotkey("Ctrl+Alt+Shift+M")
        ));
    }

    #[test]
    fn capabilities_gate_message_classes() {
        let none = Capabilities {
            input: false,
            clipboard: false,
            files: false,
            screen: false,
        };
        let input = Message::InputEvent {
            target: "local-pc".into(),
            payload: Vec::new(),
        };
        let clipboard = Message::ClipboardUpdate(mymouse_protocol::ClipboardPayload {
            device_id: "pc-b".into(),
            kind: mymouse_protocol::ClipboardKind::Text,
            text: Some("x".into()),
            data_base64: None,
            mime: None,
            timestamp_ms: 1,
            seq: 1,
        });
        let offer = Message::FileTransferOffer(mymouse_protocol::FileOffer {
            transfer_id: "t1".into(),
            from_device_id: "pc-b".into(),
            name: "a.txt".into(),
            size: 1,
            sha256: "x".into(),
            mime: None,
        });
        assert!(!message_allowed_by_capabilities(Some(&none), &input));
        assert!(!message_allowed_by_capabilities(Some(&none), &clipboard));
        assert!(!message_allowed_by_capabilities(Some(&none), &offer));
        assert!(message_allowed_by_capabilities(
            Some(&caps()),
            &input
        ));
        assert!(message_allowed_by_capabilities(
            Some(&caps()),
            &clipboard
        ));
        assert!(message_allowed_by_capabilities(
            Some(&caps()),
            &offer
        ));
        assert!(message_allowed_by_capabilities(
            None,
            &Message::Hello {
                version: 2,
                device_id: "pc-b".into(),
                name: "Peer".into(),
            }
        ));
    }
}
