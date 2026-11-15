#!/usr/bin/env python3
"""Turn the game's 32x32 Windows icon (System/hp.ico) into a macOS .icns, scaled with sharp pixels.

Usage: python3 tools/ico_to_icns.py gamedata/System/hp.ico out.icns
       python3 tools/ico_to_icns.py --placeholder out.icns   (no game data: a plain "HP1" icon)
"""
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


def write_png(path, pixels, size):
    src = len(pixels)
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        row = pixels[y * src // size]
        for x in range(size):
            raw.extend(row[x * src // size])

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


# 3x5 pixel glyphs for the placeholder icon
GLYPHS = {
    "H": ["X.X", "X.X", "XXX", "X.X", "X.X"],
    "P": ["XX.", "X.X", "XX.", "X..", "X.."],
    "1": [".X.", "XX.", ".X.", ".X.", "XXX"],
}


def placeholder_icon():
    """32x32 pixel art: gold "HP1" on a dark blue rounded square. Used until the game data is installed."""
    size = 32
    pixels = [[(0, 0, 0, 0)] * size for _ in range(size)]
    for y in range(1, size - 1):
        for x in range(1, size - 1):
            corner = min(x - 1, size - 2 - x) + min(y - 1, size - 2 - y)
            if corner >= 2 or (min(x - 1, size - 2 - x) >= 2 or min(y - 1, size - 2 - y) >= 2):
                shade = 40 + (y * 30) // size
                pixels[y][x] = (18, 24, shade + 40, 255)
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


def main():
    if sys.argv[1] == "--placeholder":
        pixels = placeholder_icon()
    else:
        pixels = decode_ico(open(sys.argv[1], "rb").read())
    with tempfile.TemporaryDirectory() as tmp:
        iconset = os.path.join(tmp, "icon.iconset")
        os.mkdir(iconset)
        for size in (16, 32, 128, 256, 512):
            write_png(os.path.join(iconset, f"icon_{size}x{size}.png"), pixels, size)
            write_png(os.path.join(iconset, f"icon_{size}x{size}@2x.png"), pixels, size * 2)
        subprocess.run(["iconutil", "-c", "icns", iconset, "-o", sys.argv[2]], check=True)


if __name__ == "__main__":
    main()
