#!/usr/bin/env python3
"""Converts a title block file (.tbf) into Katana's plot-frame JSON.

    python tools/title_frame_from_tbf.py FRAME.tbf OUT.json
        [--cells-from FRAME.json] [--palette-from FRAME.json] [--palette TOKENS.json]
        [--user-slots logo,organisation,notes] [--size WIDTHxHEIGHT] [--about TEXT]

The output is the model of resources/plot_frames/a3_landscape.json, which
crates/katana_cad/src/plotting/frame.rs reads (docs/plotting.md, "The plot
frame", and "A customisation's title frames"): the sheet's size and margins,
the drawing area, the border and the title-block box, every line of the file
with its points, colour, weight, dash and fill, every text with its position
(its offset and raise added), size, angle, slant, condensing, justification,
font and the fields it shows, and every symbol. Nothing is left out and
nothing is invented: what the file does not say is said here as the reader of
the file says it (the defaults below, each naming where it comes from), and
what this script cannot resolve - a colour token, a line style, a paper size
it has no table for - stops the conversion with the token named, rather than
being guessed.

A .tbf is text (UTF-16 or UTF-8, with or without a byte order mark) in a
nested-brace grammar, one record a line:

    title_block {
      name ""
      info { text "plot_frame_sheet_size" "A3 Landscape"  real "plot_frame_left_margin" 23 ... }
      data {
        string super { name "layer" colour white style "1" weight 0.53 closed 1
                       data_2d { x y ... } | data_3d { x y z ... }
                       radius_data { r ... } major_data { f|t ... }
                       solid_fill { colour "..." blend 1 }
                       symbol_data { properties { style colour size rotation offset raise } } }
        string text { worldsize 1 textstyle "82Arial" angle 0 x_factor 0.82 slant 0
                      offset 0 raise 0 text_colour "acad 007" justify "bottom-left"
                      ttf_weight 700  x 1 y 2 z -999  text "..." }
      }
    }

Coordinates are millimetres on the paper, the origin at its bottom-left and Y
up, which is Katana's paper convention too, so no coordinate is flipped.

What a converted frame cannot carry by itself:

- The CELLS (the named rectangles of the title block - "scale",
  "sheet_number", "legend" ...) are a person's reading of the ruled boxes, not
  something the file names. --cells-from copies them from a Katana frame of
  the same layout; without it the frame has one cell, the drawing area.
- A colour NAME other than the CAD colour index ("acad N"), the pens and
  black and white is the drawing program's palette, which the file does not
  hold. --palette-from takes the "palette" of a Katana frame, --palette an
  object of token to "#RRGGBB"; a token neither gives is refused.
- The USER SLOTS: the cells a sheet fills with the user's own logo,
  organisation and notes (frame.rs, `user_slots`). A frame that draws its own
  branding has none, which is the default here.

Python 3 standard library only. Files are read as bytes and written with
newline="\\n", so the output is the same on every system.
"""

import argparse
import json
import math
import os
import re
import sys

# ---- what the file does not say ------------------------------------------------------------
#
# The defaults of the drawing program's title-block reader, as its own
# reader of the format applies them (the owner's cross-section app, tbf.js,
# readSuper and readText): a line's weight 0.18 mm, its style "1" (solid),
# its colour "white"; a text's size 1, face "Arial", justification
# "bottom-left", condensing 1, colour "white".
DEFAULT_LINE_WEIGHT_MM = 0.18
DEFAULT_TEXT_SIZE_MM = 1.0
DEFAULT_TEXT_STYLE = "Arial"
DEFAULT_JUSTIFY = "bottom-left"
DEFAULT_COLOUR = "white"
DEFAULT_LINE_STYLE = "1"

# Arial's capital is 0.716 of its em (the font's OS/2 table: capHeight 1467 of
# unitsPerEm 2048); the file's text size is the capital's height, so the em is
# the size / 0.716. Recorded beside each text for a reader that sets the em.
ARIAL_CAP_RATIO = 0.716

# A text's lines are 1.5 x its capital height apart: the format's rule, which
# frame.rs applies (LINE_PITCH).

# ISO 216 paper, millimetres, portrait (width, height).
ISO_A_PAPER = {
    "A0": (841, 1189),
    "A1": (594, 841),
    "A2": (420, 594),
    "A3": (297, 420),
    "A4": (210, 297),
}

# Line styles by name, as dash and gap lengths in paper millimetres; None is a
# solid line. The drawing program's own table (the app's sheet.js, DASHES):
# MicroStation's "DGN Style n" line codes at the lengths it plots them.
LINE_STYLES = {
    "1": None,
    "0": None,
    "solid": None,
    "continuous": None,
    "dgn style 0": None,
    "dgn style 1": [2.2, 1.1],
    "dgn style 2": [0.4, 0.9],
    "dgn style 3": [3.5, 1.4],
    "dgn style 4": [3.5, 1.4, 0.4, 1.4],
    "dgn style 5": [5, 1.6],
    "dgn style 6": [5, 1.6, 0.4, 1.6],
    "dgn style 7": [7, 2, 0.4, 2],
    "dashed": [2.2, 1.1],
    "dot": [0.4, 0.9],
    "dashdot": [3.5, 1.4, 0.4, 1.4],
}

# An arc is drawn as straight pieces no further from it than this, in paper
# millimetres: a twentieth of the thinnest line a frame draws (0.1 mm), well
# under what a 600 dpi plot can show (0.042 mm a dot).
ARC_TOLERANCE_MM = 0.005

# How far a vertex may be from a cell's edge and still be on it: the files
# write six decimals, so a thousandth of a millimetre is equality.
ON_EDGE_MM = 0.001

# The field tokens a title block's texts carry, and the names they have in
# the frame's "fields" (the a3_landscape.json model).
USER_TEXT = re.compile(r"\$user_text<\s*(\d+)\s*,([^>]*)>")
TIME = re.compile(r"\$time(<[^>]*>)?")
DRAWING_NUMBER = "$drawing_number"


class ConversionError(Exception):
    """What stops a conversion: a file this script cannot read as a title block, or a value it
    cannot resolve. The message names the entity or the token."""


# ---- reading the file ----------------------------------------------------------------------

def decode(data):
    """The text of a .tbf: UTF-16 by its byte order mark, or by NUL bytes in its odd places,
    else UTF-8 with or without a byte order mark."""
    if data[:2] == b"\xff\xfe":
        return data[2:].decode("utf-16-le")
    if data[:2] == b"\xfe\xff":
        return data[2:].decode("utf-16-be")
    sample = data[:512]
    odd = sum(1 for i in range(1, len(sample), 2) if sample[i] == 0)
    if sample and odd > len(sample) / 4:
        return data.decode("utf-16-le")
    text = data.decode("utf-8")
    return text[1:] if text.startswith("﻿") else text


def split_line(line):
    """One line's tokens: braces, quoted text (as ("q", text), its \\n, \\t, \\" and \\\\
    unescaped) and bare words."""
    out = []
    i = 0
    n = len(line)
    while i < n:
        c = line[i]
        if c in " \t\r":
            i += 1
            continue
        if c in "{}":
            out.append(c)
            i += 1
            continue
        if c == '"':
            i += 1
            text = []
            while i < n and line[i] != '"':
                if line[i] == "\\" and i + 1 < n:
                    escaped = line[i + 1]
                    if escaped == "n":
                        text.append("\n")
                        i += 2
                        continue
                    if escaped == "t":
                        text.append("\t")
                        i += 2
                        continue
                    if escaped in '"\\':
                        text.append(escaped)
                        i += 2
                        continue
                text.append(line[i])
                i += 1
            i += 1
            out.append(("q", "".join(text)))
            continue
        start = i
        while i < n and line[i] not in ' \t\r{}"':
            i += 1
        out.append(line[start:i])
    return out


def value_of(token):
    return token[1] if isinstance(token, tuple) else token


NUMBER = re.compile(r"^-?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?$")


class Block:
    def __init__(self, name):
        self.name = name
        self.props = {}  # a record's first word -> its values (a list of lists, in order)
        self.blocks = []
        self.rows = []  # bare rows of numbers or flags

    def find(self, name):
        for block in self.blocks:
            if block.name == name:
                return block
        return None

    def first(self, key, default=None):
        """The value of the first record of `key`: its one word, or its words as a list."""
        records = self.props.get(key)
        if not records:
            return default
        record = records[0]
        return record[0] if len(record) == 1 else record


def parse_tree(text):
    root = Block("")
    stack = [root]
    for number, line in enumerate(text.replace("\r\n", "\n").split("\n"), start=1):
        tokens = split_line(line)
        while tokens and tokens[0] == "}":
            if len(stack) == 1:
                raise ConversionError("line %d: a '}' closes no block" % number)
            stack.pop()
            tokens = tokens[1:]
        if not tokens:
            continue
        top = stack[-1]
        if tokens[-1] == "{":
            block = Block(" ".join(value_of(t) for t in tokens[:-1]))
            top.blocks.append(block)
            stack.append(block)
            continue
        if "{" in tokens or "}" in tokens:
            raise ConversionError("line %d: a brace in the middle of a record" % number)
        values = [value_of(t) for t in tokens]
        head = values[0]
        if isinstance(tokens[0], str) and (len(values) == 1 or NUMBER.match(head) or head in ("f", "t")):
            top.rows.append(values)
        else:
            top.props.setdefault(head, []).append(values[1:])
    if len(stack) != 1:
        raise ConversionError("the file ends inside the block %r" % stack[-1].name)
    return root


def number_of(text, what):
    """A number as the file writes it: an int when it has no fraction or exponent, so that a
    margin of 23 is written 23."""
    if isinstance(text, list) or text is None or not NUMBER.match(str(text)):
        raise ConversionError("%s is %r, which is not a number" % (what, text))
    if re.match(r"^-?\d+$", text):
        return int(text)
    value = float(text)
    if not math.isfinite(value):
        raise ConversionError("%s is %r, which is not a finite number" % (what, text))
    return value


def optional_number(block, key, default, what):
    value = block.first(key)
    return default if value is None else number_of(value, what)


# ---- colours ---------------------------------------------------------------------------------

def aci_colour(index):
    """The CAD colour index as the drawing program plots it: 1-9 the standard colours (7 black,
    as white plots on white paper), 10-249 by hue and shade (hue = (N - 10) // 10 x 15 degrees;
    the last digit even full, odd half saturation, at values 1, 0.65, 0.5, 0.3, 0.15), 250-255
    the greys. Rounded to the nearest of 255 steps, halves up."""
    standard = {
        1: (255, 0, 0), 2: (255, 255, 0), 3: (0, 255, 0), 4: (0, 255, 255),
        5: (0, 0, 255), 6: (255, 0, 255), 7: (0, 0, 0), 8: (128, 128, 128), 9: (192, 192, 192),
    }
    greys = {250: 51, 251: 80, 252: 105, 253: 130, 254: 190, 255: 255}
    if index in standard:
        rgb = standard[index]
    elif index in greys:
        rgb = (greys[index],) * 3
    elif 10 <= index <= 249:
        hue = (index - 10) // 10 * 15.0
        shade = index % 10
        value = (1.0, 1.0, 0.65, 0.65, 0.5, 0.5, 0.3, 0.3, 0.15, 0.15)[shade]
        saturation = 1.0 if shade % 2 == 0 else 0.5
        rgb = hsv_to_rgb(hue, saturation, value)
    else:
        raise ConversionError("colour index %d is outside 1-255" % index)
    return "#%02x%02x%02x" % rgb


def hsv_to_rgb(hue, saturation, value):
    sector = int(hue // 60) % 6
    fraction = hue / 60.0 - math.floor(hue / 60.0)
    p = value * (1 - saturation)
    q = value * (1 - saturation * fraction)
    t = value * (1 - saturation * (1 - fraction))
    r, g, b = [
        (value, t, p), (q, value, p), (p, value, t), (p, q, value), (t, p, value), (value, p, q),
    ][sector]
    return tuple(int(math.floor(channel * 255 + 0.5)) for channel in (r, g, b))


class Palette:
    """Colour tokens to "#RRGGBB": the tokens given, then the rules every CAD file shares."""

    def __init__(self, given):
        self.given = {key.lower(): value for key, value in given.items()}
        self.used = {}

    def resolve(self, token, where):
        key = token.strip().lower()
        if key in self.given:
            hex_colour = self.given[key]
        elif key in ("white", "black"):
            # A CAD "white" is the colour of the background's opposite: it plots black on paper.
            hex_colour = "#000000"
        elif re.match(r"^acad\s*\d+$", key):
            hex_colour = aci_colour(int(re.sub(r"\D", "", key)))
        elif re.match(r"^pen\s*\d+$", key) and int(re.sub(r"\D", "", key)) in (0, 7, 18):
            # Pens 0, 7 and 18 are the black pens of a pen table.
            hex_colour = "#000000"
        else:
            raise ConversionError(
                "%s: the colour %r is not in the palette given (--palette-from, --palette)"
                % (where, token))
        if not re.match(r"^#[0-9A-Fa-f]{6}$", hex_colour):
            raise ConversionError("the palette's colour for %r is %r, not #RRGGBB" % (token, hex_colour))
        self.used[token] = hex_colour
        return hex_colour


# ---- geometry --------------------------------------------------------------------------------

def arc_points(start, end, radius, major):
    """The points of the arc from `start` to `end` of `radius` (positive turning left,
    counter-clockwise; negative right), the larger of the two arcs when `major`; `end` included,
    `start` not. The drawing program's rule (sheet.js polyPath): a positive radius is drawn with
    SVG's sweep flag 0 in a Y-down picture, which is counter-clockwise on the paper."""
    (x0, y0), (x1, y1) = start, end
    r = abs(radius)
    chord = math.hypot(x1 - x0, y1 - y0)
    if chord <= 1e-9 or r < chord / 2:
        return [end]  # no arc fits: a straight piece, as the program draws it
    counter_clockwise = radius > 0
    # The centre is on the left of the chord for a minor counter-clockwise arc (and a major
    # clockwise one), on the right otherwise.
    left = counter_clockwise != bool(major)
    half = chord / 2
    rise = math.sqrt(max(r * r - half * half, 0.0))
    ux, uy = (x1 - x0) / chord, (y1 - y0) / chord
    nx, ny = (-uy, ux) if left else (uy, -ux)
    cx, cy = (x0 + x1) / 2 + nx * rise, (y0 + y1) / 2 + ny * rise
    a0 = math.atan2(y0 - cy, x0 - cx)
    a1 = math.atan2(y1 - cy, x1 - cx)
    sweep = a1 - a0
    if counter_clockwise:
        while sweep <= 0:
            sweep += 2 * math.pi
    else:
        while sweep >= 0:
            sweep -= 2 * math.pi
    # Pieces of an angle whose sagitta r (1 - cos(a / 2)) is at most the tolerance.
    step = 2 * math.acos(max(-1.0, 1 - ARC_TOLERANCE_MM / r)) if r > ARC_TOLERANCE_MM else math.pi
    pieces = max(1, int(math.ceil(abs(sweep) / step)))
    out = []
    for k in range(1, pieces):
        angle = a0 + sweep * k / pieces
        out.append([round(cx + r * math.cos(angle), 6), round(cy + r * math.sin(angle), 6)])
    out.append(end)
    return out


def box_of(points):
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    return [min(xs), min(ys), max(xs), max(ys)]


def area_of(rect):
    return (rect[2] - rect[0]) * (rect[3] - rect[1])


def inside(rect, x, y, margin=ON_EDGE_MM):
    return rect[0] - margin <= x <= rect[2] + margin and rect[1] - margin <= y <= rect[3] + margin


def on_boundary(rect, points):
    """Every point on one edge of `rect`: a line that is the cell's own edge."""
    for edge in range(4):
        coordinate = 0 if edge in (0, 2) else 1
        if all(abs(p[coordinate] - rect[edge]) <= ON_EDGE_MM for p in points):
            return True
    return False


# ---- the conversion --------------------------------------------------------------------------

def sheet_size(info, size_option):
    if size_option:
        match = re.match(r"^\s*([0-9.]+)\s*[xX]\s*([0-9.]+)\s*$", size_option)
        if not match:
            raise ConversionError("--size is WIDTHxHEIGHT in millimetres, not %r" % size_option)
        return float(match.group(1)), float(match.group(2)), "%sx%s" % match.groups()
    named = info.get("plot_frame_sheet_size")
    if named is None:
        raise ConversionError("the file names no plot_frame_sheet_size; give --size WIDTHxHEIGHT")
    match = re.match(r"^\s*(A[0-4])\s+(Landscape|Portrait)\s*$", named, re.IGNORECASE)
    if not match:
        raise ConversionError("no paper size is known for %r; give --size WIDTHxHEIGHT" % named)
    short, long_side = ISO_A_PAPER[match.group(1).upper()]
    if match.group(2).lower() == "landscape":
        return long_side, short, named
    return short, long_side, named


def read_info(title_block):
    info = {}
    block = title_block.find("info")
    if block is None:
        return info
    for kind, records in block.props.items():
        for record in records:
            if len(record) < 2:
                raise ConversionError("info: a %s record without a key and a value" % kind)
            key, value = record[0], record[1]
            info[key] = value if kind == "text" else number_of(value, "info %s" % key)
    return info


def fields_of(raw):
    """The fields a text shows, in the a3_landscape.json model's names, and the user texts'
    tokens and labels."""
    fields = []
    tokens = []
    for match in USER_TEXT.finditer(raw):
        key = "user_text_%d" % int(match.group(1))
        fields.append(key)
        tokens.append((key, match.group(0), match.group(2).strip()))
    if DRAWING_NUMBER in raw:
        fields.append("drawing_number")
        tokens.append(("drawing_number", DRAWING_NUMBER, "Drawing Number"))
    for match in TIME.finditer(raw):
        fields.append("plot_date")
        tokens.append(("plot_date", match.group(0), "Plot Date"))
    return fields, tokens


def font_face_of(style):
    """The face of a text style: a style is a pen prefix and a face, "82Arial"."""
    face = re.sub(r"^[\d.]+", "", style).strip()
    return face or DEFAULT_TEXT_STYLE


def cell_named(cells, name):
    for cell in cells:
        if cell.get("id") == name:
            return cell.get("tbf")
    return None


def convert(text, source_name, cells=None, palette=None, user_slots=(), size=None, about=None,
            cells_from=None):
    """The frame JSON (a dict) of the .tbf `text`."""
    tree = parse_tree(text)
    title_block = tree.find("title_block")
    if title_block is None:
        raise ConversionError("the file holds no title_block")
    info = read_info(title_block)
    width, height, size_name = sheet_size(info, size)
    margins = {}
    for side in ("left", "right", "top", "bottom"):
        key = "plot_frame_%s_margin" % side
        if key not in info:
            raise ConversionError("the file gives no %s" % key)
        margins[side] = info[key]
    if not (margins["left"] + margins["right"] < width and margins["top"] + margins["bottom"] < height):
        raise ConversionError("the margins leave no drawing area on a %g x %g sheet" % (width, height))
    viewport = [margins["left"], margins["bottom"], width - margins["right"], height - margins["top"]]

    palette = Palette(palette or {})
    # A cell's id and rectangle; its "purpose" describes the frame it was read from, not this one.
    cells = [{"id": cell["id"], "tbf": list(cell["tbf"])} for cell in (cells or [])]
    legend = cell_named(cells, "legend")
    stamp = cell_named(cells, "stamp")

    data = title_block.find("data")
    entities = []
    field_index = {}
    styles_used = {}
    for index, block in enumerate(data.blocks if data else []):
        where = "entity %d" % index
        layer = block.first("name", "") or ""
        if block.name == "string text":
            entities.append(read_text(block, index, layer, palette, legend, stamp, field_index))
        elif block.name == "string super":
            entities.append(read_super(block, index, layer, palette, legend, width, height, styles_used))
        else:
            raise ConversionError("%s: a %r block, which a title block does not hold" % (where, block.name))

    polys = [e for e in entities if e["kind"] == "poly"]
    framing = [e for e in polys if e["closed"] and e["role"] != "construction" and len(e["points_tbf"]) > 2]
    centre = ((viewport[0] + viewport[2]) / 2, (viewport[1] + viewport[3]) / 2)
    around = [e for e in framing if inside(box_of(e["points_tbf"]), centre[0], centre[1], 0)]
    border = max(around, key=lambda e: area_of(box_of(e["points_tbf"])), default=None)
    border_box = box_of(border["points_tbf"]) if border else list(viewport)
    below = [e for e in framing if box_of(e["points_tbf"])[3] <= border_box[1] + ON_EDGE_MM]
    title = max(below, key=lambda e: area_of(box_of(e["points_tbf"])), default=None)

    def rect(box, entity=None, weight=None):
        out = {"x0": box[0], "y0": box[1], "x1": box[2], "y1": box[3]}
        if entity is not None:
            out["entity"] = entity["index"]
            out["weight_mm"] = entity["weight_mm"]
        return out

    if not any(cell.get("id") == "drawing_area" for cell in cells):
        cells.insert(0, {"id": "drawing_area", "tbf": border_box,
                         "purpose": "the whole drawing; viewports are placed in the viewport inset"})

    fields = []
    for key in sorted(field_index, key=field_order):
        token, label, used_by = field_index[key]
        fields.append({"key": key, "token": token, "label_in_tbf": label, "entities": used_by})

    counts = {kind: sum(1 for e in entities if e["kind"] == kind) for kind in ("poly", "text", "symbol")}
    frame = {
        "_about": about or (
            "A plot frame converted from the title block file %s by tools/title_frame_from_tbf.py: "
            "every line, text and symbol of the file, nothing added and nothing left out. "
            "Coordinates are millimetres on the paper, the origin at its bottom-left and Y up; "
            "anchor_tbf includes each text's offset and raise. See docs/plotting.md." % source_name),
        "converted_from": {
            "file": source_name,
            "info": info,
            "counts": counts,
            "cells_from": cells_from,
        },
        "coordinates": {
            "units": "mm",
            "tbf_origin": "bottom-left of the sheet, Y up",
            "text_anchor": "a text is drawn at (x + offset, y + raise); anchor_tbf already includes it",
            "angles": "degrees counter-clockwise",
            "arcs": "an arc of the file (radius_data) is drawn as straight pieces within %g mm of it; "
                    "the entity keeps the file's radii in \"arcs\"" % ARC_TOLERANCE_MM,
        },
        "sheet": {
            "size": size_name,
            "width_mm": width,
            "height_mm": height,
            "margins_mm": margins,
            "plot_frame_viewport": info.get("plot_frame_viewport", 1),
            "viewport_tbf": rect(viewport),
            "drawing_border_tbf": rect(border_box, border),
            "title_block_tbf": rect(box_of(title["points_tbf"]), title) if title else rect([0, 0, 0, 0]),
            "sheet_edge_tbf": rect([0, 0, width, height]),
        },
        "palette": dict(sorted(palette.used.items())),
        "line_styles": {name: dash for name, dash in sorted(styles_used.items())},
        "text_rules": {
            "size": "cap_height_mm is the capital's height; em_size_mm = cap_height_mm / %g, Arial's cap "
                    "height" % ARIAL_CAP_RATIO,
            "multiline": "anchored on the FIRST line and grows downward whatever the justification; "
                         "line pitch is 1.5 x cap height",
            "justify": "\"vertical-horizontal\", e.g. bottom-left or middle-centre",
        },
        "user_slots": list(user_slots),
        "cells": cells,
        "fields": fields,
        "entities": entities,
    }
    return frame


def field_order(key):
    if key.startswith("user_text_"):
        return (0, int(key[len("user_text_"):]))
    return (1, key)


def role_of_line(points, dash, layer_box_legend, width, height):
    if any(p[0] <= ON_EDGE_MM or p[1] <= ON_EDGE_MM or p[0] >= width - ON_EDGE_MM
           or p[1] >= height - ON_EDGE_MM for p in points):
        # On or past the paper's edge, where nothing can plot: a guide.
        return "construction"
    if layer_box_legend and all(inside(layer_box_legend, p[0], p[1]) for p in points) \
            and not on_boundary(layer_box_legend, points):
        return "legend"
    if dash:
        return "field-underline"
    return "border"


def read_super(block, index, layer, palette, legend, width, height, styles_used):
    where = "entity %d" % index
    rows = []
    for name in ("data_2d", "data_3d"):
        found = block.find(name)
        if found is not None:
            rows = found.rows
            break
    points = []
    for row in rows:
        if len(row) < 2:
            raise ConversionError("%s: a vertex of fewer than two numbers" % where)
        points.append([number_of(row[0], where + " x"), number_of(row[1], where + " y")])
    colour_token = block.first("colour", DEFAULT_COLOUR)
    symbol_block = block.find("symbol_data")
    properties = symbol_block.find("properties") if symbol_block is not None else None
    if properties is not None or block.first("breakline") == "point":
        props = properties or Block("properties")
        colour = props.first("colour", colour_token)
        at = points[0] if points else [0, 0]
        return {
            "index": index,
            "kind": "symbol",
            "layer": layer,
            "role": "legend" if legend and inside(legend, at[0], at[1]) else "border",
            "symbol_style": props.first("style", "") or "",
            "at_tbf": at,
            "size_mm": optional_number(props, "size", 1, where + " size"),
            "rotation_deg": optional_number(props, "rotation", 0, where + " rotation"),
            "offset": optional_number(props, "offset", 0, where + " offset"),
            "raise": optional_number(props, "raise", 0, where + " raise"),
            "colour_token": colour,
            "colour_hex": palette.resolve(colour, where),
        }

    style = block.first("style", DEFAULT_LINE_STYLE)
    key = style.strip().lower()
    if key not in LINE_STYLES:
        raise ConversionError("%s: the line style %r is not one this script knows" % (where, style))
    dash = LINE_STYLES[key]
    styles_used[style] = dash
    radii_block = block.find("radius_data")
    major_block = block.find("major_data")
    radii = [number_of(v, where + " radius") for row in (radii_block.rows if radii_block else []) for v in row]
    majors = [v == "t" for row in (major_block.rows if major_block else []) for v in row]
    arcs = []
    drawn = points[:1]
    for i in range(1, len(points)):
        radius = radii[i - 1] if i - 1 < len(radii) else 0
        if radius and abs(radius) > 1e-9:
            major = majors[i - 1] if i - 1 < len(majors) else False
            arcs.append({"segment": i - 1, "radius": radius, "major": major})
            drawn.extend(arc_points(points[i - 1], points[i], radius, major))
        else:
            drawn.append(points[i])
    closed = block.first("closed", "0") == "1"
    entity = {
        "index": index,
        "kind": "poly",
        "layer": layer,
        "role": role_of_line(drawn, dash, legend, width, height),
        "colour_token": colour_token,
        "colour_hex": palette.resolve(colour_token, where),
        "line_style": style,
        "dash_mm": dash,
        "weight_mm": optional_number(block, "weight", DEFAULT_LINE_WEIGHT_MM, where + " weight"),
        "closed": closed,
        "points_tbf": drawn,
    }
    if len(drawn) == 2:
        if abs(drawn[0][0] - drawn[1][0]) <= ON_EDGE_MM:
            entity["orientation"] = "vertical"
        elif abs(drawn[0][1] - drawn[1][1]) <= ON_EDGE_MM:
            entity["orientation"] = "horizontal"
    if arcs:
        entity["arcs"] = arcs
    fill = block.find("solid_fill")
    if fill is not None:
        fill_token = fill.first("colour", colour_token)
        entity["fill_token"] = fill_token
        entity["fill_hex"] = palette.resolve(fill_token, where + " fill")
    return entity


def read_text(block, index, layer, palette, legend, stamp, field_index):
    where = "entity %d" % index
    x = optional_number(block, "x", 0, where + " x")
    y = optional_number(block, "y", 0, where + " y")
    offset = optional_number(block, "offset", 0, where + " offset")
    lift = optional_number(block, "raise", 0, where + " raise")
    raw = block.first("text", "")
    if isinstance(raw, list):
        raise ConversionError("%s: its text is not one quoted string" % where)
    size = optional_number(block, "worldsize", DEFAULT_TEXT_SIZE_MM, where + " worldsize")
    style = block.first("textstyle", DEFAULT_TEXT_STYLE)
    ttf_weight = block.first("ttf_weight")
    weight = None if ttf_weight is None else number_of(ttf_weight, where + " ttf_weight")
    colour = block.first("text_colour") or block.first("colour", DEFAULT_COLOUR)
    fields, tokens = fields_of(raw)
    for key, token, label in tokens:
        if key not in field_index:
            field_index[key] = (token, label, [])
        if index not in field_index[key][2]:
            field_index[key][2].append(index)
    anchor = [x + offset, y + lift]
    if stamp and inside(stamp, anchor[0], anchor[1]):
        role = "stamp"
    elif legend and inside(legend, anchor[0], anchor[1]) and not fields:
        role = "legend"
    elif fields:
        role = "field"
    else:
        role = "label"
    return {
        "index": index,
        "kind": "text",
        "layer": layer,
        "role": role,
        "x_tbf": x,
        "y_tbf": y,
        "offset": offset,
        "raise": lift,
        "anchor_tbf": [round_sum(x, offset), round_sum(y, lift)],
        "cap_height_mm": size,
        "em_size_mm": round(size / ARIAL_CAP_RATIO, 3),
        "font_style_token": style,
        "font_face": font_face_of(style),
        "bold": weight is not None and weight >= 700,
        "ttf_weight": weight,
        "angle_deg": optional_number(block, "angle", 0, where + " angle"),
        "x_factor": optional_number(block, "x_factor", 1, where + " x_factor"),
        "slant": optional_number(block, "slant", 0, where + " slant"),
        "justify": block.first("justify", DEFAULT_JUSTIFY),
        "colour_token": colour,
        "colour_hex": palette.resolve(colour, where),
        "raw_text": raw,
        "lines": raw.count("\n") + 1 - (1 if raw.endswith("\n") and raw.count("\n") > 0 else 0),
        "fields": fields,
    }


def round_sum(a, b):
    """a + b without the binary fraction's tail: the file writes eight decimals at most, so the
    sum is rounded to ten, which is exact for every sum of two such numbers."""
    if b == 0:
        return a
    total = round(a + b, 10)
    return int(total) if total == int(total) and isinstance(a, int) and isinstance(b, int) else total


# ---- the command -----------------------------------------------------------------------------

def load_json(path):
    with open(path, "r", encoding="utf-8") as stream:
        return json.load(stream)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("tbf", help="the title block file")
    parser.add_argument("out", help="the plot-frame JSON to write")
    parser.add_argument("--cells-from", help="a Katana frame JSON whose \"cells\" the frame takes")
    parser.add_argument("--palette-from", help="a Katana frame JSON whose \"palette\" resolves colour names")
    parser.add_argument("--palette", help="a JSON object of colour token to \"#RRGGBB\"")
    parser.add_argument("--user-slots", default="",
                        help="the cells a sheet fills with the user's own: logo, organisation, notes")
    parser.add_argument("--size", help="the paper as WIDTHxHEIGHT mm, when the file's size is not A0-A4")
    parser.add_argument("--about", help="the frame's \"_about\" text")
    args = parser.parse_args(argv)

    with open(args.tbf, "rb") as stream:
        text = decode(stream.read())
    cells = load_json(args.cells_from)["cells"] if args.cells_from else None
    palette = {}
    if args.palette_from:
        palette.update(load_json(args.palette_from)["palette"])
    if args.palette:
        palette.update(load_json(args.palette))
    slots = [slot.strip() for slot in args.user_slots.split(",") if slot.strip()]
    for slot in slots:
        if slot not in ("logo", "organisation", "notes"):
            raise ConversionError("--user-slots names logo, organisation and notes, not %r" % slot)
    frame = convert(text, os.path.basename(args.tbf), cells, palette, slots, args.size, args.about,
                    os.path.basename(args.cells_from) if args.cells_from else None)
    with open(args.out, "w", encoding="utf-8", newline="\n") as stream:
        json.dump(frame, stream, indent=1, ensure_ascii=False)
        stream.write("\n")
    counts = frame["converted_from"]["counts"]
    print("%s: %d lines, %d texts, %d symbols" % (args.out, counts["poly"], counts["text"], counts["symbol"]))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except ConversionError as error:
        print("title_frame_from_tbf: %s" % error, file=sys.stderr)
        sys.exit(1)
