#!/usr/bin/env bash
# The Android link's portable core (NotchBuddy/Sources/App/AndroidLink/Core): unit tests and the
# conversation over real sockets. Runs on a Mac (CryptoKit) and on Linux (swift-crypto), so also in
# Docker: ./scripts/docker-android-link.sh does it without installing anything.
#
#   ./scripts/test-android-link.sh            build and run the tests
#   ./scripts/test-android-link.sh build      only build (the tests and the interop server)
set -euo pipefail
cd "$(dirname "$0")/.."
PKG=tests/android-link
for target in AndroidLinkTests AndroidLinkInterop; do
  rm -rf "$PKG/Sources/$target/Core" "$PKG/Sources/$target/Support"
  mkdir -p "$PKG/Sources/$target/Core" "$PKG/Sources/$target/Support"
  cp NotchBuddy/Sources/App/AndroidLink/Core/*.swift "$PKG/Sources/$target/Core/"
  cp "$PKG"/Support/*.swift "$PKG/Sources/$target/Support/"
done
FLAGS=(--package-path "$PKG" -c "${CONFIG:-debug}")
if [ "$(uname)" = "Linux" ]; then FLAGS+=(--static-swift-stdlib); fi
swift build "${FLAGS[@]}"
[ "${1:-}" = "build" ] && exit 0
"$(swift build "${FLAGS[@]}" --show-bin-path)/AndroidLinkTests"
