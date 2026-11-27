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
- Final measured values on this host (post-remediation harness re-run): input
  delivery 30.67 ms, clipboard 5.342 Mbit/s, file 4.21 Mbit/s, cold pinned-TLS
  connect 339.48 ms, idle CPU 0/0% on the two processes, idle RAM
  12.13/11.9 MiB. Reconnect after peer loss is deliberately `null`/unmeasured
  because test mode has no auto-reconnect loop. An earlier sample on the same
  harness reported 8.9 ms / 15.688 Mbit/s / 15.307 Mbit/s / 121.46 ms; the
  difference is host-load variance, not a product improvement or regression
  claim. See [benchmarks.md](benchmarks.md).
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
- A second full-branch review found a stale-pump confidentiality defect: a pump
  queued before `AttachOutbound` replaced the outbound peer could send the old
  peer's file chunks to the replacement. Pumps now carry the connection
  generation and peer id, and are rejected if either changes. Transfer failures
  now update `TransferState::Failed` in history, so `listTransfers` agrees with
  `transferUpdate`. Both regressions are unit-tested; terminal history is not
  overwritten by late notices.
- A follow-up fresh review found that production `offerFile` still labelled the
  UI's no-peer-id offer with the placeholder `"peer"` while routing the frame
  to the active peer. The accept-side peer check therefore failed every normal
  UI file transfer. Production now resolves the omitted peer from the runtime's
  active peer before creating the offer, and
  `production_offer_file_defaults_to_the_active_peer` is a red→green regression
  test. With no active peer, the RPC returns `no connected peer` instead of
  recording a placeholder offer.
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

## Verification evidence (2026-11-27, final matrix unless marked earlier)

Commands were run from `myMouse/` unless noted.

| Command | Result |
| --- | --- |
| `cargo test --manifest-path backend/Cargo.toml` | exit 0; `115 passed; 0 failed` unit tests; `1 passed; 0 failed` in each of three integration binaries (`clipboard_file_two_instance`, `test_mode_config`, `two_instance`) |
| `cargo test --manifest-path shared/protocol/Cargo.toml` | exit 0; `11 passed; 0 failed` |
| `npm run test:ci` | exit 0; `6 files / 26 tests passed` |
| `npm run build` | exit 0; Vite production build and Electron TypeScript build succeeded |
| `npm run smoke` | exit 0; `OK: pc-a forwarded one crossing to pc-b over pinned TLS; pc-b received one InputEvent.` |
| `npm run benchmark` | exit 0; wrote the final two-process loopback sample to `docs/benchmarks.md`; reconnect remains `null`/unmeasured |
| `git diff --check` (repo root) | exit 0; only pre-existing LF→CRLF normalization warnings |
| Back-to-back `npm run smoke` (earlier phase) | both runs exit 0 after making smoke log paths per-run unique |
| `npm run dist` (earlier phase) | exit 0; unsigned NSIS installer and `win-unpacked` output built |
| `powershell -NoProfile -ExecutionPolicy Bypass -File installer\verify.ps1` (earlier phase) | `OK: unsigned Windows desktop, release backend, and packaged backend resource are present.` |

The final counts above include the fresh whole-branch review fixes R1–R8: every
fix had a watched RED test and a focused GREEN rerun before the full matrix. The
benchmark row is the pre-review two-process loopback sample; it was not rerun
because the script overwrites `docs/benchmarks.md`, and reconnect remains
`null`/unmeasured. No physical, field, Android, macOS or signed-release claim
follows from these counts.

The smoke harness originally reused fixed log paths. A rapid rerun could fail
while PowerShell's asynchronous redirected stdout handle from the previous run
was still being released. The harness fix waits for child processes and gives
each run a unique log directory; two consecutive runs now pass.

`cargo fmt --all -- --check` is not clean at baseline `cc1f6b5`, including the
Rust files touched here. Only the new `backend/src/device.rs` was formatted; a
repo-wide reformat would add unrelated churn, so formatting hygiene was limited
to `git diff --check`, which is clean.

## Production-grade remediation (final, 2026-11-27)

Status: **implemented, unit-tested, integration/harness-tested and Node-level
tested; not physically tested.** The full per-finding evidence table, commit
chain, deferred minors, blocked platform work and release caveats are in
[the production-grade remediation audit](superpowers/audits/2026-11-27-production-grade-remediation.md).

- One protocol (`shared/protocol` v2) and one pinned mutual-TLS transport are
  unchanged. No second protocol, transport, or mock-only success path exists.
- Electron IPC lifecycle, file capability/ownership gates, per-peer transfer
  cleanup, connection-generation-bound routing, protocol parity, pairing
  session binding, watchdog ownership, monitor-index resolution, clipboard
  allocation bounds and test-mode config isolation each have a watched RED
  test, a minimal fix and a GREEN rerun.
- `backend/tests/test_mode_config.rs` is an integration/harness test proving
  that `--test-mode` never writes the production config root, with and without
  an explicit `MYMOUSE_CONFIG_DIR` override.
- A fresh whole-branch review found four Important and four Minor issues after
  those commits; all were fixed at source with watched RED tests: scalar IPC
  results and string-ID notifications are no longer dropped; non-handoff sends
  propagate failures and failed offers become terminal; accept/reject route to
  the offering peer; active-peer refresh fails closed; outbound frames obey the
  1 MiB cap; sender-side cancel works on incoming transfers; and duplicate-chunk
  replay tolerates short reads. Final re-grade and exact evidence are in the
  remediation audit.
- Security defaults remain mutual TLS, fingerprint pinning, explicit approval,
  capability permissions and revocable trust. Discovery never grants trust.
- Autostart remains off; Windows builds remain unsigned because no signing
  certificate was supplied. macOS notarization is unavailable on this host.
- Android first-time pairing/client-certificate provisioning and the macOS
  companion remain **source-only/unverified**; there is no physical two-PC,
  mDNS, Wi-Fi, multi-monitor/DPI or hardware `SendInput` evidence.

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
- Review-driven hardening (four review rounds, ending with the final whole-branch
  review R1–R8): handoff frames are routed to
  the explicit crossing target instead of the runtime's independent selection;
  revoking the active peer disconnects it, clears the runtime mirrors and pauses
  the bridge; a dead forwarding channel no longer suppresses local input; a
  queued handoff frame to a vanished target is dropped and acked in the runtime
  so a concurrent disconnect cannot kill the forwarding task; inbound streams
  re-check trust at attach time so a revoke cannot be bypassed by a handshake
  that finishes late. Each fix has a red→green regression test.
- Final hardening: Electron IPC exit/timeout/malformed-reply rejection; strict
  `result` object validation; file control messages gated by `files` capability
  and transfer ownership; per-peer disconnect cleanup and duplicate-chunk
  idempotence; connection-generation-bound `Hello` and active-peer-only default
  routing; protocol full-body consumption, signed-range parity and 1 MiB frame
  cap; pairing code bound to an approved session's authenticated peer and
  certificate fingerprint; watchdog release ownership; topology-derived local
  monitor index; `GlobalSize`-bounded UTF-16 scan; test-mode config resolver.
- Final whole-branch review hardening, each with a watched RED test: scalar and
  object IPC results are accepted while errors stay object-shaped; discovery
  notifications are classified before reply-ID validation; only handoff frames
  use a best-effort ack while other targeted sends propagate failure; failed
  offers become `Failed` in history; accept/reject use the transfer's peer;
  active-peer refresh clears a vanished target instead of guessing; `write_frame`
  enforces the 1 MiB cap; cancel is valid for either transfer ownership; replay
  comparison uses `read_exact` behind a reader seam.

### Integration / harness-tested

- Two real backend processes exchange a synthetic crossing over pinned TLS.
- Clipboard text and exact file bytes travel through the same pinned-TLS v2
  transport (offer → accept → chunks → complete).
- The two-process benchmark measures input, clipboard/file throughput, cold
  connect and idle resources; it completes a 1 MiB multi-chunk transfer.
- Frontend React tests and Electron bridge tests.
- A spawned test-mode backend with a simulated production `APPDATA` cannot read
  or write that production config root, with or without `MYMOUSE_CONFIG_DIR`.
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
