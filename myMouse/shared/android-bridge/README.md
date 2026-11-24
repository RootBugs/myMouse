# myMouse Android Bridge

Rust FFI bridge for the Phase 3 Android scaffold. It exposes protocol, pairing, trust, discovery, and transport primitives that the Android app can consume through UniFFI-generated Kotlin bindings.

## Status

| Area | Status | Notes |
|---|---|---|
| Protocol encode/decode | Implemented | Reuses `mymouse-protocol` wire format. |
| Pairing code/URI/session | Implemented | 6-digit code, `mymouse://pair?code=&id=` URI parse/build, in-memory session. |
| Trust model | Implemented | Capabilities, trusted device records, JSON store load/save. |
| Discovery model | Implemented | Shared peer model and service-name parsing; actual Android NSD/mDNS runtime not implemented in Rust here. |
| Transport frame helpers | Implemented | Length-prefixed frame encode/decode helpers; full pinned TLS socket runtime is not implemented in this crate. |
| UniFFI bindings | Not generated | API shape is FFI-friendly, but no UDL file or generated bindings are committed yet. Real bindings require Android NDK + cargo-ndk and a UDL definition. |

## Build (host)

```powershell
cargo test --manifest-path myMouse/shared/android-bridge/Cargo.toml
```

## UniFFI / Android NDK (not run in this environment)

When Android NDK is installed:

```powershell
cargo install cargo-ndk
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo ndk -t arm64-v8a -o ../apps/android/app/src/main/jniLibs build --release --manifest-path Cargo.toml
# Define uniffi/mymouse_bridge.udl, then generate Kotlin bindings with uniffi-bindgen
```

The generated Kotlin bindings should be placed under `myMouse/apps/android/app/src/main/java/com/mymouse/android/bridge/generated/` and replace the hand-written stubs.

## Design notes

- **No second protocol**: wire format comes from `mymouse-protocol`.
- **Fallback path**: if Rust cross-compilation is unavailable, Android can implement a Kotlin transport that uses the same frame format and pinned-TLS rules, while keeping UI/business logic aligned with this bridge API.
- **Security**: pairing approval and trust grants must remain explicit user actions; do not auto-trust discovered peers.
