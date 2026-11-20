# Milkrice icon

`artwork.png` is the full-bleed source (generated with ChatGPT image generation via the Codex CLI): kiribath cut in diamonds on a banana-leaf plate, with lunu miris in the centre.

Rebuild every asset (run from this folder):

```bash
python3 build_assets.py artwork.png   # writes out/final/
cargo tauri icon out/final/icon-1024.png -o ../../src-tauri/icons
```

- `icon-1024.png`: masked to the macOS icon grid (824 px body, 185 px radius) with a soft shadow.
- `tray.png`: menu-bar template icon (five rice diamonds, chili as the hole), 44 px.
- `favicon.png`, `apple-touch-icon.png`, `app-icon.webp`: site assets.
