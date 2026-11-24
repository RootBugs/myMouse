# myMouse — known limitations (Phase 1)

## Input capture and injection

- **UAC prompts, the secure desktop, and locked sessions are out of scope for v1.** Windows does not deliver low-level input hooks to processes running at a different integrity level or on the secure desktop. myMouse will not inject into those surfaces. Unlock the PC and use the local keyboard/mouse there.
- The escape hotkey is **Ctrl+Alt+Shift+M**. It immediately stops forwarding and releases the held modifiers so control returns to the host.
- Only one input source is active at a time. Locally captured events are forwarded while forwarding is active; remote events from the peer are applied with `SendInput`. Injected events are filtered from the capture hooks to prevent loops.
- Text translation for macOS targets currently covers the Ctrl→Cmd / Alt→Option modifier mapping. Complex layouts, IMEs, dead keys, and non-alphanumeric special keys are not yet fully mapped.
- Mouse injection uses absolute virtual-screen coordinates. Multi-monitor layouts are modelled, but per-monitor DPI scaling at handoff is not yet compensated for.

## Platform scope

- Phase 1 targets Windows host ↔ Windows target. Android is planned for a later phase. Phase 4 adds an **Apple Silicon Mac target/helper scaffold** in `myMouse/apps/mac/` (Swift package/app skeleton, CGEvent injection scaffold, ScreenCaptureKit interface/stub, trust/pairing placeholders, transport integration plan, packaging notes). It is **source-only on this Windows host**: no `swift build`/runtime verification, no macOS permission prompts, no ScreenCaptureKit capture, no pinned transport interop, and no signing/notarization were executed here. Real Apple Silicon verification is required before calling the Mac target shipped.
- Clipboard sync, file transfer / Quick Drop, and live screen streaming are planned for later phases; the UI currently shows placeholders for these features.

## Security and installation

- Builds are **unsigned** until a code-signing certificate is supplied. Windows SmartScreen may warn on first run.
- Pairing requires a six-digit code or QR confirmation and pins the peer certificate. A wrong code or an unpinned certificate is rejected before input forwarding is enabled.
- Discovery is LAN-first (mDNS). First run may require a Windows Firewall allow prompt.
- **Launch at Windows start is off by default and stays off until you explicitly enable it.** myMouse never silently enables autostart.
- myMouse is a separate application with its own protocol and storage; it does not read, modify, or depend on the installed OneMouse app.
