#!/usr/bin/env bash
# Local build signed with a stable personal identity so macOS privacy grants survive rebuilds.
# Put your identity in src-tauri/tauri.local.conf.json (git-ignored), e.g.
#   {"bundle":{"macOS":{"signingIdentity":"My Dev Cert"}}}
# Release builds use APPLE_SIGNING_IDENTITY (Developer ID) instead — see docs/release.
set -euo pipefail
cd "$(dirname "$0")/.."
cfg=()
[ -f src-tauri/tauri.local.conf.json ] && cfg=(--config src-tauri/tauri.local.conf.json)
cargo tauri build --bundles app "${cfg[@]}" "$@"
