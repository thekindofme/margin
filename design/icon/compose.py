#!/usr/bin/env python3
"""Turn full-bleed artwork into macOS icons and a comparison sheet.

usage: python3 tools/compose.py sheet gen/*.png        -> out/sheet.png (each concept at 256/64/32 + menu-bar-ish)
       python3 tools/compose.py icon gen/x.png out/x   -> out/x/icon_1024.png (masked squircle, Apple grid)
"""
import os, sys
from PIL import Image, ImageDraw, ImageFilter, ImageFont

def squircle_mask(size, inset=100, radius=185):
    # Apple's macOS icon grid: 824px body on a 1024 canvas, ~185px corner radius.
    m = Image.new('L', (size * 4, size * 4), 0)
    s = size / 1024 * 4
    ImageDraw.Draw(m).rounded_rectangle([inset * s, inset * s, (1024 - inset) * s, (1024 - inset) * s], radius=radius * s, fill=255)
    return m.resize((size, size), Image.LANCZOS)

def icon(src, size=1024, full_bleed=True):
    art = Image.open(src).convert('RGBA')
    w, h = art.size
    c = min(w, h)
    art = art.crop(((w - c) // 2, (h - c) // 2, (w + c) // 2, (h + c) // 2))
    body = int(size * 824 / 1024)
    off = (size - body) // 2
    art = art.resize((body, body), Image.LANCZOS)
    canvas = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    layer = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    layer.paste(art, (off, off))
    mask = squircle_mask(size)
    # soft drop shadow like Apple's template
    sh = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    sh.putalpha(mask.point(lambda v: v * 0.30))
    sh = sh.filter(ImageFilter.GaussianBlur(size * 0.012))
    canvas.alpha_composite(sh, (0, int(size * 0.01)))
    layer.putalpha(mask)
    canvas.alpha_composite(layer)
    return canvas

def sheet(paths, out):
    tile_w, pad = 460, 24
    cols = 4
    rows = (len(paths) + cols - 1) // cols
    W, H = cols * tile_w + pad, rows * 380 + pad
    for bg, suffix in (((236, 233, 226), 'light'), ((30, 31, 33), 'dark')):
        img = Image.new('RGBA', (W, H), bg + (255,))
        d = ImageDraw.Draw(img)
        try:
            f = ImageFont.truetype('/System/Library/Fonts/SFNS.ttf', 18)
        except Exception:
            f = ImageFont.load_default()
        for i, p in enumerate(paths):
            x = pad + (i % cols) * tile_w
            y = pad + (i // cols) * 380
            ic = icon(p, 1024)
            img.alpha_composite(ic.resize((256, 256), Image.LANCZOS), (x, y))
            img.alpha_composite(ic.resize((64, 64), Image.LANCZOS), (x + 280, y + 40))
            img.alpha_composite(ic.resize((32, 32), Image.LANCZOS), (x + 296, y + 130))
            img.alpha_composite(ic.resize((16, 16), Image.LANCZOS), (x + 304, y + 190))
            name = os.path.splitext(os.path.basename(p))[0]
            d.text((x, y + 270), f"{i + 1}. {name}", fill=(20, 20, 20) if suffix == 'light' else (230, 230, 230), font=f)
        img.convert('RGB').save(out.replace('.png', f'-{suffix}.png'))

if __name__ == '__main__':
    cmd = sys.argv[1]
    os.makedirs('out', exist_ok=True)
    if cmd == 'sheet':
        sheet(sorted(p for p in sys.argv[2:] if '.old' not in p), 'out/sheet.png')
    elif cmd == 'icon':
        os.makedirs(sys.argv[3], exist_ok=True)
        icon(sys.argv[2]).save(os.path.join(sys.argv[3], 'icon_1024.png'))
