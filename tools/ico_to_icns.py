#!/usr/bin/env python3
"""Turn the game's 32x32 Windows icon (System/hp.ico) into a macOS .icns, scaled with sharp pixels.

Usage: python3 tools/ico_to_icns.py gamedata/System/hp.ico out.icns
       python3 tools/ico_to_icns.py --placeholder out.icns   (no game data: a plain "HP1" icon)
"""
import math
import os
import struct
import subprocess
import sys
import tempfile
import zlib


def decode_ico(data):
    count = struct.unpack("<HHH", data[:6])[2]
    best = None
    for i in range(count):
        w, h, _, _, _, _, size, off = struct.unpack("<BBBBHHII", data[6 + 16 * i:22 + 16 * i])
        if best is None or (w or 256) > best[0]:
            best = (w or 256, h or 256, off)
    w, h, off = best
    hdr = struct.unpack("<IiiHHIIiiII", data[off:off + 40])
    bpp = hdr[4]
    colors = hdr[9] or (1 << bpp if bpp <= 8 else 0)
    pal_off = off + hdr[0]
    palette = [data[pal_off + 4 * i:pal_off + 4 * i + 3] for i in range(colors)]
    xor_off = pal_off + 4 * colors
    xor_pitch = ((w * bpp + 31) // 32) * 4
    and_off = xor_off + xor_pitch * h
    and_pitch = ((w + 31) // 32) * 4
    pixels = []
    for y in range(h):
        src_y = h - 1 - y
        row = []
        for x in range(w):
            if bpp == 32:
                b, g, r, a = data[xor_off + src_y * xor_pitch + 4 * x:][:4]
            else:
                bit = x * bpp
                byte = data[xor_off + src_y * xor_pitch + bit // 8]
                index = (byte >> (8 - bpp - bit % 8)) & ((1 << bpp) - 1)
                b, g, r = palette[index]
                mask = data[and_off + src_y * and_pitch + x // 8] >> (7 - x % 8) & 1
                a = 0 if mask else 255
            row.append((r, g, b, a))
        pixels.append(row)
    return pixels


# 3x5 pixel glyphs for the placeholder icon
GLYPHS = {
    "H": ["X.X", "X.X", "XXX", "X.X", "X.X"],
    "P": ["XX.", "X.X", "XX.", "X..", "X.."],
    "1": [".X.", "XX.", ".X.", ".X.", "XXX"],
}


def placeholder_icon():
    """32x32 pixel art: gold "HP1" on transparent. Used until the game data is installed."""
    size = 32
    pixels = [[(0, 0, 0, 0)] * size for _ in range(size)]
    text, scale, gap = "HP1", 2, 2
    width = len(text) * 3 * scale + (len(text) - 1) * gap
    x0, y0 = (size - width) // 2, (size - 5 * scale) // 2
    for i, ch in enumerate(text):
        for gy, row in enumerate(GLYPHS[ch]):
            for gx, cell in enumerate(row):
                if cell == "X":
                    for dy in range(scale):
                        for dx in range(scale):
                            pixels[y0 + gy * scale + dy][x0 + i * (3 * scale + gap) + gx * scale + dx] = (232, 190, 80, 255)
    return pixels


def key_out_background(art):
    """Icons without transparency have a solid background baked in: make the border color transparent
    where it touches the edge (flood fill). Must match KeyOutBackground in IcoImage.cpp."""
    h, w = len(art), len(art[0])
    if any(p[3] < 255 for row in art for p in row):
        return art
    art = [list(row) for row in art]
    bg = art[0][0]
    stack = [(x, y) for x in range(w) for y in (0, h - 1)] + [(x, y) for y in range(h) for x in (0, w - 1)]
    while stack:
        x, y = stack.pop()
        if 0 <= x < w and 0 <= y < h and art[y][x] == bg:
            art[y][x] = (0, 0, 0, 0)
            stack += [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
    return art


def mac_app_icon(art):
    """Lay out pixel art like a modern macOS app icon (1024x1024): sharp pixels on a rounded-square
    ("squircle", superellipse n=5) night-blue tile of 824x824 with a soft shadow.
    Must match LoadIcoAsMacAppIcon in engine/SurrealEngine/Utils/IcoImage.cpp."""
    size, center, half = 1024, 512.0, 412.0
    art = key_out_background(art)
    h, w = len(art), len(art[0])
    scale = max(1, 592 // max(w, h))
    art_w, art_h = w * scale, h * scale
    art_x, art_y = (size - art_w) // 2, (size - art_h) // 2

    def superellipse(x, y, cx, cy, r):
        return (abs(x - cx) / r) ** 5 + (abs(y - cy) / r) ** 5

    rows = []
    for y in range(size):
        row = bytearray()
        t = (y - (center - half)) / (2 * half)
        for x in range(size):
            f = superellipse(x + 0.5, y + 0.5, center, center, half)
            if f < 0.97:
                cover = 1.0
            elif f > 1.03:
                cover = 0.0
            else:
                hits = sum(superellipse(x + (sx + 0.5) / 4, y + (sy + 0.5) / 4, center, center, half) <= 1.0
                           for sy in range(4) for sx in range(4))
                cover = hits / 16
            sf = superellipse(x + 0.5, y + 0.5, center, center + 12, half + 6)
            shadow = 0.35 * min(max((1.12 - sf) / 0.24, 0.0), 1.0)
            if cover == 0.0:
                row += bytes((0, 0, 0, round(shadow * 255)))
                continue
            glow = max(0.0, 1.0 - math.hypot(x - center, y - center) / 420)
            r = 54 * (1 - t) + 14 * t + 40 * glow * glow
            g = 48 * (1 - t) + 12 * t + 34 * glow * glow
            b = 128 * (1 - t) + 46 * t + 50 * glow * glow
            if art_x <= x < art_x + art_w and art_y <= y < art_y + art_h:
                ar, ag, ab, aa = art[(y - art_y) // scale][(x - art_x) // scale]
                k = aa / 255
                r, g, b = ar * k + r * (1 - k), ag * k + g * (1 - k), ab * k + b * (1 - k)
            a = cover + shadow * (1 - cover)
            # straight (non-premultiplied) alpha for PNG
            row += bytes((min(255, round(r * cover / a)), min(255, round(g * cover / a)), min(255, round(b * cover / a)), round(a * 255)))
        rows.append(bytes(row))
    return rows


def write_png_rows(path, rows):
    size = len(rows)
    raw = b"".join(b"\x00" + row for row in rows)

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def main():
    if sys.argv[1] == "--placeholder":
        art = placeholder_icon()
    else:
        art = decode_ico(open(sys.argv[1], "rb").read())
    with tempfile.TemporaryDirectory() as tmp:
        master = os.path.join(tmp, "icon_1024.png")
        write_png_rows(master, mac_app_icon(art))
        iconset = os.path.join(tmp, "icon.iconset")
        os.mkdir(iconset)
        for size in (16, 32, 128, 256, 512):
            for name, px in ((f"icon_{size}x{size}.png", size), (f"icon_{size}x{size}@2x.png", size * 2)):
                subprocess.run(["sips", "-z", str(px), str(px), master, "--out", os.path.join(iconset, name)],
                               check=True, stdout=subprocess.DEVNULL)
        subprocess.run(["iconutil", "-c", "icns", iconset, "-o", sys.argv[2]], check=True)


if __name__ == "__main__":
    main()
