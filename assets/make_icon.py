"""Draw the app icon: the face of a 12V-2x6 connector, pins tinted cool to warm.

    python -I make_icon.py <out-dir>
Writes icon.ico (16-256 px, PNG entries; small sizes use a simplified 2x3-pin drawing) and preview.png.
"""
import io
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent
SS = 8  # supersampling factor

COOL = (57, 135, 229)    # chart blue
WARM = (235, 104, 52)    # chart orange
TILE_TOP = (38, 50, 68)
TILE_BOTTOM = (17, 23, 32)
BODY = (46, 58, 77)
BODY_EDGE = (88, 104, 128)


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def oklch_to_rgb(L, C, h):
    import math
    a, b = C * math.cos(math.radians(h)), C * math.sin(math.radians(h))
    l_, m_, s_ = L + 0.3963377774 * a + 0.2158037573 * b, L - 0.1055613458 * a - 0.0638541728 * b, \
        L - 0.0894841775 * a - 1.2914855480 * b
    l, m, s = l_ ** 3, m_ ** 3, s_ ** 3
    rgb = (4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
           -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
           -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s)
    enc = lambda x: 12.92 * x if x <= 0.0031308 else 1.055 * x ** (1 / 2.4) - 0.055
    return tuple(max(0, min(255, round(enc(max(0.0, v)) * 255))) for v in rgb)


def thermal(t):
    """Cool blue to warm orange the vivid way round (violet, magenta, red), not through gray."""
    hue = 255 + 150 * t          # 255 (blue) -> 405 = 45 (orange)
    return oklch_to_rgb(0.64 + 0.06 * t, 0.19, hue % 360)


def draw(size, cols):
    s = size * SS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Tile: rounded square with a vertical gradient.
    pad = round(s * 0.04)
    radius = round(s * 0.22)
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle((pad, pad, s - pad - 1, s - pad - 1), radius, fill=255)
    grad = Image.new("RGBA", (s, s))
    gd = ImageDraw.Draw(grad)
    for y in range(s):
        gd.line((0, y, s, y), fill=lerp(TILE_TOP, TILE_BOTTOM, y / s) + (255,))
    img.paste(grad, (0, 0), mask)
    # A lighter hairline so the dark tile still has an edge on dark taskbars and pages.
    ring = max(SS, round(s * 0.012))
    d.rounded_rectangle((pad, pad, s - pad - 1, s - pad - 1), radius, outline=(78, 94, 118, 255), width=ring)

    # Connector body. Tiny sizes skip the outline: at 16-24 px it would eat the space the pins need.
    small = cols == 3
    tiny = size <= 24
    bw, bh = s * (0.70 if small else 0.74), s * (0.44 if small else 0.36)
    cx, cy = s / 2, s * (0.54 if not small else 0.52)
    x0, y0, x1, y1 = cx - bw / 2, cy - bh / 2, cx + bw / 2, cy + bh / 2
    edge = max(SS, round(s * 0.018))
    if not tiny:
        d.rounded_rectangle((x0, y0, x1, y1), round(s * 0.07), fill=BODY, outline=BODY_EDGE, width=edge)
    if not small:
        # Latch on top, as on the real connector.
        lw, lh = bw * 0.30, s * 0.07
        d.rounded_rectangle((cx - lw / 2, y0 - lh, cx + lw / 2, y0 + edge), round(s * 0.025), fill=BODY_EDGE)

    # Pins: 2 rows, tinted left (cool) to right (warm).
    inner = bw * (0.96 if tiny else 0.84)
    gap = inner / cols
    pin = gap * (0.74 if tiny else 0.62 if small else 0.58)
    row_gap = bh * (0.56 if tiny else 0.40)
    for r in range(2):
        py = cy + (r - 0.5) * row_gap
        for c in range(cols):
            px = cx - inner / 2 + gap * (c + 0.5)
            color = thermal(c / (cols - 1))
            d.rounded_rectangle((px - pin / 2, py - pin / 2, px + pin / 2, py + pin / 2),
                                round(pin * 0.28), fill=color + (255,))
    return img.resize((size, size), Image.Resampling.LANCZOS)


def build():
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    images = {n: draw(n, 3 if n <= 32 else 6) for n in sizes}

    # ICO with PNG-compressed entries.
    blobs = []
    for n in sizes:
        buf = io.BytesIO()
        images[n].save(buf, "PNG")
        blobs.append(buf.getvalue())
    header = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = b""
    for n, blob in zip(sizes, blobs):
        entries += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "icon.ico").write_bytes(header + entries + b"".join(blobs))

    # Preview sheet, light row and dark row: sizes 16-128 at 1:1 (bottom-aligned), 16 and 32 enlarged
    # pixel-exact, then 256.
    row_h, w = 300, 1180
    sheet = Image.new("RGB", (w, row_h * 2), (255, 255, 255))
    ImageDraw.Draw(sheet).rectangle((0, row_h, w, row_h * 2), fill=(13, 17, 23))
    for y in (0, row_h):
        x, base = 22, y + 22 + 256
        for n in sizes[:-1]:
            sheet.paste(images[n], (x, base - n), images[n])
            x += n + 18
        for n, k in ((16, 6), (32, 4)):
            big = images[n].resize((n * k, n * k), Image.Resampling.NEAREST)
            sheet.paste(big, (x, base - n * k), big)
            x += n * k + 18
        sheet.paste(images[256], (x, base - 256), images[256])
    sheet.save(OUT / "preview.png")


build()
print(OUT / "icon.ico")
