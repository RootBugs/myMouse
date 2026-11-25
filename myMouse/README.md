# myMouse

myMouse is a local-first software KVM app: share one keyboard and mouse across
nearby computers, with its own protocol, trust store, and interface. It is a
from-scratch alternative, not a wrapper around any installed application.

**Runtime-verified scope: Windows host ↔ Windows target, driven by the
two-instance pinned-TLS harness on this host.** Clipboard updates and small
file transfers now travel between two real backend processes over the existing
v2 pinned-TLS transport; the UI sends clipboard text and offers/accepts files and
refreshes on backend notifications. No two *separate physical* PCs were available
here, so that remains harness-verified rather than field-verified.

Android (`apps/android/`) and the Rust FFI crate (`shared/android-bridge/`) are
**source-complete but source-only**: no Android SDK/NDK/Gradle is installed, so
no APK was built and no device was tested. Mac (`apps/mac/`) remains a
source-only scaffold on this Windows host. Remote screen is unimplemented.
Builds are **unsigned** until a code-signing certificate is provided.

## What is included

- Electron + React desktop UI with a device dashboard, adjacency layout editor,
  and device-management panel.
- Rust backend with a versioned binary protocol, LAN (mDNS) discovery, QR /
  six-digit pairing, pinned TLS 1.3 per pair, an adjacency layout model, and
  Windows low-level input capture/injection.
- Two-instance end-to-end tests that link two backend processes over pinned TLS
  and verify a cursor crossing, exact clipboard text delivery, and exact file
  bytes (offer → accept → chunks → complete).
- Pinned outbound connection RPC (`connectPeer`) that requires a verified
  64-hex SHA-256 fingerprint and rejects a mismatch with stored trust.
- A self-signed TLS identity generated on first run and persisted in the myMouse
  config directory. `localIdentity` exposes its public fingerprint and listen
  port; the private key is never returned over RPC or logged.
- A pinned inbound listener (`MYMOUSE_PORT`, default 24800) using mutual TLS:
  the client must present the certificate pinned for a trusted device, and its
  v2 `Hello` device ID must match that certificate. Untrusted peers are closed
  before any clipboard/file/input message is handled.
- Backend-to-UI notifications for clipboard/file events; the React app refreshes
  its state when they arrive.

## Prerequisites

- Windows 10/11 x64.
- Node.js 20+ and npm.
- Rust toolchain (stable) if building from source.
- For packaging: electron-builder downloads its Windows tooling on first use.

## Build from source

```powershell
cd myMouse
npm install
npm run build          # typecheck + Vite + Electron sources
npm run test -- --run  # frontend tests
npm run smoke          # two-instance pinned-TLS delivery smoke test
npm run dist           # unsigned Windows build (see below)
```

`npm run dist` compiles the backend in release mode, builds the UI, and runs
electron-builder. Outputs:

- `myMouse/dist/win-unpacked/myMouse.exe` — portable unpacked app.
- `myMouse/dist/myMouse Setup 0.1.0.exe` — NSIS installer.
- `myMouse/dist/win-unpacked/resources/mymouse-backend.exe` — bundled backend.

Verify the artifacts with:

```powershell
powershell -File myMouse\installer\verify.ps1
```

## Install and first run

1. Run the NSIS installer (or `dist/win-unpacked/myMouse.exe` for the portable
   build). Choose "just me" unless you intentionally want a machine-wide
   install.
2. Windows Firewall will likely prompt on first launch. Allow myMouse on the
   **private network** so mDNS discovery and the pinned TLS listener can work.
   Blocking it prevents pairing.
3. Windows SmartScreen may warn because the build is unsigned. This is expected
   until a code-signing certificate is supplied.

### Launch at Windows start

**Autostart is off by default and stays off until you explicitly enable it.**
myMouse never silently enables autostart. If a future settings control offers
it, it will require your explicit action.

## Pairing two computers

1. Install myMouse on both machines and keep them on the same LAN.
2. On the host, start a pairing session. myMouse shows a QR code and a six-digit
   code.
3. On the joining machine, scan the QR code or enter the six-digit code, then
   confirm the host name that appears.
4. The host approves the request. On approval the two installs exchange and pin
   a per-pair TLS certificate fingerprint. A wrong code or an unpinned
   certificate is rejected before input forwarding is enabled.
5. Grant only the capabilities you want (input, clipboard, files, screen).
   Capabilities can be revoked later from Device management.
6. Use **Settings → Show my fingerprint** to read this install's public pairing
   token and listen port (default 24800, override with `MYMOUSE_PORT`). Exchange
   fingerprints out of band, then open **Settings → Peer connection** and enter
   the peer's address, device ID, and *verified* 64-hex fingerprint. There is no
   unpinned fallback. Discovery → automatic connection is still a manual flow.

## Control and layout

- Each physical monitor is a node. Arrange nodes in an adjacency grid; edges
  define handoff. There is no wrap by default.
- A cursor crossing a configured edge hands control to the adjacent machine
  exactly once, scaling the entry coordinate for the target display.
- Only one input source is active at a time. Injected events are filtered from
  capture so control cannot loop back.
- **Panic escape: `Ctrl+Alt+Shift+M`** immediately stops forwarding and releases
  held modifiers, returning control to the local host.

## Trust management

Paired devices, capability grants, and pinned fingerprints are stored in the
per-user config directory. Use Device management to revoke a device or change
its capabilities. Revoking removes the pin, so the peer must pair again.

## Known limitations

See [docs/limitations.md](docs/limitations.md). In short:

- No injection into UAC prompts, the secure desktop, or locked sessions.
- Clipboard/file delivery is verified between two processes in test mode; no
  two-physical-PC session was run here. Remote screen is not implemented.
- Complex keyboard layouts, IMEs, and per-monitor DPI handoff are not complete.
- Android is source-only with a real pinned-TLS bridge implementation (no APK
  or device test: no Android SDK/NDK/Gradle); Mac is source-only on Windows.
- Builds are unsigned; autostart is off until you enable it.

## Development layout

```
myMouse/
  apps/android/     Android app sources (Kotlin pinned-TLS bridge; source-only)
  apps/mac/         Apple Silicon Mac target/helper scaffold (Swift)
  electron/         Electron main, preload, JSON-RPC bridge
  src/              React UI
  shared/protocol/  Versioned binary message protocol
  shared/android-bridge/  Rust FFI bridge scaffold for Android
  backend/          Rust/Tokio backend (discovery, pairing, TLS, layout, input)
  scripts/          Two-instance smoke test
  installer/        Windows build and artifact verification
  docs/             Limitations and design notes
```

The test-mode backend (`--test-mode`) exists only for automated smoke tests. It
uses a fixed shared test certificate and a static peer list; production always
uses mDNS discovery and per-pair pinned certificates.
