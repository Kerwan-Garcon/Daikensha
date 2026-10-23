#!/usr/bin/env bash
# The Mac-only half of the Android link: LinkServer (Network.framework, TLS) with the core's identity,
# against a client that pins the certificate like the phone. macOS only; the portable core is
# scripts/test-android-link.sh (also in Docker: scripts/docker-android-link.sh).
set -euo pipefail
cd "$(dirname "$0")/.."
TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/coucou-android-link-mac.XXXXXX")"
trap 'rm -rf "$TEST_DIR"' EXIT
swiftc -swift-version 6 -D ANDROID_LINK -parse-as-library \
    NotchBuddy/Sources/App/AndroidLink/Core/*.swift \
    NotchBuddy/Sources/App/AndroidLink/LinkServer.swift \
    NotchBuddy/Sources/App/AndroidLink/LinkKeychain.swift \
    NotchBuddy/Sources/App/AppLog.swift \
    tests/AndroidLinkMacTests.swift -o "$TEST_DIR/android-link-mac-tests"
"$TEST_DIR/android-link-mac-tests"
