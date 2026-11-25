# myMouse Android app

Source tree for the Android companion. There is **no APK in this workspace** and
**nothing was device-tested**: this host has Java but no Android SDK, no Android
NDK and no Gradle, and `kotlinc` is not installed. Kotlin changes are
source-only and must be compiled on a machine with Android SDK/Gradle before any
runtime claim is made.

## Status (honest)

| Area | Status | Notes |
|---|---|---|
| Bridge API (`MyMouseBridge`, `BridgeSession`) | Implemented, source-only | Pairing helpers, `connect`, `connectAndPair`, message send/receive, close. |
| Bridge selection | Implemented, source-only | `BridgeProvider`: real generated native binding if present → Kotlin `PinnedTlsBridge` → explicitly-labelled `StubMyMouseBridge` (network throws). |
| v2 wire codec | Implemented, source-only | `WireProtocol.kt` mirrors bincode 1.3 fixint LE + 2-byte LE version; frame header is `u32 BE`. Byte layout is pinned by the Rust golden test in `shared/android-bridge/src/transport.rs`. |
| Pinned TLS | Implemented, source-only | Custom `X509TrustManager` compares SHA-256 of the DER end-entity cert against 64 lowercase hex chars; hostname/CA chain cannot substitute. Post-handshake pin re-check. |
| Pairing UI | Implemented, source-only | QR/manual code + host id + address/port + fingerprint; trust entry is written **only after** `PairApprove` on the pinned socket. |
| File transfer UI | Implemented, source-only | Real v2 offer/accept/chunk/complete with SHA-256 and size verification, 64 MiB local cap, sanitized filenames. Requires a paired pinned host. |
| Remote screen touch input | Implemented, source-only | Touch gestures are encoded as MINP/1 and sent as v2 `InputEvent` frames. |
| Remote screen **video** | Not implemented | No decoder/renderer dependency; the UI says so and does not claim streaming. |
| Clipboard sync | Protocol supported, UI not implemented | Full v2 `ClipboardUpdate` encode/decode exists; there is no clipboard UI and no permission-safe user flow here. |
| Accessibility injection | Implemented with guards, **hardware-unverified** | Explicit user enablement, capability toggle, pinned-peer authorization, token-bucket limit (60/s, burst 8), sticky panic disable. Raw key injection is not possible via public APIs: keys are decoded but only a HOME/BACK/RECENTS whitelist is injected. |
| QR scanner | Source-only | Uses zxing dependency; needs camera permission/Play Services on a real device. |
| UniFFI native library | Not present | No UDL/generated classes/`.so` in this tree. `BridgeProvider` logs and falls through; it never fabricates JNI. |
| APK build / device test | **Not done** | Android SDK/NDK/Gradle unavailable. |

## Bridge API (`com.mymouse.android.bridge`)

- `MyMouseBridge.protocolVersion()`, `implementationName()` — honest diagnostics.
- `generatePairingCode()`, `buildPairingUri(code, peerId)`, `parsePairingUri(uri)`,
  `verifyCode(expected, entered)` — pure helpers matching `pairing.rs`.
- `normalizeFingerprint(value)` — accepts colons/spaces/uppercase, returns 64 lowercase hex chars.
- `suspend connect(request)` — TLS + pin check only, for already-trusted peers.
- `suspend connectAndPair(request)` — sends `Hello` + `PairCode`, requires `PairApprove`.
- `BridgeSession.send`, `awaitMessage`, listeners, `close` — one serialized writer, one reader coroutine.

`BridgeConnectRequest` carries host, port, peer id/name, pinned fingerprint,
pairing code, local device id/name and a timeout.

## Wire and pinning rules

Read from source, not guessed:

- `mymouse_protocol::encode` writes `u16 LE 2` then bincode 1.3 fixint LE
  `Message`. Enum tags are `u32 LE`; string/vec lengths are `u64 LE`; options are
  `u8`; structs have no names/padding.
- The Rust bridge and Windows backend wrap that with `u32 BE length` (1 byte..1 MiB).
  `WireProtocol.readFrame/writeFrame` do exactly this.
- The backend pins `sha256(DER end-entity certificate)` as lowercase hex and
  ignores intermediates and DNS names. The Kotlin trust manager and the
  post-handshake check do the same.

## InputEvent payload (MINP/1)

20 bytes: `'M''I''N''P'` (4) + version `0x01` + kind `u8` + pointerId `u8` +
buttons mask `u8` + x `i32 LE` + y `i32 LE` + keyCode `i32 LE`.
Kinds: 1 MOVE, 2 DOWN, 3 UP, 4 SWIPE, 5 HOME, 6 BACK, 7 RECENTS.
Decoding rejects any other length, magic, version, kind, or out-of-range field.
keyCode is decoded for forward compatibility but is not injected: public
Accessibility APIs cannot synthesize raw keys.

## Safety / panic

- No input path is active until the user enables `MyMouseAccessibilityService` in
  Android settings and the `cap_input` toggle is on.
- `InputRouter.authorize(peerId, fingerprint)` is called only by a paired,
  pinned session; revocation happens on session close.
- Token bucket: 60 events/second, burst 8; excess is dropped and logged.
- Settings → **PANIC: disable input now** sets a sticky disable, revokes
  authorization, clears pending gesture state and closes the socket. Clearing
  requires the explicit **Re-enable input** button.

## What to verify on a real device/emulator

1. `./gradlew assembleDebug` with Android SDK 35 installed (not run here).
2. Pair against a Windows host and confirm `PairApprove` is required before a
   trust entry is stored.
3. Confirm a wrong certificate is rejected by the pin check.
4. Inject a tap/swipe with Accessibility enabled and confirm the panic button
   stops injection immediately.
5. Send and receive a file; verify SHA-256 and saved path.
6. Verify QR scanning and deep-link handling with camera permission.
