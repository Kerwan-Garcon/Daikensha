#!/usr/bin/env bash
# Every test of the Mac's Android link that can run without a Mac, in Docker (nothing to install but Docker).
#
#   ./scripts/docker-android-link.sh            the core's tests, then the Android client against the Swift server
#   ./scripts/docker-android-link.sh core       only the core's tests (Swift on Linux)
#   ./scripts/docker-android-link.sh interop    only the interop: android/'s own Kotlin link client and its
#                                               RustDesktopInteropTest, compiled as plain Kotlin/JVM
#   ./scripts/docker-android-link.sh android    the same test through the full Android build (./gradlew in android/)
#
# What runs where:
#   * NotchBuddy/Sources/App/AndroidLink/Core is portable Swift: in the `core` image it is built against
#     swift-crypto (CryptoKit's twin) and tested, and the interop server (tests/android-link) is built.
#   * android/ is not modified. In the `android` image `cargo` starts that Swift server, so the Kotlin test that
#     normally drives the Rust desktop drives the Mac's implementation instead, over TLS with the certificate
#     the Swift core made (docker/android-link/cargo-shim.sh).
#   * What only exists on a Mac (Network.framework, the Keychain, the settings view) is built and tested by
#     .github/workflows/mac-android-link.yml on macOS.
#
# Behind a proxy: HTTPS_PROXY is passed on (with the host network when it points at this machine);
# COUCOU_DOCKER_CA (a PEM bundle) and COUCOU_DOCKER_JAVA_TRUSTSTORE (a JKS) are mounted if set.
set -euo pipefail
cd "$(dirname "$0")/.."

WHAT="${1:-all}"
IMG=coucou-android-link
# The interop tests this stage of the Mac implementation answers (protocol v1: sessions, approvals, pairing).
INTEROP_TESTS=(
  pairsReceivesSessionsAndApprovalAndAllows
  aCommandOf10000CharactersIsFlaggedAndCannotBeAllowedFromThePhone
  denyReachesTheDesktop
  aRequestAnsweredAtTheDeskCannotBeAnsweredFromThePhone
  aWrongTokenIsRefused
  pairingAgainDisconnectsThePhoneThatHadTheOldCode
)

NET=()
ENVS=()
BUILD_ARGS=()
JAVA_OPTS=""
if [ -n "${HTTPS_PROXY:-}" ]; then
  ENVS+=(-e HTTPS_PROXY -e HTTP_PROXY -e NO_PROXY)
  BUILD_ARGS+=(--build-arg HTTPS_PROXY --build-arg HTTP_PROXY)
  hostport="${HTTPS_PROXY#*://}"; hostport="${hostport%%/*}"
  host="${hostport%:*}"; port="${hostport##*:}"
  case "$host" in localhost|127.*) NET=(--network host) ;; esac
  JAVA_OPTS="-Dhttps.proxyHost=$host -Dhttps.proxyPort=$port -Dhttp.proxyHost=$host -Dhttp.proxyPort=$port -Dhttp.nonProxyHosts=localhost|127.0.0.1"
fi
if [ -n "${COUCOU_DOCKER_CA:-}" ]; then
  ENVS+=(-v "$COUCOU_DOCKER_CA:/ca.crt:ro" -e SSL_CERT_FILE=/ca.crt -e GIT_SSL_CAINFO=/ca.crt)
fi
if [ -n "${COUCOU_DOCKER_JAVA_TRUSTSTORE:-}" ]; then
  ENVS+=(-v "$COUCOU_DOCKER_JAVA_TRUSTSTORE:/cacerts:ro")
  JAVA_OPTS="$JAVA_OPTS -Djavax.net.ssl.trustStore=/cacerts -Djavax.net.ssl.trustStorePassword=changeit"
fi

image() {
  docker build -q "${NET[@]}" "${BUILD_ARGS[@]}" --target "$1" -t "$IMG-$1" docker/android-link >/dev/null
}

core() {
  image core
  # The SwiftPM build lives in a volume (Linux objects must not land in the Mac's .build), and the interop
  # server is copied to another volume for the android image.
  docker run --rm "${NET[@]}" "${ENVS[@]}" -v "$PWD":/repo \
    -v coucou-android-link-swiftpm:/repo/tests/android-link/.build -v coucou-android-link-interop:/interop \
    "$IMG-core" bash -c 'set -e
      scripts/test-android-link.sh
      cp "$(swift build --package-path tests/android-link --static-swift-stdlib --show-bin-path)/AndroidLinkInterop" /interop/'
}

gradle() {
  local dir="$1"; shift
  local filters=()
  for t in "${INTEROP_TESTS[@]}"; do filters+=(--tests "*RustDesktopInteropTest.$t"); done
  docker run --rm "${NET[@]}" "${ENVS[@]}" -e COUCOU_RUST_INTEROP=1 -e JAVA_TOOL_OPTIONS="$JAVA_OPTS" \
    -v "$PWD":/repo -v coucou-android-link-gradle:/root/.gradle -v coucou-android-link-interop:/interop \
    -w /repo "$IMG-android" android/gradlew -p "$dir" --console=plain "$@" "${filters[@]}"
}

interop() {
  image android
  gradle docker/android-link/kotlin-client test
}

android_build() {
  image android
  gradle android testDebugUnitTest
}

case "$WHAT" in
  core) core ;;
  interop) interop ;;
  android) android_build ;;
  all) core; interop ;;
  *) echo "usage: $0 [all|core|interop|android]" >&2; exit 2 ;;
esac
