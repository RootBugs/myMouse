# myMouse — known limitations

Last accuracy pass: 2026-11-27. Scope statements below distinguish what was
**verified on this Windows host** from what is only source-complete. See
[phase1-audit.md](phase1-audit.md) for the full status inventory.

## Input capture and injection

- **UAC prompts, the secure desktop, and locked sessions are out of scope for v1.** Windows does not deliver low-level input hooks to processes running at a different integrity level or on the secure desktop. myMouse will not inject into those surfaces. Unlock the PC and use the local keyboard/mouse there.
- **Emergency stop** (Dashboard → **Emergency stop**) pauses input capture without unhooking OS hooks and releases held modifiers so control returns to the host. **Resume input** reopens capture cleanly. Hotkey events are rejected from the wire; only a locally captured hotkey can trigger the escape, and no raw keystroke bytes are logged in input notifications.
- Only one input source is active at a time. In topology mode, ordinary locally
  captured events stay local until a configured edge crossing hands off to the
  single connected peer; after handoff, events are forwarded only to that peer.
  Remote events from the peer are applied with `SendInput` only when the gate is
  open, the target exactly matches this device, capability is granted, and the
  250 events/s budget is not exceeded. Injected events are filtered from the
  capture hooks to prevent loops. These gates are code/test verified; no
  hardware-level injection audit was performed here.
- Windows capture/injection now share one key vocabulary: modifiers, navigation
  keys, F1–F24, common special keys, and exact `VK(n)` values (`1..=255`) map to
  stable names in both directions. Key down/up, mouse buttons (including X1/X2),
  and wheel events are represented explicitly. Complex layouts, IMEs, and dead
  keys remain incomplete, and macOS translation still covers only Ctrl→Cmd /
  Alt→Option.
- Mouse injection uses absolute virtual-screen coordinates. Multi-monitor layouts are modelled, but per-monitor DPI scaling at handoff is not yet compensated for.
- Production topology persistence and edge handoff are implemented: `setLayout`
  validates and atomically stores `layout.json`, updates the live hook state, and
  `getLayout` feeds the React view. A captured `MouseMove` crossing sends one
  scaled event to the matching connected peer and then forwards to that peer
  until Escape/local stop. This is unit-tested and wired to the Windows hook
  path, but no live cursor crossing was exercised on two physical machines; the
  two-instance harness still uses a synthetic crossing over pinned TLS.

## Clipboard and file transfer

- Clipboard updates and file transfers now travel between two backends over the
  **existing pinned-TLS v2 transport**. Verified here with a real two-process
  integration test (`backend/tests/clipboard_file_two_instance.rs`) and the
  pinned-TLS smoke script: exact clipboard text and exact file bytes arrive at
  the peer. The UI has a clipboard send control and file offer/accept controls,
  and refreshes automatically on backend notifications.
- Test mode uses a fixed shared test certificate and disables OS clipboard
  writes and OS input injection; it validates protocol delivery and manager
  state, not the host OS clipboard surface.
- Production clipboard/file sends require a connected peer and are sent to
  **exactly one** target, not broadcast to every trusted device. Inbound
  clipboard size and timestamp are bounded, protocol frames use a byte-budgeted
  decoder, only 16 incoming transfers can be active, and `.part` files are
  removed when the last peer disconnects. Chunks require approval, exact offset,
  size and sequence checks, and completion verifies actual file size and SHA-256.
- Remote/live screen streaming is still unimplemented; the screen-related UI
  is a placeholder.

## Platform scope

- The runtime-verified scope is **Windows host ↔ Windows target**, driven by
  the two-instance test harness on one machine. Two separate physical PCs were
  not available here, so no real two-machine session was executed. The
  discovery → pairing → auto-connect flow is implemented in the backend and
  covered by unit/integration tests, but there was no real two-PC/tablet field
  verification of mDNS discovery, pairing, or the subsequent connection.
- Android (Phase 3 scaffold): app sources in `apps/android/` and Rust FFI bridge in
  `shared/android-bridge/` implement pairing, trust storage, accessibility service
  scaffolding, notification helper, file transfer UI, remote screen placeholder,
  settings, and v2 pinned-TLS/wire contracts. No APK was built and no
  emulator/device test was run because Android SDK/NDK/Gradle are unavailable on
  this Windows host. Treat all Android behaviour as source-only until built and
  exercised on a device/emulator.
- Apple Silicon Mac target/helper in `apps/mac/` is a **source-only scaffold**
  on this Windows host: no `swift build` or runtime verification, no macOS
  permission prompts, no ScreenCaptureKit capture, no pinned transport interop,
  and no signing/notarization were executed here. Real Apple Silicon
  verification is required before calling the Mac target shipped.

## Security and installation

- Builds are **unsigned** until a code-signing certificate is supplied. Windows
  SmartScreen may warn on first run.
- Pairing always requires explicit approval: either a single-use six-digit code
  plus the joining peer's device ID and full 64-hex fingerprint with an
  out-of-band verification checkbox, or `pairDiscovered` with `peerId`,
  `fingerprint`, and `verified: true` for a discovered peer. Both paths pin the
  peer certificate; a wrong code or an unpinned certificate is rejected before
  input forwarding is enabled. Discovery alone never grants trust, and both
  peers must approve each other for mutual TLS.
- Manual outbound production connections via `connectPeer` require an
  **explicit 64-hex SHA-256 fingerprint**; there is no unpinned fallback. The
  discovery path uses the fingerprint advertised in mDNS TXT records, but still
  requires the explicit `pairDiscovered` approval before that fingerprint is
  pinned and the connection is made. The inbound listener is pinned TLS and
  requires a v2 `Hello` naming a trusted device with a usable capability;
  untrusted peers are closed before any application data is dispatched.
- Inbound peer identity is bound to the certificate pinned for a trusted device:
  the TLS listener requires a client certificate and the v2 `Hello` device ID
  must match that certificate's pin. A peer cannot impersonate a trusted device
  merely by knowing its ID.
- TLS is configured TLS 1.3-only via rustls protocol-version builders on both
  client and server. This is a configuration-level check only; no live TLS 1.2
  downgrade attempt was executed on this host.
- Safety gates are covered by unit/integration tests (protocol decode limits,
  strict device IDs, one-time pairing codes, capability enforcement, hotkey
  rejection, injection gate, rate limit, clipboard bounds, file limits and
  cleanup). No second physical PC/tablet was available, so no real mDNS,
  pairing, or injection behaviour was field-verified.
- Discovery is LAN-first (mDNS, service `_mymouse._tcp.local.`). It advertises
  the actual TLS listener port (default 24800, configurable via `MYMOUSE_PORT`)
  and local certificate fingerprint in TXT records `port` and `fp`; the old
  fixed discovery port 45552 is gone. `discoveredPeers` returns id/name/address/
  port/fingerprint, and a `discoveredPeer` notification is emitted when a peer
  is found. The advertised `fp` is unauthenticated, so `pairDiscovered` requires
  the full fingerprint to be entered and verified out of band; it then connects
  with that pin and only saves `trust.json` and the inbound mutual-TLS client
  pin if the handshake succeeds. A failed pairing leaves no trust or pin behind.
  A peer cannot connect inbound unless its certificate is pinned for the exact
  device ID in its v2 `Hello`. First run may require a Windows Firewall allow
  prompt.
- **Launch at Windows start is off by default and stays off until you explicitly
  enable it.** myMouse never silently enables autostart.
- myMouse is a separate application with its own protocol and storage; it does
  not read, modify, or depend on the installed OneMouse app.
