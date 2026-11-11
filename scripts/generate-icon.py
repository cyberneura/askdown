#!/usr/bin/env python3
"""Generate the app icon artwork and its layout description.

Outputs:
  resources/app-icons/mac-original.png  fixed 1024x1024 master
  resources/app-icons/layout.json       rectangles the macOS size-specific
                                        preparation (prepare-macos-icons.swift)
                                        cuts the master with
  crates/askdown-cli/icons/icon.png     bundle.icon in tauri.conf.json (a copy)

The icon is generated rather than drawn by hand so that the macOS geometry and
the glyph live in the source as named constants. The layout file is written by
the same run as the artwork so the glyph rectangle cannot drift from the pixels.
Run this after changing anything below, then regenerate the size-specific
images:

    python3 scripts/generate-icon.py
    bash scripts/generate-macos-icons.sh

It only uses the standard library (zlib for the PNG stream). This repository has
no image toolchain, and a single asset does not justify adding one.

    python3 scripts/generate-icon.py --check

renders in memory and compares with the committed files instead of writing
them, for CI. Pixels are compared with a tolerance of one step per channel
rather than byte for byte: the compressed stream depends on the zlib build, and
the trigonometry can differ by an ulp between C libraries, which flips a
rounding at a .5 boundary somewhere in a million pixels.

macOS icon geometry
-------------------
The widely used macOS app icon template (Big Sur and later) draws on a 1024x1024
canvas but does not fill it. The icon body is an 824x824 rounded rectangle
centred on the canvas, leaving a 100px transparent margin on every side, and the
corner radius is 185.4 - 22.5% of the body.

The margin is what keeps icons looking the same size as each other: an icon that
fills its canvas edge to edge is drawn larger than every icon following the
template beside it. It also leaves room for the shadow and for badges.

Apple's real shape is a continuous-curvature "squircle" rather than a circular
arc, but the circular approximation at this radius is what the template grid
uses and is indistinguishable at the sizes an icon is actually seen.

The glyph
---------
A question mark: askdown exists to put a question in front of a person. It is
built from three primitives with signed distance functions - a ring arc with
round caps, a capsule stem and a dot - so the stroke keeps one weight and the
joins are exact at every size. The mark is white on a gradient because the
macOS preparation step recovers the glyph's alpha from how far each pixel's red
channel sits above the background's; a coloured glyph or a background with a
high red component would break that extraction.
"""

import json
import math
import struct
import sys
import zlib
from pathlib import Path

CANVAS = 1024
# See the module docstring: the body is deliberately smaller than the canvas.
BODY = 824
MARGIN = (CANVAS - BODY) / 2  # 100.0
CORNER_RADIUS_RATIO = 0.225
CORNER_RADIUS = BODY * CORNER_RADIUS_RATIO  # 185.4

# A vertical violet gradient. Flat fills read as cheap next to the shaded icons
# around them. Violet keeps the icon apart from runandlog's blue in the Dock.
TOP_COLOR = (0x8B, 0x5C, 0xF6)
BOTTOM_COLOR = (0x5B, 0x21, 0xB6)

# A faint highlight along the top edge, for material rather than for contrast.
# Turning it up makes the icon look plastic.
HIGHLIGHT_ALPHA = 0.16
HIGHLIGHT_SPAN = 0.45  # fraction of the body height over which it fades out

MARK_COLOR = (0xFF, 0xFF, 0xFF)

# Question mark proportions, as fractions of the body. The stroke is heavy so
# the mark survives the 16px rendering, where the preparation step fits the
# glyph into the central 80% of the icon.
STROKE = BODY * 0.095
HOOK_RADIUS = BODY * 0.15  # radius of the stroke's centreline
STEM_LENGTH = BODY * 0.075
DOT_GAP = BODY * 0.06
DOT_RADIUS = STROKE / 2 * 1.2
# The hook's open end, in degrees counter-clockwise from the right, y up. The
# arc runs from the stem join at the bottom (-90) over the top to this angle.
HOOK_OPEN_ANGLE = 200.0

ROOT = Path(__file__).resolve().parent.parent
MASTER_PATH = ROOT / "resources/app-icons/mac-original.png"
LAYOUT_PATH = ROOT / "resources/app-icons/layout.json"
BUNDLE_ICON_PATH = ROOT / "crates/askdown-cli/icons/icon.png"


def rounded_rect_distance(x: float, y: float) -> float:
    """Signed distance to the icon body, negative inside.

    Shrink the rectangle by the corner radius, measure the distance to that, and
    subtract the radius back off; the corners fall out of it for free.
    """
    half = BODY / 2 - CORNER_RADIUS
    dx = abs(x - CANVAS / 2) - half
    dy = abs(y - CANVAS / 2) - half
    outside = math.hypot(max(dx, 0.0), max(dy, 0.0))
    inside = min(max(dx, dy), 0.0)
    return outside + inside - CORNER_RADIUS


class QuestionMark:
    """Signed distance to the question mark, in canvas coordinates (y down).

    The parts are laid out with y up around the hook's centre and flipped at
    the end, which keeps the angles in the usual mathematical orientation.
    """

    def __init__(self) -> None:
        half_width = HOOK_RADIUS + STROKE / 2
        top = half_width
        bottom = -(HOOK_RADIUS + STEM_LENGTH + DOT_GAP + DOT_RADIUS * 2)
        # Centre the mark's bounding box on the canvas.
        self.center_x = CANVAS / 2
        self.center_y = CANVAS / 2 + (top + bottom) / 2
        self.stem_top = -HOOK_RADIUS
        self.stem_bottom = -HOOK_RADIUS - STEM_LENGTH
        self.dot_y = self.stem_bottom - DOT_GAP - DOT_RADIUS
        open_angle = math.radians(HOOK_OPEN_ANGLE)
        self.open_end = (
            HOOK_RADIUS * math.cos(open_angle),
            HOOK_RADIUS * math.sin(open_angle),
        )
        # (x, y, width, height) in canvas pixels, y down.
        self.bounding_box = (
            self.center_x - half_width,
            self.center_y - half_width,
            2 * half_width,
            top - bottom,
        )

    def distance(self, x: float, y: float) -> float:
        px = x - self.center_x
        py = self.center_y - y
        return min(self._hook(px, py), self._stem(px, py), self._dot(px, py))

    def _hook(self, px: float, py: float) -> float:
        angle = math.degrees(math.atan2(py, px))
        # The arc covers -90 .. HOOK_OPEN_ANGLE counter-clockwise; the gap sits
        # at the lower left. Points in the gap measure to the nearer round cap.
        if -90.0 <= angle <= HOOK_OPEN_ANGLE or angle <= HOOK_OPEN_ANGLE - 360.0:
            centreline = abs(math.hypot(px, py) - HOOK_RADIUS)
        else:
            centreline = min(
                math.hypot(px - self.open_end[0], py - self.open_end[1]),
                math.hypot(px, py + HOOK_RADIUS),
            )
        return centreline - STROKE / 2

    def _stem(self, px: float, py: float) -> float:
        clamped = min(max(py, self.stem_bottom), self.stem_top)
        return math.hypot(px, py - clamped) - STROKE / 2

    def _dot(self, px: float, py: float) -> float:
        return math.hypot(px, py - self.dot_y) - DOT_RADIUS


def coverage(distance: float) -> float:
    """Antialiased coverage from a signed distance, over a one-pixel transition."""
    return min(max(0.5 - distance, 0.0), 1.0)


def build_pixels(mark: QuestionMark):
    rows = []
    for py in range(CANVAS):
        y = py + 0.5
        # The gradient and the highlight only vary down the icon, so they are
        # resolved once per row rather than once per pixel.
        t = min(max((y - MARGIN) / BODY, 0.0), 1.0)
        base = [TOP_COLOR[i] + (BOTTOM_COLOR[i] - TOP_COLOR[i]) * t for i in range(3)]
        highlight = max(0.0, 1.0 - t / HIGHLIGHT_SPAN) ** 2 * HIGHLIGHT_ALPHA
        body_color = [c + (255 - c) * highlight for c in base]

        row = bytearray()
        for px in range(CANVAS):
            x = px + 0.5
            body_alpha = coverage(rounded_rect_distance(x, y))
            if body_alpha <= 0.0:
                row += b"\x00\x00\x00\x00"
                continue
            mark_alpha = coverage(mark.distance(x, y))
            color = [
                body_color[i] + (MARK_COLOR[i] - body_color[i]) * mark_alpha
                for i in range(3)
            ]
            row += bytes(
                (
                    int(round(color[0])),
                    int(round(color[1])),
                    int(round(color[2])),
                    int(round(body_alpha * 255)),
                )
            )
        rows.append(bytes(row))
    return rows


def encode_png(rows) -> bytes:
    def chunk(kind: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
        )

    # Filter 0 (None) throughout. With nothing in the image but a rounded
    # rectangle and a glyph, choosing filters per row buys almost nothing.
    raw = b"".join(b"\x00" + row for row in rows)
    header = struct.pack(">IIBBBBB", CANVAS, CANVAS, 8, 6, 0, 0, 0)
    # ImageIO reads untagged RGB as sRGB anyway; the chunk says so for the other
    # decoders the artwork meets (browsers, the GitHub preview).
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"sRGB", b"\x00")
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def decode_png(data: bytes):
    """Rows of RGBA bytes from a PNG this script wrote (8-bit RGBA, filter 0).

    It is not a general decoder: anything else is reported as a mismatch, which
    is the right answer for a file that is supposed to be this script's output.
    """
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    idat = b""
    offset = 8
    while offset < len(data):
        (length,) = struct.unpack(">I", data[offset : offset + 4])
        kind = data[offset + 4 : offset + 8]
        payload = data[offset + 8 : offset + 8 + length]
        if kind == b"IHDR":
            width, height, depth, color = struct.unpack(">IIBB", payload[:10])
            if (width, height, depth, color) != (CANVAS, CANVAS, 8, 6):
                raise ValueError("not a 1024x1024 8-bit RGBA PNG")
        elif kind == b"IDAT":
            idat += payload
        offset += 12 + length
    raw = zlib.decompress(idat)
    stride = CANVAS * 4 + 1
    rows = []
    for py in range(CANVAS):
        line = raw[py * stride : (py + 1) * stride]
        if line[0] != 0:
            raise ValueError("a row uses a PNG filter this script never writes")
        rows.append(line[1:])
    return rows


def layout_for(mark: QuestionMark) -> str:
    # Integer rectangles: the preparation step crops pixels. The glyph box is
    # grown to whole pixels so no antialiased edge is cut off.
    x, y, w, h = mark.bounding_box
    left, top = math.floor(x), math.floor(y)
    right, bottom = math.ceil(x + w), math.ceil(y + h)
    layout = {
        "source": MASTER_PATH.name,
        "body": [int(MARGIN), int(MARGIN), BODY, BODY],
        "glyph": [left, top, right - left, bottom - top],
        "radius": CORNER_RADIUS_RATIO,
        "whiteGradientGlyph": True,
    }
    return json.dumps(layout, indent=2) + "\n"


def check(rows, layout: str) -> list[str]:
    """Differences between what would be written and what is on disk."""
    problems = []
    for path in (MASTER_PATH, BUNDLE_ICON_PATH):
        try:
            committed = decode_png(path.read_bytes())
        except (OSError, ValueError, zlib.error) as error:
            problems.append(f"{path.relative_to(ROOT)}: {error}")
            continue
        worst = max(
            abs(a - b)
            for ours, theirs in zip(rows, committed)
            for a, b in zip(ours, theirs)
        )
        if worst > 1:
            problems.append(
                f"{path.relative_to(ROOT)}: a channel differs by {worst} steps"
            )
    if LAYOUT_PATH.read_text() != layout:
        problems.append(f"{LAYOUT_PATH.relative_to(ROOT)}: differs")
    return problems


def main() -> None:
    mark = QuestionMark()
    rows = build_pixels(mark)
    layout = layout_for(mark)
    if sys.argv[1:] == ["--check"]:
        problems = check(rows, layout)
        for problem in problems:
            print(problem, file=sys.stderr)
        if problems:
            sys.exit(1)
        print("committed artwork matches the generator")
        return
    if sys.argv[1:]:
        sys.exit("usage: generate-icon.py [--check]")
    png = encode_png(rows)
    MASTER_PATH.write_bytes(png)
    BUNDLE_ICON_PATH.write_bytes(png)
    LAYOUT_PATH.write_text(layout)
    for path in (MASTER_PATH, BUNDLE_ICON_PATH, LAYOUT_PATH):
        print(f"wrote {path.relative_to(ROOT)} ({path.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
