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

# The background colour chafa is given, the emblem's ink colour. chafa's
# glyphs at the figure's edge change with it, also for a picture without
# half transparent pixels, and the templates the user approved were made
# with this one.
EMBLEM_INK = "#09171a"

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
# The colour picture
# =====================================================================
#
# The emblem's first path is an ink silhouette under everything else. Its
# edge is a dark rim around the figure, which the colour templates leave
# out (the user, 2026-10-16). The other paths draw the eyes, the mouth,
# the paws and the panel with ink of their own. Where the silhouette shows
# through gaps between the other paths on purpose, the picture keeps it:
# in the band between chin and panel, which is closed all round, and in
# the line between crest and head, a narrow gap that opens to the outside.

# The comment that stands before the silhouette's path in the SVG.
SILHOUETTE = "<!-- 1. ink: silhouette -->"

# Gaps narrower than about twice this many pixels of the 800 pixel picture
# keep the silhouette's ink; wider ones are outside.
GAP_RADIUS = 9

# chafa works in cells of 8x8 pixels. A picture of exactly that grid, with
# `--stretch`, is used as it is.
CHAFA_CELL = 8


def without_silhouette(svg):
    """`svg`'s text without the silhouette's comment and path."""
    lines = svg.split("\n")
    at = lines.index(SILHOUETTE)
    if not lines[at + 1].startswith("<path"):
        sys.exit("error: no path follows the silhouette's comment")
    return "\n".join(lines[:at] + lines[at + 2 :])


def rgba(png):
    return np.asarray(Image.open(png).convert("RGBA"), float)


def grown(mask, steps):
    """`mask` grown by `steps` pixels in every direction, in turn across
    and down, then diagonally too, so the growth is about round."""
    for step in range(steps):
        before = mask
        mask = before.copy()
        mask[1:] |= before[:-1]
        mask[:-1] |= before[1:]
        mask[:, 1:] |= before[:, :-1]
        mask[:, :-1] |= before[:, 1:]
        if step % 2:
            mask[1:, 1:] |= before[:-1, :-1]
            mask[:-1, :-1] |= before[1:, 1:]
            mask[1:, :-1] |= before[:-1, 1:]
            mask[:-1, 1:] |= before[1:, :-1]
    return mask


def outside(clear):
    """The pixels of `clear` that reach the picture's border through
    `clear` pixels."""
    reached = np.zeros_like(clear)
    reached[[0, -1], :] = clear[[0, -1], :]
    reached[:, [0, -1]] = clear[:, [0, -1]]
    while True:
        more = grown(reached, 1) & clear
        if (more == reached).all():
            return reached
        reached = more


def over(top, bottom):
    """`top` composited over `bottom`, both RGBA with straight alpha."""
    top_alpha, bottom_alpha = top[..., 3:] / 255, bottom[..., 3:] / 255
    alpha = top_alpha + bottom_alpha * (1 - top_alpha)
    colour = (top[..., :3] * top_alpha + bottom[..., :3] * bottom_alpha * (1 - top_alpha)) / np.maximum(
        alpha, 1e-9
    )
    return np.concatenate([colour, alpha * 255], -1)


def rimless(whole, bare):
    """The emblem without its outer ink rim: `bare`, rendered without the
    silhouette, over the silhouette's ink of `whole` where `bare` has an
    enclosed hole or a narrow gap."""
    clear = bare[..., 3] < 8
    holes = clear & ~outside(clear)
    # A closing of the figure fills the gaps narrower than about twice
    # GAP_RADIUS, also those that open to the outside.
    solid = ~clear
    closed = ~grown(~grown(solid, GAP_RADIUS), GAP_RADIUS)
    kept = (holes | (closed & clear)) & (whole[..., 3] > 0)
    # Two pixels more, so the anti-aliased edges around a hole cover ink
    # instead of the terminal's background.
    under = whole.copy()
    under[..., 3] *= grown(kept, 2)
    return over(bare, under)


def chafa_grid(picture):
    """`picture` scaled with Lanczos to chafa's own grid of the templates'
    cells, its alpha cut at half. chafa then neither scales the picture nor
    blends a half transparent edge with `--bg`, which leaves no fringe of
    any colour around the figure on a light or a dark terminal."""
    image = Image.fromarray(np.clip(picture, 0, 255).astype(np.uint8), "RGBA")
    image = image.resize((COLUMNS * CHAFA_CELL, ROWS * CHAFA_CELL), Image.LANCZOS)
    cut = np.asarray(image).copy()
    cut[..., 3] = np.where(cut[..., 3] >= 128, 255, 0)
    return Image.fromarray(cut, "RGBA")


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
        whole = scratch / "talqi-terminal.png"
        bare_svg = scratch / "talqi-terminal-bare.svg"
        bare = scratch / "talqi-terminal-bare.png"
        colour_picture = scratch / "colour.png"
        hollow = scratch / "talqi-terminal-hollow.png"
        mask = scratch / "outline-mask.png"
        render_svg(EMBLEM / "talqi-terminal.svg", whole)
        bare_svg.write_text(without_silhouette((EMBLEM / "talqi-terminal.svg").read_text()))
        render_svg(bare_svg, bare)
        chafa_grid(rimless(rgba(whole), rgba(bare))).save(colour_picture)
        render_svg(EMBLEM / "talqi-terminal-hollow.svg", hollow)
        outline_mask(hollow).save(mask)

        for symbols in GLYPH_SETS:
            templates = {
                f"talqi-colour-{symbols}.ansi": (
                    chafa(colour_picture, symbols, "--colors", "full", "--stretch", "--bg", EMBLEM_INK),
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
