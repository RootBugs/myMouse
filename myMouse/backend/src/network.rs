//! Peer session runtime.
//!
//! Owns the outbound pinned-TLS connection and every accepted inbound
//! connection, dispatches protocol messages to the existing clipboard/file
//! managers, and gives synchronous/async RPC handlers a safe channel-based
//! way to send outbound messages.

use crate::clipboard;
use crate::file_transfer::TransferRecord;
use crate::input::{CapturedEvent, InputBridge};
use crate::transport::{Client, PeerStream};
use crate::{emit, AppState};
use mymouse_protocol::{ClipboardKind, Message};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

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
    peer_device_id: String,
    message: Message,
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
    ConnectionClosed(PeerKey),
    Status {
        ack: oneshot::Sender<bool>,
    },
    Shutdown,
}

/// Cloneable handle used by RPC handlers and test-mode crossing code.
#[derive(Clone)]
pub struct PeerRuntimeHandle {
    commands: mpsc::UnboundedSender<RuntimeCommand>,
}

impl PeerRuntimeHandle {
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
        let stream = Client::connect(address, &[fingerprint])
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
    connections: HashMap<PeerKey, mpsc::UnboundedSender<OutgoingFrame>>,
    next_accepted: u64,
}

impl PeerRuntime {
    fn start(
        state: Arc<Mutex<AppState>>,
        local_device_id: String,
        peer_device_id: String,
        os_integration: bool,
    ) -> PeerRuntimeHandle {
        let (command_sender, commands) = mpsc::unbounded_channel();
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let handle = PeerRuntimeHandle {
            commands: command_sender.clone(),
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
                if let Some(tx) = self.connections.get(&target) {
                    let _ = tx.send(OutgoingFrame {
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
                let tx = spawn_connection(
                    stream,
                    PeerKey::Outbound,
                    peer_device_id.clone(),
                    self.inbound_tx.clone(),
                    self.command_sender.clone(),
                );
                self.connections.insert(PeerKey::Outbound, tx);
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
                let tx = spawn_connection(
                    stream,
                    key,
                    peer_device_id,
                    self.inbound_tx.clone(),
                    self.command_sender.clone(),
                );
                self.connections.insert(key, tx);
                let _ = ack.send(());
            }
            RuntimeCommand::ConnectionClosed(key) => {
                self.connections.remove(&key);
            }
            RuntimeCommand::Status { ack } => {
                let _ = ack.send(!self.connections.is_empty());
            }
            RuntimeCommand::Shutdown => {}
        }
    }

    fn send_to(&mut self, key: PeerKey, message: Message) {
        if let Some(tx) = self.connections.get(&key) {
            let _ = tx.send(OutgoingFrame {
                message,
                ack: None,
            });
        }
    }

    fn dispatch(&mut self, frame: InboundFrame) {
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
                emit(json!({
                    "notification": "input",
                    "target": target,
                    "payloadHex": payload.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
                }));
                if self.os_integration && target == self.local_device_id {
                    if let Ok(event) = bincode::deserialize::<CapturedEvent>(&payload) {
                        InputBridge::inject(&event);
                    }
                }
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
) -> PeerRuntimeHandle {
    PeerRuntime::start(state, local_device_id, peer_device_id, os_integration)
}

fn spawn_connection(
    stream: PeerStream,
    key: PeerKey,
    peer_device_id: String,
    inbound_tx: mpsc::UnboundedSender<InboundFrame>,
    command_tx: mpsc::UnboundedSender<RuntimeCommand>,
) -> mpsc::UnboundedSender<OutgoingFrame> {
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<OutgoingFrame>();
    let (mut reader, mut writer) = stream.into_split();

    let read_tx = inbound_tx.clone();
    let read_commands = command_tx.clone();
    let read_peer_id = peer_device_id;
    tokio::spawn(async move {
        loop {
            match reader.recv().await {
                Ok(message) => {
                    if read_tx
                        .send(InboundFrame {
                            key,
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
        let _ = read_commands.send(RuntimeCommand::ConnectionClosed(key));
    });

    tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            let result = writer.send(&frame.message).await.map_err(|e| e.to_string());
            if let Some(ack) = frame.ack {
                let _ = ack.send(result);
            }
        }
    });

    out_tx
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
