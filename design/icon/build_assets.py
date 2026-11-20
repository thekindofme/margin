#!/usr/bin/env python3
"""Build every Milkrice icon asset from the chosen artwork.

usage: python3 tools/build_assets.py gen/r2c-refined-original.png
Writes out/final/: icon-1024.png (masked, for `cargo tauri icon`), tray.png (menu-bar template),
favicon.png, apple-touch-icon.png, app-icon.webp, tray-preview.png.
"""
import os, sys
from PIL import Image, ImageDraw
sys.path.insert(0, os.path.dirname(__file__))
from compose import icon

OUT = 'out/final'

def tray(px=44, ss=8):
    """Five kiribath diamonds (a plus of rhombi) with the chili as a hole in the centre piece. Black on clear."""
    S = px * ss
    im = Image.new('L', (S, S), 0)
    d = ImageDraw.Draw(im)
    c = S / 2
    r = S * 0.175          # half-diagonal of one piece
    gap = S * 0.035        # space between pieces
    step = 2 * r + gap * 1.414
    centres = [(0, 0), (step / 2, -step / 2), (-step / 2, -step / 2), (step / 2, step / 2), (-step / 2, step / 2)]
    # rotate lattice so pieces sit as a big diamond: up/down/left/right neighbours
    centres = [(0, 0), (0, -step / 1.414), (0, step / 1.414), (-step / 1.414, 0), (step / 1.414, 0)]
    for x, y in centres:
        cx, cy = c + x, c + y
        d.polygon([(cx, cy - r), (cx + r, cy), (cx, cy + r), (cx - r, cy)], fill=255)
    dot = r * 0.42
    d.ellipse([c - dot, c - dot, c + dot, c + dot], fill=0)
    alpha = im.resize((px, px), Image.LANCZOS)
    out = Image.new('RGBA', (px, px), (0, 0, 0, 0))
    out.putalpha(alpha)
    return out

def main(src):
    os.makedirs(OUT, exist_ok=True)
    master = icon(src, 1024)
    master.save(f'{OUT}/icon-1024.png')
    master.resize((256, 256), Image.LANCZOS).save(f'{OUT}/app-icon.webp', quality=92)
    master.resize((64, 64), Image.LANCZOS).save(f'{OUT}/favicon.png')
    # apple-touch: iOS adds its own rounding and hates transparency -> full-bleed art on cream
    art = Image.open(src).convert('RGB')
    art.resize((180, 180), Image.LANCZOS).save(f'{OUT}/apple-touch-icon.png')
    t = tray(44)
    t.save(f'{OUT}/tray.png')
    # preview: tray icon on light and dark menu bars at 1x and 2x
    pv = Image.new('RGBA', (240, 60), (0, 0, 0, 0))
    for i, bg in enumerate([(236, 236, 236, 255), (40, 40, 42, 255)]):
        tile = Image.new('RGBA', (120, 60), bg)
        ink = (0, 0, 0) if i == 0 else (255, 255, 255)
        for size, x in ((22, 20), (44, 60)):
            g = t.resize((size, size), Image.LANCZOS)
            col = Image.new('RGBA', g.size, ink + (255,))
            col.putalpha(g.getchannel('A'))
            tile.alpha_composite(col, (x, (60 - size) // 2))
        pv.alpha_composite(tile, (i * 120, 0))
    pv.resize((480, 120), Image.NEAREST).save(f'{OUT}/tray-preview.png')

if __name__ == '__main__':
    main(sys.argv[1])
