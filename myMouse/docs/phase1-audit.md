# Phase 1 engineering audit — 2026-11-27

This document records the Phase 1 architecture decision, the upstream comparison,
the implementation status after milestones 1 and 2, and the exact verification
evidence available on this host. It intentionally separates **implemented**,
**unit-tested**, **integration/harness-tested**, **physically tested**, and
**not implemented**. It does not claim hardware compatibility that was not
exercised.

## Scope and inputs

- Project: `myMouse`, branch `master`, baseline commit `cc1f6b5`.
- Architecture sources reviewed: Electron main/preload and React UI, Rust
  backend modules (`main`, `network`, `transport`, `discovery`, `pairing`,
  `trust`, `input`, `layout`, `clipboard`, `file_transfer`, `test_mode`), the
  shared v2 binary protocol, installer scripts, README, and limitations.
- Runtime host: Windows 10/11 x64 workspace. No second physical PC or tablet
  was available. No Android SDK/NDK/Gradle, no `kotlinc`, and no Swift
  toolchain were available. No code-signing certificate was supplied.

## Strategy: retain and improve the existing engine

The Phase 1 decision is to **retain and improve the existing Rust engine**. It
already owns the single v2 protocol, pinned mutual-TLS transport, trust store,
mDNS discovery, input capture/injection, clipboard, file transfer, and the UI
RPC contract. Replacing it with an upstream engine would discard that coherent
contract and require a second protocol bridge, which the directive forbids.

Upstream projects were used as references only; no upstream source was copied
into this repository.

| Project | Useful reference | Why not integrate |
| --- | --- | --- |
| Mousehop / Lan Mouse | Clear active/inactive state discipline and simple event forwarding model | Different implementation stack and protocol; no benefit to replacing a working Rust core |
| Deskflow | Mature multi-screen topology, clipboard and file-transfer design | C++/GPL-heavy; copying it would create license obligations and a second transport |
| Input Leap | Historical KVM topology experience | Archived upstream; not a safe active dependency for a new product |
| RoboMouse | Windows-only KVM UX reference | C#/Windows-only with unclear license metadata |

If upstream code is ever incorporated, its license and attribution obligations
must be recorded here before the merge. The current source tree is independent.

## Milestone 1 — persistent identity and real peer targeting

Status: **implemented, unit-tested, and harness-tested; not field-tested on two
physical machines.**

- Added `backend/src/device.rs`: a stable per-install UUID and a system-derived
  device name persisted in `device.json`. Production no longer hardcodes
  `"local-pc"`.
- `AppState::new()` now returns `Result<Self, String>` and loads/creates the
  device identity.
- `Hello`, `localIdentity`, and mDNS discovery now carry the real device name.
- Added a dynamic connected-peer target provider to `InputBridge` and
  `PeerRuntimeHandle::active_peer_id()`. Production capture events are stamped
  with the ID of the currently connected peer, never the local device ID.
- Emergency stop now pauses capture and resumes it without tearing down OS
  hooks; held modifiers are released on RPC stop.

## Milestone 2 — complete local input event model

Status: **implemented and unit-tested; test-mode forwarding is harness-tested;
Windows `SendInput` effects are not hardware-audited.**

- Added `MouseButton { Left, Right, Middle, X1, X2 }` and a `down` flag on key
  events.
- Added `CapturedEvent::MouseButton` and `CapturedEvent::MouseWheel`; inbound
  `Hotkey(_)` remains rejected from the wire.
- Windows capture now emits key down/up, button press/release, X1/X2, and wheel
  events. Windows injection handles key press/release, target modifier-state
  preservation, absolute mouse positioning, per-button press/release flags, and
  wheel injection.
- Capture and injection share one stable key vocabulary. Windows VKs map to
  canonical names for modifiers, navigation keys, F1–F24, and common special
  keys; `VK(n)` names are parsed exactly with `1..=255` validation. This fixes
  the observed regression where `VK(112)` was injected as `V` (VK 0x56) and
  `Shift` arrived as `VK(16)`.
- `network.rs` input notifications distinguish `key`, `mouse`, `mouseButton`,
  `wheel`, and `hotkey` without logging raw keystroke bytes.

The failing tests before the fix showed `left: 86 / right: 112` for `VK(112)`
and `left: "VK(16)" / right: "Shift"`. After the minimal fix the backend suite
returned `52 passed; 0 failed`.

## Product excellence increment — measured baseline and input liveness

Status: benchmark **implemented and harness-tested**; watchdog **implemented,
unit-tested and wired**; both **not physically tested**.

- `npm run benchmark` starts two real `mymouse-backend.exe` processes and writes
  [benchmarks.md](benchmarks.md). It is a **two-process loopback harness** on
  one Windows host: not field latency, not a physical two-PC session, and not a
  comparison with any upstream product.
- Measured values on this host: input delivery 25.31 ms, clipboard 7.019
  Mbit/s, file 6.966 Mbit/s, cold pinned-TLS connect 236.6 ms, idle CPU 0% on
  both processes, idle RAM 12.12/11.82 MiB. Reconnect after peer loss is
  deliberately `null`/unmeasured because test mode has no auto-reconnect loop.
- Benchmarking exposed a real production bug: file transfers above one 256 KiB
  chunk stalled after the first chunk. The regression test
  `accept_sends_every_chunk_for_large_file` went red → green, and the 1 MiB
  benchmark now observes a genuinely completed transfer.
- The liveness watchdog reuses v2 `Status` as a one-second heartbeat. After more
  than three seconds without a live frame from the peer that supplied input,
  tracked injected keys/buttons are released once and `inputLost` is emitted
  when held input existed. Frames from a different peer do not mask that
  silence. The tracker, timeout boundary, one-shot latch, heartbeat target and
  gate preservation are unit-tested. The watchdog never writes `input_gate`, so
  emergency stop remains authoritative. Large file chunks are now pumped one
  event-loop turn at a time, and both quit and IPC-EOF release injected input.
- Windows `SendInput` release effects and a physical silent-peer/held-key
  interruption were **not** hardware-audited. No superiority or field-latency
  claim follows from this increment.

## Security posture

Status: **implemented and code/test-verified; no live downgrade or physical
adversary test was run.**

- One versioned protocol (`shared/protocol` v2) and one pinned-TLS transport.
- mDNS advertises the real listener port and certificate fingerprint as a hint.
  The fingerprint is never trusted without explicit out-of-band verification.
- Discovery never grants trust. Pairing requires explicit approval and pins the
  peer certificate for mutual TLS.
- Capabilities are enforced per inbound message class; revocation removes the
  pin; outbound messages go to exactly one connected peer.
- Hotkey events are rejected from the wire; emergency stop is local-only.
- Protocol decoding is byte-budgeted; device IDs are strict; pairing codes are
  single-use; clipboard/file bounds and transfer completion hashes are checked.
- TLS 1.3-only is configured in rustls. No live TLS 1.2 downgrade attempt was
  executed, so this is a configuration-level statement.

## Verification evidence (fresh, 2026-11-27)

Commands were run from `myMouse/` unless noted.

| Command | Result |
| --- | --- |
| `cargo test` in `myMouse/backend` | `77 passed; 0 failed` unit tests; `1 passed; 0 failed` for each of the two integration test binaries |
| `cargo test` in `myMouse/shared/protocol` | `9 passed; 0 failed` |
| `npm test -- --run` | `5 files / 19 tests passed`, exit 0 (non-fatal Vite CJS Node API deprecation warning) |
| `npm run build` | exit 0; Vite produced `index.html`, CSS, and JS assets; Electron TypeScript build succeeded |
| `npm run smoke` | `OK: pc-a forwarded one crossing to pc-b over pinned TLS; pc-b received one InputEvent.` |
| `npm run benchmark` | exit 0; wrote two-process loopback values to `docs/benchmarks.md`, reconnect unmeasured |
| `git diff --check` | exit 0; only pre-existing LF→CRLF normalization warnings |
| Back-to-back `npm run smoke` | both runs exit 0 after making smoke log paths per-run unique |
| `npm run dist` | exit 0; unsigned NSIS installer and `win-unpacked` output built |
| `powershell -NoProfile -ExecutionPolicy Bypass -File installer\verify.ps1` | `OK: unsigned Windows desktop, release backend, and packaged backend resource are present.` |

The smoke harness originally reused fixed log paths. A rapid rerun could fail
while PowerShell's asynchronous redirected stdout handle from the previous run
was still being released. The harness fix waits for child processes and gives
each run a unique log directory; two consecutive runs now pass.

`cargo fmt --all -- --check` is not clean at baseline `cc1f6b5`, including the
Rust files touched here. Only the new `backend/src/device.rs` was formatted; a
repo-wide reformat would add unrelated churn, so formatting hygiene was limited
to `git diff --check`, which is clean.

## Status inventory

### Implemented and unit-tested

- Persistent device identity and name; dynamic peer target stamping.
- Full local event model with key down/up, buttons, wheel, and stable key names.
- Layout serde/validation/atomic persistence, `setLayout`/`getLayout` local IPC,
  topology-driven input bridge, crossing math, protocol v2, trust/capability
  gates, clipboard and file manager bounds, pairing state machine, rate
  limiting, and hotkey rejection.
- Backend-driven React layout view with an explicit empty state.
- Multi-chunk file-transfer regression test, injected-input tracker, liveness
  timeout boundary, one-shot watchdog latch, and Status heartbeat dispatch.
- Review-driven hardening (three review rounds): handoff frames are routed to
  the explicit crossing target instead of the runtime's independent selection;
  revoking the active peer disconnects it, clears the runtime mirrors and pauses
  the bridge; a dead forwarding channel no longer suppresses local input; a
  queued handoff frame to a vanished target is dropped and acked in the runtime
  so a concurrent disconnect cannot kill the forwarding task; inbound streams
  re-check trust at attach time so a revoke cannot be bypassed by a handshake
  that finishes late. Each fix has a red→green regression test.

### Integration / harness-tested

- Two real backend processes exchange a synthetic crossing over pinned TLS.
- Clipboard text and exact file bytes travel through the same pinned-TLS v2
  transport (offer → accept → chunks → complete).
- The two-process benchmark measures input, clipboard/file throughput, cold
  connect and idle resources; it completes a 1 MiB multi-chunk transfer.
- Frontend React tests and Electron bridge tests.
- Windows packaging and artifact presence.

### Physically tested

- None. No second physical PC/tablet was available. There was no real mDNS
  discovery session, real pairing between devices, real keyboard/mouse handoff,
  multi-monitor/DPI validation, or Wi-Fi interruption test. The benchmark is
  two-process loopback only, and the liveness release path has no hardware-level
  `SendInput` audit or physical silent-peer test.

### Not implemented / still open

- Physical validation of live boundary detection/handoff from the Windows
  capture hook on two separate PCs/tablets; handoff is implemented and
  unit-tested but only the synthetic harness crossing ran here.
- Per-monitor DPI compensation and robust multi-monitor coordinate mapping.
- A full drag-and-drop visual topology editor; the current view renders
  backend-provided state.
- Replay protection (nonces/timestamps) and stronger per-peer rate limiting.
- Real multi-peer routing/topology beyond one connected peer.
- Remote screen streaming; Android APK and macOS builds; code signing.
- Hardware-level audit of Windows `SendInput`, including watchdog key/button
  release, and live TLS downgrade rejection.
- Production reconnect-time measurement after peer loss; the loopback harness
  reports it as unmeasured.
- Return-to-local re-entry when the cursor crosses back from the peer (the
  emergency stop/Escape path is authoritative today).

## Next milestone

Validate the implemented topology/handoff path and the liveness release path
with two physical machines, real multi-monitor geometry, and an actual silent
peer while a key/button is held. Measure production reconnect time in that
setting, then add return-to-local re-entry only after the handoff behaviour is
verified. Do not claim field readiness before physical testing.
