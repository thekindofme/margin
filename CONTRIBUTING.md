# Contributing to Margin

Thanks for helping. Margin is a Rust + Tauri v2 macOS app: the engine lives in `crates/margin-core`, the desktop shell in `src-tauri`, and the UI in `ui` (plain HTML/CSS/JS, no build step).

## Prerequisites

- macOS 14.2 or later on Apple Silicon (audio capture uses ScreenCaptureKit and EventKit, so the app is macOS-only)
- Rust (stable) via [rustup](https://rustup.rs)
- Tauri CLI: `cargo install tauri-cli --version "^2" --locked`
- cmake (`brew install cmake`), needed to build whisper.cpp

## Build and run

```bash
scripts/build-local.sh
open target/release/bundle/macos/Margin.app
```

The script runs `cargo tauri build --bundles app`. macOS ties privacy grants (microphone, calendars, screen and system audio) to the app's code signature, so an ad-hoc build asks again after every rebuild. To avoid that, create a self-signed code-signing certificate in Keychain Access and put its name in `src-tauri/tauri.local.conf.json` (git-ignored):

```json
{"bundle": {"macOS": {"signingIdentity": "My Dev Cert"}}}
```

Release builds are described in [docs/RELEASING.md](docs/RELEASING.md).

## Tests and checks

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
```

CI runs both on every pull request. Please don't add new clippy warnings.

## Headless tools

`margin-cli` runs the engine without the UI:

```bash
cargo run --release -p margin-core --bin margin-cli -- models
cargo run --release -p margin-core --bin margin-cli -- simulate parakeet-v2 mic.wav system.wav claude
```

`simulate` streams two 16 kHz mono WAVs (mic and call audio) through the live pipeline, then names speakers, writes notes and answers a question. Run it without arguments for the full list of `dev` subcommands.

Set `MARGIN_HOME=/some/dir` to point Margin at a throwaway data directory instead of `~/Library/Application Support/Margin`.

### End-to-end self-test hooks

These run the real app binary with test audio in place of live devices (see the README for details):

```bash
MARGIN_SELFTEST=dir/ target/release/margin
MARGIN_TEST_EVENT=50 MARGIN_QUIET_SECS=15 MARGIN_TEST_AUDIO=dir/ target/release/margin
```

`dir/` holds `mic.wav` and `system.wav`. `MARGIN_SELFTEST_MODEL` picks the speech model (default `parakeet-v2`; it must already be downloaded).

## Pull requests

- Keep changes focused, with a short description of what and why.
- Use made-up names and data in tests, fixtures and examples. Never commit real meetings, transcripts, calendar data, keys or tokens.
- Don't bump the version or edit the changelog's released sections; maintainers do that when cutting a release.

By contributing you agree your work is released under the project's MIT license.
