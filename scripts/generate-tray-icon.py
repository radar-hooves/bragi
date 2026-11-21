#!/usr/bin/env python3
"""Generate the macOS menu-bar template icon from the master app icon.

The Bragi mark (the gold Tonlist rune) exists only as a raster master at
static/icon.png -- there is no vector source. This script lifts the rune off
its dark tile by goldness, producing the black-with-alpha silhouette macOS
template icons require (the system tints them for light/dark menu bars),
then writes the @1x/@2x pair the tray embeds, plus the gold in-app
placeholder variant:

    src-tauri/icons/tray-22.png   (22x22, @1x)
    src-tauri/icons/tray-44.png   (44x44, @2x -- the one tray.rs embeds)
    static/bragi-rune.png         (512x512 gold rune, no tile -- the UI's
                                   artwork placeholder / LCD idle mark)

Mirrors the approach proven in the adjacent Thoth project
(scripts/generate_icons.py there): silhouette -> bbox crop -> square pad
with margin -> box-filtered downsample.

Standard library only (own PNG codec) -- runs with any python3, no
dependencies, no network.
"""

import struct
import zlib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SOURCE = REPO / "static" / "icon.png"
OUT_DIR = REPO / "src-tauri" / "icons"

# The rune is a tall, narrow glyph (~1:2.3 W:H), so unlike a squarish mark
# its optical size is carried almost entirely by its height — at the usual
# ~82% fill it reads noticeably smaller than neighbouring status items.
# Fill 95% of the canvas so the rune stands the full menu-bar item height.
GLYPH_FILL = 0.95

# Gold ramp: brightness alone cannot separate the rune from the master's
# white tile (both have high R), so goldness is the product of two ramps:
# brightness (R) and warm chroma (R - B). Gold is bright AND warm
# (R=204, B=64 -> R-B=140); the white tile is bright but neutral (R-B~0);
# the shadowed tile edge is warm-ish but dark. Blended edge pixels land
# partway up the ramps, giving anti-aliased edges for free.
RAMP_LO, RAMP_HI = 60, 190
CHROMA_LO, CHROMA_HI = 40, 120

BRAND_GOLD = (204, 147, 64)  # #CC9340


def read_png(path: Path) -> tuple[int, int, bytearray]:
    """Decode a PNG to (width, height, flat RGBA bytes). 8-bit only."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path} is not a PNG")
    pos, width, height, colour_type, idat = 8, 0, 0, 0, b""
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        ctype = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if ctype == b"IHDR":
            width, height, depth, colour_type, _, _, interlace = struct.unpack(
                ">IIBBBBB", body
            )
            if depth != 8 or interlace != 0 or colour_type not in (2, 6):
                raise SystemExit(
                    f"{path}: only 8-bit non-interlaced RGB/RGBA supported"
                )
        elif ctype == b"IDAT":
            idat += body
        elif ctype == b"IEND":
            break
        pos += 12 + length

    channels = 4 if colour_type == 6 else 3
    stride = width * channels
    raw = zlib.decompress(idat)

    # Un-filter scanlines (PNG filters 0-4: None, Sub, Up, Average, Paeth).
    out = bytearray(height * stride)
    prev = bytearray(stride)
    for y in range(height):
        fbyte = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        if fbyte == 1:  # Sub
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif fbyte == 2:  # Up
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif fbyte == 3:  # Average
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + (left + prev[i]) // 2) & 0xFF
        elif fbyte == 4:  # Paeth
            for i in range(stride):
                a = line[i - channels] if i >= channels else 0
                b = prev[i]
                c = prev[i - channels] if i >= channels else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else b if pb <= pc else c
                line[i] = (line[i] + pred) & 0xFF
        out[y * stride : (y + 1) * stride] = line
        prev = line

    if channels == 3:  # expand RGB to RGBA
        rgba = bytearray(width * height * 4)
        for p in range(width * height):
            rgba[p * 4 : p * 4 + 3] = out[p * 3 : p * 3 + 3]
            rgba[p * 4 + 3] = 255
        return width, height, rgba
    return width, height, out


def write_png(path: Path, width: int, height: int, rgba: bytes) -> None:
    """Encode flat RGBA bytes as an 8-bit PNG (filter 0 throughout)."""
    stride = width * 4
    raw = b"".join(
        b"\x00" + bytes(rgba[y * stride : (y + 1) * stride]) for y in range(height)
    )

    def chunk(ctype: bytes, body: bytes) -> bytes:
        return (
            struct.pack(">I", len(body))
            + ctype
            + body
            + struct.pack(">I", zlib.crc32(ctype + body))
        )

    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def goldness_alpha(r: int, b: int, a: int) -> int:
    """Map a source pixel to silhouette alpha via the gold ramps."""
    bright = (r - RAMP_LO) / (RAMP_HI - RAMP_LO)
    bright = max(0.0, min(1.0, bright))
    warm = ((r - b) - CHROMA_LO) / (CHROMA_HI - CHROMA_LO)
    warm = max(0.0, min(1.0, warm))
    return round(bright * warm * a)


def box_resize_alpha(alpha: list[int], src_w: int, src_h: int, dst: int) -> list[float]:
    """Area-averaged (box filter) resample of an alpha plane to dst x dst."""
    out = []
    for oy in range(dst):
        y0, y1 = oy * src_h / dst, (oy + 1) * src_h / dst
        for ox in range(dst):
            x0, x1 = ox * src_w / dst, (ox + 1) * src_w / dst
            acc = area = 0.0
            for sy in range(int(y0), min(int(y1) + 1, src_h)):
                wy = min(y1, sy + 1) - max(y0, sy)
                if wy <= 0:
                    continue
                row = sy * src_w
                for sx in range(int(x0), min(int(x1) + 1, src_w)):
                    wx = min(x1, sx + 1) - max(x0, sx)
                    if wx <= 0:
                        continue
                    acc += alpha[row + sx] * wx * wy
                    area += wx * wy
            out.append(acc / area if area else 0.0)
    return out


def emit(path: Path, size: int, alpha_plane: list[float], rgb) -> None:
    r, g, b = rgb
    rgba = bytearray(size * size * 4)
    for i, a in enumerate(alpha_plane):
        ai = round(a)
        if ai:
            rgba[i * 4 : i * 4 + 4] = bytes((r, g, b, ai))
    write_png(path, size, size, rgba)
    print(f"wrote {path.relative_to(REPO)} ({size}x{size})")


def main() -> None:
    w, h, src = read_png(SOURCE)

    # Silhouette alpha plane of the gold rune.
    alpha = [
        goldness_alpha(src[p * 4], src[p * 4 + 2], src[p * 4 + 3]) for p in range(w * h)
    ]

    # Bounding box of the rune.
    xs = [p % w for p, a in enumerate(alpha) if a > 10]
    ys = [p // w for p, a in enumerate(alpha) if a > 10]
    if not xs:
        raise SystemExit("no gold pixels found -- ramp thresholds need tuning")
    bx0, bx1, by0, by1 = min(xs), max(xs) + 1, min(ys), max(ys) + 1
    gw, gh = bx1 - bx0, by1 - by0

    # Centred square canvas sized so the glyph's longest side occupies
    # GLYPH_FILL of it.
    side = round(max(gw, gh) / GLYPH_FILL)
    ox, oy = (side - gw) // 2, (side - gh) // 2
    canvas = [0] * (side * side)
    for y in range(gh):
        srow = (by0 + y) * w
        drow = (oy + y) * side + ox
        for x in range(gw):
            canvas[drow + x] = alpha[srow + bx0 + x]

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for size in (44, 22):
        emit(
            OUT_DIR / f"tray-{size}.png",
            size,
            box_resize_alpha(canvas, side, side, size),
            (0, 0, 0),
        )

    # Gold variant for in-app placeholder art (LCD idle mark, artwork
    # fallback): same silhouette, brand gold instead of black, no tile.
    emit(
        REPO / "static" / "bragi-rune.png",
        512,
        box_resize_alpha(canvas, side, side, 512),
        BRAND_GOLD,
    )


if __name__ == "__main__":
    main()
