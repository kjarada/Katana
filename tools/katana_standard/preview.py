"""Draw Katana Standard linestyles and symbols to PNG with Pillow, with no Katana build needed.

A definition here is the plain dict of the customisation file: `name`, `units`, `length`, `anchors`,
`strokes` (and `atVertices`, `group`, which this module ignores).  The drawing follows how Katana lays a
definition (docs/survey_coding.md, "From a definition to geometry"):

- the units are millimetres of paper, +x runs along the line and +y is to its LEFT;
- instance k of a linestyle starts at k times `length`; every stroke point is placed by its OWN distance
  along the line, so a pattern bends round a corner instead of cutting it;
- a symbol is drawn once, unrotated, centred on the origin;
- a `twoPoint` definition is stretched so its two anchors land on the first and last vertex;
- `arc` and `circle` are drawn as 24 chords a turn, as Katana does, so a preview shows the faceting
  the program will show.

It is a preview, not the renderer: Katana's own drawing (`cad::styleDrawing`) decides what the program
shows, and a definition is only judged finished after it has been drawn by `katana --screenshot` as well
(docs/headless.md).
"""
import math
from PIL import Image, ImageDraw, ImageFont

GROUND = {"dark": (0x1E, 0x23, 0x29), "light": (255, 255, 255)}
INK = {"dark": (0xB8, 0xC0, 0xCC), "light": (0x33, 0x3A, 0x44)}
SUPERSAMPLE = 3
CHORDS_A_TURN = 24


def _font(pixels):
    for name in ("arial.ttf", "DejaVuSans.ttf", "LiberationSans-Regular.ttf"):
        try:
            return ImageFont.truetype(name, max(int(round(pixels)), 6))
        except OSError:
            continue
    return ImageFont.load_default()


def hex_rgb(text):
    text = text.lstrip("#")
    return tuple(int(text[i:i + 2], 16) for i in (0, 2, 4))


def _arc_points(cx, cy, r, a0, a1):
    sweep = a1 - a0
    steps = max(int(math.ceil(abs(sweep) / (360.0 / CHORDS_A_TURN))), 1)
    return [(cx + abs(r) * math.cos(math.radians(a0 + sweep * i / steps)),
             cy + abs(r) * math.sin(math.radians(a0 + sweep * i / steps))) for i in range(steps + 1)]


def flatten(defn):
    """The definition as ('poly', [(x, y), ...]), ('dot', x, y, r) and ('text', x, y, spec) items."""
    items, poly, x, y = [], [], 0.0, 0.0
    ox, oy = defn.get("origin", [0, 0])
    factor = defn.get("factor", 1)

    def flush():
        nonlocal poly
        if len(poly) > 1:
            items.append(("poly", poly))
        poly = []

    for stroke in defn.get("strokes", []):
        kind = stroke[0]
        if kind == "move":
            flush()
            x, y = (stroke[1] - ox) * factor, (stroke[2] - oy) * factor
        elif kind == "draw":
            if not poly:
                poly = [(x, y)]
            x, y = (stroke[1] - ox) * factor, (stroke[2] - oy) * factor
            poly.append((x, y))
        elif kind == "arc":
            flush()
            items.append(("poly", _arc_points(x, y, stroke[1] * factor, stroke[2], stroke[3])))
        elif kind == "circle":
            flush()
            items.append(("poly", _arc_points(x, y, stroke[1] * factor, 0, 360)))
        elif kind == "dot":
            flush()
            items.append(("dot", x, y, stroke[1] * factor))
        elif kind == "text":
            flush()
            items.append(("text", x, y, stroke[1]))
        elif kind == "pen":
            pass
        else:
            raise ValueError(f"{defn.get('name')}: unknown stroke kind {kind!r}")
    flush()
    return items


class Path:
    """A polyline with distances along it and a left normal on each leg."""

    def __init__(self, points):
        self.points = [tuple(p) for p in points]
        self.at = [0.0]
        for a, b in zip(self.points, self.points[1:]):
            self.at.append(self.at[-1] + math.dist(a, b))
        self.length = self.at[-1]

    def locate(self, s):
        s = min(max(s, 0.0), self.length)
        k = 0
        while k < len(self.points) - 2 and s >= self.at[k + 1]:
            k += 1
        a, b = self.points[k], self.points[k + 1]
        leg = max(self.at[k + 1] - self.at[k], 1e-12)
        t = (s - self.at[k]) / leg
        ux, uy = (b[0] - a[0]) / leg, (b[1] - a[1]) / leg
        return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t), (ux, uy)

    def place(self, s, y):
        (px, py), (ux, uy) = self.locate(s)
        return (px - uy * y, py + ux * y)

    def corners_between(self, s0, s1):
        return [d for d in self.at[1:-1] if min(s0, s1) < d < max(s0, s1)]


def lay_linestyle(defn, path):
    """Geometry of a linestyle along `path`: polylines and texts in the path's own units (millimetres)."""
    out = []
    if defn.get("units") == "twoPoint":
        a1, a2 = defn.get("anchors", [[0, 0], [1, 0]])
        p0, p1 = path.points[0], path.points[-1]
        sx, sy = a2[0] - a1[0], a2[1] - a1[1]
        scale = math.dist(p0, p1) / max(math.hypot(sx, sy), 1e-12)
        rot = math.atan2(p1[1] - p0[1], p1[0] - p0[0]) - math.atan2(sy, sx)
        c, s = math.cos(rot) * scale, math.sin(rot) * scale

        def tp(x, y):
            x, y = x - a1[0], y - a1[1]
            return (p0[0] + c * x - s * y, p0[1] + s * x + c * y)
        for item in flatten(defn):
            if item[0] == "poly":
                out.append(("poly", [tp(x, y) for x, y in item[1]]))
            elif item[0] == "text":
                out.append(("text", *tp(item[1], item[2]), item[3], math.degrees(rot)))
        return out
    period = defn.get("length") or 0.0
    items = flatten(defn)
    if period <= 0:
        xs = [p[0] for it in items if it[0] == "poly" for p in it[1]] or [1.0]
        period = max(max(xs) - min(min(xs), 0.0), 1e-6)
    k = 0
    while k * period < path.length + 1e-9:
        base = k * period
        for item in items:
            if item[0] == "poly":
                pts = item[1]
                dense = []
                for (xa, ya), (xb, yb) in zip(pts, pts[1:]):
                    dense.append((xa, ya))
                    for d in path.corners_between(base + xa, base + xb):
                        t = (d - base - xa) / (xb - xa) if xb != xa else 0
                        dense.append((d - base, ya + (yb - ya) * t))
                dense.append(pts[-1])
                mapped = [path.place(base + x, y) for x, y in dense if base + x <= path.length + 1e-9]
                if len(mapped) > 1:
                    out.append(("poly", mapped))
            elif item[0] == "dot":
                if base + item[1] <= path.length:
                    out.append(("dot", *path.place(base + item[1], item[2]), item[3]))
            elif item[0] == "text":
                if base + item[1] <= path.length:
                    (_, (ux, uy)) = path.locate(base + item[1])
                    out.append(("text", *path.place(base + item[1], item[2]), item[3], math.degrees(math.atan2(uy, ux))))
        k += 1
    return out


def bounds(defn):
    """Bounding box (x0, y0, x1, y1) of a definition's strokes, with a box for each text."""
    xs, ys = [], []
    for item in flatten(defn):
        if item[0] == "poly":
            xs += [p[0] for p in item[1]]; ys += [p[1] for p in item[1]]
        elif item[0] == "dot":
            xs += [item[1] - item[3], item[1] + item[3]]; ys += [item[2] - item[3], item[2] + item[3]]
        else:
            spec = item[3]
            h = spec.get("height", 2.0)
            w = 0.62 * h * max(len(spec.get("text", "")), 1) * spec.get("widthFactor", 1)
            j = spec.get("justify", "")
            cx = item[1] - (w / 2 if "centre" in j else (w if "right" in j else 0))
            cy = item[2] - (h / 2 if "middle" in j else (h if "top" in j else 0))
            xs += [cx, cx + w]; ys += [cy, cy + h]
    return (min(xs), min(ys), max(xs), max(ys)) if xs else (0, 0, 0, 0)


class Sheet:
    """A Pillow canvas drawn in millimetres at `ppmm` pixels a millimetre, supersampled.  Sheet space is
    y-DOWN (like the page); a definition is y-UP, so `to_sheet` flips it when it is drawn."""

    def __init__(self, width_mm, height_mm, ppmm, theme):
        self.ppmm, self.theme = ppmm, theme
        self.w, self.h = int(width_mm * ppmm), int(height_mm * ppmm)
        self.image = Image.new("RGB", (self.w * SUPERSAMPLE, self.h * SUPERSAMPLE), GROUND[theme])
        self.draw = ImageDraw.Draw(self.image)

    def px(self, x, y):
        return (x * self.ppmm * SUPERSAMPLE, y * self.ppmm * SUPERSAMPLE)

    def poly(self, points, colour, width_mm=0.25):
        self.draw.line([self.px(*p) for p in points], fill=colour,
                       width=max(int(width_mm * self.ppmm * SUPERSAMPLE), 1), joint="curve")

    def dot(self, x, y, r, colour):
        cx, cy = self.px(x, y)
        rr = max(r * self.ppmm * SUPERSAMPLE, 0.12 * self.ppmm * SUPERSAMPLE)
        self.draw.ellipse([cx - rr, cy - rr, cx + rr, cy + rr], fill=colour)

    def text(self, x, y, spec, colour, angle=0.0):
        """Characters anchored at sheet (x, y), turned `angle` degrees counter-clockwise."""
        h = spec.get("height", 2.0) * self.ppmm * SUPERSAMPLE
        font = _font(h)
        label = spec.get("text", "")
        box = self.draw.textbbox((0, 0), label, font=font)
        tw, th = box[2] - box[0], box[3] - box[1]
        pad = 4
        layer = Image.new("L", (int(tw) + 2 * pad, int(th) + 2 * pad), 0)
        ImageDraw.Draw(layer).text((pad - box[0], pad - box[1]), label, font=font, fill=255)
        j = spec.get("justify", "")
        ax = pad + (tw / 2 if "centre" in j else (tw if "right" in j else 0))
        ay = pad + (th / 2 if "middle" in j else (0 if "top" in j else th))
        vx, vy = ax - layer.size[0] / 2, ay - layer.size[1] / 2
        r = math.radians(angle)
        rot = layer.rotate(angle, expand=True, resample=Image.BICUBIC)
        rx = vx * math.cos(r) + vy * math.sin(r)
        ry = -vx * math.sin(r) + vy * math.cos(r)
        px, py = self.px(x, y)
        left = int(round(px - (rot.size[0] / 2 + rx)))
        top = int(round(py - (rot.size[1] / 2 + ry)))
        self.image.paste(colour, (left, top, left + rot.size[0], top + rot.size[1]), mask=rot)

    def label(self, x, y, text, size_mm=2.4, colour=None, anchor="la"):
        font = _font(size_mm * self.ppmm * SUPERSAMPLE)
        self.draw.text(self.px(x, y), text, font=font, fill=colour or INK[self.theme], anchor=anchor)

    def rule(self, y, colour=None):
        self.draw.line([self.px(0, y), self.px(self.w / self.ppmm, y)], fill=colour or (80, 88, 98), width=SUPERSAMPLE)

    def draw_items(self, items, colour, to_sheet, width_mm=0.25):
        for item in items:
            if item[0] == "poly":
                self.poly([to_sheet(*p) for p in item[1]], colour, width_mm)
            elif item[0] == "dot":
                self.dot(*to_sheet(item[1], item[2]), item[3], colour)
            elif item[0] == "text":
                self.text(*to_sheet(item[1], item[2]), item[3], colour, item[4] if len(item) > 4 else 0.0)

    def save(self, path):
        self.image.resize((self.w, self.h), Image.LANCZOS).save(path)


def _wrap(name, width_chars=14):
    lines, line = [], ""
    for word in name.split():
        if line and len(line) + 1 + len(word) > width_chars:
            lines.append(line)
            line = word
        else:
            line = (line + " " + word).strip()
    return lines + [line]


def symbol_sheet(definitions, path, colours=None, theme="dark", ppmm=26.0, columns=8, title="", cell_mm=12.0):
    """One cell per symbol, its name (wrapped) beneath.  `colours` maps a definition name to a hex colour."""
    definitions = list(definitions)
    rows = (len(definitions) + columns - 1) // columns
    head = 7.0 if title else 1.0
    row_h = cell_mm + 3.6
    sheet = Sheet(columns * cell_mm, head + rows * row_h, ppmm, theme)
    if title:
        sheet.label(2, 2.0, title, 3.6)
    for i, d in enumerate(definitions):
        col, row = i % columns, i // columns
        ox = col * cell_mm + cell_mm / 2
        oy = head + row * row_h + cell_mm / 2 - 0.6
        colour = hex_rgb((colours or {}).get(d["name"], "#9A8EA4"))
        sheet.draw_items(flatten(d), colour, lambda x, y, ox=ox, oy=oy: (ox + x, oy - y))
        for n, line in enumerate(_wrap(d["name"])[:3]):
            sheet.label(ox, oy + 3.7 + n * 1.5, line, 1.35, anchor="ma")
    sheet.save(path)


def linestyle_sheet(definitions, path, colours=None, theme="dark", ppmm=9.0, line_mm=88.0, title=""):
    """One row per linestyle: a straight run that turns through an obtuse corner, its name at the left."""
    definitions = list(definitions)
    name_w, row_h = 46.0, 9.0
    head = 7.0 if title else 1.0
    sheet = Sheet(name_w + line_mm + 4, head + len(definitions) * row_h, ppmm, theme)
    if title:
        sheet.label(2, 2.0, title, 3.6)
    for i, d in enumerate(definitions):
        yc = head + i * row_h + row_h / 2
        if d.get("units") == "twoPoint":
            path_pts = [(0, -1.0), (50, 1.0)]
        else:
            path_pts = [(0, -1.0), (52, -1.0), (62, 1.4), (line_mm - 8, 1.4)]
        colour = hex_rgb((colours or {}).get(d["name"], "#9A8EA4"))
        sheet.draw_items(lay_linestyle(d, Path(path_pts)), colour, lambda x, y, yc=yc: (name_w + x, yc - y))
        sheet.label(2, yc - 1.0, d["name"], 2.1)
        sheet.rule(head + (i + 1) * row_h, (44, 50, 58) if theme == "dark" else (225, 228, 232))
    sheet.save(path)


def colour_map(codes, palette):
    """definition name -> hex of the first code that draws it; `palette` maps a colour name to its hex."""
    out = {}
    for c in codes:
        for field in ("linestyle", "symbol"):
            n = c.get(field)
            if n and n not in out and c.get("colour") in palette:
                out[n] = palette[c["colour"]]
    return out
