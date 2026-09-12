#!/bin/bash
set -euo pipefail
CAPY_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
case "${PLATFORM_NAME:-iphoneos}" in
  iphoneos) CAPY_TARGET=aarch64-apple-ios ;;
  iphonesimulator) CAPY_TARGET=aarch64-apple-ios-sim ;;
  macosx) CAPY_TARGET=aarch64-apple-darwin ;;
  *) echo "Unsupported Apple platform: ${PLATFORM_NAME}" >&2; exit 1 ;;
esac
cd "$CAPY_ROOT"
CAPY_PROFILE=dev
if [[ "${CONFIGURATION:-Debug}" == Release ]]; then CAPY_PROFILE=release; fi
if [[ "$CAPY_TARGET" == aarch64-apple-darwin ]]; then
  # Xcode invokes toolchain clang directly; bundled SQLite needs the SDK's
  # standard C headers. Native host and target use the same macOS SDK here.
  export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
else
  # Cross builds resolve host and iOS target SDKs independently.
  unset SDKROOT
fi
cargo build --locked -p layer-apple --target "$CAPY_TARGET" --profile "$CAPY_PROFILE"
