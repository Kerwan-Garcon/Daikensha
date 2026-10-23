#!/usr/bin/env bash
# Stands in for `cargo` in the android test image. RustDesktopInteropTest runs
#   cargo test --manifest-path windows/Cargo.toml … -- --ignored --nocapture phone_link::interop::interop_server
# to start the Windows/Linux desktop; here the Mac's Swift implementation answers instead, with the same
# stdin/stdout protocol (windows/src-tauri/src/phone_link/interop.rs). The Kotlin test is not modified,
# and it names the desktop "Rust desktop", so the Swift server is given that name.
exec /interop/AndroidLinkInterop --name "Rust desktop"
