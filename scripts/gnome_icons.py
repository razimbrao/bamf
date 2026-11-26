#!/usr/bin/env python3
"""Generates the GNOME panel icons (SVG) from the tray artwork in
crates/bamf-server/src/icon.rs, so both machines show the same imp.

Run from the repository root after changing the art:

    python3 scripts/gnome_icons.py
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "crates/bamf-server/src/icon.rs"
OUT = ROOT / "gnome-extension/bamf@razimbrao.github.io/icons"

src = SOURCE.read_text(encoding="utf-8")


def block(start):
    """Text from `start` to the end of that item (the next line `}` or `];`)."""
    i = src.index(start)
    j = re.compile(r"^(\}|\];)", re.M).search(src, i).end()
    return src[i:j]


ART = re.findall(r'"([.A-Za-z]+)"', block("const ART"))
N = len(ART)
SLIT_ROW = int(re.search(r"const SLIT_ROW: usize = (\d+);", src).group(1))
Z = [tuple(map(int, p)) for p in re.findall(r"\((\d+), (\d+)\)", block("const Z:"))]
Z_COLOR = tuple(int(h, 16) for h in re.search(r"const Z_COLOR: Rgb = \[0x(..), 0x(..), 0x(..)\]", src).groups())


def palette(fn):
    body = block(f"fn {fn}(")
    colors = {
        ch: tuple(int(h, 16) for h in rgb)
        for ch, *rgb in re.findall(r"b'(\w)' => \[0x(..), 0x(..), 0x(..)\]", body)
    }
    colors["G"] = tuple(int(h, 16) for h in re.search(r"_ => \[0x(..), 0x(..), 0x(..)\]", body).groups())
    return colors


AWAKE, ASLEEP = palette("awake"), palette("asleep")


def at(x, y):
    return ART[y][x] if 0 <= x < N and 0 <= y < N else "."


def mix(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def pixel(look, x, y):
    """Same rules as `pixel` in icon.rs."""
    if look == "asleep" and (x, y) in Z:
        return Z_COLOR
    ch = at(x, y)
    if ch == ".":
        return None
    if look == "awake":
        c = AWAKE[ch]
        if ch not in "KEG":
            near = [at(x + dx, y + dy) for dx, dy in ((0, -1), (-1, 0), (1, 0), (0, 1))]
            if "E" in near:
                c = mix(c, AWAKE["E"], 0.4)
            elif "G" in near:
                c = mix(c, AWAKE["G"], 0.2)
        return c
    if ch == "E" and y != SLIT_ROW:
        ch = "S"
    return ASLEEP[ch]


def svg(look):
    rects = []
    for y in range(N):
        for x in range(N):
            c = pixel(look, x, y)
            if c:
                rects.append(f'<rect x="{x}" y="{y}" width="1" height="1" fill="#{c[0]:02x}{c[1]:02x}{c[2]:02x}"/>')
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {N} {N}" '
        f'width="{N}" height="{N}" shape-rendering="crispEdges">\n'
        + "\n".join(rects)
        + "\n</svg>\n"
    )


OUT.mkdir(parents=True, exist_ok=True)
for look in ("awake", "dozing", "asleep"):
    path = OUT / f"bamf-{look}.svg"
    path.write_text(svg(look), encoding="utf-8", newline="\n")
    print(f"wrote {path.relative_to(ROOT)}")
