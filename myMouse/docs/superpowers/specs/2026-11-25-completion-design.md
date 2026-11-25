# myMouse completion round — design note (2026-11-25)

## Intent

The person wants myMouse to actually replace their OneMouse setup, not just
have source files for it. The specific gap after Phase 1–4 was that clipboard
sync and file transfer had protocol messages, local state managers and UI, but
no peer-to-peer delivery; Android had a scaffold with a stub bridge; Mac was a
source-only scaffold. This round closes the first two gaps as far as this host
allows and records the remaining honest limits.

The user explicitly opted out of a design conversation ("make everything"), so
the choices below are the implementer's calls, stated plainly.

## Goals

- Clipboard updates and file transfers travel between two Windows backends over
  the existing pinned-TLS transport, and are observable by the UI and tests.
- The Android app uses the project's wire format and certificate pinning rather
  than a placeholder bridge; the stub remains only as a labelled fallback.
- Existing Windows input forwarding, pairing and smoke tests keep passing.
- Every claim in docs and chat matches what was actually verified.

## Non-goals / anti-goals

- No new protocol: `shared/protocol` v2 stays the single wire format.
- No weakening of pinned TLS, no silent trust escalation, no autostart.
- No claim that an APK, Mac app, or real two-PC/tablet session was built or
  verified on hardware from this Windows host.
- Remote/live screen remains out of scope for this round.

## Constraints found in the code

- `backend/src/main.rs` has JSON-RPC over stdio for settings, transfers and
  clipboard read, but never sends a `FileTransferOffer` and never processes
  inbound messages.
- `backend/src/transport.rs` already provides `Server`, `Client`, and
  `PeerStream` with length-prefixed pinned TLS. `backend/src/test_mode.rs`
  already runs two real instances with a fixed test certificate and emits
  stdout JSON notifications.
- `electron/bridge.ts` can already route unsolicited backend stdout JSON lines
  to a notification callback, but `electron/main.ts` does not subscribe, and
  the React app does not listen for updates.
- Android's `BridgeProvider.kt` always returned `StubMyMouseBridge`; the Rust
  `shared/android-bridge` crate compiled and had 8 passing tests before this
  round.
- No Android SDK/NDK/Gradle and no Swift toolchain are installed here.

## Approaches considered

1. **Reuse the existing Rust transport and test-mode harness (chosen).**
   Add a peer session component that owns outbound/accepted `PeerStream`s and
   dispatches v2 `Message`s into the existing clipboard/file managers. Extend
   the same stdio JSON-RPC used by Electron and the two-instance test. This
   keeps one protocol, one crypto path, and gives deterministic delivery tests.
2. Separate WebSocket/HTTP relay for clipboard and files. Rejected: a second
   transport would duplicate trust, framing and firewall behaviour and would
   not match the existing pinned-TLS pairing model.
3. Kotlin-only / Swift-only implementations for Windows too. Rejected: it would
   fork the protocol and leave the Windows backend and UI out of sync.

For Android, a native Kotlin TLS client using the same v2 framing was chosen
over waiting for UniFFI/NDK bindings that cannot be generated on this host.
For Mac, the existing Rust-FFI-preferred plan stands; source-only Swift work
cannot be compiled here, so no unverified runtime claims will be made.

## Implementation split

- **Agent A — Windows delivery:** `myMouse/backend/` only. Peer session,
  outbound sends from RPC, inbound dispatch, notifications, and a two-instance
  integration test for clipboard + small file transfer.
- **Agent B — Android bridge:** `myMouse/apps/android/` and
  `myMouse/shared/android-bridge/` only. Real Kotlin bridge with pinned TLS and
  v2 framing, native-library preference, safe accessibility injection path,
  Rust unit tests, honest READMEs.
- **CEO — integration:** Electron notification subscription, React refresh on
  transfer/clipboard notifications, full test/build/packaging run, docs,
  commit and push.

## Verification standard

Before claiming completion: backend tests (including both two-instance tests),
android-bridge tests, frontend tests, typecheck/build, Windows packaging and
artifact verification must all pass on this host. Anything requiring a second
physical PC, an Android device, or macOS must be listed as unverified, not
reported as working.
