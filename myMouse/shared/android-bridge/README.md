# myMouse Android Bridge (Rust crate)

FFI-shaped primitives shared with the Android app. The crate reuses
`mymouse-protocol` for the wire format; it never defines a second protocol.

## Status (honest, source-only unless stated)

| Area | Status | Notes |
|---|---|---|
| Protocol encode/decode | Implemented + host-tested | Reuses `mymouse-protocol` v2 (`u16 LE version 2` + bincode 1.3 fixint LE `Message`). |
| Length-prefixed frames | Implemented + host-tested | `u32 BE payload length` + payload; `MAX_FRAME_LEN = 1 MiB`. `encode_frame`, `decode_frame`, `frame_length`, `is_valid_frame_length`. |
| Golden wire bytes | Implemented + host-tested | `input_event_frame_matches_kotlin_wire_layout` pins the exact bytes that `WireProtocol.kt` must emit. |
| Pin helpers | Implemented + host-tested | `normalize_fingerprint` (colon/space/case tolerant -> 64 lowercase hex) and `certificate_fingerprint` (SHA-256 of DER end-entity cert), matching the Windows backend. |
| Pairing code/URI/session | Implemented + host-tested | 6-digit code, `mymouse://pair?code=&id=`, TTL-checked in-memory session. |
| Trust model | Implemented + host-tested | Capabilities, records, JSON store load/save. |
| Discovery model | Implemented + host-tested | Shared peer model and service-name parsing; Android NSD runtime lives in platform code, not here. |
| Pinned TLS socket runtime | **Source-only in Kotlin**, not in this crate | Rust crate has config/fingerprint helpers only; the live TLS socket is `PinnedTlsBridge.kt` (`javax.net.ssl`) in `apps/android`. |
| UniFFI bindings | **Not generated** | No UDL, no generated Kotlin classes, no `.so` packaged. `BridgeProvider` will use a generated class only if one is actually added; nothing is fabricated. |
| APK / device verification | **Not done** | No Android SDK/NDK/Gradle on this host. |

## Host verification (actually run here)

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --manifest-path myMouse/shared/android-bridge/Cargo.toml
```

12 tests pass on the host toolchain (including 4 new frame/fingerprint tests).
Host tests do **not** prove Android runtime behaviour, TLS handshake with a real
Windows host, or accessibility injection.

## Wire contract (do not drift)

- Transport frame: `u32 big-endian length` + exactly that many payload bytes.
  Length must be `1..=1_048_576`; trailing bytes are rejected by `decode_frame`.
- Payload: bincode 1.3 `serialize()` defaults (fixint, little-endian):
  enum variant `u32 LE`; string/vec length `u64 LE`; `Option` = `u8` 0/1;
  `bool` = `u8`; struct fields in declaration order; 2-byte LE version header = 2.
- TLS identity: SHA-256 hex of the **DER end-entity certificate only**, compared
  to the pinned value (lowercase, 64 hex digits). No CA chain or DNS name is
  accepted as an alternative trust anchor. SNI `mymouse.local` matches the backend.
- InputEvent bytes are defined by `InputEventCodec.kt` (MINP/1, 20 bytes). This
  crate deliberately treats them as opaque, exactly like `mymouse-protocol`.

## UniFFI / Android NDK (not run in this environment)

With NDK installed:

```powershell
cargo install cargo-ndk
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo ndk -t arm64-v8a -o ../apps/android/app/src/main/jniLibs build --release --manifest-path Cargo.toml
# Add uniffi/mymouse_bridge.udl, then generate Kotlin bindings with uniffi-bindgen.
```

Generated bindings must implement `MyMouseBridge` and be placed under
`apps/android/app/src/main/java/com/mymouse/android/bridge/generated/` to be
selected by `BridgeProvider`. Until then the Kotlin pinned-TLS bridge is used.
