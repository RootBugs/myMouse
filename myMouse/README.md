# myMouse

myMouse is a local-first software KVM app: share one keyboard and mouse across
nearby computers, with its own protocol, trust store, and interface. It is a
from-scratch alternative, not a wrapper around any installed application.

**Current scope: Phase 1 — Windows host ↔ Windows target (implemented).**
Phase 4 adds an **Apple Silicon Mac target/helper scaffold** under
`apps/mac/` (Swift package/app skeleton, docs, packaging notes); it has not
been built or runtime-verified on this Windows host. Android, clipboard sync,
file drop, and remote screen remain planned/incomplete. Builds are **unsigned**
until a code-signing certificate is provided.

## What is included

- Electron + React desktop UI with a device dashboard, adjacency layout editor,
  and device-management panel.
- Rust backend with a versioned binary protocol, LAN (mDNS) discovery, QR /
  six-digit pairing, pinned TLS 1.3 per pair, an adjacency layout model, and
  Windows low-level input capture/injection.
- A two-instance end-to-end smoke test that links two backend processes over
  pinned TLS and verifies a cursor crossing is delivered.

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
- Clipboard sync, file transfer, and remote screen are not in Phase 1.
- Complex keyboard layouts, IMEs, and per-monitor DPI handoff are not complete.
- Android/Mac targets cannot be verified on a Windows host.
- Builds are unsigned; autostart is off until you enable it.

## Development layout

```
myMouse/
  apps/mac/         Phase 4 Apple Silicon Mac target/helper scaffold (Swift)
  electron/         Electron main, preload, JSON-RPC bridge
  src/              React UI
  shared/protocol/  Versioned binary message protocol
  backend/          Rust/Tokio backend (discovery, pairing, TLS, layout, input)
  scripts/          Two-instance smoke test
  installer/        Windows build and artifact verification
  docs/             Limitations and design notes
```

The test-mode backend (`--test-mode`) exists only for automated smoke tests. It
uses a fixed shared test certificate and a static peer list; production always
uses mDNS discovery and per-pair pinned certificates.
