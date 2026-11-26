# UniFFI scaffold

This directory contains a **scaffold** UDL for the Android bridge. It documents the intended UniFFI export surface but is not wired into a build in this environment.

## Why scaffold only

- No Android NDK / cargo-ndk / uniffi-bindgen installed on this Windows host.
- Generating and packaging `.so` binaries for Android ABIs was not possible here.
- The Kotlin app currently uses a source-level bridge (`BridgeContracts.kt`, `WireProtocol.kt`, `PinnedTlsBridge.kt`) that mirrors the Rust/protocol contract.

## To generate real bindings (on a machine with Android toolchain)

```powershell
cargo install cargo-ndk uniffi-bindgen-cli@0.28
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo ndk -t arm64-v8a -o ../../apps/android/app/src/main/jniLibs build --release --manifest-path ../Cargo.toml
# Generate Kotlin bindings from this UDL and place them under:
# apps/android/app/src/main/java/com/mymouse/android/bridge/generated/
```

After generation, wire `BridgeProvider` to prefer the generated binding when the native library is present. Do not fake a native binding; fallback must remain explicit.
