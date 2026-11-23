# myMouse Core Windows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Phase 1 of myMouse: two Windows PCs discover, pair over LAN, and share one mouse/keyboard across configured screen edges, with an Electron control UI and a Rust backend.

**Architecture:** Electron + React + TypeScript UI talks over a local JSON-RPC bridge to a Rust/Tokio backend. The backend owns mDNS discovery, QR/6-digit pairing with pinned per-pair certificates, the adjacency-grid layout, and Windows low-level input capture/injection. The UI renders dashboard, device list, layout editor, settings, and tray/Quick Drop shell.

**Tech Stack:** Windows 10/11, Node.js 20+, npm, Rust 1.78+, Cargo, Electron 30+, React 18+, TypeScript, Vite, Vitest, Tokio, rustls, rcgen, mdns-sd, serde, windows crate.

**Spec:** `docs/superpowers/specs/2026-11-23-myMouse-alternative-design.md`

## Global Constraints

- Windows 10/11 host only in this phase.
- Do not modify, launch, or depend on the installed OneMouse app.
- Autostart must not be enabled by installer or app code; expose a toggle only.
- Local-first: no mandatory account or third-party relay.
- Pairing: host displays QR/6-digit code, joiner confirms, host approves; per-pair TLS certificate pinned.
- Trust grants input, clipboard, files and screen control with per-capability toggles and revocation.
- Each install can be host and target; roles are per connection; only one input source active; loop prevention required.
- Layout: arbitrary adjacency grid; each physical monitor is a node; no wrap by default; escape hotkey restores host focus.
- Elevation limitation: v1 does not inject into UAC/secure desktop or locked sessions; document it.
- All generated work stays under this project folder unless the user gives another destination.
- Builds are unsigned until a code-signing certificate is supplied.

## Review Focus

- Multi-monitor edge crossing with shared or mismatched edges: moving the cursor to a configured edge hands off exactly once; test in Task 6.
- Untrusted or wrong peer during pairing: mismatched code or unpinned certificate is rejected before input is enabled; test in Tasks 4 and 5.
- Lost focus / panic escape: pressing the escape hotkey immediately returns input to the host and stops forwarding; test in Task 7.
- Malformed or version-mismatched protocol message: connection closes without panicking; test in Task 2.
- Trust store corruption or path traversal in device IDs: app falls back safely and cannot write outside its config dir; test in Task 4.

---

### Task 1: Project scaffold and test harness

**Files:**
- Create: `myMouse/package.json`
- Create: `myMouse/tsconfig.json`
- Create: `myMouse/vite.config.ts`
- Create: `myMouse/electron/main.ts`
- Create: `myMouse/backend/Cargo.toml`
- Create: `myMouse/backend/src/main.rs`
- Test: `myMouse/backend/src/main.rs` (unit test module)

**Interfaces:**
- Consumes: nothing.
- Produces: `npm run dev` starts Electron; `cargo test --manifest-path myMouse/backend/Cargo.toml` runs backend tests.

- [ ] **Step 1: Write the failing test**

In `myMouse/backend/src/main.rs`, add:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mymouse-backend");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml scaffold_compiles`
Expected: FAIL because the crate does not exist.

- [ ] **Step 3: Implement minimal scaffold**

Run `git init` in the workspace root and set local `user.name` / `user.email` so the commit steps below work (the workspace is not currently a git repo). Create `Cargo.toml` with package name `mymouse-backend`, edition 2021, Tokio dependency with `full` feature. Create `main.rs` with `fn main() { println!("mymouse-backend"); }` plus the test module above. Create Node/Electron/Vite/TypeScript config with scripts `dev`, `build`, `test`, and a minimal Electron main that loads a blank window.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml scaffold_compiles`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse
git commit -m "feat: scaffold myMouse desktop and backend"
```

### Task 2: Shared protocol messages

**Files:**
- Create: `myMouse/shared/protocol/Cargo.toml`
- Create: `myMouse/shared/protocol/src/lib.rs`
- Modify: `myMouse/backend/Cargo.toml`
- Test: `myMouse/shared/protocol/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `mymouse_protocol::{Message, ProtocolResult, PROTOCOL_VERSION, PROTOCOL_HEADER_LEN}` with `encode(&Message) -> Vec<u8>` and `decode(&[u8]) -> ProtocolResult<Message>`. `Message` variants and fields: `Hello { version: u16, device_id: String, name: String }`, `PairCode { code: String }`, `PairApprove`, `PairReject`, `InputEvent { target: String, payload: Vec<u8> }`, `Status { connected: bool, devices: Vec<String> }`, `Error { message: String }`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn hello_round_trip() {
    let m = Message::Hello { version: 1, device_id: "pc-a".into(), name: "PC A".into() };
    assert_eq!(decode(&encode(&m)).unwrap(), m);
}

#[test]
fn rejects_wrong_version() {
    let bytes = encode(&Message::Hello { version: 99, device_id: "x".into(), name: "X".into() });
    assert!(decode(&bytes).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path myMouse/shared/protocol/Cargo.toml`
Expected: FAIL because `Message` does not exist.

- [ ] **Step 3: Implement the protocol crate**

Define `PROTOCOL_VERSION: u16 = 1` and `PROTOCOL_HEADER_LEN: usize = 2`. Define the enum with exactly the variants and field types listed in Interfaces; derive `Serialize`, `Deserialize`, `PartialEq`, `Debug`. `encode` writes the 2-byte little-endian framing version followed by the bincode body. `decode` rejects input shorter than the header, rejects a framing version other than `PROTOCOL_VERSION`, rejects `Message::Hello` whose `version` field differs from `PROTOCOL_VERSION`, rejects a body longer than 1 MiB, then deserializes. Add the crate as a path dependency of the backend.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path myMouse/shared/protocol/Cargo.toml`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse/shared/protocol myMouse/backend/Cargo.toml
git commit -m "feat: add versioned myMouse protocol"
```

### Task 3: LAN discovery and peer model

**Files:**
- Create: `myMouse/backend/src/discovery.rs`
- Modify: `myMouse/backend/src/main.rs`
- Test: `myMouse/backend/src/discovery.rs`

**Interfaces:**
- Consumes: `mymouse_protocol::Message`.
- Produces: `discovery::{Peer, parse_service_name, Discovery}` with `Peer { id: String, name: String, address: String }`, `parse_service_name(&str) -> Result<Peer, InvalidServiceName>`, and `Discovery::start(&self) -> tokio::sync::mpsc::Receiver<Peer>`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn parses_peer_from_service_name() {
    let peer = parse_service_name("mymouse._tcp.local:PC-A@deadbeef").unwrap();
    assert_eq!(peer.id, "deadbeef");
    assert_eq!(peer.name, "PC-A");
}

#[test]
fn ignores_malformed_service_name() {
    assert!(parse_service_name("garbage").is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml discovery`
Expected: FAIL because `discovery` module is absent.

- [ ] **Step 3: Implement discovery**

Use `mdns-sd` to advertise `_mymouse._tcp.local.` with instance name `{name}@{id}`. `parse_service_name` splits on `@`, validates non-empty id and name, and returns `Peer { id, name, address }`. `Discovery::start` returns a channel of discovered peers and filters out the local device ID.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml discovery`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse/backend/src/discovery.rs myMouse/backend/src/main.rs
git commit -m "feat: add LAN peer discovery"
```

### Task 4: Pairing ceremony and trust store

**Files:**
- Create: `myMouse/backend/src/pairing.rs`
- Create: `myMouse/backend/src/trust.rs`
- Modify: `myMouse/backend/src/main.rs`
- Test: `myMouse/backend/src/pairing.rs`, `myMouse/backend/src/trust.rs`

**Interfaces:**
- Consumes: `discovery::Peer`.
- Produces: `pairing::{generate_code, PairSession}` where `PairSession::pairing_uri(&self) -> String` returns `mymouse://pair?code=<code>&id=<device_id>`; and `trust::{TrustStore, Capabilities}` with `TrustStore::load(path) -> TrustStore`, `save(&self, path) -> Result<()>`, `is_trusted(&self, id: &str) -> bool`, `grant(&mut self, id: &str, Capabilities)`, `revoke(&mut self, id: &str)`. `Capabilities { input: bool, clipboard: bool, files: bool, screen: bool }` with all four true by default.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn code_is_six_digits() {
    let code = generate_code();
    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|c| c.is_ascii_digit()));
}

#[test]
fn trust_store_stays_inside_config_dir() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = TrustStore::default();
    store.grant("pc/../evil", Capabilities::default());
    store.save(dir.path().join("trust.json")).unwrap();
    assert!(dir.path().join("trust.json").exists());
    assert_eq!(dir.path().read_dir().unwrap().count(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml pairing`
Expected: FAIL because modules are absent.

- [ ] **Step 3: Implement pairing and trust**

`generate_code` uses `rand` to produce a zero-padded six-digit decimal string. `PairSession` stores pending code, peer id, expiry, and `approve()` callback. `TrustStore` sanitizes IDs with a whitelist `[A-Za-z0-9_-]` before using them in filenames; `grant` writes the `Capabilities` passed in; `save` writes atomically via a temp file and rename. `load` replaces corrupt JSON with default and logs a warning. Add `tempfile` as a dev-dependency for the test.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml pairing`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse/backend/src/pairing.rs myMouse/backend/src/trust.rs
git commit -m "feat: add pairing and trust store"
```

### Task 5: Encrypted peer transport

**Files:**
- Create: `myMouse/backend/src/transport.rs`
- Modify: `myMouse/backend/Cargo.toml`
- Test: `myMouse/backend/src/transport.rs`

**Interfaces:**
- Consumes: `mymouse_protocol::Message`, `trust::TrustStore`.
- Produces: `transport::{Server, Client, PeerStream}` with `Server::bind(addr: &str, cert_der: Vec<u8>, key_der: Vec<u8>) -> Result<Server>`, `Server::local_addr(&self) -> SocketAddr`, `Server::accept(&self) -> Result<PeerStream>`, `Client::connect(addr: SocketAddr, pinned_fingerprints: &[String]) -> Result<PeerStream>`, `PeerStream::connected(&self) -> bool`, `send(&mut self, &Message) -> Result<()>`, `recv(&mut self) -> Result<Message>`. The test module defines a `test_cert() -> (Vec<u8>, Vec<u8>, String)` helper using `rcgen`.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn pinned_certificate_required() {
    let (server_cert, server_key, fingerprint) = test_cert();
    let server = Server::bind("127.0.0.1:0", server_cert, server_key).await.unwrap();
    let addr = server.local_addr();
    assert!(Client::connect(addr, &["wrong-fingerprint".into()]).await.is_err());
    let stream = Client::connect(addr, &[fingerprint]).await.unwrap();
    assert!(stream.connected());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml transport`
Expected: FAIL because `transport` module is absent.

- [ ] **Step 3: Implement transport**

Use `rcgen` to generate a self-signed cert per device and `rustls` with `ServerConfig`/`ClientConfig`. Pin the peer certificate by comparing its SHA-256 hex fingerprint against `pinned_fingerprints`. Frame messages as 4-byte big-endian length plus the complete `encode` output from Task 2. Reject frames larger than 1 MiB and close on decode error.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml transport`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse/backend/src/transport.rs myMouse/backend/Cargo.toml
git commit -m "feat: add pinned TLS transport"
```

### Task 6: Layout and cursor edge crossing

**Files:**
- Create: `myMouse/backend/src/layout.rs`
- Modify: `myMouse/backend/src/main.rs`
- Test: `myMouse/backend/src/layout.rs`

**Interfaces:**
- Consumes: `Peer`, physical monitor list.
- Produces: `layout::{Node, Edge, Side, Layout, Crossing}` with `Node::monitor(device_id: &str, monitor_index: u32, width: u32, height: u32) -> Node`, `Edge { from: String, to: String, side: Side }`, `Side::{Left, Right, Top, Bottom}`, `Layout::new()`, `add_node(&mut self, Node)`, `set_edge(&mut self, Edge)`, `cross(&self, from: &str, x: u32, y: u32) -> Option<Crossing>`, `Crossing { target: String, x: u32, y: u32 }`, and `escape_hotkey() -> &'static str`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn crosses_only_configured_edge() {
    let mut layout = Layout::new();
    layout.add_node(Node::monitor("pc-a", 0, 1920, 1080));
    layout.add_node(Node::monitor("pc-b", 1, 1920, 1080));
    layout.set_edge(Edge { from: "pc-a".into(), to: "pc-b".into(), side: Side::Right });
    assert!(layout.cross("pc-a", 1919, 500).is_some());
    assert!(layout.cross("pc-a", 0, 500).is_none());
}

#[test]
fn no_wrap_by_default() {
    let layout = Layout::new();
    assert!(layout.cross("pc-a", 0, 0).is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml layout`
Expected: FAIL because `layout` module is absent.

- [ ] **Step 3: Implement layout**

Model each monitor as `Node { device_id, monitor_index, width, height }`. `Edge` stores source, target and side. `cross` accepts a local coordinate, checks only the matching edge, and returns `Crossing { target, x, y }` with coordinates mapped to the target monitor. No opposite-edge wrap. `escape_hotkey` returns `"Ctrl+Alt+Shift+M"`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml layout`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse/backend/src/layout.rs
git commit -m "feat: add adjacency layout model"
```

### Task 7: Windows input capture and injection

**Files:**
- Create: `myMouse/backend/src/input.rs`
- Modify: `myMouse/backend/Cargo.toml`
- Test: `myMouse/backend/src/input.rs`

**Interfaces:**
- Consumes: `layout::Crossing`, `mymouse_protocol::Message`.
- Produces: `input::{InputBridge, CapturedEvent, TranslatedEvent, translate}`. `CapturedEvent::Key { key: String, ctrl: bool, alt: bool, shift: bool }`, `CapturedEvent::Hotkey(&'static str)`, `CapturedEvent::MouseMove { x: i32, y: i32 }`; `TranslatedEvent { key: String }`. `translate(&CapturedEvent, target_os: &str) -> Option<TranslatedEvent>`. `InputBridge::new() -> InputBridge`, public field `forwarding: bool`, `handle(&mut self, CapturedEvent)`, `InputBridge::start(bridge) -> tokio::task::JoinHandle<()>`, `InputBridge::inject(&CapturedEvent)`, `stop(&mut self)`. `CapturedEvent` derives `Serialize`, `Deserialize` so it can fill `Message::InputEvent.payload`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn translates_ctrl_c_to_command_c_on_mac_target() {
    let event = CapturedEvent::Key { key: "C", ctrl: true, alt: false, shift: false };
    assert_eq!(translate(&event, "mac").unwrap().key, "Cmd+C");
}

#[test]
fn escape_hotkey_stops_forwarding() {
    let mut bridge = InputBridge::new();
    bridge.forwarding = true;
    bridge.handle(CapturedEvent::Hotkey("Ctrl+Alt+Shift+M"));
    assert!(!bridge.forwarding);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml input`
Expected: FAIL because `input` module is absent.

- [ ] **Step 3: Implement input bridge**

Use Windows low-level mouse and keyboard hooks via the `windows` crate. `InputBridge` tracks `forwarding` and `target`. When forwarding is active, the bincode-serialized `CapturedEvent` becomes `Message::InputEvent { target, payload }`; when inactive, events are injected locally via `SendInput`. The escape hotkey sets `forwarding=false` and calls `inject` with a focus-restoring event. `translate` maps Ctrl→Cmd and Alt→Option for `target_os == "mac"` (returning `"Cmd+C"` for the test event) and returns the unchanged key for `"windows"`. Document UAC/locked-session limitation in `myMouse/docs/limitations.md`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml input`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse/backend/src/input.rs myMouse/docs/limitations.md
git commit -m "feat: add Windows input bridge"
```

### Task 8: Electron ↔ backend JSON-RPC bridge

**Files:**
- Create: `myMouse/electron/bridge.ts`
- Create: `myMouse/electron/preload.ts`
- Modify: `myMouse/electron/main.ts`
- Test: `myMouse/electron/bridge.test.ts`

**Interfaces:**
- Consumes: backend process stdout/stdin.
- Produces: `class Bridge` in `bridge.ts` with `send(method: string, params?: unknown) -> Promise<unknown>` and `onNotification(cb: (msg: unknown) => void)`. `preload.ts` exposes `window.mymouse` with `status()`, `devices()`, `pairingUri()`, `approvePairing(code: string)`, `setLayout(layout: unknown)`.

- [ ] **Step 1: Write the failing test**

```ts
it('round trips a status request', async () => {
  const mockProcess = makeMockBackend(); // responds to newline-delimited JSON on stdin
  const bridge = new Bridge(mockProcess);
  await expect(bridge.send('status')).resolves.toMatchObject({ ok: true });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test -- --run bridge.test.ts`
Expected: FAIL because `Bridge` does not exist.

- [ ] **Step 3: Implement bridge**

Electron main spawns `mymouse-backend --ipc stdio`. `bridge.ts` writes newline-delimited JSON requests with incrementing ids, matches responses by id, and forwards backend notifications. Define `makeMockBackend()` in the test to emulate the process. `preload.ts` exposes `window.mymouse` exactly as listed in Interfaces.

- [ ] **Step 4: Run test to verify it passes**

Run: `npm test -- --run bridge.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse/electron
git commit -m "feat: add electron backend bridge"
```

### Task 9: Desktop UI shell

**Files:**
- Create: `myMouse/src/App.tsx`
- Create: `myMouse/src/components/Dashboard.tsx`
- Create: `myMouse/src/components/DeviceLayout.tsx`
- Create: `myMouse/src/components/DeviceManagement.tsx`
- Create: `myMouse/src/components/Settings.tsx`
- Create: `myMouse/src/components/TrayMenu.ts`
- Test: `myMouse/src/App.test.tsx`

**Interfaces:**
- Consumes: `window.mymouse` from Task 8.
- Produces: screens for dashboard, device layout, file transfer placeholder, device management, settings; tray menu with Quit and Show.

- [ ] **Step 1: Write the failing test**

```tsx
it('shows connected state and device list', async () => {
  (window as any).mymouse = {
    status: async () => ({ ok: true, connected: true }),
    devices: async () => [{ name: 'PC-B' }],
    pairingUri: async () => 'mymouse://pair?code=123456&id=pc-a',
    approvePairing: async () => ({ ok: true }),
    setLayout: async () => ({ ok: true }),
  };
  render(<App />);
  expect(await screen.findByText('Connected')).toBeInTheDocument();
  expect(screen.getByText('PC-B')).toBeInTheDocument();
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npm test -- --run App.test.tsx`
Expected: FAIL because components do not exist.

- [ ] **Step 3: Implement UI**

Use React with a dark, compact layout: left sidebar with Dashboard, Device layout, File transfer, Device management, Settings; main panel with status cards, an SVG adjacency editor, a pairing card rendering the `pairingUri()` as a QR with `qrcode.react`, a device list with capability toggles and Revoke, and settings for language, auto reconnect, startup toggle (default off), Quick Drop placeholder, escape hotkey display. Tray menu uses Electron `Tray` with Show and Quit only.

- [ ] **Step 4: Run test to verify it passes**

Run: `npm test -- --run App.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse/src
git commit -m "feat: add desktop UI shell"
```

### Task 10: Two-instance Windows smoke test

**Files:**
- Create: `myMouse/scripts/smoke-two-instances.ps1`
- Modify: `myMouse/backend/src/main.rs`
- Test: `myMouse/backend/tests/two_instance.rs`

**Interfaces:**
- Consumes: Tasks 2, 4, 5, 6, 7, 8.
- Produces: `npm run smoke` starts two backend instances in test mode and verifies one cursor-crossing event is delivered.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn forwards_one_crossing_event() {
    let (a, b) = spawn_test_pair().await; // starts two in-process test-mode backends
    a.cross_to("pc-b", 100, 200).await.unwrap();
    let msg = b.recv().await.unwrap();
    assert!(matches!(msg, Message::InputEvent { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml --test two_instance`
Expected: FAIL because test harness is absent.

- [ ] **Step 3: Implement test mode**

Add `--test-mode --device-id <id> --port <port>` flags. In test mode, discovery is replaced by a static peer list, pairing is auto-approved with a fixed fingerprint, and input capture uses synthetic events instead of OS hooks. The PowerShell script starts two instances with ids `pc-a` and `pc-b`, waits for status, injects a crossing, and asserts both logs show delivery.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path myMouse/backend/Cargo.toml --test two_instance` and `npm run smoke`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse/scripts myMouse/backend/tests myMouse/backend/src/main.rs
git commit -m "test: add two-instance smoke test"
```

### Task 11: Packaging and docs

**Files:**
- Create: `myMouse/installer/build-windows.ps1`
- Create: `myMouse/README.md`
- Create: `myMouse/docs/limitations.md`
- Modify: `myMouse/package.json`
- Test: `myMouse/installer/verify.ps1`

**Interfaces:**
- Consumes: all previous tasks.
- Produces: unsigned Windows build output and setup instructions.

- [ ] **Step 1: Write the failing verification**

```powershell
if (-not (Test-Path 'myMouse/dist/win-unpacked/myMouse.exe')) { throw 'desktop build missing' }
if (-not (Test-Path 'myMouse/backend/target/release/mymouse-backend.exe')) { throw 'backend build missing' }
```

- [ ] **Step 2: Run verification to confirm it fails**

Run: `powershell -File myMouse/installer/verify.ps1`
Expected: FAIL because artifacts do not exist.

- [ ] **Step 3: Build and document**

Add npm script `dist`: build React, package Electron with electron-builder (NSIS target), compile Rust in release mode, copy backend exe into the app resources. README covers install, firewall prompt, pairing steps, escape hotkey, trust management, limitations, and explicit note that autostart is off until the user enables it.

- [ ] **Step 4: Run verification to confirm it passes**

Run: `npm run dist` then `powershell -File myMouse/installer/verify.ps1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add myMouse/installer myMouse/README.md myMouse/docs myMouse/package.json
git commit -m "chore: add packaging and docs"
```
