# Product Excellence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish an honest measurement baseline and eliminate stuck injected input when a peer disappears mid-session.

**Architecture:** Keep the existing Rust/Tokio engine and v2 pinned-TLS protocol. Add a two-instance PowerShell benchmark driven by the existing test-mode stdin RPCs, then add a liveness watchdog that reuses the existing `Status` message as a heartbeat and releases held injected input on timeout.

**Tech Stack:** Rust 2021 / Tokio / rustls, PowerShell 7-compatible Windows scripting, Electron + React unchanged.

**Spec:** `docs/superpowers/specs/2026-11-27-product-excellence-design.md`

## Global Constraints

- One versioned protocol: `shared/protocol` v2. No protocol bump and no new wire message.
- One pinned mutual-TLS transport. Discovery never grants trust.
- Emergency stop remains authoritative. The watchdog never re-enables injection.
- TDD: failing test → red → minimal fix → green → full suite.
- No upstream source copied. No hardware, APK, macOS, signing or autostart claims.
- `cargo fmt --all -- --check` is red at baseline; use `git diff --check` instead.

## Review Focus

- Silent peer during held key: expected release of injected keys/buttons within three seconds; pinned by `liveness_expires_after_timeout` and watchdog runtime test.
- Heartbeat before input: expected one `Status` frame per second to the attached peer; pinned by heartbeat delivery test.
- Emergency stop during timeout: watchdog must not reopen injection; pinned by gate assertion in watchdog test.
- Benchmark reports invented numbers: expected every metric to come from a measured process/log observation; pinned by script self-check and report review.
- Benchmark leaves orphan processes: expected cleanup in `finally`; pinned by rerun of `npm run benchmark`.

---

### Task 1: Ranked roadmap and reproducible benchmark baseline

**Files:**
- Create: `docs/product-roadmap.md`
- Create: `scripts/benchmark.ps1`
- Create: `docs/benchmarks.md`
- Modify: `package.json`

**Interfaces:**
- Consumes: `--test-mode` stdin RPCs `status`, `crossTo`, `sendClipboard`, `offerFile`, `acceptTransfer`, `listTransfers`, `quit` from `backend/src/test_mode.rs`.
- Produces: `npm run benchmark`; `docs/benchmarks.md` with fields `input_delivery_ms`, `clipboard_mbps`, `file_mbps`, `reconnect_ms`, `idle_cpu_percent`, `idle_ram_mb`; `docs/product-roadmap.md` with ranked gaps.

- [x] **Step 1: Write the roadmap document from recon evidence**

Write `docs/product-roadmap.md` with a ranked table. Each row has `Rank`, `Gap`, `Evidence`, `Status`, `Next action`. Rank 1 must be "no reproducible performance baseline"; rank 2 "injected input can stay held after silent peer loss". Include a section stating that no upstream product was benchmarked here and that no physical testing was performed.

- [x] **Step 2: Write the benchmark script**

Create `scripts/benchmark.ps1`. It must:
  - build the debug backend with `cargo build --manifest-path backend/Cargo.toml`;
  - start two `--test-mode` backends (`pc-a`, `pc-b`) with distinct free ports and per-run log directory under `backend/target/benchmark-logs/<guid>`;
  - use `System.Diagnostics.Process` with a real redirected stdin pipe and file-redirected stdout/stderr through `cmd.exe /s /c`;
  - wait for `ready` and `connected` notifications with a 30 second deadline;
  - measure `input_delivery_ms` as elapsed wall time from writing a `crossTo` RPC to detecting `"notification"\s*:\s*"input"` in pc-b's log, with a 10 ms poll;
  - measure `clipboard_mbps` by sending a deterministic 256 KiB ASCII payload through `sendClipboard` and timing the matching `clipboardUpdate` notification;
  - measure `file_mbps` by creating a deterministic 1 MiB file, sending `offerFile` from pc-a, calling `acceptTransfer` on pc-b after the `transferUpdate` offer appears, and timing the `state":"completed"` notification on pc-b;
  - report `reconnect_ms` as unmeasured because test mode has no auto-reconnect loop;
  - sample `Get-Process` for both PIDs during a 2 second idle window and compute average `CPU` delta divided by wall time and average `WorkingSet64` in MiB;
  - write `docs/benchmarks.md` with a Markdown table of measured values, a JSON block with raw values, and explicit labels `two-process loopback harness`, `not field latency`, `no upstream comparison`;
  - stop both process trees and call `WaitForExit(5000)` in a `finally` block.

- [x] **Step 3: Add the npm script**

Add `"benchmark": "powershell -NoProfile -ExecutionPolicy Bypass -File scripts/benchmark.ps1"` to `package.json` scripts.

- [x] **Step 4: Run and verify the benchmark**

Run: `npm run benchmark` from `myMouse/`.
Expected: exit 0; `docs/benchmarks.md` contains all six metrics with non-empty measured values; no `NaN`; no orphan `mymouse-backend.exe` processes from the run.

- [x] **Step 5: Verify hygiene and commit**

Run: `git diff --check` and `npm test -- --run`.
Expected: both exit 0.

Task 1 also fixed a discovered production bug: file transfers above one 256 KiB chunk stalled; `accept_sends_every_chunk_for_large_file` was red, then green after the runtime began draining all chunks.

---

### Task 2: Input-liveness watchdog with held-input release

**Files:**
- Modify: `backend/src/input.rs`
- Modify: `backend/src/network.rs`

**Interfaces:**
- Consumes: `InputBridge::inject`, `CapturedEvent`, `Message::Status`, `PeerRuntimeHandle`.
- Produces:
  - `InjectedInputTracker::new() -> Self`
  - `InjectedInputTracker::note_key_down(&mut self, vk: u16)`
  - `InjectedInputTracker::note_key_up(&mut self, vk: u16)`
  - `InjectedInputTracker::note_button_down(&mut self, button: MouseButton)`
  - `InjectedInputTracker::note_button_up(&mut self, button: MouseButton)`
  - `InjectedInputTracker::clear(&mut self) -> Vec<InjectedRelease>`
  - `InjectedInputTracker::is_empty(&self) -> bool`
  - `InputBridge::release_all_injected(&self) -> bool`
  - `liveness_expires(last: Instant, now: Instant, timeout: Duration) -> bool`
  - `PeerRuntime::send_heartbeat(&self)`
  - `PeerRuntime::check_liveness(&mut self, now: Instant) -> bool`
  - `PeerRuntime::last_peer_frame` / `watchdog_notified` fields

- [ ] **Step 1: Write failing tracker and timeout tests**

In `backend/src/input.rs` tests add:
  - `tracked_keys_and_buttons_are_released_once`: insert key down 65, button `Left` down, duplicate key down 65, then `clear()` returns exactly one key release for 65 and one button release for `Left`, tracker is empty, second `clear()` is empty.
  - `key_up_removes_tracked_key`: key down 16 then key up 16 leaves tracker empty.
In `backend/src/network.rs` tests add:
  - `liveness_expires_after_timeout`: timeout 3 s, `now = last + 2_999 ms` is false, `now = last + 3_001 ms` is true.
  - `watchdog_fires_once_until_a_peer_frame_arrives`: construct `test_runtime`, set `last_peer_frame` to `now - 4 s`, assert `check_liveness(now)` is true, assert a second call is false, set `last_peer_frame = now`, assert `check_liveness(now)` is false and `watchdog_notified` is false.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path backend/Cargo.toml tracked_keys_and_buttons_are_released_once` then the other three named tests.
Expected: compile errors for missing types/functions.

- [ ] **Step 3: Implement the tracker and timeout helper**

In `backend/src/input.rs`:
  - derive `Hash` on `MouseButton`;
  - add `InjectedRelease` enum with `Key(u16)` and `Button(MouseButton)` variants;
  - add `InjectedInputTracker` backed by `HashSet<u16>` and `HashSet<MouseButton>`; `clear()` drains both sets into `Vec<InjectedRelease>`;
  - add `pub fn release_all_injected(&self) -> bool` that calls the Windows release function when compiled for Windows and returns whether anything was held. On non-Windows return false.
  - add top-level `pub fn liveness_expires(last: Instant, now: Instant, timeout: Duration) -> bool` in `network.rs` using `now.duration_since(last)` with a safe false on reversed time.
  - Windows injection path: track modifier VK transitions and every non-modifier key down/up; track mouse button down/up; `release_*` functions clear the tracker and send real key-up/button-up `SendInput` calls. Keep existing loop filtering intact.

- [ ] **Step 4: Run the focused tests to verify they pass**

Run: `cargo test --manifest-path backend/Cargo.toml tracked_keys_and_buttons_are_released_once`, then the other three.
Expected: four named tests pass.

- [ ] **Step 5: Wire heartbeats and the watchdog into the runtime**

In `backend/src/network.rs`:
  - add constants `HEARTBEAT_INTERVAL = Duration::from_secs(1)` and `INPUT_WATCHDOG_TIMEOUT = Duration::from_secs(3)`;
  - add `last_peer_frame: Instant` and `watchdog_notified: bool` to `PeerRuntime` and both constructors/tests;
  - in `run()`, add a `tokio::time::interval(HEARTBEAT_INTERVAL)` branch calling `send_heartbeat()`;
  - `send_heartbeat()` sends `Message::Status { connected: true, devices: vec![self.local_device_id.clone()] }` through `select_send_key(&self.connections, self.active_peer_id mirror)` without an ack;
  - update `last_peer_frame` on every inbound frame in `dispatch()` and add a no-op `Message::Status { .. }` branch so heartbeats do not spam generic debug notifications;
  - `check_liveness(now)` must: if `liveness_expires` and `InputBridge::release_all_injected()` reports held input and `!watchdog_notified`, emit `{"notification":"inputLost","reason":"peer heartbeat timeout"}` and set `watchdog_notified = true`; if fresh, reset `watchdog_notified = false`; return whether the timeout condition held.
  - Never touch `input_gate` in the watchdog.

- [ ] **Step 6: Run focused and full backend tests**

Run: `cargo test --manifest-path backend/Cargo.toml`.
Expected: all tests pass, including the two integration binaries.

- [ ] **Step 7: Verify repo hygiene and commit**

Run: `git diff --check`, `npm test -- --run`, `npm run build`, `npm run smoke`.
Expected: all exit 0.
Commit: `fix: release held input when a peer heartbeat stops`.

---

### Task 3: Document status and finish the increment

**Files:**
- Modify: `docs/phase1-audit.md`
- Modify: `docs/limitations.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: measured report from Task 1 and watchdog behaviour from Task 2.
- Produces: updated status inventory and verification evidence.

- [ ] **Step 1: Update the three docs**

State that the benchmark is two-process loopback harness data, not field or comparative data. State that held-input release is unit-tested and wired, with no hardware-level `SendInput` audit. Keep the autostart and unsigned-build statements intact.

- [ ] **Step 2: Verify and commit**

Run: `git diff --check`.
Expected: exit 0.
Commit: `docs: record product excellence increment status`.
