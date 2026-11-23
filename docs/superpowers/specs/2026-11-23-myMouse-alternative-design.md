# myMouse — OneMouse Alternative Design

> Status: design draft for approval. No product code has been written yet.

## Intent

Build a from-scratch alternative to the installed OneMouse app: software KVM that lets one Windows PC share its mouse, keyboard, clipboard and files with nearby paired devices, and lets a phone/tablet control the PC screen. The product is called **myMouse** (builder call; rename is cheap and can be done before implementation).

Why: the user wants their own full alternative, not a UI mock and not a wrapper around the installed OneMouse.

## Goals

- Windows 10/11 host can control another Windows PC, an Android device, and an Apple Silicon Mac target using one mouse/keyboard.
- Cursor crosses the configured screen edge; keyboard follows the cursor; modifier keys are translated between Windows/macOS.
- Clipboard sync (text, rich text, images) and file transfer by drag/drop or Quick Drop zone.
- Phone/tablet can open a paired PC screen and click/type/scroll; PC can capture a screenshot and send it to a device.
- Local-first pairing with QR code or 6-digit code; no mandatory account or cloud relay.
- Auto reconnect, trusted-device list, and an app settings UI comparable to OneMouse.
- Own branding, own codebase, own signing identity when certificates are available.

## Non-goals

- No Linux host, no iPhone/iPad target in v1 (matches the reference product's supported scope).
- No hardware KVM, no USB gadget mode, no Android accessibility bypass.
- No copying OneMouse's code, assets, trademarks or private protocol.
- No cloud account requirement for basic use; optional relay is self-hostable.
- Autostart will not be enabled silently; the user handles that themselves.

## Anti-goals

- A screen recording that only looks like OneMouse but does not move input.
- A build that depends on the installed OneMouse binaries being present.
- A release that silently grants remote-control permission to unknown devices.
- Cloud storage of files or clipboard content.

## Constraints

- Development host available now: Windows 10/11, Node.js, Python, PowerShell, no Mac and no Android device attached to this session. Mac/Android parts can be implemented and packaged but cannot be fully verified here.
- Native input capture/injection needs OS permissions: Windows low-level hooks, Android Accessibility Service, Mac Accessibility / Input Monitoring / Screen Recording where applicable.
- Unsigned builds are expected locally; code signing needs a certificate the user has not supplied.
- Must not modify or interfere with the installed OneMouse app while developing.
- Keep all generated work in this project folder unless the user gives a destination.

## Approaches considered

1. **Electron + Node + native helper per OS.** Fastest matching UI and familiar stack, but Node input hooks rely on native addons and platform quirks.
2. **Electron UI + Rust backend (chosen).** Electron/React/TypeScript for the app shell; a Rust backend using Tokio for discovery, pairing, secure transport, input capture/injection (`rdev` / `enigo` style crates), screen capture and file transfer. Rust gives one well-tested native core across Windows/macOS with clear unsafe boundaries. Trade-off: more compile time and platform-specific test gaps here.
3. **Tauri UI + Rust backend.** Smaller binary than Electron, but webview differences and tray/overlay behaviour are less predictable for a mouse-crossing app.

Choice: approach 2. The UI is web-like and quick to iterate; the risky native input work is isolated in a Rust service with explicit interfaces.

## Architecture

- `apps/desktop/` — Electron + React + TypeScript UI: dashboard, device layout, file transfer, device management, settings, tray and Quick Drop overlay.
- `backend/` — Rust service: LAN discovery (mDNS), pairing, encrypted peer connections, input bridge, clipboard bridge, file transfer, screen capture RPC, trusted-device storage, auto reconnect.
- `apps/android/` — Kotlin/Android app: Accessibility Service input receiver, pairing, notification/code copy, file receive/send, remote screen viewer.
- `apps/mac/` — Swift helper: input injection, screen capture, pairing UI; Apple Silicon target.
- `shared/protocol/` — versioned message schema (capnp/protobuf or serde JSON with explicit version) used by all clients.
- `installer/` — Windows NSIS/MSIX-style packaging (unsigned unless a cert is supplied), Android APK/AAB, Mac .app/DMG.
- `docs/` — design, plans, runbooks.

Communication: peers find each other on LAN via mDNS, pair with QR/6-digit code, then open an encrypted TCP connection (TLS 1.3 with per-pair certificates). Optional self-hosted relay uses the same encrypted protocol. No input or file data goes through a third-party server.

Input model: the host captures local input events and, when the cursor is assigned to a remote device, suppresses local injection and forwards translated events to that device's agent. Each agent injects events with OS-native APIs and reports status back.

## Phasing (one product, verifiable slices)

1. **Core Windows ↔ Windows:** discovery, pairing, cursor crossing, keyboard forwarding, tray UI, dashboard/settings shell. This is the first runnable slice and proves the architecture.
2. **Clipboard + files:** clipboard sync, drag-drop transfer, Quick Drop zone, save-folder setting, transfer history.
3. **Android target:** pairing, accessibility input, notifications/code copy, file transfer, remote screen view (LAN).
4. **Mac target:** pairing, input injection, screen/window share, Apple Silicon packaging.
5. **Hardening + release:** trusted-device management, auto reconnect, firewall guidance, installer, update channel, docs.

Each phase produces working software and is planned separately; the full design stays the source of truth.

## Decisions made here (builder should not re-litigate)

- Product name: **myMouse**.
- UI: Electron + React + TypeScript; backend: Rust + Tokio.
- Transport: LAN-first mDNS + TLS 1.3 per-pair certificates; no mandatory cloud.
- Pairing: QR code or 6-digit code, stored per-device trust.
- v1 platforms: Windows host; Windows, Android, Apple Silicon Mac targets. No Linux/iOS.
- Autostart remains user-controlled; the app may expose a toggle but must not enable it itself.

## Resolved builder-check questions (design calls)

- **Screen control:** phone/tablet → PC remote view is a continuous live stream (H.264, adaptive quality, started on demand); screenshot sending is one-shot. Windows capture lives in the Rust backend via Graphics Capture/DXGI; Electron only renders the viewer.
- **Android networking:** Kotlin UI plus the shared Rust core through UniFFI bindings. No second networking stack; discovery, pairing and transport are shared.
- **Windows elevation:** v1 runs per-user and does not inject into UAC/secure desktop or locked sessions. This limit is documented; a global escape hotkey returns input to the host.
- **Pairing:** host displays QR/6-digit code, joiner confirms, host approves. Each pair gets a generated TLS certificate that is pinned. Trust grants input, clipboard, files and screen control, with per-capability toggles and revocation in device management.
- **Roles:** every Windows install can be host and target; roles are per connection, only one input source is active at a time, and loop prevention is built into the bridge.
- **Layout:** arbitrary adjacency grid; each physical monitor is a node; cursor crosses only configured edges; no wrap by default; escape hotkey restores host focus.
- **Clipboard/files:** clipboard sync is global last-write-wins among connected trusted peers, with an optional cursor-scoped mode. Files from trusted peers auto-save to the configured folder; a per-transfer approval toggle is available. Resumable transfers and size limits are later additions.
- **Done criteria:** Windows phases are fully tested here. Android compiles/packages and gets an emulator smoke test; real-device verification is required before that phase is called shipped. Mac compiles/packages with documented gaps; real Apple Silicon verification is required before shipped.

## Left to the builder

- Exact crate/library versions after checking availability on this Windows host.
- Exact message schema fields once each phase's tests pin the behavior.
- OS-specific permission prompts and uninstall cleanup details.
- Whether to add a self-hosted relay after local LAN works; not needed for the first release.
- Code signing and notarization require credentials from the user; until then builds are local/dev only.
