# myMouse — known limitations

Last accuracy pass: 2026-11-25. Scope statements below distinguish what was
**verified on this Windows host** from what is only source-complete.

## Input capture and injection

- **UAC prompts, the secure desktop, and locked sessions are out of scope for v1.** Windows does not deliver low-level input hooks to processes running at a different integrity level or on the secure desktop. myMouse will not inject into those surfaces. Unlock the PC and use the local keyboard/mouse there.
- The escape hotkey is **Ctrl+Alt+Shift+M**. It immediately stops forwarding and releases the held modifiers so control returns to the host.
- Only one input source is active at a time. Locally captured events are forwarded while forwarding is active; remote events from the peer are applied with `SendInput`. Injected events are filtered from the capture hooks to prevent loops.
- Text translation for macOS targets currently covers the Ctrl→Cmd / Alt→Option modifier mapping. Complex layouts, IMEs, dead keys, and non-alphanumeric special keys are not yet fully mapped.
- Mouse injection uses absolute virtual-screen coordinates. Multi-monitor layouts are modelled, but per-monitor DPI scaling at handoff is not yet compensated for.

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
  **exactly one** target, not broadcast to every trusted device.
- Remote/live screen streaming is still unimplemented; the screen-related UI
  is a placeholder.

## Platform scope

- The runtime-verified scope is **Windows host ↔ Windows target**, driven by
  the two-instance test harness on one machine. Two separate physical PCs were
  not available here, so no real two-machine session was executed.
- Android: the app sources use the project's v2 framing and pinned-TLS rules;
  no APK was built and no device test was run because Android SDK/NDK/Gradle
  are unavailable on this Windows host. Treat all Android behaviour as
  source-only until built and exercised on a device.
- Apple Silicon Mac target/helper in `apps/mac/` is a **source-only scaffold**
  on this Windows host: no `swift build` or runtime verification, no macOS
  permission prompts, no ScreenCaptureKit capture, no pinned transport interop,
  and no signing/notarization were executed here. Real Apple Silicon
  verification is required before calling the Mac target shipped.

## Security and installation

- Builds are **unsigned** until a code-signing certificate is supplied. Windows
  SmartScreen may warn on first run.
- Pairing requires a six-digit code or QR confirmation and pins the peer
  certificate. A wrong code or an unpinned certificate is rejected before input
  forwarding is enabled.
- Outbound production connections require an **explicit 64-hex SHA-256
  fingerprint** via `connectPeer`; there is no unpinned fallback. The inbound
  listener is pinned TLS and requires a v2 `Hello` naming a trusted device with
  a usable capability; untrusted peers are closed before any application data
  is dispatched. Discovery to automatic connection is still a manual flow.
- Inbound peer identity is bound to the certificate pinned for a trusted device:
  the TLS listener requires a client certificate and the v2 `Hello` device ID
  must match that certificate's pin. A peer cannot impersonate a trusted device
  merely by knowing its ID.
- Discovery is LAN-first (mDNS). First run may require a Windows Firewall allow
  prompt.
- **Launch at Windows start is off by default and stays off until you explicitly
  enable it.** myMouse never silently enables autostart.
- myMouse is a separate application with its own protocol and storage; it does
  not read, modify, or depend on the installed OneMouse app.
