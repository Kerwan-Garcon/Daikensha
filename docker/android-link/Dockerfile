# Containers for the Android link's tests (scripts/docker-android-link.sh drives them).
#
#   core     Swift 6.1 on Ubuntu 22.04: the portable core of NotchBuddy/Sources/App/AndroidLink, its
#            tests, and the interop server (socat puts TLS in front of it, with the certificate the core makes).
#   android  The Android SDK and JDK: the Kotlin client's own tests (android/), unchanged, against that server.
#
# Same Ubuntu release in both, so the interop server built in `core` runs in `android`.

FROM swift:6.1-jammy AS core
RUN apt-get update && apt-get install -y --no-install-recommends socat openssl && rm -rf /var/lib/apt/lists/*
WORKDIR /repo

FROM cimg/android:2025.04 AS android
USER root
RUN apt-get update && apt-get install -y --no-install-recommends socat && rm -rf /var/lib/apt/lists/*
# RustDesktopInteropTest starts `cargo test … interop_server`; in this image `cargo` is the Swift server.
COPY cargo-shim.sh /usr/local/bin/cargo
RUN chmod +x /usr/local/bin/cargo
WORKDIR /repo
