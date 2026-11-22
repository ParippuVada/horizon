#!/usr/bin/env python3
"""Builds packaging/icons/minecraft.icns and packaging/icons/minecraft.ico from the clean-room
grass block isometric model.
"""

import os
import random
import shutil
import subprocess
import tempfile
from pathlib import Path
from PIL import Image

CELLS = 12

GRASS = [(0x5F, 0xA8, 0x3A), (0x6D, 0xBB, 0x45), (0x4E, 0x93, 0x30), (0x7C, 0xC6, 0x53)]
DIRT = [(0x8A, 0x5A, 0x3B), (0x7A, 0x4E, 0x33), (0x9B, 0x6A, 0x47), (0x6B, 0x41, 0x29)]
SHADE = {"top": 1.0, "left": 0.82, "right": 0.64}


def pattern(rng, palette, weights):
    return [[rng.choices(palette, weights)[0] for _ in range(CELLS)] for _ in range(CELLS)]


def render_grass_block(size=512, ss=2):
    rng = random.Random(26_3)
    top = pattern(rng, GRASS, [5, 3, 2, 1])
    sides = {}
    for face in ("left", "right"):
        cells = pattern(rng, DIRT, [5, 3, 2, 2])
        for x in range(CELLS):
            depth = 2 + rng.choices([0, 1, 2], [5, 3, 1])[0]
            for y in range(depth):
                cells[y][x] = rng.choices(GRASS, [4, 2, 3, 1])[0]
        sides[face] = cells

    s = size * ss

    def p(x, y):
        return (x * s / 128, y * s / 128)

    t, r, b, l = p(64, 6), p(119, 35), p(64, 64), p(9, 35)
    bd, ld = p(64, 122), p(9, 93)
    faces = [
        ("top", t, r, l, top),
        ("left", l, b, ld, sides["left"]),
        ("right", b, r, bd, sides["right"]),
    ]
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    px = img.load()
    for face, origin, u_end, v_end, cells in faces:
        ux, uy = u_end[0] - origin[0], u_end[1] - origin[1]
        vx, vy = v_end[0] - origin[0], v_end[1] - origin[1]
        det = ux * vy - uy * vx
        for y in range(s):
            for x in range(s):
                dx, dy = x + 0.5 - origin[0], y + 0.5 - origin[1]
                u = (dx * vy - dy * vx) / det
                v = (ux * dy - uy * dx) / det
                if 0 <= u < 1 and 0 <= v < 1:
                    c = cells[int(v * CELLS)][int(u * CELLS)]
                    k = SHADE[face]
                    px[x, y] = (round(c[0] * k), round(c[1] * k), round(c[2] * k), 255)
    return img.resize((size, size), Image.Resampling.BOX if ss == 1 else Image.Resampling.LANCZOS)


def main():
    repo = Path(__file__).resolve().parent.parent
    packaging = repo / "packaging/icons"
    packaging.mkdir(parents=True, exist_ok=True)

    print("Rendering 512x512 grass block master...")
    master = render_grass_block(size=512, ss=2)
    master_1024 = master.resize((1024, 1024), Image.Resampling.NEAREST)

    with tempfile.TemporaryDirectory() as tmpdir:
        iconset = Path(tmpdir) / "minecraft.iconset"
        iconset.mkdir()

        sizes = [
            ("icon_16x16.png", 16),
            ("icon_16x16@2x.png", 32),
            ("icon_32x32.png", 32),
            ("icon_32x32@2x.png", 64),
            ("icon_128x128.png", 128),
            ("icon_128x128@2x.png", 256),
            ("icon_256x256.png", 256),
            ("icon_256x256@2x.png", 512),
            ("icon_512x512.png", 512),
            ("icon_512x512@2x.png", 1024),
        ]

        for filename, sz in sizes:
            if sz == 1024:
                resized = master_1024
            elif sz == 512:
                resized = master
            else:
                resized = master.resize((sz, sz), Image.Resampling.LANCZOS)
            resized.save(iconset / filename)

        icns_path = packaging / "minecraft.icns"
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(icns_path)], check=True)
        print(f"Generated {icns_path} ({icns_path.stat().st_size} bytes)")

    ico_path = packaging / "minecraft.ico"
    ico_sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    master.save(ico_path, format="ICO", sizes=ico_sizes)
    print(f"Generated {ico_path} ({ico_path.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
