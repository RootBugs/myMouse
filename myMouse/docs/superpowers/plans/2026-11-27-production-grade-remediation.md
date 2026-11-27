# Production-grade Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the confirmed Electron IPC, peer-authorization, transfer-state, routing, protocol-parity, and pairing-binding defects without changing the single v2 protocol or pinned mutual-TLS transport.

**Architecture:** Keep the existing Rust/Tokio backend, v2 binary codec, pinned TLS transport, Electron bridge, and React UI. Fix each defect at its source with narrow regression tests: peer-bound runtime authority, connection-generation-bound setup sends, strict IPC lifecycle handling, and explicit transfer ownership.

**Tech Stack:** Rust 2021 / Tokio / rustls, TypeScript / Electron 30 / Vitest 2, PowerShell two-process harness.

**Spec:** `docs/superpowers/specs/2026-11-27-production-grade-remediation-design.md`

## Global Constraints

- One versioned protocol: `shared/protocol` v2. No protocol bump and no new wire message.
- One pinned mutual-TLS transport. Do not add a second transport or relax certificate pinning.
- No mocks that replace real functionality with hardcoded success; mocks are only test doubles.
- TDD per task: write the failing test, observe red, apply the minimal fix, observe green.
- After each task: focused test, full backend suite, protocol suite, frontend suite, `npm run build`, `npm run smoke`, `git diff --check`, then commit.
- Autostart stays off; builds stay unsigned until a signing certificate exists.
- Do not commit untracked sibling projects, archives, or screenshots at workspace root.
- Do not reformat the whole repository; the `cargo fmt --all -- --check` failure is pre-existing.
- Evidence labels must distinguish unit-tested, integration-tested, harness-tested, Node-level, source-only, physically tested, and not implemented.

## Builder-check rulings

The five ambiguities raised by a fresh engineer review are settled in the spec
under "Builder-check rulings": no pre-auth pairing bootstrap; session binding
recorded at the UI/RPC boundary; code verification acknowledges only after
explicit approval; IPC validation requires a present `result` object rather than
a boolean `ok`; and the 1 MiB frame bound is enforced at the encoded-frame
boundary. These rulings bind Tasks 1, 5, and 6.

## Review Focus

- Backend exits while an RPC is in flight: the UI call must reject, not hang. Test: `electron/bridge.test.ts` exit/timeout cases in Task 1.
- Backend executable is missing: the app must report an error, not emit an unhandled `ENOENT`. Test: `electron/main.test.ts` spawn-error case in Task 1.
- A peer without `files` capability sends a file control message: it must be rejected before state mutation. Test: `backend/src/network.rs::control_messages_require_files_and_ownership` in Task 2.
- A peer disconnects while another peer remains: only that peer's transfers must terminalize and its `.part` files must be removed. Test: `backend/src/network.rs::disconnect_terminalizes_only_that_peers_transfers` in Task 3.
- Two `connectPeer` RPCs race: the first `Hello` must not be delivered to the replacement peer. Test: `backend/src/network.rs::hello_is_bound_to_its_connection_generation` in Task 4.
- A frame has trailing bytes or a `u64` above Kotlin's signed range: Rust must reject it. Test: `shared/protocol/src/lib.rs::decode_rejects_trailing_bytes` and `decode_rejects_unsigned_values_above_i64_max` in Task 5.
- A pairing code is replayed from an arbitrary peer or certificate: the backend must reject it. Test: `backend/src/main.rs::pair_code_requires_bound_authenticated_peer` in Task 6.

---

### Task 1: Electron IPC lifecycle

**Files:**
- Modify: `electron/bridge.ts`, `electron/main.ts`, `electron/bridge.test.ts`
- Create: `electron/main.test.ts`
- Modify: `package.json` (add `test:ci` script used by verification commands)

**Interfaces:**
- Consumes: existing `IpcProcess` and `Bridge` API used by `electron/main.ts:14-23`.
- Produces: `Bridge.onExit(cb: () => void)`, `Bridge.close(reason?: string)`, and a `Bridge` constructor accepting an optional timeout in milliseconds; `send()` rejects after timeout, exit, or malformed reply.

- [ ] **Step 1: Write failing tests in `electron/bridge.test.ts`**

Add cases:
1. `rejects_pending_sends_when_backend_exits` — emit a synthetic exit after `send('status')`, assert rejection with `/backend exited/i`.
2. `rejects_pending_send_after_timeout` — construct with a 20 ms timeout and no reply, assert rejection with `/timed out/i`.
3. `ignores_malformed_and_duplicate_replies` — emit `null`, a primitive, an object with non-integer `id`, then two replies for the same ID; assert the promise resolves once and no notification handler throws.

- [ ] **Step 2: Run focused red test**

Run: `npx vitest run electron/bridge.test.ts`
Expected: FAIL (timeout/exit tests hang or fail; malformed reply throws).

- [ ] **Step 3: Implement the minimal bridge lifecycle fix**

Extend `IpcProcess` with optional `onExit`, store pending entries with timers, validate replies as non-null objects with an integer `id` and an `ok` result object, reject on exit/timeout, and clear timers on settlement. Notify once for malformed or duplicate replies.

- [ ] **Step 4: Write failing `electron/main.test.ts` spawn-error test**

Mock `electron` and `node:child_process`; simulate `spawn` emitting `error` with `ENOENT`. Assert no unhandled error reaches the process and a `backendError` notification is sent.

- [ ] **Step 5: Implement `main.ts` error and exit handling**

Attach `error`, `exit`, and `close` listeners to the spawned backend; wire exit into `Bridge.onExit`; emit `backendError`/`backendExit`; make the `mymouse:call` handler reject when the bridge is gone. Add `app.on('activate', ...)` so macOS recreates the window.

- [ ] **Step 6: Run focused green tests and full verification**

Run: `npx vitest run electron/bridge.test.ts electron/main.test.ts`, `npm run test:ci`, `npm run build`.
Expected: all tests pass; build exits 0.

- [ ] **Step 7: Commit**

`git add electron/bridge.ts electron/main.ts electron/bridge.test.ts electron/main.test.ts package.json && git commit -m "fix: reject stale Electron IPC calls on backend lifecycle changes"`

---

### Task 2: File capability and ownership gate

**Files:**
- Modify: `backend/src/network.rs`, `backend/src/file_transfer.rs`
- Test: `backend/src/network.rs` unit tests

**Interfaces:**
- Consumes: `Capabilities.files`, `InboundFrame.peer_device_id`, `TransferRecord`.
- Produces: `FileTransferManager::owns_outgoing(&self, transfer_id: &str, peer_device_id: &str) -> bool`, `owns_incoming(...) -> bool`.

- [ ] **Step 1: Write failing tests**

In `network.rs` tests add `control_messages_require_files_and_ownership`:
- `message_allowed_by_capabilities(Some(&no_files), &Message::FileTransferCancel{..})` is false.
- `message_allowed_by_capabilities(None, &Message::FileTransferAccept{..})` is false.
- Dispatch `FileTransferCancel` from `pc-b` for an outgoing offer owned by `pc-c`; assert history still `Offered` and a `messageRejected` notification was emitted.

In `file_transfer.rs` add ownership query tests for outgoing and incoming records.

- [ ] **Step 2: Run focused red test**

Run: `cargo test --manifest-path backend/Cargo.toml network::tests::control_messages_require_files_and_ownership -- --exact`
Expected: FAIL (`--exact` needs the fully qualified Rust test path).

- [ ] **Step 3: Implement the gate**

Make every `FileTransfer*` variant require `capabilities.files`. Before `handle_accept`, `handle_reject`, `handle_cancel`, `handle_chunk`, or `handle_complete`, check ownership with the new query methods and return early with `messageRejected` on mismatch.

- [ ] **Step 4: Run focused green and full backend suite**

Run: `cargo test --manifest-path backend/Cargo.toml`
Expected: all backend tests pass, including both integration binaries.

- [ ] **Step 5: Commit**

`git commit -am "fix: bind file transfer messages to capability and peer ownership"`

---

### Task 3: Transfer identity and disconnect integrity

**Files:**
- Modify: `backend/src/file_transfer.rs`, `backend/src/network.rs`
- Test: `backend/src/file_transfer.rs`, `backend/src/network.rs` unit tests

**Interfaces:**
- Consumes: `TransferRecord.peer_device_id`, `IncomingTransfer.part_path`.
- Produces: `FileTransferManager::fail_peer_transfers(&mut self, peer_device_id: &str) -> Vec<TransferRecord>`, and idempotent duplicate-chunk handling in `handle_chunk`.

- [ ] **Step 1: Write failing tests**

Add tests:
1. `duplicate_offer_id_is_rejected` — an incoming offer reusing an active ID must be rejected and must not overwrite the outgoing history record.
2. `disconnect_terminalizes_only_that_peers_transfers` — with `pc-b` and `pc-c` connected, `DisconnectPeer("pc-b")` terminalizes `pc-b` records, removes its `.part`, and leaves `pc-c` active.
3. `duplicate_identical_chunk_is_idempotent` — resending the already-written bytes leaves state unchanged; conflicting bytes fail.

- [ ] **Step 2: Run focused red tests**

Run: `cargo test --manifest-path backend/Cargo.toml file_transfer::tests::duplicate_offer_id_is_rejected -- --exact`
Expected: FAIL (`--exact` needs the fully qualified Rust test path).

- [ ] **Step 3: Implement transfer state fixes**

Reject duplicate IDs before insertion; add `fail_peer_transfers`; call it on `DisconnectPeer` and stale `ConnectionClosed`; treat already-applied chunks whose bytes match disk as successful no-ops.

- [ ] **Step 4: Run focused green and full backend suite**

Run: `cargo test --manifest-path backend/Cargo.toml`
Expected: all tests pass.

- [ ] **Step 5: Commit**

`git commit -am "fix: keep transfer identity and cleanup peer-bound"`

---

### Task 4: Active-peer routing and connection-bound Hello

**Files:**
- Modify: `backend/src/network.rs`, `backend/src/main.rs`
- Test: `backend/src/network.rs`, `backend/src/main.rs` unit tests

**Interfaces:**
- Consumes: `RuntimeCommand::Send`, `PeerRuntimeHandle::connect`.
- Produces: `connect(...) -> Result<ConnectionTicket, String>` where `ConnectionTicket { peer_device_id: String, generation: u64 }`; `send_to_connection(&self, message, ticket) -> Result<(), String>`; `send_to(message, Option<String>)` remains targeted and best-effort.

- [ ] **Step 1: Write failing tests**

1. `default_send_never_uses_arbitrary_accepted_peer` — with two accepted connections and no active peer, `select_send_key` returns `None`.
2. `hello_is_bound_to_its_connection_generation` — attach outbound A then B, attempt a generation-bound Hello for A, assert it is not written to B and returns an error.
3. `production_offer_file_sends_to_recorded_peer` — `offerFile` resolves the active peer, sends with that explicit target, and records the same ID.

- [ ] **Step 2: Run focused red tests**

Run: `cargo test --manifest-path backend/Cargo.toml network::tests::hello_is_bound_to_its_connection_generation -- --exact`
Expected: FAIL (`--exact` needs the fully qualified Rust test path).

- [ ] **Step 3: Implement routing fixes**

Return a generation ticket from `connect`; add generation-checked confirmed sends; remove the arbitrary `HashMap::keys().next()` fallback; use targeted confirmed sends for Hello and targeted sends for `offerFile`.

- [ ] **Step 4: Run focused green and full verification**

Run: `cargo test --manifest-path backend/Cargo.toml`, then `npm run smoke`.
Expected: all backend tests pass; smoke reports one forwarded crossing.

- [ ] **Step 5: Commit**

`git commit -am "fix: bind outbound sends to the selected peer connection"`

---

### Task 5: Protocol and clipboard parity

**Files:**
- Modify: `shared/protocol/src/lib.rs`, `backend/src/clipboard.rs`, `backend/src/network.rs`
- Test: `shared/protocol/src/lib.rs`, `backend/src/clipboard.rs` unit tests

**Interfaces:**
- Consumes: v2 `ClipboardPayload`, `FileOffer`, `FileChunk`.
- Produces: `ClipboardManager::accept_remote(&self, payload: &ClipboardPayload, source_device_id: &str) -> Option<ClipboardSnapshot>`, `MAX_CLIPBOARD_IMAGE_BYTES` below the transport frame cap.

- [ ] **Step 1: Write failing tests**

Add:
1. `decode_rejects_trailing_bytes`.
2. `decode_rejects_unsigned_values_above_i64_max` for clipboard timestamp and chunk offset.
3. `remote_clipboard_requires_authenticated_device_id`.
4. `image_payload_fits_transport_frame` — a maximum allowed image produces a frame `<= 1 MiB`.

- [ ] **Step 2: Run focused red tests**

Run: `cargo test --manifest-path shared/protocol/Cargo.toml`, `cargo test --manifest-path backend/Cargo.toml image_payload_fits_transport_frame -- --exact`
Expected: FAIL.

- [ ] **Step 3: Implement parity fixes**

In Rust `decode`, require full cursor consumption and signed-range fields. Lower the image bound and verify the encoded frame length. Pass the frame's peer ID into `accept_remote` and use it in emitted notifications.

- [ ] **Step 4: Run focused green and full verification**

Run: `cargo test --manifest-path shared/protocol/Cargo.toml`, `cargo test --manifest-path backend/Cargo.toml`, `npm run smoke`.
Expected: all pass.

- [ ] **Step 5: Commit**

`git commit -am "fix: align protocol validation and bind clipboard identity"`

---

### Task 6: Pairing session binding

**Files:**
- Modify: `backend/src/pairing.rs`, `backend/src/network.rs`, `backend/src/main.rs`
- Test: `backend/src/main.rs`, `backend/src/network.rs` unit tests

**Interfaces:**
- Consumes: `InboundFrame.peer_device_id`, accepted stream certificate fingerprint.
- Produces: `PairSession::bind_peer(&mut self, peer_device_id: &str, fingerprint: &str) -> bool`; inbound `PairCode` handler emits `PairApprove` or `PairReject`.

- [ ] **Step 1: Write failing tests**

Add:
1. `pair_code_requires_bound_authenticated_peer` — a valid code from a peer whose device ID or certificate does not match the session gets a reject and no trust grant.
2. `approve_pairing_rejects_peer_id_outside_session` — `approvePairing` with a peer ID different from `PairSession::peer_id()` returns an error and does not grant trust.

- [ ] **Step 2: Run focused red tests**

Run: `cargo test --manifest-path backend/Cargo.toml pair_code_requires_bound_authenticated_peer -- --exact`
Expected: FAIL.

- [ ] **Step 3: Implement binding**

Carry the accepted client certificate fingerprint into inbound frames; handle `PairCode` by validating the session, binding peer ID and fingerprint, consuming the session, and replying with `PairApprove` only when all checks pass. Require `approvePairing` to match the session's recorded peer ID.

- [ ] **Step 4: Run focused green and full backend suite**

Run: `cargo test --manifest-path backend/Cargo.toml`
Expected: all pass. Record explicitly that Android client-certificate provisioning is still source-only/unverified.

- [ ] **Step 5: Commit**

`git commit -am "fix: bind pairing approval to the authenticated peer"`

---

### Task 7: Watchdog, monitor index, and clipboard allocation bounds

**Files:**
- Modify: `backend/src/network.rs`, `backend/src/main.rs`, `backend/src/clipboard.rs`
- Test: `backend/src/network.rs`, `backend/src/main.rs`, `backend/src/clipboard.rs` unit tests

**Interfaces:**
- Consumes: `Layout.nodes()`, `InputBridge::with_topology`, Windows `CF_UNICODETEXT` buffer.
- Produces: `local_monitor_index(layout: &Layout, local_device_id: &str) -> usize`, `utf16_prefix_len(bytes: &[u8], max_chars: usize) -> Option<usize>`.

- [ ] **Step 1: Write failing tests**

Add tests that the watchdog arming requires a real release, disconnect clears input ownership, monitor index uses the persisted local node instead of hardcoded `0`, and `utf16_prefix_len` rejects odd lengths and unterminated buffers.

- [ ] **Step 2: Run focused red tests**

Run: `cargo test --manifest-path backend/Cargo.toml local_monitor_index -- --exact`
Expected: FAIL.

- [ ] **Step 3: Implement fixes**

Clear input ownership on peer removal; arm the watchdog notice only when a release occurs; resolve the monitor index from `Layout`; bound the Windows text scan with `GlobalSize` and the pure helper.

- [ ] **Step 4: Run focused green and full verification**

Run: `cargo test --manifest-path backend/Cargo.toml`, `npm run smoke`.
Expected: all pass.

- [ ] **Step 5: Commit**

`git commit -am "fix: correct liveness state and platform buffer bounds"`

---

### Task 8: Harness hygiene, status docs, and final verification

**Files:**
- Modify: `backend/src/test_mode.rs`, `docs/phase1-audit.md`, `docs/benchmarks.md`, `.superpowers/sdd/2026-11-27-production-grade-verification/progress.md`
- Create: `docs/superpowers/audits/2026-11-27-production-grade-remediation.md`

**Interfaces:**
- Consumes: all prior task outputs.
- Produces: one final evidence table and a fresh full-branch review request.

- [ ] **Step 1: Write failing test for test-mode config isolation**

Set a temporary `MYMOUSE_CONFIG_DIR` in a harness test and assert test mode does not modify the real config directory.

- [ ] **Step 2: Run focused red test**

Run: `cargo test --manifest-path backend/Cargo.toml test_mode_does_not_touch_production_config -- --exact`
Expected: FAIL.

- [ ] **Step 3: Implement isolation and update docs**

Use the explicit test-config override; update status docs with exact evidence labels, deferred minors, blocked platform work, and unchanged unsigned/autostart caveats.

- [ ] **Step 4: Run final verification suite**

Run: `cargo test --manifest-path backend/Cargo.toml`; `cargo test --manifest-path shared/protocol/Cargo.toml`; `npm run test:ci`; `npm run build`; `npm run smoke`; `npm run benchmark`; `git diff --check`; `git status --short --branch`.
Expected: all pass; benchmark values labelled one-host loopback; reconnect remains `null`.

- [ ] **Step 5: Fresh review and push**

Dispatch a fresh read-only reviewer over the whole remediation branch. If the verdict is Yes with no Critical/Important finding, commit docs, push `origin/master`, and verify `git rev-parse HEAD origin/master`; otherwise fix and re-review.

- [ ] **Step 6: Regenerate delivery zip and report**

Regenerate `myMouse-product-excellence.zip` after the final verified commit. Do not commit the zip or untracked sibling assets.
