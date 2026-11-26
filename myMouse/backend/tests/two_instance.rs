//! End-to-end two-instance test.
//!
//! Spawns two real `mymouse-backend` processes in `--test-mode`, links them
//! over pinned TLS, drives a synthetic cursor crossing through PC A, and
//! asserts PC B observes exactly one `Message::InputEvent` delivery. This is
//! the same path the PowerShell smoke script exercises.

use serde_json::{json, Value};
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
    id: String,
}

impl TestInstance {
    async fn start(id: &str, port: u16, peer_id: &str, peer_port: Option<u16>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mymouse-backend"));
        command
            .arg("--test-mode")
            .arg("--device-id")
            .arg(id)
            .arg("--peer-id")
            .arg(peer_id)
            .arg("--port")
            .arg(port.to_string())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());
        if let Some(peer_port) = peer_port {
            command
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

    async fn wait_for(&mut self, notification: &str) -> Value {
        loop {
            let value = self.next_line().await;
            if value.get("notification").and_then(Value::as_str) == Some(notification) {
                return value;
            }
        }
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
        }
    }
}

impl Drop for TestInstance {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[tokio::test]
async fn forwards_one_crossing_event() {
    let port_b = free_port();
    let mut port_a = free_port();
    while port_a == port_b {
        port_a = free_port();
    }

    // PC B listens first; PC A connects with the pinned test fingerprint.
    let mut b = TestInstance::start("pc-b", port_b, "pc-a", None).await;
    let ready_b = b.wait_for("ready").await;
    assert_eq!(ready_b["deviceId"], "pc-b");

    let mut a = TestInstance::start("pc-a", port_a, "pc-b", Some(port_b)).await;
    let ready_a = a.wait_for("ready").await;
    assert_eq!(ready_a["deviceId"], "pc-a");
    let connected = a.wait_for("connected").await;
    assert_eq!(connected["peer"], format!("127.0.0.1:{port_b}"));

    // A synthetic cursor at (100, 200) sits on pc-a's configured right edge.
    let response = a
        .rpc(
            1,
            "crossTo",
            json!({ "target": "pc-b", "x": 100, "y": 200 }),
        )
        .await;
    let result = &response["result"];
    assert_eq!(result["ok"], true, "crossTo failed: {response}");
    assert_eq!(result["target"], "pc-b");
    // A right-edge handoff places the cursor at the target's left edge.
    assert_eq!(result["x"], 0);
    assert_eq!(result["y"], 200);
    // `input` is emitted only for Message::InputEvent on the receiving peer.
    // Raw payload bytes are never logged; only the event kind and size are.
    let delivered = b.wait_for("input").await;
    assert_eq!(delivered["deviceId"], "pc-a");
    assert_eq!(delivered["kind"], "mouse");
    let payload_bytes = delivered["payloadBytes"]
        .as_u64()
        .expect("InputEvent payloadBytes");
    assert!(payload_bytes > 0, "InputEvent carried an empty payload");
}
