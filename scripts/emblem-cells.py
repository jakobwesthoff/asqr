#!/usr/bin/env -S uv run --script
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["numpy==2.5.4", "pillow==12.3.0"]
# ///
"""Generates the four templates of Talqi on the empty queue (ADR 43).

asqr draws the terminal emblem as a fixed block of 44x22 text cells, in
one of two glyph sets: chafa's `sextant` set where the terminal draws
sextants itself, and its `quad` set everywhere else. With colours, each
cell carries a 24-bit foreground and background; under NO_COLOR, asqr
draws an outline with no colour at all. That makes four templates, which
this script writes to src/tui/render/mascot/ and which are committed
there. asqr itself never runs chafa.

Run it from anywhere in the repository:

    scripts/emblem-cells.py

It needs rsvg-convert and chafa 1.18.2 on the PATH, and uv for the
Python dependencies pinned above. Another chafa version may pick other
glyphs, so the script refuses to run with one.
"""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
EMBLEM = ROOT / "assets" / "emblem"
TEMPLATES = ROOT / "src" / "tui" / "render" / "mascot"

# The templates' size in cells. A cell is about twice as tall as wide, so
# 44x22 cells hold a square.
COLUMNS, ROWS = 44, 22

CHAFA_VERSION = "1.18.2"

# The two glyph sets as chafa names them; each template file carries the
# name of its set.
GLYPH_SETS = ("sextant", "quad")

# The body colour of the emblem. chafa blends a cell's edge pixels against
# its background colour; against the default black, a light terminal would
# show a dark rim around the emblem.
EMBLEM_BODY = "#09171a"

# Both pictures are rendered at this many pixels square, which is far more
# than the 88x66 sextant pixels the largest template resolves.
RENDER_PIXELS = 800


# =====================================================================
# Tools
# =====================================================================


def require_tools():
    """Stops with a message when a tool is missing or chafa is another
    version than the one the templates were made with."""
    for tool in ("rsvg-convert", "chafa"):
        if shutil.which(tool) is None:
            sys.exit(f"error: {tool} is not on the PATH")
    version = subprocess.run(
        ["chafa", "--version"], capture_output=True, text=True, check=True
    ).stdout.split()
    if version[:3] != ["Chafa", "version", CHAFA_VERSION]:
        sys.exit(f"error: needs chafa {CHAFA_VERSION}, found {' '.join(version[:3])}")


def render_svg(svg, png):
    subprocess.run(
        ["rsvg-convert", "-w", str(RENDER_PIXELS), "-h", str(RENDER_PIXELS), str(svg), "-o", str(png)],
        check=True,
    )


def chafa(picture, symbols, *options):
    """chafa's text for `picture` in the glyph set `symbols`.

    `--polite=on` leaves out the sequences that hide and show the cursor
    around the picture, which asqr has no use for. `--work 9` lets chafa
    try the most glyphs per cell, and `--dither none` keeps flat areas
    flat."""
    return subprocess.run(
        [
            "chafa",
            "--format", "symbols",
            "--symbols", symbols,
            "--size", f"{COLUMNS}x{ROWS}",
            "--work", "9",
            "--dither", "none",
            "--polite=on",
            *options,
            str(picture),
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout


# =====================================================================
# The outline mask
# =====================================================================
#
# chafa without colours draws a glyph's ink where the picture is bright.
# The outline is therefore a mask, white lines on black, made from the
# emblem whose terminal bar is an outlined box (talqi-terminal-hollow.svg):
# drawn from the plain emblem, the bar's dark slab would become one block
# of ink.

# Pixels of the mask per cell: 20 across and 40 down, so a mask pixel is
# square on a cell of about 1:2.
MASK_X, MASK_Y = 20, 40

# A pixel is outline when its contrast-stretched luminance is below this.
DARK = 0.30


def blur(values, radius):
    """`values` (0..1) through a Gaussian blur of `radius` pixels."""
    picture = Image.fromarray((np.clip(values, 0, 1) * 255).astype(np.uint8))
    return np.asarray(picture.filter(ImageFilter.GaussianBlur(radius)), float) / 255


def sobel_magnitude(values):
    """The gradient's magnitude from central differences."""
    across = np.roll(values, -1, 1) - np.roll(values, 1, 1)
    down = np.roll(values, -1, 0) - np.roll(values, 1, 0)
    return np.hypot(across, down)


def fitted(png):
    """The figure in `png`, cropped to its pixels with alpha above 8 and
    scaled with Lanczos to fill the mask's box on one axis, centred on a
    transparent canvas of the box. Returns (luminance, alpha), 0..1 each,
    with transparent pixels counted as white."""
    picture = Image.open(png).convert("RGBA")
    picture = picture.crop(picture.getchannel("A").point(lambda alpha: 255 if alpha > 8 else 0).getbbox())
    width, height = COLUMNS * MASK_X, ROWS * MASK_Y
    # Fit in cell widths, as a cell is twice as tall as wide, then convert
    # back to the mask's pixels on each axis.
    scale = min(COLUMNS / picture.width, 2 * ROWS / picture.height)
    picture = picture.resize(
        (max(1, round(picture.width * scale * MASK_X)), max(1, round(picture.height * scale * MASK_Y / 2))),
        Image.LANCZOS,
    )
    canvas = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    canvas.paste(picture, ((width - picture.width) // 2, (height - picture.height) // 2))
    alpha = np.asarray(canvas.getchannel("A"), float) / 255
    rgb = np.asarray(canvas.convert("RGB"), float) / 255
    luminance = rgb @ np.array([0.299, 0.587, 0.114])
    return luminance * alpha + (1 - alpha), alpha


def outline_mask(png):
    """The outline of the figure in `png` as a white-on-black picture of
    the mask's size."""
    luminance, alpha = fitted(png)
    # Stretch the luminance between its 2nd and 98th percentile inside the
    # figure, so the cut at DARK does not depend on the source's exposure.
    low, high = np.percentile(luminance[alpha > 0.5], [2, 98])
    tone = np.clip((luminance - low) / (high - low + 1e-9), 0, 1)
    ink = (tone < DARK) & (alpha > 0.5)
    # The silhouette's edge counts as outline too, so the contour holds
    # where the source has no dark line on that side.
    ink |= (sobel_magnitude(blur(alpha, 0.8)) > 0.25) & (alpha > 0.3)
    # One soft pixel of anti-aliasing, as a drawn line has.
    lines = (blur(ink.astype(float), 1.2) * 255).astype(np.uint8)
    return Image.fromarray(lines, "L").convert("RGB")


# =====================================================================
# The templates
# =====================================================================


def check_shape(name, text, colour):
    """Stops unless `text` is 22 lines and, for an outline, free of
    escape characters. asqr's tests check the cells; this catches a chafa
    that changed its output before anything is written."""
    lines = text.split("\n")
    if lines[-1] == "":
        lines.pop()
    if len(lines) != ROWS:
        sys.exit(f"error: {name} has {len(lines)} lines, not {ROWS}")
    if not colour and "\x1b" in text:
        sys.exit(f"error: {name} holds an escape character")


def main():
    require_tools()
    with tempfile.TemporaryDirectory() as scratch:
        scratch = Path(scratch)
        emblem = scratch / "talqi-terminal.png"
        hollow = scratch / "talqi-terminal-hollow.png"
        mask = scratch / "outline-mask.png"
        render_svg(EMBLEM / "talqi-terminal.svg", emblem)
        render_svg(EMBLEM / "talqi-terminal-hollow.svg", hollow)
        outline_mask(hollow).save(mask)

        for symbols in GLYPH_SETS:
            templates = {
                f"talqi-colour-{symbols}.ansi": (
                    chafa(emblem, symbols, "--colors", "full", "--bg", EMBLEM_BODY),
                    True,
                ),
                f"talqi-outline-{symbols}.txt": (
                    chafa(mask, symbols, "--colors", "none", "--fg", "white", "--bg", "black"),
                    False,
                ),
            }
            for file, (text, colour) in templates.items():
                check_shape(file, text, colour)
                (TEMPLATES / file).write_text(text)
                print(f"wrote {(TEMPLATES / file).relative_to(ROOT)}")


if __name__ == "__main__":
    main()
