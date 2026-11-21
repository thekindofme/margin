# Releasing Milkrice

A pushed `v*` tag runs `.github/workflows/release.yml` on a macOS 14 (arm64) runner. It builds `milkrice-cli`, bundles the app, signs and notarizes it (when the Apple secrets exist), and creates a **draft** GitHub release containing:

- `Milkrice_<version>_aarch64.dmg`
- `Milkrice.app.tar.gz` and `Milkrice.app.tar.gz.sig` (the updater bundle and its signature)
- `latest.json` (what the in-app updater reads)
- `SHA256SUMS.txt`

Publishing the draft then triggers `.github/workflows/homebrew.yml`, which updates the cask in `thekindofme/homebrew-tap`.

Without the Apple secrets the build still works but is unsigned, and the release body says "Unsigned test build". Don't publish those.

## Cut a release

1. Pick the version (semver). Update it in **all** of: `src-tauri/tauri.conf.json` (`version`), `Cargo.toml` (`[workspace.package] version`) and `.claude-plugin/plugin.json` (`version`; Claude Code plugin users only get updates when it changes). Run `cargo build` so `Cargo.lock` follows.
2. In `CHANGELOG.md`, rename the Unreleased heading to the version and date, and add a fresh Unreleased section.
3. Commit, push to `main` and wait for CI to pass.
4. Tag and push:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
   The workflow fails early if the tag doesn't match `tauri.conf.json`'s version.
5. When the workflow finishes, open the draft release on GitHub. Check the assets are all there, and (on a clean Mac) that the DMG opens without a Gatekeeper warning.
6. Click **Publish release**. The Homebrew workflow runs and pushes the new version and checksum to the tap. Check its run in the Actions tab; re-run it by hand with the `tag` input if needed.
7. Smoke test: `brew update && brew install --cask thekindofme/tap/milkrice`, and from an older build, tray menu > "Check for updates…".

Note: the updater reads `releases/latest/download/latest.json`, which only exists once the release is published (not draft, not prerelease).

## Required secrets

Add under the repository's Settings > Secrets and variables > Actions. Everything except the updater key is optional for an unsigned test build.

| Secret | What it is |
|---|---|
| `APPLE_CERTIFICATE` | Base64 of the Developer ID Application certificate exported as .p12 |
| `APPLE_CERTIFICATE_PASSWORD` | Password you chose when exporting the .p12 |
| `APPLE_SIGNING_IDENTITY` | Certificate name, e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_API_KEY` | App Store Connect API key ID (10 characters) |
| `APPLE_API_ISSUER` | App Store Connect issuer ID (a UUID) |
| `APPLE_API_KEY_P8` | Base64 of the `AuthKey_<ID>.p8` file |
| `TAURI_SIGNING_PRIVATE_KEY` | Updater private key (the contents of the key file) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for the updater key |
| `HOMEBREW_TAP_TOKEN` | Fine-grained PAT for the tap repository |

Signing needs the first three. Notarization additionally needs the three `APPLE_API_*` secrets; without them the build is signed but not notarized (the release body says so).

### How to produce each

**Developer ID certificate (`APPLE_CERTIFICATE`, `_PASSWORD`, `APPLE_SIGNING_IDENTITY`)**

1. Join the Apple Developer Program. In the developer portal, Certificates, create a **Developer ID Application** certificate (upload a CSR made in Keychain Access > Certificate Assistant), download it and double-click to install it.
2. In Keychain Access > My Certificates, right-click the certificate (expand it to confirm the private key is attached) > Export, format .p12, and choose a password. That password is `APPLE_CERTIFICATE_PASSWORD`.
3. `base64 -i certificate.p12 | pbcopy` and paste as `APPLE_CERTIFICATE`. Delete the .p12 afterwards.
4. `security find-identity -v -p codesigning` prints the identity name for `APPLE_SIGNING_IDENTITY`.

**Notarization API key (`APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_P8`)**

1. App Store Connect > Users and Access > Integrations > App Store Connect API > Team Keys. Generate a key with the **Developer** role.
2. The key ID is `APPLE_API_KEY`; the issuer ID shown at the top of the page is `APPLE_API_ISSUER`.
3. Download the `AuthKey_<ID>.p8` (only possible once). `base64 -i AuthKey_<ID>.p8 | pbcopy` gives `APPLE_API_KEY_P8`.

**Updater key (`TAURI_SIGNING_PRIVATE_KEY`, `_PASSWORD`)**

The key pair was generated once with `cargo tauri signer generate`. The public half is in `src-tauri/tauri.conf.json` and must never change, or installed apps can no longer verify updates. The private half and its password are kept outside the repo. Keep a backup somewhere safe: losing the key means shipping a manual re-install to every user.

The maintainer keeps every signing secret in one 1Password item, **"Milkrice code signing (Developer ID + updater key)"** (Personal vault): the Developer ID `.p12`, its private key and certificate, the App Store Connect `.p8`, the updater key, all their passwords, and the Key/Issuer IDs. Nothing is left on disk. To refill a secret, read it from 1Password with the item's ID (`op item list | grep Milkrice`):

```bash
op read "op://Personal/<item-id>/updater-key" | gh secret set TAURI_SIGNING_PRIVATE_KEY -R thekindofme/milkrice
```

**Homebrew tap token (`HOMEBREW_TAP_TOKEN`)**

Create a fine-grained personal access token (GitHub > Settings > Developer settings): resource owner is the account that owns the tap, repository access is **only** `thekindofme/homebrew-tap`, permission **Contents: Read and write**. Set a calendar reminder for its expiry.

## Local checks

To confirm bundling and updater artifacts without CI (unsigned):

Local builds (`scripts/build-local.sh`) skip updater artifacts via the git-ignored `src-tauri/tauri.local.conf.json` (`"createUpdaterArtifacts": false`), so no secrets are needed. To test updater artifacts too, pull the key from 1Password for that one command:

```bash
TAURI_SIGNING_PRIVATE_KEY="$(op read "op://Personal/<item-id>/updater-key")" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(op read "op://Personal/<item-id>/<password-field>")" \
cargo tauri build --bundles app,dmg
```

Artifacts land in `target/release/bundle/`.

## Homebrew cask

The cask lives in `thekindofme/homebrew-tap` (`Casks/milkrice.rb`). Create that repository on GitHub and push it once before the first release; after that the workflow keeps `version` and `sha256` current. The initial checksum in the file is a placeholder.
