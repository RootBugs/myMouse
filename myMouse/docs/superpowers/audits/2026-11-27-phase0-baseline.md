# myMouse — Phase 0 baseline audit

Date: 2026-11-27 Asia/Calcutta.
Auditor: Agent 00, Chief Engineering Orchestrator.
Directive: autonomous multi-agent engineering and verification, Phase 0.

This report is the pre-change baseline for the new engineering mandate. It was
recorded before any further source modification. It deliberately separates
**implemented**, **unit-tested**, **integration/harness-tested**,
**physically tested**, and **not verified**.

## Repository state

| Item | Evidence |
| --- | --- |
| Repository root | workspace root; Git remote `origin` = `https://github.com/RootBugs/myMouse.git` |
| Branch | `master`, tracking `origin/master`, `[ahead 4]` |
| HEAD | `d0d990a fix: harden file transfer and liveness review findings` |
| `origin/master` | `d66b27b feat: wire validated topology and one-peer handoff into production` |
| Pending local commits | `4ece335`, `277471e`, `2a86158`, `d0d990a`; not pushed |
| Uncommitted tracked files | `myMouse/backend/src/file_transfer.rs`, `myMouse/backend/src/network.rs`, `myMouse/docs/benchmarks.md`, `myMouse/docs/phase1-audit.md` |
| Untracked siblings | `DevCard/`, `DevDash/`, `JSON2Type/`, `JSONDiff/`, `QRForge/`, `SnapClip/`, `myMouse-product-excellence.zip`, `myMouse-project.zip`, `myMouse-source.zip`, `mymouse-window.png`, `shot1.png`, `shot2.png`, `test-cert-values.txt` |
| Tracked `myMouse` files | 126 |

The four dirty tracked files are the second-review fixes from the prior product
excellence increment: generation/peer-scoped file-transfer pumps, failed-transfer
history persistence, and regenerated documentation. They are **not** discarded.
They are part of this baseline and must be committed before the next clean
review batch.

The untracked sibling projects and archives are outside `myMouse/` and must not
be committed with this project.

## Build system and environment

| Component | Evidence |
| --- | --- |
| Rust backend | `myMouse/backend/Cargo.toml`, package `mymouse-backend` 0.1.0, edition 2021 |
| Shared protocol | `myMouse/shared/protocol/Cargo.toml`, package `mymouse-protocol` 0.1.0, edition 2021 |
| Desktop app | `myMouse/package.json`: Vite 5, React 18, Electron 30, Vitest 2, TypeScript 5 |
| Harnesses | `scripts/smoke-two-instances.ps1`, `scripts/benchmark.ps1`, `installer/build-windows.ps1` |
| `rustc` | `rustc 1.100.0 (0ef1bc386 2026-11-09)` |
| `cargo` | `cargo 1.100.0 (3acc79c0c 2026-11-05)` |
| `node` | `v26.12.0` |
| `npm` | `11.20.0` |
| Available | `adb.exe`, `java.exe` |
| Not available on `PATH` | `kotlinc`, `gradle`, `swift` |
| Physical constraints | No second physical PC/tablet; no code-signing certificate |

## Baseline command results

All commands were run from `myMouse/` unless noted. These are fresh results on
the working tree described above.

| Command | Exit | Result / evidence |
| --- | ---: | --- |
| `cargo test --manifest-path backend/Cargo.toml` | 0 | 80 unit tests passed; `clipboard_file_two_instance` 1 passed; `two_instance` 1 passed; 0 failed. Compiler emitted existing dead-code/unused warnings, not errors. |
| `cargo test --manifest-path shared/protocol/Cargo.toml` | 0 | 9 tests passed, 0 failed; 0 doc-tests. |
| `npm test -- --run` | 0 | 5 files / 19 tests passed; non-fatal Vite CJS Node API deprecation warning. |
| `npm run build` | 0 | `tsc --noEmit`, Vite production build, and Electron TypeScript build succeeded. |
| `npm run smoke` | 0 | `OK: pc-a forwarded one crossing to pc-b over pinned TLS; pc-b received one InputEvent.` |
| `npm run benchmark` | 0 | Wrote `docs/benchmarks.md`; input 8.9 ms, clipboard 15.688 Mbit/s, file 15.307 Mbit/s, connect 121.46 ms, idle CPU 0/0%, idle RAM 12.07/11.81 MiB, reconnect `null`/unmeasured. |
| `cargo fmt --all -- --check` (from `myMouse/backend`) | 1 | Formatting diffs remain. This was already red at baseline `cc1f6b5`; no repo-wide reformat was performed. |
| `git diff --check` (repo root) | 0 | No whitespace errors; only pre-existing LF→CRLF normalization warnings. |
| `(Get-Process mymouse-backend \| Measure-Object).Count` | n/a | `0`; smoke/benchmark left no backend process behind. |

`npm run benchmark` modified `myMouse/docs/benchmarks.md` during Phase 0. The
new values supersede the older numbers still quoted in `docs/phase1-audit.md`;
that documentation drift must be corrected in the next documentation batch.

## Implementation inventory at baseline

### Implemented and unit-tested

- Persistent device identity/name; stable device ID in `Hello`, discovery and RPC.
- Versioned v2 binary protocol with byte-budgeted decoding and malformed-frame
  rejection (`shared/protocol`).
- Pinned mutual-TLS transport: TLS 1.3-only configuration, server pin checking,
  client certificate verification, trusted-device Hello gate.
- Pairing state machine: single-use six-digit code, full fingerprint, explicit
  approval, trust persistence, capability grants and revocation.
- Input model: key down/up, mouse buttons including X1/X2, wheel, stable key
  vocabulary, hotkey rejection from the wire, injected-event filtering.
- Topology layout validation/persistence and one-peer edge crossing handoff.
- Clipboard and file managers: bounds, manual approval, exact chunk offset and
  sequence checks, completion size and SHA-256 verification, partial cleanup.
- Injected-input tracker, one-second `Status` heartbeat, three-second silence
  watchdog, one-shot `inputLost` emission, and release-before-stop shutdown.
- Electron/React UI, IPC bridge, device management, layout view, clipboard and
  file controls, and frontend/Electron tests.

### Integration / harness-tested

- Two real backend processes exchange a synthetic crossing over pinned TLS.
- Clipboard text and exact file bytes travel through the pinned v2 transport.
- Two-process loopback benchmark measures input delivery, clipboard/file
  throughput, cold connect and idle resource use. It completes a 1 MiB
  multi-chunk transfer.
- Windows packaging and artifact verification were reported in the prior
  audit; not re-run in this Phase 0 pass.

### Physically tested

- None. No second physical PC/tablet was available. There was no real mDNS
  discovery, pairing between devices, keyboard/mouse handoff, multi-monitor or
  DPI validation, Wi-Fi interruption, or physical silent-peer/held-key test.

### Not implemented or not verified

- Return-to-local re-entry when the cursor crosses back from the peer.
- Per-monitor DPI compensation and robust mixed-DPI coordinate mapping.
- Replay protection (nonces/timestamps) for input frames.
- General multi-peer routing and a drag-and-drop topology editor.
- Image clipboard end-to-end harness test.
- Remote/live screen streaming.
- Android APK build/device test; macOS Swift build/runtime test.
- Signed Windows release; autostart remains off by default.
- Hardware-level proof of Windows `SendInput` release and live TLS 1.2 rejection.
- Production reconnect-time measurement; harness reconnect is unmeasured.
- Field readiness against any upstream product; no upstream comparison was run.

## Baseline defects and drift recorded before Phase 1

1. **Pending commit**: the four second-review fixes are uncommitted. They must
   be committed and re-reviewed before any new implementation batch.
2. **Documentation drift**: `docs/phase1-audit.md` quotes benchmark values from
   an earlier run; the fresh Phase 0 run changed `docs/benchmarks.md`.
3. **Formatting hygiene**: `cargo fmt --all -- --check` is red at the historical
   baseline. It is not a new regression, but it prevents a clean lint gate.
4. **Compiler warnings**: backend has existing dead-code/unused warnings. They
   are not failures, but they are an audit signal for dead or unfinished paths.
5. **Physical verification gap**: all cross-device, OS-injection, discovery,
   pairing, topology and network-interruption claims remain at source/test or
   loopback-harness level only.

## Phase 1 dispatch constraints

Phase 1 must use independent domains with no overlapping file ownership. No
agent may claim physical verification. Every finding must include file paths,
reproduction steps, severity, root cause, proposed fix, required tests and
dependencies. Findings must be labelled confirmed defect, suspected defect,
missing functionality, platform limitation, or improvement opportunity.
