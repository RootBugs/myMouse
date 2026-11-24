# myMouse Mac (Phase 4 scaffold)

Apple Silicon macOS target/helper scaffold for myMouse. This directory was authored on a Windows host; **it has not been built or runtime-verified on macOS**.

## Status

| Area | Status | Notes |
|---|---|---|
| Swift package/app skeleton | Scaffold | `Package.swift`, SwiftUI app, core library, tests. |
| Pairing UI placeholder | Scaffold | Local code generation/approve-reject state; no real QR rendering or network pairing. |
| Input injection | Partial scaffold | `CGEvent` key/mouse move posting and modifier mapping; no buttons, scroll, drag, IME, or secure-input handling. |
| Screen capture | Interface/stub | `ScreenCapturing` protocol + `ScreenCaptureKitStub`; no SCStream wiring. |
| Trust storage | Scaffold | JSON file store with SHA-256 certificate fingerprint helper; not yet wired to TLS pinning. |
| mDNS/transport integration | Plan only | `TransportPlan` documents shared Rust core / UniFFI preferred path; `ScaffoldTransport` throws. |
| Packaging/signing/notarization | Notes/scripts | `scripts/package-macos.sh` assembles bundle; signing/notarization are placeholders with no credentials. |

## Layout

```
myMouse/apps/mac/
  Package.swift
  Info.plist
  MyMouseMac.entitlements
  Sources/
    MyMouseMacCore/
      Permissions.swift      # Accessibility/Input Monitoring/Screen Recording prompts
      KeyMapping.swift       # Windows key names/modifiers -> macOS key codes/flags
      InputInjector.swift    # CGEvent posting, display coordinate mapping
      ScreenCapture.swift    # ScreenCaptureKit protocol/stub + plan
      TrustStore.swift       # Local trusted-peer JSON store
      Pairing.swift          # Pairing state/code placeholder
      Transport.swift        # Rust-core/Swift transport integration plan
    MyMouseMacApp/
      MyMouseMacApp.swift
      AppModel.swift
      ContentView.swift      # Placeholder pairing/permissions/trust UI
  Tests/
    MyMouseMacCoreTests/
      KeyMappingTests.swift
      TrustStoreTests.swift
  scripts/
    package-macos.sh
  README.md
```

## macOS permissions

The app must document and request these permissions before the corresponding feature works:

- **Accessibility** — required for CGEvent posting/input injection. The app can trigger the system prompt via `AXIsProcessTrustedWithOptions`; user must grant in System Settings > Privacy & Security > Accessibility.
- **Input Monitoring** — required for observing local input events/global hooks where macOS demands it. The scaffold opens System Settings > Privacy & Security > Input Monitoring; there is no stable public preflight query.
- **Screen Recording** — required for ScreenCaptureKit capture. The scaffold opens System Settings > Privacy & Security > Screen Recording and declares `NSScreenCaptureUsageDescription` in `Info.plist`.

Do not claim permissions are granted based on UI state alone; verify by exercising the real API on macOS.

## Build on macOS (Apple Silicon)

Prerequisites:

- macOS 13+ on Apple Silicon (arm64).
- Xcode 15+ or Command Line Tools with Swift 5.9+.
- No Apple signing credentials required for local debug builds.

Commands:

```bash
cd myMouse/apps/mac
swift --version
swift build
swift test
swift run MyMouseMac
```

For a release binary:

```bash
swift build -c release --arch arm64
```

## Packaging (.app / DMG) — placeholders

`scripts/package-macos.sh` creates a minimal `.app` bundle from the release binary and `Info.plist`. It does **not** sign or notarize.

Manual steps after credentials are available:

```bash
# Sign (Developer ID Application certificate required)
codesign --deep --force --options runtime \
  --entitlements MyMouseMac.entitlements \
  --sign "Developer ID Application: YOUR TEAM" \
  dist/MyMouse.app

# Create DMG
hdiutil create -volname myMouse -srcfolder dist/MyMouse.app -ov -format UDZO dist/myMouse-mac-arm64.dmg

# Notarize (Apple ID / app-specific password / team id required)
xcrun notarytool submit dist/myMouse-mac-arm64.dmg \
  --apple-id "you@example.com" --team-id TEAMID --password "app-specific-password" --wait

# Staple
xcrun stapler staple dist/myMouse-mac-arm64.dmg
```

Do not commit certificates, app-specific passwords, API keys, or notarization credentials.

## Transport / shared Rust core plan

The Windows backend already owns discovery, pairing, pinned TLS, protocol framing, trust semantics, and input bridge logic. The Mac target should prefer reusing that core:

1. **Preferred** — expose a narrow Rust FFI/UniFFI surface from `myMouse/backend` or a dedicated shared crate. Swift calls it for connect/pair/send/status; Swift owns CGEvent injection and ScreenCaptureKit.
2. **Fallback** — native Swift transport implementing the same versioned protocol as `myMouse/shared/protocol/src/lib.rs`. Only acceptable with cross-platform interop tests against the Rust implementation.

Either path must preserve: pinned TLS per pair, explicit host approval, no forwarding before trust is established, and revocation removing the pin.

## Verification limits (this environment)

Verified here on Windows:

- Files were created and structured as a Swift package.
- Basic static sanity: Swift sources are plain text and follow the expected package layout.
- `swift` toolchain is **not installed** in this Windows host, so `swift build`/`swift test` could not be run.

Not verified here:

- macOS compilation, SwiftUI runtime, app bundle behavior.
- Accessibility/Input Monitoring/Screen Recording prompts.
- CGEvent injection, modifier behavior, IME/dead keys, secure input fields.
- ScreenCaptureKit capture, frame encoding, or streaming.
- mDNS/TLS pairing interop with the Windows backend.
- Code signing, notarization, Gatekeeper, or DMG install flow.

## Known gaps

- No real protocol decode/encode for `CapturedEvent` bincode payloads in Swift; Rust FFI or a tested Swift codec is required.
- No button click, scroll, drag, or gesture injection.
- No ScreenCaptureKit implementation, only interface/stub.
- No QR rendering/scanning dependency; pairing code is plain text.
- No auto-update, menu bar extra, login item, or hardened runtime validation.
- No real-device verification on Apple Silicon.