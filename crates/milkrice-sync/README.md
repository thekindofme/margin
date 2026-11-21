# milkrice-sync

End-to-end encrypted sync between Milkrice for Mac and Milkrice for iPhone, through a folder both can see (iCloud Drive › Milkrice Sync by default; Google Drive, Dropbox or Syncthing work the same way). There's no Milkrice server. The folder only holds sealed files with random names, and the key lives in each device's keychain.

```bash
cargo build -p milkrice-sync --features desktop --release   # optional: the app has this built in
milkrice-sync init            # create the vault in iCloud Drive/Milkrice Sync
milkrice-sync pair            # QR code + matching 6-digit code on both screens
milkrice-sync serve           # keep syncing; write notes the iPhone asks for with your AI
```

- Format, crypto, pairing and merge: `src/` (the library has no desktop dependencies).
- `tests/vectors/v1.json` is shared with the iOS app's `MilkriceSync` Swift package, and both test suites check it byte for byte.
- Design: `docs/SYNC.md` in the milkrice-ios repo.
- The Milkrice app does all of this itself (Settings › iPhone sync, `src-tauri/src/sync.rs`) through `src/desktop.rs`; this command is for scripting and tests. Don't run `serve` while the app is open.
- `tests/desktop.rs` runs the Mac side end to end against a simulated iPhone.
