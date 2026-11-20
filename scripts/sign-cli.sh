#!/bin/sh
# Tauri `beforeBundleCommand`: sign the bundled milkrice-cli (bundle.macOS.files) with the
# release identity. Tauri signs only the main binary and the .app, and notarization rejects
# any unsigned or ad-hoc Mach-O inside the bundle. No-op when APPLE_SIGNING_IDENTITY is unset
# (local dev builds), so it never needs secrets.
set -eu
bin="${MILKRICE_CLI_PATH:-target/release/milkrice-cli}"
if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
  echo "sign-cli: APPLE_SIGNING_IDENTITY unset, leaving $bin unsigned"
  exit 0
fi
codesign --force --timestamp --options runtime --sign "$APPLE_SIGNING_IDENTITY" "$bin"
codesign --verify --strict --verbose=2 "$bin"
