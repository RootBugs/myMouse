# Production-grade remediation design — 2026-11-27

## Intent

The autonomous mandate requires evidence-backed production hardening of
`myMouse`, not a feature expansion. Four Phase 1 investigations and one fresh
verification review have now reported. The verified six-commit checkpoint is
pushed to `origin/master` at `655166b`. This design turns the confirmed
findings into ranked, testable work while preserving the existing architecture.

The human partner explicitly required autonomous execution with no
clarification round-trip. That instruction replaces the normal brainstorming
approval gate: this document records the design decisions made under that
authority, and implementation proceeds only after a written plan and TDD red
evidence for each batch.

## Evidence labels used here

- **Static**: derived from source inspection only.
- **Node-level**: executed against Node/mock processes, not a GUI.
- **Unit-tested**: executed by Rust/Vitest unit tests.
- **Integration-tested**: executed by the two-process harness.
- **Physically tested**: requires real machines/OS input; not claimed here.
- **Source-only**: code exists but could not be compiled or run on this host.
- **Not implemented**: no runtime code exists.

All findings below are Static or Node-level unless explicitly labelled
otherwise. No physical, Android-device, macOS, two-PC, mDNS, signing, or
live-TLS-downgrade claim is made.

## Goals

1. Eliminate confirmed reliability and authorization defects that can strand UI
   callers, bypass capabilities, misroute data, or corrupt transfer state.
2. Keep one versioned protocol (`shared/protocol` v2) and one pinned mutual-TLS
   transport. No protocol bump, second transport, or mock-only success.
3. Preserve every working implementation: fix at the source, do not remove
   features to make tests pass.
4. Prove each fix with a failing test first, then a minimal fix, then the full
   backend/protocol/frontend suites, build, smoke, and whitespace check.
5. Record honest status for findings that require unavailable toolchains or
   hardware.

## Non-goals / anti-goals

- No new wire message and no protocol version bump.
- No remote-screen implementation; the capability remains exposed but is
  documented as not implemented until a real capture/stream path exists.
- No Android APK, Swift build, signing certificate, autostart enablement, or
  physical two-PC validation on this host.
- No invented latency/throughput numbers, no superiority claims, no claims that
  harness tests prove production TLS/pairing behavior.
- No repo-wide `cargo fmt` reformatting: the formatting gate is red at the
  baseline commit and remains a deferred pre-existing issue.

## Approaches considered

### A. Broad redesign (rejected)

Replace the v2 codec, transport, or runtime with a new protocol/transport. This
would break the single-protocol constraint, invalidate existing harness and
pairing work, and cannot be verified physically here. It does not fit the
mandate.

### B. Targeted hardening inside the existing architecture (chosen)

Keep v2 framing, pinned mutual TLS, Tokio runtime, Electron bridge, and React UI.
Fix each confirmed defect at its source, bind authority to the authenticated
peer and connection generation, add narrow regression tests, and preserve the
public RPC surface. This is verifiable with the existing unit/integration/Node
harnesses and keeps the blast radius small.

### C. Documentation-only deferral (rejected)

Record findings without code changes. This would leave confirmed Critical and
High defects in the production path, contrary to the mandate. Documentation is
used only for genuinely blocked items (hardware, signing, missing toolchains).

## Ranked findings and rulings

### P0 — fix before release readiness

| ID | Source | Ruling | Fix |
| --- | --- | --- | --- |
| F1 | Frontend / archaeology CD-1 / adversarial QA | Confirmed Critical (Node-level): backend exit/crash leaves IPC promises pending forever; missing executable raises unhandled `ENOENT`. | `Bridge` rejects all pending requests on process exit and per-request timeout; validates replies strictly; `main.ts` installs `error`/`exit` listeners and refuses calls after backend death. |
| F2 | Security / adversarial QA D1 | Confirmed HIGH (Static): `FileTransferAccept/Reject/Cancel` bypass `files=false` and are not peer-bound; `None` capabilities still admit control messages. | All file message classes require `files`; dispatcher checks transfer ownership before mutating state. |
| F3 | Adversarial QA D2 | Confirmed MEDIUM-HIGH (Static): peer disconnect skips transfer cleanup when another peer remains; `.part` files and history can strand. | Per-peer terminalization of incoming and outgoing transfers on disconnect; remove only that peer's partial files. |
| F4 | Adversarial QA D3 | Confirmed MEDIUM (Static): duplicate/attacker-chosen transfer IDs overwrite history and orphan partial files. | Reject duplicate IDs and protect history direction/peer invariants. |
| F5 | Adversarial QA D4 / security | Confirmed MEDIUM (Static): concurrent `connectPeer` can deliver `Hello` to the replacement peer; RPC default sends can fall through to arbitrary accepted peers. | Connection-generation-bound `Hello` send; active-peer-only default routing; targeted `offerFile` send so the history peer and frame destination cannot diverge. |
| F6 | Security / archaeology CD-4 | Confirmed HIGH (Static): Android pairing protocol has no backend `PairCode`/`PairApprove` handler; no client certificate is presented by the Kotlin bridge. | Backend handles `PairCode` only from an authenticated TLS peer whose Hello identity and presented client certificate are bound to a live pairing session; emits `PairApprove`/`PairReject`. Kotlin client-cert provisioning remains source-only/blocked and is reported as such. |
| F7 | Protocol D1 / security | Confirmed (Static): maximum clipboard image can exceed the 1 MiB frame cap and drop the connection. | Bound raw image bytes and base64 length so the encoded frame provably fits the transport cap; enforce on both outbound and inbound. |
| F8 | Protocol D2/D3 | Confirmed (Static): Rust accepts trailing bytes and full `u64`, Kotlin rejects both. | Rust decoder requires full body consumption and rejects values outside signed 64-bit range for fields mirrored as Kotlin `Long`. |
| F9 | Protocol D4 / security | Confirmed (Static): clipboard `device_id` is self-asserted. | `accept_remote` takes the authenticated frame peer ID and rejects mismatches; UI notification uses the authoritative peer ID. |

### P1 — fix in the same mandate after P0 is green

| ID | Source | Ruling | Fix |
| --- | --- | --- | --- |
| F10 | Protocol D7 | Confirmed (Static): a duplicate chunk marks an otherwise valid transfer failed. | Idempotent replay of already-written bytes after content comparison; conflicting replay fails safely. |
| F11 | Adversarial QA D5 | Confirmed Low-Medium (Static): watchdog notification can arm without a release; `input_source` survives disconnect. | Arm notification only when a release occurred; clear input ownership on peer removal. Actual OS release remains physically untested. |
| F12 | Archaeology CD-2 | Confirmed (Static): production topology hardcodes local monitor index `0`. | Resolve the local node's persisted `monitorIndex` from `Layout`, with a validated fallback. Physical multi-monitor behavior remains untested. |
| F13 | Archaeology SD-1 | Suspected (Static): Windows clipboard text scan ignores `GlobalSize`. | Bound the UTF-16 scan by the allocated size and test the pure slice helper. Hostile clipboard-owner behavior remains untested. |
| F14 | Archaeology CD-6 | Confirmed (harness safety, Static): test mode writes the real config directory. | Add explicit temp config override for test mode and verify the production config path is untouched. |
| F15 | Adversarial QA observation | Confirmed: default `npm test` enters watch mode and exited 1 with `EBUSY` while watching the backend binary. | Add a one-shot `test:ci` script; keep the interactive default unchanged. |

### Deferred and explicitly not claimed

- Remote screen/live video: **Not implemented**; do not enable the capability UI
  as if it worked.
- Replay protection/nonces: **Not implemented**; rate limiting is not replay
  protection.
- General multi-peer routing, drag-and-drop topology editor, return-to-local
  re-entry, mixed-DPI compensation: **Not implemented / partial**.
- Android APK, client-certificate provisioning, UniFFI generation, Android
  clipboard UI and device pairing: **Source-only or not verified**; no
  Android SDK/NDK/Gradle/`kotlinc` on this host.
- macOS companion: **Source-only scaffold**; transport and screen capture
  throw; no Swift toolchain.
- Signed Windows release and macOS notarization: **Not implemented**; no
  signing certificate.
- Reconnect-time benchmark remains `null`/unmeasured.
- Physical two-PC handoff, mDNS, QR camera, Wi-Fi interruption, real silent-peer
  input release, multi-monitor/DPI: **not tested**.
- `cargo fmt --all -- --check` remains red from the pre-existing baseline; no
  repo-wide reformatting in this mandate.

## Design details

### 1. Electron IPC lifecycle

`Bridge` gains a lifecycle contract: `send()` registers a pending entry with a
timeout; `onExit(cb)` rejects every pending promise with a typed error and
prevents new sends after death; parsed lines must be JSON objects with an
integer `id` and an `ok` result object. Duplicate replies for a consumed ID are
notified as malformed, never resolved twice. Non-object JSON and primitive
payloads are ignored or reported as notifications without throwing.

`main.ts` attaches `error` and `exit` listeners to the spawned backend, wires
exit into `Bridge`, emits a user-visible notification, and makes the IPC handler
reject cleanly when the bridge is gone. Windows/macOS lifecycle stays intact:
the `activate` handler recreates the window on macOS; normal platforms keep the
existing quit path.

### 2. Peer-bound runtime authority

The runtime keeps one authoritative active peer for unaddressed sends. It never
selects an arbitrary `HashMap` key. Explicit targets are matched by device ID;
generation-checked sends are used for connection setup so a replacement peer
cannot receive another connection's `Hello`.

For inbound messages, every file message is gated by the `files` capability and
by transfer ownership from `FileTransferManager`: a peer may act only on a
transfer whose record names that peer. Authorization occurs before any state
mutation. Outgoing file offers are sent with the same explicit target recorded
in history.

### 3. Transfer state integrity

Duplicate transfer IDs are rejected before insertion. Disconnecting a peer
terminalizes only that peer's active transfers and removes only that peer's
partial files. Duplicate chunks whose bytes match what is already on disk are
idempotent; conflicting bytes fail the transfer. All terminal outcomes are
pushed to history and emitted once.

### 4. Protocol and clipboard parity

Rust `decode` requires the cursor to consume the entire body and rejects
`u64` values above `i64::MAX` for fields mirrored as Kotlin `Long`. Clipboard
image bounds are lowered so the encoded frame is below the 1 MiB transport cap
with room for all string fields. Remote clipboard payload device IDs must equal
the authenticated peer ID.

### 5. Pairing binding

The backend accepts `PairCode` only on a live authenticated peer connection.
It consumes one unused pairing session, verifies the code, and verifies that the
peer's Hello device ID and client-certificate fingerprint match the session's
recorded values. `PairApprove` is sent only after those checks; otherwise
`PairReject` is sent. The manual `approvePairing` RPC additionally requires the
supplied peer ID to equal the device ID recorded when the pairing session was
created. This removes the current arbitrary-peer/fingerprint binding gap.

Because the Android bridge does not present a client certificate and no Android
toolchain is available, the Android-side half remains source-only and is
reported as unimplemented in this environment; no pairing claim is made.

## Builder-check rulings

A fresh engineer review found five ambiguities. Under the no-question
directive they are settled here:

1. **No pre-auth bootstrap path.** `PairCode` is accepted only on a connection
   that already passed the pinned trusted-Hello gate. First-time Android
   pairing remains source-only and unimplemented here; the single pinned
   mutual-TLS transport is not relaxed. Cost: no Android first-pair claim.
2. **Session binding is recorded when the joining peer is selected.** The
   existing `pairingUri`/`approvePairing` flow may gain the peer ID/fingerprint
   fields it already needs at the UI boundary, but no wire field changes. Cost:
   a small UI/RPC signature adjustment.
3. **Successful code verification acknowledges only.** A valid code emits
   `PairApprove` only after explicit approval has been recorded. It does not
   itself persist trust, a pin, or attach the connection. Cost: Android flows
   expecting automatic trust still cannot be claimed.
4. **Strict IPC validation requires a present, non-null `result` object, not a
   boolean `ok`.** Backend successes are heterogeneous arrays/objects; errors
   are `result: { ok: false, error }`. Bridge rejects only explicit error
   results. Cost: no normalization of existing RPC payloads.
5. **The 1 MiB bound is enforced at the encoded-frame boundary in the protocol
   crate, not by per-string caps.** Oversized inbound frames keep the existing
   transport behavior (decode failure closes the stream); oversized outbound
   frames fail before write. Cost: unusually large text/metadata strings are
   rejected as oversized frames.

## What is left to the builder

The implementation plan translates these batches into bite-sized TDD tasks.
Each batch must show a red test before its fix and a full green suite after.
Any deviation requires an explicit ledger entry with evidence.
