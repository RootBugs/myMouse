//! End-to-end clipboard and file delivery between two real backend processes.
//!
//! PC A connects to PC B with the pinned test certificate, sends a clipboard
//! update and a small file, PC B accepts the offer, and the receiver verifies
//! the exact saved bytes plus the clipboard notification.

use serde_json::{json, Value};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{timeout, Duration};

const WAIT: Duration = Duration::from_secs(20);

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind a free port")
        .local_addr()
        .expect("local address")
        .port()
}

struct TestInstance {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    /// Notifications observed while waiting for an RPC response.
    pending: Vec<Value>,
    id: String,
}

impl TestInstance {
    async fn start(id: &str, port: u16, peer: Option<(&str, u16)>, config_dir: &Path) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mymouse-backend"));
        command
            .arg("--test-mode")
            .arg("--device-id")
            .arg(id)
            .arg("--port")
            .arg(port.to_string())
            .env("MYMOUSE_CONFIG_DIR", config_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());
        if let Some((peer_id, peer_port)) = peer {
            command
                .arg("--peer-id")
                .arg(peer_id)
                .arg("--peer")
                .arg(format!("127.0.0.1:{peer_port}"));
        }
        let mut child = command.spawn().expect("spawn backend test instance");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            pending: Vec::new(),
            id: id.to_string(),
        }
    }

    async fn next_line(&mut self) -> Value {
        let line = timeout(WAIT, self.lines.next_line())
            .await
            .unwrap_or_else(|_| {
                let status = self.child.try_wait().ok().flatten();
                panic!(
                    "{}: timed out waiting for a backend line (child status: {status:?})",
                    self.id
                )
            })
            .expect("stdout read error")
            .unwrap_or_else(|| panic!("{}: backend closed stdout", self.id));
        serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("{}: invalid JSON line `{line}`: {error}", self.id))
    }

    async fn rpc(&mut self, id: u64, method: &str, params: Value) -> Value {
        let request = json!({ "id": id, "method": method, "params": params });
        let mut line = serde_json::to_string(&request).expect("serialize request");
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .expect("write request to stdin");
        self.stdin.flush().await.expect("flush stdin");
        loop {
            let value = self.next_line().await;
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                return value;
            }
            self.pending.push(value);
        }
    }

    async fn wait_for<F>(&mut self, predicate: F) -> Value
    where
        F: Fn(&Value) -> bool,
    {
        if let Some(index) = self.pending.iter().position(|value| predicate(value)) {
            return self.pending.remove(index);
        }
        loop {
            let value = self.next_line().await;
            if predicate(&value) {
                return value;
            }
            self.pending.push(value);
        }
    }

    async fn wait_for_notification(&mut self, notification: &str) -> Value {
        self.wait_for(|value| {
            value.get("notification").and_then(Value::as_str) == Some(notification)
        })
        .await
    }

    async fn wait_for_transfer(&mut self, transfer_id: &str, state: &str) -> Value {
        self.wait_for(|value| {
            value.get("notification").and_then(Value::as_str) == Some("transferUpdate")
                && value["transferId"].as_str() == Some(transfer_id)
                && value["state"].as_str() == Some(state)
        })
        .await
    }
}

impl Drop for TestInstance {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[tokio::test]
async fn transfers_clipboard_and_file_over_pinned_tls() {
    timeout(Duration::from_secs(45), run_transfer()).await.expect("two-instance transfer test timed out");
}

async fn run_transfer() {
    let temp = tempfile::tempdir().expect("create test temp dir");
    let config_a = temp.path().join("config-pc-a");
    let config_b = temp.path().join("config-pc-b");
    let save_dir = temp.path().join("received");
    let source = temp.path().join("payload.bin");
    let bytes = b"myMouse clipboard + file delivery payload\n".to_vec();
    std::fs::write(&source, &bytes).expect("write source file");

    let port_b = free_port();
    let mut port_a = free_port();
    while port_a == port_b {
        port_a = free_port();
    }

    let mut b = TestInstance::start("pc-b", port_b, None, &config_b).await;
    let ready_b = b.wait_for_notification("ready").await;
    assert_eq!(ready_b["deviceId"], "pc-b");

    let mut a = TestInstance::start("pc-a", port_a, Some(("pc-b", port_b)), &config_a).await;
    let ready_a = a.wait_for_notification("ready").await;
    assert_eq!(ready_a["deviceId"], "pc-a");
    let connected = a.wait_for_notification("connected").await;
    assert_eq!(connected["peer"], format!("127.0.0.1:{port_b}"));
    b.wait_for_notification("peer-stream").await;

    let settings = b
        .rpc(
            1,
            "setFileTransferSettings",
            json!({ "saveDir": save_dir.to_string_lossy(), "autoAccept": false }),
        )
        .await;
    assert_eq!(settings["result"]["ok"], true, "settings failed: {settings}");

    let marker = "clipboard delivered over pinned TLS";
    let sent = a
        .rpc(2, "sendClipboard", json!({ "text": marker }))
        .await;
    assert_eq!(sent["result"]["ok"], true, "sendClipboard failed: {sent}");
    let clipboard = b.wait_for_notification("clipboardUpdate").await;
    assert_eq!(clipboard["deviceId"], "pc-a");
    assert_eq!(clipboard["kind"], "text");
    assert_eq!(clipboard["text"], marker);
    assert_eq!(clipboard["applied"], true);

    let offered = a
        .rpc(
            3,
            "offerFile",
            json!({ "path": source.to_string_lossy(), "peerDeviceId": "pc-b" }),
        )
        .await;
    assert_eq!(offered["result"]["ok"], true, "offerFile failed: {offered}");
    let transfer_id = offered["result"]["offer"]["transfer_id"]
        .as_str()
        .expect("offer transfer_id")
        .to_string();

    let incoming_offer = b.wait_for_transfer(&transfer_id, "offered").await;
    assert_eq!(incoming_offer["direction"], "incoming");
    assert_eq!(incoming_offer["name"], "payload.bin");
    assert_eq!(incoming_offer["size"], bytes.len() as u64);

    let accepted = b
        .rpc(4, "acceptTransfer", json!({ "transferId": transfer_id }))
        .await;
    assert_eq!(accepted["result"]["ok"], true, "acceptTransfer failed: {accepted}");

    let completed = b.wait_for_transfer(&transfer_id, "completed").await;
    let saved_path = completed["path"]
        .as_str()
        .expect("completed transfer path")
        .to_string();
    let saved = std::fs::read(&saved_path).expect("read transferred file");
    assert_eq!(saved, bytes, "receiver saved different bytes");
    assert_eq!(completed["bytesTransferred"], bytes.len() as u64);
}
