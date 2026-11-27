//! Test-mode harness hygiene.
//!
//! `--test-mode` is a local test harness only. It must never read or write the
//! production config directory, even when a harness forgets to set
//! `MYMOUSE_CONFIG_DIR`. This test spawns the real backend with a fake APPDATA
//! (standing in for the production config root) and asserts which directory
//! actually receives the test trust grant.

use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tokio::time::{timeout, Duration};

const WAIT: Duration = Duration::from_secs(20);

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.start_kill();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind a free port")
        .local_addr()
        .expect("local address")
        .port()
}

/// Start one real test-mode backend with a simulated production APPDATA and a
/// private temp root, with or without an explicit harness config override.
async fn spawn_test_mode(
    appdata: &Path,
    temp_root: &Path,
    explicit_config: Option<&Path>,
) -> (ChildGuard, Lines<BufReader<ChildStdout>>) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mymouse-backend"));
    command
        .arg("--test-mode")
        .arg("--device-id")
        .arg("pc-hygiene")
        .arg("--port")
        .arg(free_port().to_string())
        .env("APPDATA", appdata)
        .env("TEMP", temp_root)
        .env("TMP", temp_root)
        .env_remove("MYMOUSE_CONFIG_DIR")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());
    if let Some(dir) = explicit_config {
        command.env("MYMOUSE_CONFIG_DIR", dir);
    }
    let mut child = command.spawn().expect("spawn test-mode backend");
    let stdout = child.stdout.take().expect("child stdout");
    (ChildGuard(child), BufReader::new(stdout).lines())
}

async fn wait_ready(lines: &mut Lines<BufReader<ChildStdout>>) -> Value {
    loop {
        let line = timeout(WAIT, lines.next_line())
            .await
            .expect("test-mode backend became ready")
            .expect("stdout read error")
            .expect("test-mode backend closed stdout before ready");
        let value: Value =
            serde_json::from_str(&line).unwrap_or_else(|error| panic!("invalid JSON `{line}`: {error}"));
        if value.get("notification").and_then(Value::as_str) == Some("ready") {
            return value;
        }
    }
}

fn production_sentinel(appdata: &Path) -> PathBuf {
    let dir = appdata.join("myMouse");
    std::fs::create_dir_all(&dir).expect("create simulated production config dir");
    let path = dir.join("trust.json");
    std::fs::write(&path, r#"{"sentinel":"production"}"#).expect("write production sentinel");
    path
}

fn assert_production_untouched(sentinel: &Path) {
    let contents = std::fs::read_to_string(sentinel).expect("production sentinel still readable");
    assert_eq!(
        contents,
        r#"{"sentinel":"production"}"#,
        "test mode must never overwrite the production config directory"
    );
    for name in ["device.json", "settings.json", "layout.json"] {
        assert!(
            !sentinel.parent().expect("sentinel parent").join(name).exists(),
            "test mode must not create production file {name}"
        );
    }
}

#[tokio::test]
async fn test_mode_does_not_touch_production_config() {
    let root = tempfile::tempdir().expect("temp root");
    let temp_root = root.path().join("harness-temp");
    std::fs::create_dir_all(&temp_root).expect("create harness temp root");

    // Case 1: no explicit override. Production APPDATA is still off-limits.
    let appdata = root.path().join("appdata");
    let sentinel = production_sentinel(&appdata);
    let (mut child, mut lines) = spawn_test_mode(&appdata, &temp_root, None).await;
    wait_ready(&mut lines).await;
    child.0.kill().await.expect("stop test-mode backend");
    child.0.wait().await.expect("reap test-mode backend");

    let isolated = temp_root.join("myMouse-test").join("pc-hygiene");
    assert!(
        isolated.join("trust.json").exists(),
        "test mode must use its own harness config directory when no override is set"
    );
    assert_production_untouched(&sentinel);

    // Case 2: an explicit harness override still works and also stays out of
    // the production config directory.
    let explicit_appdata = root.path().join("appdata-explicit");
    let explicit_sentinel = production_sentinel(&explicit_appdata);
    let explicit_config = root.path().join("explicit-config");
    let (mut child, mut lines) =
        spawn_test_mode(&explicit_appdata, &temp_root, Some(&explicit_config)).await;
    wait_ready(&mut lines).await;
    child.0.kill().await.expect("stop test-mode backend");
    child.0.wait().await.expect("reap test-mode backend");

    assert!(
        explicit_config.join("trust.json").exists(),
        "an explicit MYMOUSE_CONFIG_DIR override must still be honoured"
    );
    assert_production_untouched(&explicit_sentinel);
}
