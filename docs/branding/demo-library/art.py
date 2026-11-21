"""Procedural cover and artist art for the demo library.

Eight families, one per broad genre group. Each is seeded from the title, so
the same title always yields the same SVG. Everything is flat geometry,
gradients and type: no raster assets, nothing that could resemble real artwork.
"""

import colorsys
import hashlib
import html
import math
import random

S = 600
SANS = "'Helvetica Neue', Helvetica, Arial, sans-serif"
SERIF = "Georgia, 'Times New Roman', serif"


def rng_for(seed):
    return random.Random(
        int.from_bytes(hashlib.sha256(seed.encode()).digest()[:8], "big")
    )


def hsl(h, s, l):
    r, g, b = colorsys.hls_to_rgb((h % 360) / 360, max(0, min(1, l)), max(0, min(1, s)))
    return f"#{int(r * 255):02x}{int(g * 255):02x}{int(b * 255):02x}"


def wrap(text, width):
    lines, cur = [], ""
    for w in text.split():
        if cur and len(cur) + 1 + len(w) > width:
            lines.append(cur)
            cur = w
        else:
            cur = f"{cur} {w}".strip()
    if cur:
        lines.append(cur)
    return lines


def text_block(
    lines,
    x,
    y,
    size,
    fill,
    family=SANS,
    weight=700,
    anchor="start",
    spacing=0,
    lh=1.05,
    upper=False,
    opacity=1,
):
    out = []
    for i, ln in enumerate(lines):
        ln = html.escape(ln.upper() if upper else ln)
        out.append(
            f'<text x="{x}" y="{y + i * size * lh:.1f}" font-family="{family}" font-weight="{weight}" '
            f'font-size="{size}" fill="{fill}" text-anchor="{anchor}" letter-spacing="{spacing}" opacity="{opacity}">{ln}</text>'
        )
    return "".join(out)


def fit_size(lines, box, maximum, minimum, factor=0.58):
    longest = max(len(x) for x in lines)
    return max(minimum, min(maximum, box / (longest * factor)))


def svg(body, defs=""):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {S} {S}" width="{S}" height="{S}">'
        f"<defs>{defs}</defs>{body}</svg>"
    )


def sunset(r, title, artist):
    h = r.randint(240, 340)
    top, mid, bot = hsl(h - 30, 0.7, 0.10), hsl(h, 0.75, 0.30), hsl(h + 50, 0.9, 0.55)
    sun_a, sun_b = hsl(h + 70, 1, 0.62), hsl(h + 20, 0.95, 0.52)
    horizon = 360
    defs = (
        f'<linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{top}"/>'
        f'<stop offset="0.65" stop-color="{mid}"/><stop offset="1" stop-color="{bot}"/></linearGradient>'
        f'<linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{sun_a}"/>'
        f'<stop offset="1" stop-color="{sun_b}"/></linearGradient>'
        f'<mask id="m"><rect width="{S}" height="{S}" fill="#fff"/>'
        + "".join(
            f'<rect x="0" y="{horizon - 10 - i * 24 + i * i * 1.6:.1f}" width="{S}" height="{2 + i * 1.8:.1f}" fill="#000"/>'
            for i in range(1, 8)
        )
        + "</mask>"
    )
    body = f'<rect width="{S}" height="{S}" fill="url(#g)"/>'
    for _ in range(30):
        body += f'<circle cx="{r.randint(0, S)}" cy="{r.randint(0, 230)}" r="{r.choice([1, 1.5, 2])}" fill="#fff" opacity="{r.uniform(0.3, 0.9):.2f}"/>'
    body += (
        f'<circle cx="300" cy="{horizon - 20}" r="150" fill="url(#s)" mask="url(#m)"/>'
    )
    body += f'<rect y="{horizon}" width="{S}" height="{S - horizon}" fill="{hsl(h - 20, 0.8, 0.07)}"/>'
    for i in range(-10, 11):
        body += f'<line x1="300" y1="{horizon}" x2="{300 + i * 90}" y2="{S}" stroke="{sun_a}" stroke-width="1.5" opacity=".55"/>'
    for k in range(1, 9):
        y = horizon + (k / 8) ** 2 * (S - horizon)
        body += f'<line x1="0" y1="{y:.1f}" x2="{S}" y2="{y:.1f}" stroke="{sun_a}" stroke-width="1.5" opacity=".55"/>'
    if title:
        lines = wrap(title, 14)
        size = fit_size(lines, 500, 54, 30, 0.62)
        body += text_block(
            lines,
            300,
            78,
            size,
            "#fff",
            anchor="middle",
            spacing=4,
            upper=True,
            weight=800,
        )
        body += text_block(
            [artist],
            300,
            560,
            20,
            "#fff",
            anchor="middle",
            spacing=6,
            upper=True,
            weight=500,
            opacity=0.85,
        )
    return svg(body, defs)


def rings(r, title, artist):
    h = r.randrange(360)
    bg = hsl(h, 0.55, 0.09)
    a, b = hsl(h, 0.9, 0.55), hsl(h + 180, 0.7, 0.6)
    cx, cy = r.choice([220, 300, 380]), r.choice([260, 300, 340])
    body = f'<rect width="{S}" height="{S}" fill="{bg}"/>'
    n = r.randint(7, 11)
    for i in range(n):
        rad = 40 + i * 34
        col = a if i % 2 == 0 else b
        sw = 3 + (i % 3) * 5
        body += f'<circle cx="{cx}" cy="{cy}" r="{rad}" fill="none" stroke="{col}" stroke-width="{sw}" opacity="{0.95 - i * 0.05:.2f}"/>'
    body += f'<circle cx="{cx}" cy="{cy}" r="26" fill="{b}"/>'
    for i in range(8):
        ang = math.radians(r.randint(0, 359))
        rad = 40 + r.randint(0, n - 1) * 34
        body += f'<circle cx="{cx + rad * math.cos(ang):.1f}" cy="{cy + rad * math.sin(ang):.1f}" r="9" fill="#fff"/>'
    if title:
        lines = wrap(title, 12)
        size = fit_size(lines, 330, 40, 26, 0.6)
        body += text_block(
            lines,
            36,
            540 - (len(lines) - 1) * size * 1.05,
            size,
            "#fff",
            spacing=2,
            upper=True,
            weight=800,
        )
        body += f'<rect width="{S}" height="84" fill="{bg}" opacity=".92"/>'
        body += text_block([artist], 36, 52, 18, a, spacing=5, upper=True, weight=600)
    return svg(body)


def blocks(r, title, artist):
    h = r.randrange(360)
    bg = hsl(h, 0.5, 0.88)
    ink = hsl(h + 10, 0.55, 0.12)
    c1, c2, c3 = hsl(h, 0.8, 0.62), hsl(h + 150, 0.7, 0.5), hsl(h + 40, 0.9, 0.58)
    body = f'<rect width="{S}" height="{S}" fill="{bg}"/>'
    body += f'<rect x="0" y="0" width="{r.choice([200, 260, 300])}" height="{S}" fill="{c1}"/>'
    body += f'<circle cx="{r.choice([380, 420, 440])}" cy="{r.choice([350, 365])}" r="{r.choice([85, 100])}" fill="{c3}"/>'
    body += f'<rect x="{r.choice([300, 340])}" y="{r.choice([300, 320])}" width="300" height="{r.choice([36, 50])}" fill="{c2}"/>'
    body += f'<rect x="0" y="{r.choice([480, 500])}" width="{S}" height="{S}" fill="{ink}"/>'
    if title:
        lines = wrap(title, 11)
        size = fit_size(lines, 330, 62, 34, 0.6)
        total = (len(lines) - 1) * size * 1.0
        body += text_block(
            lines,
            30,
            120 - 0 + 0,
            size,
            ink,
            spacing=-1,
            upper=True,
            weight=900,
            lh=1.0,
        )
        body += text_block([artist], 30, 556, 24, bg, spacing=5, upper=True, weight=600)
    return svg(body)


def landscape(r, title, artist):
    h = r.randrange(360)
    sky_a, sky_b = hsl(h + 10, 0.65, 0.82), hsl(h - 10, 0.75, 0.68)
    defs = f'<linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{sky_a}"/><stop offset="1" stop-color="{sky_b}"/></linearGradient>'
    body = f'<rect width="{S}" height="{S}" fill="url(#g)"/>'
    body += f'<circle cx="{r.randint(150, 450)}" cy="{r.randint(250, 330)}" r="{r.randint(60, 90)}" fill="{hsl(h - 5, 0.9, 0.93)}" opacity=".9"/>'
    layers = 5
    for i in range(layers):
        base = 330 + i * 52
        amp = 40 - i * 4
        ph = r.uniform(0, 6.28)
        pts = [f"0,{S}"]
        for x in range(0, S + 20, 20):
            y = base + amp * math.sin(x / 95 + ph) + 14 * math.sin(x / 41 + ph * 2)
            pts.append(f"{x},{y:.1f}")
        pts.append(f"{S},{S}")
        col = hsl(h + i * 12, 0.45 - i * 0.04, 0.60 - i * 0.11)
        body += f'<polygon points="{" ".join(pts)}" fill="{col}"/>'
    if title:
        lines = wrap(title, 16)
        size = fit_size(lines, 500, 54, 32, 0.56)
        ink = hsl(h, 0.5, 0.14)
        body += text_block(
            lines, 300, 82, size, ink, family=SERIF, anchor="middle", weight=700, lh=1.1
        )
        body += text_block(
            [artist],
            300,
            82 + len(lines) * size * 1.1 + 6,
            19,
            ink,
            family=SERIF,
            anchor="middle",
            weight=400,
            spacing=3,
            upper=True,
            opacity=0.8,
        )
    return svg(body, defs)


def sigil(r, title, artist):
    h = r.randrange(360)
    bg = "#0a0a0b"
    fg = hsl(h, 0.08, 0.88)
    acc = hsl(h, 0.8, 0.45)
    cx, cy = 300, 285
    body = f'<rect width="{S}" height="{S}" fill="{bg}"/>'
    body += f'<rect x="22" y="22" width="{S - 44}" height="{S - 44}" fill="none" stroke="{fg}" stroke-width="2" opacity=".7"/>'
    n = r.choice([5, 6, 7, 8, 9])
    for k in range(r.randint(22, 36)):
        ang = k / 28 * 2 * math.pi
        L = r.randint(150, 215)
        body += f'<line x1="{cx + 70 * math.cos(ang):.1f}" y1="{cy + 70 * math.sin(ang):.1f}" x2="{cx + L * math.cos(ang):.1f}" y2="{cy + L * math.sin(ang):.1f}" stroke="{fg}" stroke-width="1.6" opacity=".35"/>'
    for j, rad in enumerate([175, 135, 95]):
        rot = r.uniform(0, 6.28)
        pts = " ".join(
            f"{cx + rad * math.cos(rot + i * 2 * math.pi / n * (2 if j == 1 else 1)):.1f},{cy + rad * math.sin(rot + i * 2 * math.pi / n * (2 if j == 1 else 1)):.1f}"
            for i in range(n)
        )
        body += f'<polygon points="{pts}" fill="none" stroke="{acc if j == 1 else fg}" stroke-width="{3 if j == 1 else 2}"/>'
    body += f'<circle cx="{cx}" cy="{cy}" r="175" fill="none" stroke="{fg}" stroke-width="1.5" opacity=".6"/>'
    body += f'<circle cx="{cx}" cy="{cy}" r="38" fill="{acc}"/><circle cx="{cx}" cy="{cy}" r="14" fill="{bg}"/>'
    if title:
        lines = wrap(title, 18)
        size = fit_size(lines, 440, 30, 20, 0.78)
        body += text_block(
            lines,
            300,
            540 - (len(lines) - 1) * size * 1.2,
            size,
            fg,
            family=SERIF,
            anchor="middle",
            spacing=5,
            upper=True,
            weight=700,
            lh=1.2,
        )
        body += text_block(
            [artist],
            300,
            62,
            17,
            fg,
            family=SERIF,
            anchor="middle",
            spacing=8,
            upper=True,
            weight=400,
            opacity=0.75,
        )
    return svg(body)


def halftone(r, title, artist):
    h = r.randrange(360)
    bg = hsl(h, 0.9, 0.58)
    ink = hsl(h + 200, 0.5, 0.10)
    alt = hsl(h + 180, 0.7, 0.55)
    defs = f'<radialGradient id="g" cx="0.5" cy="0.5" r="0.7"><stop offset="0" stop-color="{alt}"/><stop offset="1" stop-color="{hsl(h, 0.8, 0.5)}"/></radialGradient>'
    body = f'<rect width="{S}" height="{S}" fill="{bg}"/>'
    ox, oy = r.choice([180, 300, 420]), r.choice([230, 280])
    step = 20
    for gx in range(0, S + step, step):
        for gy in range(0, S + step, step):
            d = math.hypot(gx - ox, gy - oy)
            rad = max(0, step / 2 * (1 - d / 440)) * 1.25
            if rad > 0.8:
                body += f'<circle cx="{gx + (step / 2 if (gy // step) % 2 else 0)}" cy="{gy}" r="{rad:.1f}" fill="{ink}" opacity=".85"/>'
    body += f'<circle cx="{ox}" cy="{oy}" r="{r.choice([85, 100, 115])}" fill="{alt}" stroke="{ink}" stroke-width="8"/>'
    if title:
        lines = wrap(title, 10)
        size = fit_size(lines, 340, 70, 36, 0.6)
        body += f'<rect x="0" y="{S - 40 - len(lines) * size * 1.0 - 16:.0f}" width="{S}" height="{S}" fill="{ink}"/>'
        body += text_block(
            lines,
            30,
            S - 40 - (len(lines) - 1) * size * 1.0 - 4,
            size,
            bg,
            upper=True,
            weight=900,
            lh=1.0,
            spacing=-1,
        )
    return svg(body, defs)


def haze(r, title, artist):
    h = r.randrange(360)
    bg_a, bg_b = hsl(h, 0.55, 0.14), hsl(h + 30, 0.6, 0.30)
    defs = (
        f'<linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{bg_a}"/><stop offset="1" stop-color="{bg_b}"/></linearGradient>'
        '<filter id="b" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="46"/></filter>'
    )
    body = f'<rect width="{S}" height="{S}" fill="url(#g)"/><g filter="url(#b)">'
    for _ in range(5):
        body += f'<circle cx="{r.randint(60, 540)}" cy="{r.randint(60, 540)}" r="{r.randint(90, 190)}" fill="{hsl(h + r.randint(-50, 70), 0.8, r.uniform(0.4, 0.65))}" opacity="{r.uniform(0.5, 0.85):.2f}"/>'
    body += "</g>"
    y = r.randint(330, 400)
    body += f'<rect y="{y}" width="{S}" height="2" fill="#fff" opacity=".35"/>'
    body += (
        f'<circle cx="{r.randint(180, 420)}" cy="{y}" r="5" fill="#fff" opacity=".9"/>'
    )
    if title:
        lines = wrap(title, 16)
        size = fit_size(lines, 480, 40, 26, 0.66)
        body += text_block(
            lines,
            40,
            520 - (len(lines) - 1) * size * 1.1,
            size,
            "#fff",
            weight=300,
            spacing=6,
            upper=True,
            lh=1.1,
            opacity=0.95,
        )
        body += text_block(
            [artist], 40, 58, 16, "#fff", weight=400, spacing=7, upper=True, opacity=0.7
        )
    return svg(body, defs)


def arches(r, title, artist):
    h = r.randrange(360)
    bg = hsl(h + 20, 0.4, 0.92)
    ink = hsl(h, 0.6, 0.14)
    cols = [
        hsl(h, 0.8, 0.5),
        hsl(h + 40, 0.9, 0.6),
        hsl(h + 180, 0.65, 0.5),
        hsl(h - 40, 0.7, 0.35),
    ]
    body = f'<rect width="{S}" height="{S}" fill="{bg}"/>'
    n = r.choice([3, 4])
    w = S / n
    for i in range(n):
        top = r.randint(150, 300)
        c = cols[(i + r.randint(0, 3)) % 4]
        body += f'<path d="M{i * w},{S} L{i * w},{top + w / 2} A{w / 2},{w / 2} 0 0 1 {(i + 1) * w},{top + w / 2} L{(i + 1) * w},{S} Z" fill="{c}"/>'
        body += f'<circle cx="{i * w + w / 2}" cy="{top + w / 2}" r="{w / 5}" fill="{bg}" opacity=".9"/>'
    body += f'<rect y="0" width="{S}" height="130" fill="{bg}"/>'
    if title:
        lines = wrap(title, 22)
        size = fit_size(lines, 520, 40, 26, 0.58)
        body += text_block(lines, 30, 66, size, ink, weight=800, spacing=-0.5, lh=1.05)
        body += text_block(
            [artist],
            30,
            66 + len(lines) * size * 1.05 + 4,
            18,
            ink,
            weight=500,
            spacing=4,
            upper=True,
            opacity=0.75,
        )
    return svg(body)


FAMILIES = {
    "sunset": sunset,
    "rings": rings,
    "blocks": blocks,
    "landscape": landscape,
    "sigil": sigil,
    "halftone": halftone,
    "haze": haze,
    "arches": arches,
}


def cover(family, title, artist):
    return FAMILIES[family](rng_for("cover|" + title), title, artist)


def artist_image(family, name):
    """Abstract art from the artist's family with a monogram."""
    r = rng_for("artist|" + name)
    base = FAMILIES[family](r, None, None)
    letters = "".join(w[0] for w in name.replace("&", " ").split() if w[0].isalpha())[
        :2
    ].upper()
    mono = (
        f'<circle cx="300" cy="300" r="170" fill="#000" opacity=".28"/>'
        f'<text x="300" y="352" font-family="{SANS}" font-weight="800" font-size="150" fill="#fff" text-anchor="middle" letter-spacing="4">{html.escape(letters)}</text>'
    )
    return base.replace("</svg>", mono + "</svg>")
