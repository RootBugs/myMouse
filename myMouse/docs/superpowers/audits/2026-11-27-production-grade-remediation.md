# myMouse — production-grade remediation audit, 2026-11-27

## Scope and authority

This is the final evidence record for the autonomous production-grade
remediation mandate. The human partner required autonomous execution with no
clarification round-trip, so design ambiguities were ruled in the
[design spec](../specs/2026-11-27-production-grade-remediation-design.md) and
[implementation plan](../plans/2026-11-27-production-grade-remediation.md)
rather than by asking questions. The verified six-commit checkpoint pushed to
`origin/master` is `655166b`. Work then proceeded directly on `master`, one
small commit per task, with one fresh read-only whole-branch review at the end.

No second protocol and no second transport were introduced. `shared/protocol`
remains v2; the pinned mutual-TLS transport remains the only transport. No mock
replaces real functionality with hardcoded success; the only test doubles are
deterministic input-release and IPC seams.

## Evidence labels

| Label | Means |
| --- | --- |
| **Static** | Source inspection only; no runtime proof. |
| **Node-level** | Executed under Vitest/Node with test doubles for OS/Electron processes. |
| **Unit-tested** | Executed by a Rust unit test in this host. |
| **Integration-tested** | Executed against two real backend processes or a spawned backend binary. |
| **Harness-tested** | Executed by the PowerShell two-process or config-isolation harness on this Windows host. |
| **Source-only** | Code exists but could not be compiled or run here (missing SDK/toolchain). |
| **Not implemented** | No runtime implementation exists. |
| **Physically tested** | Requires real machines/OS input. **No finding in this report has this label.** |

## Commit chain

| Commit | Subject |
| --- | --- |
| `655166b` | Verified checkpoint, pushed before remediation. |
| `bb685b8` | `fix: reject stale Electron IPC calls on backend lifecycle changes` |
| `556ed88` | `fix: bind file transfer messages to capability and peer ownership` |
| `5f8bc12` | `fix: keep transfer identity and cleanup peer-bound` |
| `80b9af0` | `fix: bind outbound sends to the selected peer connection` |
| `e72156a` | `fix: align protocol validation and bind clipboard identity` |
| `4f08b60` | `fix: bind pairing approval to the authenticated peer` |
| `64d3c7b` | `fix: correct liveness state and platform buffer bounds` |
| Final commit | Harness config isolation, status docs, this audit, and all final-review fixes R1–R8. Pushed SHA is verified with `git rev-parse HEAD origin/master` after the push. |

The final pushed SHA is verified with `git rev-parse HEAD origin/master` after
the push, not before.

## Per-finding rulings

Each row was fixed at its source after a watched failing test. "Unit/integration"
below is an evidence label, not a promise of physical behavior.

| ID | Source severity / label | Ruling and fix | Evidence | Commit |
| --- | --- | --- | --- | --- |
| F1 | Critical, Node-level: backend exit/crash left IPC promises pending; missing executable produced unhandled `ENOENT`. | `Bridge` rejects pending sends on exit/timeout, accepts any present non-null `result` (scalar or object), classifies notifications before reply-ID validation, ignores malformed/duplicate replies; `main.ts` installs `error`/`exit` handlers and refuses calls after death. Refined by review R1/R2. | Node-level + unit-tested; `npm run test:ci` green | `bb685b8`, final commit |
| F2 | High, Static: file control messages bypassed `files=false` and were not peer-owned. | All `FileTransfer*` variants require `files`; accept/reject/chunk/complete check record ownership before mutating state. Cancel accepts either side's ownership of a peer-bound record (review R7) because both sender and receiver may end a transfer. | Unit-tested | `556ed88`, final commit |
| F3 | Medium-high, Static: disconnect cleanup skipped a peer when another remained; `.part` files and history stranded. | `fail_peer_transfers` terminalizes only the removed peer's records and partials, on explicit disconnect and stale `ConnectionClosed` with no live replacement. | Unit-tested | `5f8bc12`, `64d3c7b` |
| F4 | Medium, Static: duplicate/attacker-chosen transfer IDs overwrote history. | Duplicate active IDs are rejected before insertion; direction/peer invariants protected. | Unit-tested | `5f8bc12` |
| F5 | Medium, Static: concurrent `connectPeer` could deliver `Hello` to a replacement; default sends could hit an arbitrary accepted peer. | `ConnectionTicket { peer_device_id, generation }`; generation-bound sends; active-peer-only default routing; targeted `offerFile` so history and frame destination agree. | Unit-tested + integration-tested | `80b9af0` |
| F6 | High, Static: no backend `PairCode` handler; pairing authority not bound to the authenticated peer. | Backend handles `PairCode` only on an already pinned/trusted authenticated peer; session peer ID and client-certificate fingerprint must match; `PairApprove` follows explicit approval and acknowledges only; it never persists trust or pin by itself. Android client-certificate provisioning remains source-only/unverified. | Unit-tested | `4f08b60` |
| F7 | Confirmed, Static: maximum clipboard image could exceed the 1 MiB frame cap. | Raw image bytes and base64 length bounded; inbound oversize keeps existing decode-failure/close behavior. Cap enforced at the encoded-frame boundary in the protocol crate and again in `write_frame` before header/body write (review R6). | Unit-tested | `e72156a`, final commit |
| F8 | Confirmed, Static: Rust accepted trailing bytes and full `u64` values that Kotlin rejects. | Decoder requires full body consumption and rejects values above `i64::MAX` for fields mirrored as Kotlin `Long`. | Unit-tested | `e72156a` |
| F9 | Confirmed, Static: clipboard `device_id` was self-asserted. | `accept_remote` uses the authenticated frame peer ID and rejects mismatches; notifications use the authoritative ID. | Unit-tested + integration-tested | `e72156a` |
| F10 | Confirmed, Static: duplicate chunk marked a valid transfer failed. | Identical replay after byte comparison is idempotent; conflicting replay fails safely. Replay comparison uses `read_exact` behind a reader seam so a legal short read is not a conflict (review R8). | Unit-tested | `5f8bc12`, final commit |
| F11 | Low-medium, Static: watchdog could arm without a release; `input_source` survived disconnect. | Release seam returns whether input was actually held; notice armed only on a real release; peer removal releases/forgets ownership; stale close cleanup requires no live replacement. | Unit-tested; OS release physically untested | `64d3c7b` |
| F12 | Confirmed, Static: production topology hardcoded local monitor `0`. | Lowest persisted local node index from `Layout`, fallback `0`. | Unit-tested; multi-monitor physically untested | `64d3c7b` |
| F13 | Suspected, Static: Windows clipboard scan ignored `GlobalSize`. | Pure `utf16_prefix_len` bounds the scan by byte length, terminator, and char cap; Windows reader uses `GlobalSize` before reading. | Unit-tested via pure helper; hostile clipboard owner untested | `64d3c7b` |
| F14 | Confirmed, Static/harness: test mode wrote the real config directory. | `test_config_dir()` honours explicit `MYMOUSE_CONFIG_DIR`, otherwise uses `<temp>/myMouse-test/<validated-device-id>`; production root is never consulted by test mode. | Integration/harness-tested | Task 8 commit |
| F15 | Confirmed: default `npm test` entered watch mode and exited 1 with `EBUSY`. | One-shot `npm run test:ci` script added; interactive default unchanged. | Harness-tested | `bb685b8` |

## Builder-check rulings carried into code

1. **No pre-auth pairing bootstrap.** `PairCode` is accepted only on a
   connection that already passed the pinned trusted-Hello gate. First-time
   Android pairing is source-only/unverified here; the transport is not relaxed.
2. **Session binding recorded when the joining peer is selected.**
   `pairingUri(peerId, fingerprint)` and `approvePairing` bind the expected peer
   ID and fingerprint at the UI/RPC boundary. No wire field changed.
3. **Successful code verification acknowledges only.** A valid code emits
   `PairApprove` only after explicit approval; it does not persist trust, a pin,
   or attach a connection.
4. **IPC validation requires a present, non-null `result`**, not a boolean
   `ok`. Successes may be strings, arrays or objects; errors remain
   `result: { ok: false, error }`. The earlier object-only rule wrongly dropped
   the scalar `pairingUri` reply (review R1); notification frames are classified
   before reply-ID validation (review R2).
5. **The 1 MiB bound is enforced at the encoded-frame boundary.** Oversized
   inbound frames keep the existing decode-failure/close behavior; oversized
   outbound frames fail before write.

## Watched RED evidence

Every production fix was preceded by a failing test. The exact observations are
kept in the execution ledger
`.superpowers/sdd/2026-11-27-production-grade-remediation/progress.md`; the
salient failures are repeated here so the report stands alone.

| Task | Focused command | Watched RED observation |
| --- | --- | --- |
| 1 | `npx vitest run electron/bridge.test.ts` | 3 failed / 1 passed; exit test resolved instead of rejecting, timeout test hit the 5 s harness timeout, malformed reply threw `Cannot read properties of null (reading 'id')`. `electron/main.test.ts` failed `childHandlers.has('error') === false`. |
| 2 | `cargo test --manifest-path backend/Cargo.toml network::tests::control_messages_require_files_and_ownership -- --exact` | Capability assertion failed; after the first fix the test showed `left: Cancelled / right: Offered` for a `pc-b` cancel of a `pc-c` offer. |
| 3 | `cargo test --manifest-path backend/Cargo.toml file_transfer::tests::duplicate_offer_id_is_rejected -- --exact` and companion tests | `a reused active transfer id must be rejected: []`; duplicate chunk failed `"chunk is out of sequence"`; peer cleanup returned `left: 0 / right: 1`; network disconnect left `pc-b` `Offered` instead of `Cancelled`. |
| 4 | `cargo test --manifest-path backend/Cargo.toml network::tests::hello_is_bound_to_its_connection_generation -- --exact` | `default_send_never_uses_arbitrary_accepted_peer` failed `left: Some(Accepted(1)) / right: None`; bound `Hello` failed `a Hello bound to a replaced connection must fail`; production offer timed out on the recorded active peer. |
| 5 | `cargo test --manifest-path shared/protocol/Cargo.toml` plus fully qualified clipboard/backend tests | Protocol suite 9 passed / 2 failed: trailing bytes decoded as `Ok(Hello { version: 2, device_id: "pc-a", name: "A" })` and `timestamp_ms: 9223372036854775808` decoded as `Ok(ClipboardUpdate(...))`. Clipboard failed `an image above the bound must be rejected before it can break the frame cap`; remote clipboard failed `a self-asserted device ID must not override the authenticated peer`. |
| 6 | Fully qualified network and main tests plus `npx vitest run src/DeviceManagement.test.tsx` | Pairing tests failed `the peer must receive one pairing verdict: Empty`; `a peer outside the session must not gain trust`; `the approved session must remain available for the PairCode acknowledgement`; frontend failed `expected "spy" to not be called at all, but actually been called 1 times`. |
| 7 | Fully qualified network/main/clipboard tests | Watchdog failed `no held input means there is no watchdog notice to arm`; disconnect tests failed `left: Some("pc-b") / right: None`; monitor index failed `left: 0 / right: 2` and `left: 0 / right: 1`; four UTF-16 helper cases failed against a permissive `Some(0)` surface with `Some(0)/None`, `Some(0)/Some(2)`, `Some(0)/None`, `Some(0)/Some(3)`. |
| 8 | `cargo test --manifest-path backend/Cargo.toml test_mode_does_not_touch_production_config -- --exact` | Exit 101; `test test_mode_does_not_touch_production_config ... FAILED`; panic `test mode must use its own harness config directory when no override is set`; child logged `warning: corrupt trust store at ...\appdata\myMouse\trust.json`, proving production config was consulted. |
| R1 | `npx vitest run electron/bridge.test.ts -t 'resolves a scalar pairing URI result from the real backend shape'` | Exit 1; `promise rejected "Error: IPC request 'pairingUri' timed out after 50ms" instead of resolving`; the scalar URI was emitted as `malformedIpcMessage`. |
| R2 | `npx vitest run electron/bridge.test.ts -t 'delivers a discoveredPeer notification with a string device id'` | Exit 1; expected the six-field discovery notification but received the two-field `malformedIpcMessage`. |
| R3 | `cargo test --manifest-path backend/Cargo.toml network::tests::targeted_non_handoff_send_reports_a_dead_writer -- --exact` and `tests::offer_file_marks_history_failed_when_targeted_send_fails -- --exact` | Exit 101; `a file offer with a dead writer must not be reported as sent`; `left: Offered / right: Failed`. |
| R4 | `cargo test --manifest-path backend/Cargo.toml transfer_targets_the_offering_peer` | Exit 101; both accept/reject tests timed out waiting for the verdict on the offering peer. A first GREEN attempt hit `The file exists. (os error 80)` from a stale global `.part` file; fixtures were isolated and the one test-generated artifact removed. |
| R5 | `cargo test --manifest-path backend/Cargo.toml network::tests::refresh_active_peer_never_guesses_among_accepted_peers -- --exact` | Exit 101; `left: Some("pc-b") / right: None`. |
| R6 | `cargo test --manifest-path backend/Cargo.toml transport::tests::oversized_outbound_frame_is_rejected_before_write -- --exact` | Exit 101; `an encoded frame above the 1 MiB cap must fail before it is written`. |
| R7 | `cargo test --manifest-path backend/Cargo.toml network::tests::sender_can_cancel_an_incoming_transfer -- --exact` | Exit 101; emitted `messageRejected / transfer ownership mismatch` and `left: Offered / right: Cancelled`. |
| R8 | `cargo test --manifest-path backend/Cargo.toml file_transfer::tests::duplicate_chunk_replay_tolerates_short_reads -- --exact` | Exit 101; `a legal short read of matching bytes must not be treated as a conflict`. |

Plan-command qualifications are part of the evidence: Tasks 2–7 needed fully
qualified Rust test names because the plan's unqualified `--exact` filters ran
0 tests. Task 8's integration test name is unqualified, so the plan command
worked exactly as written. `cargo test` defaults to watch mode for `npm test`,
so `npm run test:ci` is the one-shot frontend command used everywhere.

## Final verification matrix

All commands were run from `myMouse/` (except the repo-root whitespace check)
after the final source state, including all review fixes. Final logs are
`final-backend.log`, `final-protocol.log`, `final-npm-test.log`,
`final-npm-build.log` and `final-npm-smoke.log` under
`C:\Users\hp\AppData\Local\Temp\opencode\`; the Task 8 pre-review logs remain
under `task8-final\`.

| Command | Exit | Result |
| --- | ---: | --- |
| `cargo test --manifest-path backend/Cargo.toml` | 0 | `115 passed; 0 failed` unit tests; `1 passed; 0 failed` in each of `clipboard_file_two_instance`, `test_mode_config`, `two_instance`. |
| `cargo test --manifest-path shared/protocol/Cargo.toml` | 0 | `11 passed; 0 failed`. |
| `npm run test:ci` | 0 | `6 files / 26 tests passed` (duration 5.70 s). |
| `npm run build` | 0 | Vite production build (`built in 1.09s`) and `tsc -p tsconfig.electron.json` succeeded. |
| `npm run smoke` | 0 | `OK: pc-a forwarded one crossing to pc-b over pinned TLS; pc-b received one InputEvent.` |
| `npm run benchmark` (pre-review source state) | 0 | Wrote the two-process loopback sample to `docs/benchmarks.md`; input 30.67 ms, clipboard 5.342 Mbit/s, file 4.21 Mbit/s, connect 339.48 ms, idle CPU 0/0%, idle RAM 12.13/11.9 MiB, reconnect `null`. Not rerun after review fixes because the script rewrites that document; the numbers are a host-specific sample, not a product claim. |
| `git diff --check` (repo root) | 0 | No whitespace errors; only pre-existing LF→CRLF normalization warnings on touched Rust/Markdown files. |

Compiler emitted pre-existing dead-code/unused warnings; they are not failures.
`npm run dist` and `installer/verify.ps1` were green in an earlier phase of the
same day, not after this remediation; Windows packaging is therefore
**earlier-harness evidence**, not freshly re-verified here.

## Fresh whole-branch review

A single fresh-context, read-only reviewer was dispatched over the whole branch
after source completion. It reported four Important findings (R1–R4), four
Minor findings (R5–R8), and one repository-state item (R9). All eight code
findings were **accepted and fixed at source** with a watched RED test, a
minimal fix and a GREEN rerun. Final re-grade: R1 is Node-level-tested; R2 is
Node-level-tested; R3 and R4 are unit-tested plus integration/harness-tested;
R5–R8 are unit-tested. None of these has a physical or field evidence label.

| ID | Confirmed defect | Source fix | RED → GREEN evidence |
| --- | --- | --- | --- |
| R1 (Important) | `pairingUri` returns a scalar JSON string; `Bridge` rejected primitives, so the pairing QR promise timed out. | Accept any present non-null `result` for success; keep strict object shape only for `ok: false` errors. | RED exit 1 with IPC timeout after 50 ms and `malformedIpcMessage`; GREEN `npx vitest run electron/bridge.test.ts` exit 0, 6/6 after R2 (5/5 at R1). |
| R2 (Important) | `discoveredPeer` notifications carry a string `id`; non-integer `id` was classified as malformed and dropped. | Classify `notification` frames before integer reply-ID validation. | RED exit 1 expected six-field discovery notification but got two-field `malformedIpcMessage`; GREEN 6/6 bridge tests. |
| R3 (Important) | Every targeted `RuntimeCommand::Send` acked `Ok(())` before queueing; a dead writer could make `offerFile` report success while history stayed `Offered`. | Early ack only for `Message::InputEvent` handoff; propagate missing-target and channel/writer errors for all other sends; mark failed offers terminal in history. Two older handoff tests were updated from `Hello` to `InputEvent` so they still assert best-effort handoff. | RED exit 101 `a file offer with a dead writer must not be reported as sent` and `left: Offered / right: Failed`; GREEN both focused tests exit 0; full suite 111 unit + 3 integration and `npm run test:ci` 6/26. |
| R4 (Important) | Local `acceptTransfer`/`rejectTransfer` used the default active-peer route instead of the offering peer's connection. | Resolve the transfer peer from history before mutation and use explicit `send_to`. | RED both two-peer tests timed out; GREEN 2/2 after isolating `save_dir` per tempdir and using distinct IDs. The first GREEN attempt hit `The file exists. (os error 80)` from a stale global `.part` created by the RED run; the single test-generated file was removed and older harness artifacts were left untouched. |
| R5 (Minor) | `refresh_active_peer` fell back to `connections.values().next()`, silently redirecting default sends to an arbitrary accepted peer. | Keep the previously selected peer only while still live; otherwise clear the active target. | RED exit 101 `left: Some("pc-b") / right: None`; GREEN exit 0; full suite 112 unit + 3 integration. |
| R6 (Minor) | `read_frame` enforced `MAX_FRAME_LEN` but `write_frame` did not. | Reject encoded frames above 1 MiB before writing header or body. | RED exit 101 `an encoded frame above the 1 MiB cap must fail before it is written`; GREEN exit 0; full suite 113 unit + 3 integration. |
| R7 (Minor) | `FileTransferCancel` was hardcoded to outgoing ownership, so the sender's cancel was rejected at the receiver where the record is `Incoming`. | Either-ownership gate for Cancel only; accept/reject stay outgoing-only and chunk/complete incoming-only; capability and peer checks unchanged. | RED emitted `messageRejected / transfer ownership mismatch` and `left: Offered / right: Cancelled`; GREEN exit 0; full suite 114 unit + 3 integration. |
| R8 (Minor) | Duplicate-chunk replay used one `Read::read`, so a legal short read could be misclassified as conflicting bytes. | Extract `replayed_chunk_matches` behind a reader seam; use `read_exact`, mapping only genuine EOF to a conflict. | RED exit 101 `a legal short read of matching bytes must not be treated as a conflict`; GREEN exit 0; final full suite 115 unit + 3 integration. |
| R9 (Repository state) | Task 8 sources/tests/docs were unstaged or untracked; `git commit -am` could silently omit them. | Explicitly stage every intended project file; never stage root sibling projects, archives or screenshots; verify HEAD equals `origin/master` after push. | `git status --short`, explicit `git add`, and post-push `git rev-parse HEAD origin/master` in the final report. |

No additional findings were reported in these areas, and no change was made to
them: pairing session binding and approval semantics, the capability gate,
per-peer cleanup and stale-close handling, generation-bound `Hello`, protocol
decoder full-body consumption and `i64` bounds, UTF-16 `GlobalSize` bounds,
local monitor index selection, watchdog release ownership, and the absence of a
second protocol or transport. The reviewer did not have hardware, Android,
macOS or signing evidence either; those areas stay source-only or unverified as
recorded below.

## Deferred minors and explicitly not claimed

- No final-review finding was deferred: R1–R8 were fixed and regraded above, and
  R9 staging was completed before commit. The review's remaining caveats are
  evidence limits, not accepted defects: no physical two-PC pairing/handoff, no
  Android/macOS runtime, no signing certificate, and a benchmark sample taken
  before the review fixes (not rerun because the script overwrites
  `docs/benchmarks.md`).
- `cargo fmt --all -- --check` is red from the pre-existing baseline; a
  repo-wide reformat was not performed to avoid unrelated churn. Cost: formatting
  gate remains red; `git diff --check` is clean.
- Compiler dead-code/unused warnings remain; they are not new regressions but
  are an audit signal.
- `git diff --check` emits LF→CRLF normalization warnings on Windows; no
  whitespace errors were reported.
- The default `npm test` still enters watch mode and previously exited 1 with
  `EBUSY`; `npm run test:ci` is the supported one-shot command. Cost: developers
  must choose the CI script for non-interactive runs.
- Benchmark host-load variance is not interpreted as a product improvement or
  regression. Reconnect time remains `null`/unmeasured; test mode has no
  auto-reconnect loop.
- Windows packaging was verified earlier, not re-run after this remediation.
- Remote/live screen streaming is **Not implemented**; its capability flag must
  not be presented as working.
- Replay protection/nonces are **Not implemented**; rate limiting is not replay
  protection.
- General multi-peer routing, drag-and-drop topology editor, return-to-local
  re-entry, mixed-DPI/multi-monitor compensation and image clipboard
  end-to-end harness coverage remain partial or **Not implemented**.
- Android APK, client-certificate provisioning, UniFFI generation, Android
  clipboard UI and first-time pairing are **Source-only/unverified**: no Android
  SDK/NDK/Gradle/`kotlinc` on this host.
- macOS companion is a **Source-only** scaffold; transport and screen capture
  throw; no Swift toolchain.
- Signed Windows builds and macOS notarization are **Not implemented**: no
  signing certificate. Autostart stays off.

## Release readiness

This branch is **not field-ready**. It is code-complete for the confirmed
findings and the final whole-branch review, and green under unit, Node-level,
integration and one-host harness evidence, but there is no physical two-PC validation, no real mDNS/pairing between
devices, no hardware `SendInput` audit, no Wi-Fi interruption test, no
multi-monitor/DPI test, no Android/macOS runtime build, and no signed release.
Security defaults are preserved: mutual TLS, fingerprint pinning, explicit
approval, capability permissions and revocable trust; discovery never grants
trust. No superiority, field-latency, upstream-comparison or physical claim is
made.

## Untracked workspace artifacts

`DevCard/`, `DevDash/`, `JSON2Type/`, `JSONDiff/`, `QRForge/`, `SnapClip/`,
`myMouse-project.zip`, `myMouse-source.zip`, `myMouse-product-excellence.zip`,
`mymouse-window.png`, `shot1.png`, `shot2.png` and `test-cert-values.txt` at the
workspace root are untracked sibling projects/archives/screenshots. They are not
part of `myMouse` and were not committed. The stale delivery zip is regenerated
only after the final verified commit and is also not committed.
