"""Katana Standard: the symbol library (123 point symbols).

    python tools/katana_standard/symbols.py --check [--contract contract.json]
    python tools/katana_standard/symbols.py --sheets OUTDIR --contract contract.json

`SYMBOLS` is a list of 123 dicts in the customisation file's own member names (`name`, `group`, `units`,
`atVertices`, `strokes`) plus one extra, `description` (at most 160 characters: what it looks like and when to
use it).  Every symbol is drawn in `paper` units: 1 unit is 1 mm of page, so a symbol keeps its size as the view
zooms and no rule scales one.  Nothing here is copied from another library: each shape is built from the
construction kit below, from a short written brief.

THE HOUSE STYLE (read this once, then every symbol is learnt)

Cell.      Three size classes by larger extent: S about 3 mm (small furniture, posts, marks), M about 4.4 mm
           (most objects), L about 6 mm (canopies, tanks, stations, primary control).  The origin is the
           centre of the bounding box, so a symbol sits on its vertex without a rule offsetting it.  Where the
           surveyed point is NOT the box centre (a valve beside its letter, a doorway's wall line, a building
           corner, a camera's pole, a station's dot, a north arrow's shaft) the origin is within 0.6 mm of the
           box centre and the description says where it is; the check holds both.
Weight.    One weight: every symbol is outline.  The renderer applies the entity's own pen, so a symbol never
           carries a colour or a width.  Gaps are fixed: 0.4 mm between the rings of a double ring, 0.3 mm
           between a ring and a ray or tick, and a letter is always 2.4 mm high.
Fill.      There are no fills.  A solid centre mark (a very small ring with a dot in it, because Katana draws a
           `dot` as one pen-width point whatever its radius) means "a physical mark or stem is here and this is
           its exact point": pegs, pins, pipes, stations, poles, trunks.  A shape WITHOUT one is a cover, a sign
           or an observation.  Hatching (at most four strokes) means "solid": a borehole's quadrants.
           A dashed ring means a temporary or former state: a pothole, or a trunk whose width was not taken.  Rays
           mean a light (street light, bollard light, signal lamp).
Services.  One grammar for every service, learnt once; the service colour comes from the rule and the letter
           names the service (W water, S sewer, G gas, E electricity, C communications, R recycled, F fire,
           P fuel, I traffic systems, ? unknown).  Square = pit or box; single ring = round cover or pole;
           double ring = maintenance hole; bow-tie = valve; diamond = regulator or marker; ring with a bar =
           meter.  A letter is inside a pit, a ring or a hole, and BELOW a valve.  A fitting that several
           services share (air release valve, scour valve, test point, earthing point, tap) carries no letter:
           the rule's colour and layer name the service.  A fitting with a lettered family keeps its letter (a
           vent, a flow meter, a maintenance hole, a pit with two or three lids).
Trees.     A tree is shot by its canopy or by its trunk, and both are drawn as plain shapes with a centre dot.  A
           measured crown is a plain ring with a trunk dot and a radius line, in six sizes; a measured trunk is an
           eight-sided outline round a trunk dot, in six sizes, dashed when no width was taken.  The sizes step by
           about 0.8 mm so each is told from the next at screen size, and they are not to scale: a symbol keeps its
           paper size.
Control.   Distinguished by class: triangle with ring and dot = primary station; ring with long cross-hairs and
           dot = secondary mark; ring with a bar and arrow = benchmark; diamond with dot = fixed mark; open
           diamond with a plus = temporary; ring and square frame with a plus = the centre of an aerial photograph.
Direction. A symbol is not rotated, so a symbol that points (an arrow, a lens, a gate) points to +x or +y; the
           rule and the line it sits on say the rest.
"""
import math
import os
import re
import sys

# ---------------------------------------------------------------------------------------------------------
# Construction kit
# ---------------------------------------------------------------------------------------------------------

LABEL_HEIGHT = 2.4          # a letter in a symbol is always this high (millimetres)
BEAD = 0.3                  # a solid centre mark (radius): a ring with a dot in it
BEAD_SMALL = 0.2            # the same in a cramped place
CHORDS_A_TURN = 24          # Katana draws an arc or a circle as 24 chords a turn


def _n(v):
    """A number as the file writes it: at most 3 decimals, no negative zero, integers without a point."""
    v = round(float(v), 3)
    if v == 0:
        v = 0.0
    return int(v) if v == int(v) else v


def polar(r, deg, cx=0.0, cy=0.0):
    a = math.radians(deg)
    return (cx + r * math.cos(a), cy + r * math.sin(a))


def ngon(n, r, phase=90.0, cx=0.0, cy=0.0):
    """A regular polygon's corners, the first at angle `phase` (90 is straight up)."""
    return [polar(r, phase + 360.0 * i / n, cx, cy) for i in range(n)]


def star(n, r_out, r_in, phase=90.0, cx=0.0, cy=0.0):
    """A star's 2n corners alternating between the outer and inner radius."""
    pts = []
    for i in range(2 * n):
        pts.append(polar(r_out if i % 2 == 0 else r_in, phase + 180.0 * i / n, cx, cy))
    return pts


def triangle(side, up=True, cx=0.0, cy=0.0):
    """An equilateral triangle by side whose bounding box is centred on (cx, cy)."""
    h = side * math.sqrt(3) / 2
    if up:
        return [(cx, cy + h / 2), (cx - side / 2, cy - h / 2), (cx + side / 2, cy - h / 2)]
    return [(cx, cy - h / 2), (cx - side / 2, cy + h / 2), (cx + side / 2, cy + h / 2)]


def bezier(p0, p1, p2, p3, steps=8):
    """Points along a cubic curve, for a smooth outline (a flame) drawn as short chords."""
    pts = []
    for i in range(steps + 1):
        t = i / steps
        u = 1 - t
        pts.append((u ** 3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t ** 3 * p3[0],
                    u ** 3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t ** 3 * p3[1]))
    return pts


def _circle_cross(c0, r0, c1, r1):
    """The two points where two circles meet, or None."""
    dx, dy = c1[0] - c0[0], c1[1] - c0[1]
    d = math.hypot(dx, dy)
    if d == 0 or d > r0 + r1 or d < abs(r0 - r1):
        return None
    a = (r0 * r0 - r1 * r1 + d * d) / (2 * d)
    h = math.sqrt(max(r0 * r0 - a * a, 0.0))
    mx, my = c0[0] + a * dx / d, c0[1] + a * dy / d
    return [(mx + h * dy / d, my - h * dx / d), (mx - h * dy / d, my + h * dx / d)]


class Sketch:
    """Builds a stroke list.  Every mark starts with its own `move`, so the pen never carries over (arc, circle,
    dot and text do not move it in the file format)."""

    def __init__(self):
        self.strokes = []

    # -- primitives
    def _move(self, x, y):
        self.strokes.append(["move", _n(x), _n(y)])

    def _draw(self, x, y):
        self.strokes.append(["draw", _n(x), _n(y)])

    def line(self, x0, y0, x1, y1):
        self._move(x0, y0)
        self._draw(x1, y1)
        return self

    def path(self, pts, close=False):
        pts = list(pts)
        self._move(*pts[0])
        for p in pts[1:]:
            self._draw(*p)
        if close:
            self._draw(*pts[0])
        return self

    def box(self, w, h, cx=0.0, cy=0.0):
        return self.path([(cx - w / 2, cy - h / 2), (cx + w / 2, cy - h / 2), (cx + w / 2, cy + h / 2),
                          (cx - w / 2, cy + h / 2)], close=True)

    def rounded_box(self, w, h, r, cx=0.0, cy=0.0):
        """A rectangle with round corners of radius r."""
        a, b = w / 2 - r, h / 2 - r
        self.line(cx - a, cy + h / 2, cx + a, cy + h / 2).line(cx + w / 2, cy + b, cx + w / 2, cy - b)
        self.line(cx + a, cy - h / 2, cx - a, cy - h / 2).line(cx - w / 2, cy - b, cx - w / 2, cy + b)
        self.arc(r, 0, 90, cx + a, cy + b).arc(r, 90, 180, cx - a, cy + b)
        self.arc(r, 180, 270, cx - a, cy - b).arc(r, 270, 360, cx + a, cy - b)
        return self

    def ring(self, r, cx=0.0, cy=0.0):
        self._move(cx, cy)
        self.strokes.append(["circle", _n(r)])
        return self

    def arc(self, r, a0, a1, cx=0.0, cy=0.0):
        self._move(cx, cy)
        self.strokes.append(["arc", _n(r), _n(a0), _n(a1)])
        return self

    def dot(self, cx=0.0, cy=0.0, r=0.3):
        self._move(cx, cy)
        self.strokes.append(["dot", _n(r)])
        return self

    def bead(self, cx=0.0, cy=0.0, r=BEAD):
        """A solid mark.  Katana draws a `dot` as one pen-width point whatever its radius, so a mark that must
        read as solid is a very small ring with a dot at its centre."""
        self.ring(r, cx, cy)
        self.dot(cx, cy, r)
        return self

    def letter(self, text, cx=0.0, cy=0.0):
        self._move(cx, cy)
        self.strokes.append(["text", {"text": text, "height": LABEL_HEIGHT, "justify": "middle-centre"}])
        return self

    # -- composed marks
    def plus(self, half, cx=0.0, cy=0.0):
        self.line(cx - half, cy, cx + half, cy)
        self.line(cx, cy - half, cx, cy + half)
        return self

    def cross(self, half, cx=0.0, cy=0.0):
        self.line(cx - half, cy - half, cx + half, cy + half)
        self.line(cx - half, cy + half, cx + half, cy - half)
        return self

    def ticks(self, r0, r1, angles, cx=0.0, cy=0.0):
        for a in angles:
            p, q = polar(r0, a, cx, cy), polar(r1, a, cx, cy)
            self.line(p[0], p[1], q[0], q[1])
        return self

    def bowtie(self, w, h, cx=0.0, cy=0.0):
        a, b = w / 2, h / 2
        return self.path([(cx - a, cy + b), (cx + a, cy - b), (cx + a, cy + b), (cx - a, cy - b)], close=True)

    def dashed_ring(self, r, n, fraction, phase=0.0, cx=0.0, cy=0.0):
        step = 360.0 / n
        for i in range(n):
            a0 = phase + i * step
            self.arc(r, a0, a0 + step * fraction, cx, cy)
        return self

    def bow(self, a, b, radius, side=1):
        """A circular arc from point a to point b bulging to the left (side 1) or right (-1) of a to b."""
        (ax, ay), (bx, by) = a, b
        d = math.hypot(bx - ax, by - ay)
        h = math.sqrt(max(radius * radius - d * d / 4, 0.0))
        mx, my = (ax + bx) / 2, (ay + by) / 2
        nx, ny = -(by - ay) / d, (bx - ax) / d
        cx, cy = mx - side * h * nx, my - side * h * ny
        aa = math.degrees(math.atan2(ay - cy, ax - cx))
        ab = math.degrees(math.atan2(by - cy, bx - cx))
        sweep = (ab - aa) % 360.0
        if sweep <= 180.0:
            return self.arc(radius, aa, aa + sweep, cx, cy)
        return self.arc(radius, ab, ab + (360.0 - sweep), cx, cy)

    def scallops(self, lobes, centre=(0.0, 0.0)):
        """A closed outline made of round lobes, in order round the shape: (cx, cy, r) each.  Each lobe is
        drawn between the two points where it meets its neighbours, so the outline is one clean scalloped line
        and no lobe is seen through another."""
        n = len(lobes)
        cusp = []
        for i in range(n):
            a, b = lobes[i], lobes[(i + 1) % n]
            pair = _circle_cross((a[0], a[1]), a[2], (b[0], b[1]), b[2])
            if pair is None:
                raise ValueError("scallop lobes do not meet")
            cusp.append(max(pair, key=lambda p: math.hypot(p[0] - centre[0], p[1] - centre[1])))
        for i in range(n):
            cx, cy, r = lobes[i]
            p0, p1 = cusp[i - 1], cusp[i]
            a0 = math.degrees(math.atan2(p0[1] - cy, p0[0] - cx))
            a1 = math.degrees(math.atan2(p1[1] - cy, p1[0] - cx))
            self.arc(r, a0, a0 + (a1 - a0) % 360.0, cx, cy)
        return self

    def done(self):
        return self.strokes


def _bounds(strokes):
    """(x0, y0, x1, y1) as the checker measures it: arcs as 24 chords a turn, dots by radius, a letter by a
    box 2.4 high and 0.62 times that wide.  A `move` alone is not geometry (an arc is drawn about it)."""
    xs, ys = [], []
    x = y = 0.0
    run = False                     # the pen is down and has been placed on the paper
    for s in strokes:
        k = s[0]
        if k == "move":
            x, y = s[1], s[2]
            run = False
        elif k == "draw":
            if not run:
                xs.append(x)
                ys.append(y)
                run = True
            x, y = s[1], s[2]
            xs.append(x)
            ys.append(y)
        elif k == "circle":
            run = False
            for i in range(CHORDS_A_TURN):
                p = polar(s[1], 360.0 * i / CHORDS_A_TURN, x, y)
                xs.append(p[0])
                ys.append(p[1])
        elif k == "arc":
            run = False
            sweep = s[3] - s[2]
            steps = max(int(math.ceil(abs(sweep) / (360.0 / CHORDS_A_TURN))), 1)
            for i in range(steps + 1):
                p = polar(abs(s[1]), s[2] + sweep * i / steps, x, y)
                xs.append(p[0])
                ys.append(p[1])
        elif k == "dot":
            run = False
            xs += [x - s[1], x + s[1]]
            ys += [y - s[1], y + s[1]]
        elif k == "text":
            run = False
            h = s[1].get("height", LABEL_HEIGHT)
            w = 0.62 * h * max(len(s[1].get("text", "")), 1)
            xs += [x - w / 2, x + w / 2]
            ys += [y - h / 2, y + h / 2]
    return (min(xs), min(ys), max(xs), max(ys)) if xs else (0.0, 0.0, 0.0, 0.0)


def _shifted(strokes, dx, dy):
    out = []
    for s in strokes:
        if s[0] in ("move", "draw"):
            out.append([s[0], _n(s[1] + dx), _n(s[2] + dy)])
        else:
            out.append(list(s))
    return out


def centred(sketch):
    """The strokes moved so the bounding box is centred on the origin (for symbols whose parts are placed by
    their shape and not around the surveyed point)."""
    x0, y0, x1, y1 = _bounds(sketch.strokes)
    return _shifted(sketch.strokes, -(x0 + x1) / 2, -(y0 + y1) / 2)


# ---------------------------------------------------------------------------------------------------------
# The library
# ---------------------------------------------------------------------------------------------------------

G_MARKS = "Boundaries and Cadastre/Boundary Marks"
G_MARKINGS = "Roads and Pavements/Markings"
G_ACCESS = "Roads and Pavements/Paths and Access"
G_SIGNS = "Street Furniture and Signs/Signs"
G_LIGHTING = "Street Furniture and Signs/Lighting"
G_POSTS = "Street Furniture and Signs/Posts and Barriers"
G_AMENITIES = "Street Furniture and Signs/Amenities"
G_TRAFFIC = "Street Furniture and Signs/Traffic Control"
G_PITS = "Kerbs and Drainage/Pits and Structures"
G_TRACK = "Rail and Transit/Track"
G_SIGNALS = "Rail and Transit/Overhead and Signals"
G_WATER = "Utilities/Water"
G_SEWER = "Utilities/Sewer"
G_GAS = "Utilities/Gas"
G_ELEC = "Utilities/Electricity"
G_COMMS = "Utilities/Communications"
G_RECYCLED = "Utilities/Recycled Water"
G_FIRE = "Utilities/Fire Service"
G_FUEL = "Utilities/Fuel"
G_ITS = "Utilities/Traffic Systems"
G_UNKNOWN = "Utilities/Unknown Services"
G_ALL = "Utilities/All Services"
G_TREES = "Vegetation and Landscape/Trees"
G_BUILDINGS = "Buildings and Structures/Buildings"
G_STRUCTURES = "Buildings and Structures/Structures"
G_GATES = "Fences and Walls/Gates and Posts"
G_POINTS = "Terrain and Breaklines/Ground Points"
G_ROCK = "Terrain and Breaklines/Rock and Cliff"
G_CONTROL = "Survey Control and Annotation/Control Marks"
G_INVEST = "Survey Control and Annotation/Investigation Points"
G_PLAN = "Survey Control and Annotation/Plan Marks"
G_HAZARD = "Miscellaneous/Hazards and Heritage"
G_GENERAL = "Miscellaneous/General"
G_WATEREDGE = "Terrain and Breaklines/Water Edges"

SYMBOLS = []


def symbol(name, group, description, centre=False):
    """Register the drawing function below as symbol `name`."""
    def register(build):
        sketch = build()
        SYMBOLS.append({
            "name": name,
            "group": group,
            "units": "paper",
            "atVertices": True,
            "description": description,
            "strokes": centred(sketch) if centre else sketch.done(),
        })
        return build
    return register


# ---- Boundaries and Cadastre / Boundary Marks (a dot means a monument is set here) ------------------------

@symbol("Corner Peg", G_MARKS, "Square with a centre dot. A peg or stake set at a boundary corner.")
def _():
    return Sketch().box(3.2, 3.2).bead(0, 0, 0.36)


@symbol("Iron Pin", G_MARKS, "Ring with a centre dot. A round iron pin or rod set at a corner.")
def _():
    return Sketch().ring(1.6).bead(0, 0, 0.36)


@symbol("Iron Pipe", G_MARKS, "Double ring with a centre dot. An iron pipe: the inner ring is its bore.")
def _():
    return Sketch().ring(1.7).ring(1.0).bead(0, 0, 0.26)


@symbol("Survey Spike", G_MARKS, "Small ring held by four ticks. A spike or nail driven into a hard surface; no dot, the ring is the head.")
def _():
    return Sketch().ring(0.4).ticks(0.75, 1.3, [0, 90, 180, 270])


@symbol("Winged Drill Hole", G_MARKS, "Drilled Hole with an open wedge running out to +x, the wing cut beside it to lead the eye to the hole. The origin is the hole.")
def _():
    return Sketch().ring(1.0).cross(0.4).line(0.93, 0.36, 2.1, 0.95).line(0.93, -0.36, 2.1, -0.95)


@symbol("Star Picket", G_MARKS, "Three arms at 120 degrees round a centre dot: the section of a steel picket driven in as a mark. The origin is its centre dot.")
def _():
    k = Sketch().bead(0, 0, 0.3)
    for a in (90, 210, 330):
        p, q = polar(0.6, a), polar(1.55, a)
        k.line(p[0], p[1], q[0], q[1])
    return k


# ---- Roads and Pavements -----------------------------------------------------------------------------------

@symbol("Raised Pavement Marker", G_MARKINGS, "Ring with a small solid mark each side. A domed marker fixed to the road, the dome between its two reflectors.")
def _():
    return Sketch().ring(0.65).bead(-1.3, 0, 0.25).bead(1.3, 0, 0.25)


# ---- Street Furniture and Signs ---------------------------------------------------------------------------

@symbol("Bus Stop Sign", G_SIGNS, "Round-cornered square holding B. A bus stop sign face; the origin is its middle, a little off the post it stands on.")
def _():
    return Sketch().rounded_box(3.2, 3.2, 0.7).letter("B", 0, 0)


@symbol("Sign Post", G_SIGNS, "Ring with a centre dot and a plate touching its right side. The post of a sign whose face is not classified.")
def _():
    return Sketch().ring(0.8).bead(0, 0, 0.2).box(0.4, 1.8, 1.0, 0)


@symbol("Double Sided Sign Post", G_SIGNS, "Ring with a centre dot and a plate touching each side. A post carrying a sign face on both of its sides.")
def _():
    return Sketch().ring(0.8).bead(0, 0, 0.2).box(0.4, 1.8, 1.0, 0).box(0.4, 1.8, -1.0, 0)


@symbol("Street Light", G_LIGHTING, "Ring with a centre dot and eight rays. A street light pole and its lantern.")
def _():
    return Sketch().ring(1.0).bead(0, 0, 0.2).ticks(1.3, 2.2, range(0, 360, 45))


@symbol("Bollard Light", G_LIGHTING, "Small ring with four short rays. A light built into a bollard.")
def _():
    return Sketch().ring(0.7).ticks(0.95, 1.55, [0, 90, 180, 270])


@symbol("Hanging Light", G_LIGHTING, "Ring hung from a wire by a short hanger, with five rays shining down. A light hung from an overhead wire; the origin is the ring centre.")
def _():
    return Sketch().ring(0.8).line(0, 0.8, 0, 1.5).line(-2.0, 1.5, 2.0, 1.5).ticks(1.1, 1.9, [180, 225, 270, 315, 0])


@symbol("Bollard", G_POSTS, "Ring inside a ring. A fixed bollard.")
def _():
    return Sketch().ring(1.0).ring(0.45)


@symbol("Litter Bin", G_AMENITIES, "Hexagon with a centre dot. A litter or waste bin.")
def _():
    return Sketch().path(ngon(6, 1.3, 90), close=True).bead(0, 0, BEAD_SMALL)


@symbol("Fireplace", G_AMENITIES, "Square hearth holding a flame with a tongue inside it. A built fireplace or barbecue hearth.")
def _():
    r, cy = 0.7, -0.5                # a round foot, a left side that bellies and a right side that leans in to the tip
    left = bezier((-r, cy), (-r - 0.1, 0.3), (-0.2, 0.5), (0.2, 1.15))
    right = bezier((0.2, 1.15), (0.55, 0.5), (0.75, 0.2), (r, cy))
    k = Sketch().box(3.0, 3.0).path(left + right[1:]).arc(r, 180, 360, 0, cy)
    return k.path(bezier((-0.2, -0.75), (-0.3, -0.25), (0.0, -0.15), (0.15, 0.25)))


@symbol("Parking Meter", G_AMENITIES, "Ring with a centre dot carrying a flat head on its top. A kerbside parking meter; the origin is the pole, the ring centre.")
def _():
    return Sketch().ring(0.8).bead(0, 0, 0.2).box(1.9, 0.6, 0, 1.1)


@symbol("Letterbox", G_AMENITIES, "Wide box with a half-round lid and a slot across its face. A letterbox at a property frontage.", centre=True)
def _():
    return Sketch().path([(-1.4, 0.2), (-1.4, -0.9), (1.4, -0.9), (1.4, 0.2)]).arc(1.4, 0, 180, 0, 0.2).line(-0.9, -0.35, 0.9, -0.35)


@symbol("Signal Pole", G_TRAFFIC, "Ring with a dot beside a tall box holding three lamp dots. A traffic signal pole and its signal head.")
def _():
    k = Sketch().ring(1.1, -0.4, 0).bead(-0.4, 0, 0.16).box(1.0, 2.8, 1.5, 0)
    for y in (-0.8, 0, 0.8):
        k.dot(1.5, y, 0.2)
    return k


@symbol("Roadside Camera", G_TRAFFIC, "Ring with a triangular lens pointing +x. A speed, red-light or traffic camera; the origin is its pole, the ring's centre.")
def _():
    return Sketch().ring(1.1, -0.5, 0).path([(0.7, 0.62), (2.2, 0), (0.7, -0.62)], close=True)


@symbol("Signal Controller Cabinet", G_TRAFFIC, "Square holding the letter T. The traffic signal controller cabinet.")
def _():
    return Sketch().box(3.2, 3.2).letter("T", 0, 0)


@symbol("Camera Strobe", G_TRAFFIC, "Ring holding a lightning bolt. The flash unit of an enforcement camera, shot at its pole.")
def _():
    return Sketch().ring(1.6).path([(0.45, 1.0), (-0.55, -0.15), (-0.05, -0.15), (-0.4, -1.0), (0.6, 0.2), (0.1, 0.2)], close=True)


# ---- Kerbs and Drainage / Pits and Structures --------------------------------------------------------------

@symbol("Gully Pit", G_PITS, "Square with three grate bars. A stormwater gully pit with its grating.")
def _():
    k = Sketch().box(3.2, 3.2)
    for y in (-0.8, 0, 0.8):
        k.line(-1.6, y, 1.6, y)
    return k


@symbol("Junction Pit", G_PITS, "Square with both diagonals. A stormwater junction pit.")
def _():
    return Sketch().box(3.2, 3.2).cross(1.6)


@symbol("Stormwater Manhole", G_PITS, "Double ring holding the letter D. A stormwater maintenance hole (D for drainage).")
def _():
    return Sketch().ring(1.7).ring(1.3).letter("D", 0, 0)


@symbol("Headwall", G_PITS, "Three sides of a box, open to -x, with wings flaring outward. A pipe headwall; the pipe arrives from the open side.")
def _():
    return (Sketch().path([(-2.2, 1.2), (2.4, 1.2), (2.4, -1.2), (-2.2, -1.2)])
            .line(-2.2, 1.2, -2.9, 1.9).line(-2.2, -1.2, -2.9, -1.9))


@symbol("Subsoil Inspection Point", G_PITS, "Small ring holding a smaller square, the cap of the riser. An inspection point on a subsoil drain.")
def _():
    return Sketch().ring(1.1).box(0.8, 0.8)


@symbol("Wingwall End", G_PITS, "Short wall stub at 45 degrees, two lines apart and closed at the outer end. The free end of a wing wall; the origin is the middle of the stub.", centre=True)
def _():
    u, n = polar(1.0, 45), polar(1.0, 135)
    c = [(s * u[0] + t * n[0], s * u[1] + t * n[1]) for s, t in ((0.0, -0.4), (2.6, -0.4), (2.6, 0.4), (0.0, 0.4))]
    return Sketch().path(c)


@symbol("Sump Inlet", G_PITS, "Square holding a downward arrow that lands on a bar. Drainage dropping into a sump.")
def _():
    return Sketch().box(3.2, 3.2).line(0, 1.0, 0, -0.4).path([(-0.5, 0.1), (0, -0.4), (0.5, 0.1)]).line(-0.8, -0.95, 0.8, -0.95)


# ---- Rail and Transit ---------------------------------------------------------------------------------------

@symbol("Signal Mast", G_SIGNALS, "Pole ring with a dot, joined to a lamp ring with rays. A railway signal on its mast.")
def _():
    return (Sketch().ring(0.8, -1.3, 0).bead(-1.3, 0, 0.16).line(-0.5, 0, 0.45, 0).ring(0.7, 1.15, 0)
            .ticks(1.0, 1.45, [0, 40, -40], 1.15, 0))


@symbol("Overhead Mast", G_SIGNALS, "Square with a centre dot and two arms. A mast carrying overhead wires.")
def _():
    return Sketch().box(2.4, 2.4).bead(0, 0, BEAD_SMALL).line(-2.2, 0.6, -1.2, 0.6).line(1.2, 0.6, 2.2, 0.6)


@symbol("Rail Control Box", G_TRACK, "Square holding two parallel bars, the rails. A control or relay box beside the track.")
def _():
    return Sketch().box(2.8, 2.8).line(-0.9, 0.45, 0.9, 0.45).line(-0.9, -0.45, 0.9, -0.45)


@symbol("Rail Cable Trough", G_TRACK, "Long narrow box cut into three lid slabs by two cross lines. A lineside trough carrying signalling cables.")
def _():
    return Sketch().box(3.4, 1.2).line(-0.57, 0.6, -0.57, -0.6).line(0.57, 0.6, 0.57, -0.6)


# ---- Utilities (one grammar: square pit, ring cover, double ring hole, bow-tie valve, ring and bar meter) ----

def _valve(letter):
    return Sketch().bowtie(3.6, 2.0, 0, 0.95).letter(letter, 0, -1.25)


def _pit(letter):
    return Sketch().box(3.4, 3.4).letter(letter, 0, 0)


def _hole(letter):
    return Sketch().ring(1.9).ring(1.5).letter(letter, 0, 0)


def _meter(letter):
    return Sketch().ring(1.7).letter(letter, 0, 0.5).line(-0.8, -1.05, 0.8, -1.05)


@symbol("Water Isolation Valve", G_WATER, "Bow-tie with the letter W below. A water main valve; the origin is between valve and letter.")
def _():
    return _valve("W")


@symbol("Water Flow Meter", G_WATER, "Ring holding W above a bar. A water meter on the line.")
def _():
    return _meter("W")


@symbol("Sewer Maintenance Hole", G_SEWER, "Double ring holding S. A sewer maintenance hole.")
def _():
    return _hole("S")


@symbol("Sewer Vent", G_SEWER, "Ring holding S with a chevron above. A sewer vent shaft.")
def _():
    return Sketch().ring(1.4, 0, -0.55).letter("S", 0, -0.55).path([(-0.8, 1.45), (0, 2.1), (0.8, 1.45)])


@symbol("Sewer Isolation Valve", G_SEWER, "Bow-tie with the letter S below. A sewer stop valve; the origin is between valve and letter.")
def _():
    return _valve("S")


@symbol("Sewer Inspection Opening", G_SEWER, "Single ring holding S. A small round cover giving a lamp a view along a sewer; the double ring is the man-entry hole.")
def _():
    return Sketch().ring(1.6).letter("S", 0, 0)


@symbol("Gas Pit", G_GAS, "Square holding G. A gas valve or meter pit.")
def _():
    return _pit("G")


@symbol("Gas Flow Meter", G_GAS, "Ring holding G above a bar. A gas meter.")
def _():
    return _meter("G")


@symbol("Gas Pressure Regulator", G_GAS, "Diamond holding G. A gas pressure regulating set.")
def _():
    return Sketch().path([(0, 1.9), (1.9, 0), (0, -1.9), (-1.9, 0)], close=True).letter("G", 0, 0)


@symbol("Gas Maintenance Hole", G_GAS, "Double ring holding the letter G. A gas maintenance hole.")
def _():
    return _hole("G")


@symbol("Gas Vent", G_GAS, "Ring holding G with a chevron above. A vent riser on a gas main or casing.")
def _():
    return Sketch().ring(1.4, 0, -0.55).letter("G", 0, -0.55).path([(-0.8, 1.45), (0, 2.1), (0.8, 1.45)])


@symbol("High Pressure Marker Post", G_GAS, "Marker Post ring holding two stacked triangles. A pipeline marker for a high pressure gas main.")
def _():
    return Sketch().ring(1.5).path(triangle(1.0, True, 0, -0.57), close=True).path(triangle(1.0, True, 0, 0.43), close=True)


@symbol("Power Pole", G_ELEC, "Ring, inner ring and centre dot. A power pole: the rings are the pole and its footing.")
def _():
    return Sketch().ring(1.55).ring(0.8).bead(0, 0, 0.2)


@symbol("Guy Anchor", G_ELEC, "Line with an arrowhead at -x and an anchor bar at +x. A pole's stay wire running to its ground anchor.")
def _():
    return (Sketch().line(-1.8, 0, 1.8, 0).path([(-1.25, 0.45), (-1.8, 0), (-1.25, -0.45)])
            .line(1.8, 0.6, 1.8, -0.6))


@symbol("Transformer", G_ELEC, "Two overlapping rings. A pole or pad transformer.")
def _():
    return Sketch().ring(1.2, -0.7, 0).ring(1.2, 0.7, 0)


@symbol("Electrical Pillar", G_ELEC, "Square with one diagonal. A pillar, kiosk or service cabinet; the slash marks a pillar, as on the communications one.")
def _():
    return Sketch().box(3.0, 3.0).line(-1.5, -1.5, 1.5, 1.5)


@symbol("Electrical Pit", G_ELEC, "Square holding E. An electrical pit.")
def _():
    return _pit("E")


@symbol("Electrical Maintenance Hole", G_ELEC, "Double ring holding the letter E. A maintenance hole giving access to underground cables.")
def _():
    return _hole("E")


@symbol("Electrical Fuse", G_ELEC, "Narrow box with a line through its length that runs past both ends. A fuse on a distribution cable, in a pole or a cabinet.")
def _():
    return Sketch().box(2.6, 1.1).line(-1.6, 0, 1.6, 0)


@symbol("Power and Light Pole", G_ELEC, "Power Pole with two short sprays of rays at its upper left and right. A pole carrying both power lines and a lantern.")
def _():
    return Sketch().ring(1.55).ring(0.8).bead(0, 0, 0.2).ticks(1.85, 2.45, [35, 55, 125, 145])


@symbol("Stay Pole", G_ELEC, "Small ring with a centre dot and a brace running to the upper right, ending in a cross tick. A short pole carrying a stay wire; the origin is the ring centre.")
def _():
    return Sketch().ring(0.8).bead(0, 0, 0.2).line(0.57, 0.57, 1.4, 1.4).line(1.0, 1.8, 1.8, 1.0)


@symbol("Transmission Pylon", G_ELEC, "Four leg squares at the corners joined by sides and diagonals round a small ring. The footprint of a lattice tower on high voltage lines.")
def _():
    k = Sketch()
    for sx in (-1, 1):
        for sy in (-1, 1):
            k.box(1.0, 1.0, sx * 2.4, sy * 2.4)
    k.line(-1.9, 2.4, 1.9, 2.4).line(-1.9, -2.4, 1.9, -2.4).line(-2.4, -1.9, -2.4, 1.9).line(2.4, -1.9, 2.4, 1.9)
    for sx in (-1, 1):
        for sy in (-1, 1):
            k.line(sx * 1.9, sy * 1.9, sx * 0.4, sy * 0.4)
    return k.ring(0.5)


@symbol("Communications Pit", G_COMMS, "Square holding C. A communications pit.")
def _():
    return _pit("C")


@symbol("Communications Pillar", G_COMMS, "Square holding C with a broken diagonal. A communications pillar or cabinet.")
def _():
    k = Sketch().box(3.2, 3.2).letter("C", 0, 0)
    for s in (1, -1):
        p, q = polar(1.2, 45, 0, 0), polar(2.26, 45, 0, 0)
        k.line(s * p[0], s * p[1], s * q[0], s * q[1])
    return k


@symbol("Communications Twin Pit", G_COMMS, "Box of two square cells, each holding C. A communications pit with two lids side by side.")
def _():
    return Sketch().box(4.7, 3.0).line(0, -1.5, 0, 1.5).letter("C", -1.175, 0).letter("C", 1.175, 0)


@symbol("Communications Triple Pit", G_COMMS, "Box of three square cells, each holding C. A communications pit with three lids in a row.")
def _():
    return Sketch().box(6.3, 3.0).line(-1.05, -1.5, -1.05, 1.5).line(1.05, -1.5, 1.05, 1.5).letter("C", -2.1, 0).letter("C", 0, 0).letter("C", 2.1, 0)


@symbol("Communications Main Pit", G_COMMS, "Large square holding a smaller square lid with C. A main pit at a major cable junction, twice the size of the ordinary pit.")
def _():
    return Sketch().box(6.0, 6.0).box(3.4, 3.4).letter("C", 0, 0)


@symbol("Telephone Box", G_COMMS, "Square holding a telephone handset, an arch with a ring at each end. A public call box on the footpath.")
def _():
    return Sketch().box(4.4, 4.4).arc(2.2, 52, 128, 0, -1.55).ring(0.5, -1.35, 0.0).ring(0.5, 1.35, 0.0)


@symbol("Recycled Isolation Valve", G_RECYCLED, "Bow-tie with the letter R below. A recycled water valve; the origin is between valve and letter.")
def _():
    return _valve("R")


@symbol("Recycled Flow Meter", G_RECYCLED, "Ring holding R above a bar. A recycled water meter.")
def _():
    return _meter("R")


@symbol("Fire Hydrant", G_FIRE, "Ring with a plus and a nozzle tick each side. A fire hydrant.")
def _():
    return Sketch().ring(1.5).plus(0.7).line(1.5, 0, 2.3, 0).line(-1.5, 0, -2.3, 0)


@symbol("Petrol Pump", G_FUEL, "Tall box holding P, with a hose running out of its side to a nozzle tick. A fuel dispenser; the origin is the middle of its body.")
def _():
    return Sketch().box(2.4, 3.2).letter("P", 0, 0).path([(1.2, 0.6), (2.0, 0.6), (2.0, -0.4)]).line(1.7, -0.7, 2.3, -0.7)


@symbol("Loop Detector", G_ITS, "Square holding a three-peak zigzag. A traffic detector loop.")
def _():
    return (Sketch().box(3.4, 3.4).path([(-1.3, -0.6), (-0.9, 0.6), (-0.45, -0.6), (0, 0.6), (0.45, -0.6), (0.9, 0.6), (1.3, -0.6)]))


@symbol("Traffic Systems Pit", G_ITS, "Square holding I. A traffic systems or ITS pit.")
def _():
    return _pit("I")


@symbol("Unknown Pit", G_UNKNOWN, "Square holding a question mark. A pit whose service is not known.")
def _():
    return _pit("?")


@symbol("Pumping Station", G_ALL, "Square holding a ring with a flow arrow. A pumping station of any service.")
def _():
    return (Sketch().box(5.4, 5.4).ring(1.5).line(-0.8, 0, 0.8, 0).path([(0.35, 0.4), (0.8, 0), (0.35, -0.4)]))


@symbol("Marker Post", G_ALL, "Ring holding an upward triangle. A route marker post for any service.")
def _():
    return Sketch().ring(1.0).path(triangle(1.1, True, 0, -0.05), close=True)


@symbol("Air Release Valve", G_ALL, "Bow-tie with a stem and an arrowhead rising from it. A valve that lets air out at a high point of a pressure main.")
def _():
    return Sketch().bowtie(3.2, 1.8, 0, -0.4).line(0, -0.4, 0, 1.5).path([(-0.5, 1.0), (0, 1.5), (0.5, 1.0)])


@symbol("Scour Valve", G_ALL, "Bow-tie with a stem and an arrowhead falling from it. A valve that empties a main at a low point.")
def _():
    return Sketch().bowtie(3.2, 1.8, 0, 0.4).line(0, 0.4, 0, -1.5).path([(-0.5, -1.0), (0, -1.5), (0.5, -1.0)])


@symbol("Test Point", G_ALL, "Small ring with a stem and a cross bar rising from it. A fitting on a pipeline where pressure or protection is tested; the origin is the ring centre.")
def _():
    return Sketch().ring(0.8).line(0, 0.8, 0, 1.5).line(-0.7, 1.5, 0.7, 1.5)


@symbol("Earthing Point", G_ALL, "Small ring on a stem that ends in three bars of falling length. The point where a pipe is bonded to earth; the origin is the middle of the sign.")
def _():
    return (Sketch().ring(0.5, 0, 1.0).line(0, 0.5, 0, -0.2).line(-0.9, -0.2, 0.9, -0.2)
            .line(-0.6, -0.6, 0.6, -0.6).line(-0.3, -1.0, 0.3, -1.0))


@symbol("Tap", G_ALL, "Small ring with a spout that rises and turns down. A tap, outlet or standpipe; the origin is the ring centre.")
def _():
    return Sketch().ring(0.8).path([(0, 0.8), (0, 1.5), (0.9, 1.5), (0.9, 1.0)])


# ---- Vegetation and Landscape (all trees keep a trunk dot) -------------------------------------------------

def _canopy(across):
    """A tree whose crown has been measured: a plain ring, a trunk dot, and a radius line to the ring.  The
    ring grows with the spread but is not to scale, since a symbol keeps its paper size."""
    r = across / 2.0
    return Sketch().ring(r).bead(0, 0, 0.2).line(0.45, 0, r, 0)


@symbol("Tiny Tree Canopy", G_TREES, "Plain ring 2.2 mm across with a trunk dot and a radius line. A tree with a measured spread of one or two metres; not to scale.")
def _():
    return _canopy(2.2)


@symbol("Small Tree Canopy", G_TREES, "Plain ring 3.0 mm across with a trunk dot and a radius line. A tree with a measured spread of three or four metres; not to scale.")
def _():
    return _canopy(3.0)


@symbol("Medium Tree Canopy", G_TREES, "Plain ring 3.8 mm across with a trunk dot and a radius line. A tree with a measured spread of five or six metres; not to scale.")
def _():
    return _canopy(3.8)


@symbol("Large Tree Canopy", G_TREES, "Plain ring 4.6 mm across with a trunk dot and a radius line. A tree with a measured spread of seven or eight metres; not to scale.")
def _():
    return _canopy(4.6)


@symbol("Broad Tree Canopy", G_TREES, "Plain ring 5.4 mm across with a trunk dot and a radius line. A tree with a measured spread of nine or ten metres; not to scale.")
def _():
    return _canopy(5.4)


@symbol("Giant Tree Canopy", G_TREES, "Plain ring 6.2 mm across with a trunk dot and a radius line. A tree with a measured spread of twelve metres or more; not to scale.")
def _():
    return _canopy(6.2)


def _trunk(across, bead, dashed=False):
    """A trunk's cross-section: an eight-sided outline (so it is never mistaken for a ring) round a solid
    centre mark.  `across` is the width between its flats."""
    r = across / (2.0 * math.cos(math.radians(22.5)))
    pts = ngon(8, r, 22.5)
    k = Sketch()
    if dashed:
        for i in range(8):
            a, b = pts[i], pts[(i + 1) % 8]
            k.line(a[0] + (b[0] - a[0]) * 0.18, a[1] + (b[1] - a[1]) * 0.18, a[0] + (b[0] - a[0]) * 0.82, a[1] + (b[1] - a[1]) * 0.82)
    else:
        k.path(pts, close=True)
    return k.bead(0, 0, bead)


@symbol("Hundred Millimetre Tree Trunk", G_TREES, "Eight-sided outline 1.8 mm across round a centre dot. A tree trunk about 100 mm across, shot at its middle.")
def _():
    return _trunk(1.8, 0.2)


@symbol("Two Hundred Millimetre Tree Trunk", G_TREES, "Eight-sided outline 2.6 mm across round a centre dot. A tree trunk about 200 mm across, shot at its middle.")
def _():
    return _trunk(2.6, 0.22)


@symbol("Three Hundred Millimetre Tree Trunk", G_TREES, "Eight-sided outline 3.4 mm across round a centre dot. A tree trunk about 300 mm across, shot at its middle.")
def _():
    return _trunk(3.4, 0.26)


@symbol("Narrow Tree Trunk", G_TREES, "Eight-sided outline 4.2 mm across round a centre dot. A tree trunk from 400 to 600 mm across, shot at its middle.")
def _():
    return _trunk(4.2, 0.3)


@symbol("Medium Tree Trunk", G_TREES, "Eight-sided outline 5.2 mm across round a centre dot. A tree trunk from 700 to 900 mm across, shot at its middle.")
def _():
    return _trunk(5.2, 0.32)


@symbol("Wide Tree Trunk", G_TREES, "Eight-sided outline 6.2 mm across round a centre dot. A tree trunk of 1000 mm or more across, shot at its middle.")
def _():
    return _trunk(6.2, 0.35)


@symbol("Unmeasured Tree Trunk", G_TREES, "Dashed eight-sided outline 4.2 mm across round a centre dot. A tree trunk whose width was not measured.")
def _():
    return _trunk(4.2, 0.3, dashed=True)


# ---- Buildings and Structures ---------------------------------------------------------------------------

@symbol("Building Corner", G_BUILDINGS, "Right-angle bracket with its corner at the origin. A building corner; it opens to +x and -y.")
def _():
    return Sketch().path([(1.2, 0), (0, 0), (0, -1.2)])


@symbol("Awning Point", G_BUILDINGS, "Box whose lower edge is a row of four scallops. A point on an awning or canopy over a footpath.")
def _():
    k = Sketch().path([(-1.6, -0.4), (-1.6, 0.8), (1.6, 0.8), (1.6, -0.4)])
    for x in (-1.2, -0.4, 0.4, 1.2):
        k.arc(0.4, 180, 360, x, -0.4)
    return k


@symbol("Bridge Column", G_STRUCTURES, "Ring crossed by an X. A bridge pier or column.")
def _():
    return Sketch().ring(1.6).cross(1.13)


@symbol("Mast", G_STRUCTURES, "A-frame with a crossbar and a small ring on its apex. A communications or lighting mast.", centre=True)
def _():
    return (Sketch().path([(-1.3, -1.8), (0, 1.6), (1.3, -1.8)]).line(-0.8, -0.5, 0.8, -0.5)
            .ring(0.3, 0, 1.9))


@symbol("Storage Tank", G_STRUCTURES, "Ring inside a ring. A storage tank or silo seen from above.")
def _():
    return Sketch().ring(2.9).ring(2.3)


@symbol("Flagpole", G_STRUCTURES, "Ring with a pennant flying to +x. A flagpole.")
def _():
    return Sketch().ring(0.6, -0.75, 0).path([(-0.15, 0.45), (1.5, 0), (-0.15, -0.45)], close=True)


def _abutment(top):
    """A square with a banded edge: the band is the top or the foot of the abutment."""
    s = 1.0 if top else -1.0
    k = Sketch().box(2.8, 2.8).line(-1.4, s * 0.8, 1.4, s * 0.8)
    for x in (-1.0, -0.2, 0.6):
        k.line(x, s * 0.8, x + 0.6, s * 1.4)
    return k


@symbol("Top Abutment Point", G_STRUCTURES, "Square with a hatched band along its upper edge. A point on the top of a bridge abutment.")
def _():
    return _abutment(True)


@symbol("Bottom Abutment Point", G_STRUCTURES, "Square with a hatched band along its lower edge. A point at the foot of a bridge abutment.")
def _():
    return _abutment(False)


@symbol("Pile Cap", G_STRUCTURES, "Square holding a ring. A bridge pile standing in its cap, seen from above.")
def _():
    return Sketch().box(3.0, 3.0).ring(0.85)


@symbol("Windmill", G_STRUCTURES, "Four leaning kite-shaped vanes round a hub ring. A windmill, shot at the middle of its tower.")
def _():
    k = Sketch()
    for i in range(4):
        t = 90.0 * i
        k.path([polar(0.5, t), polar(1.7, t + 30), polar(3.0, t + 8), polar(1.3, t - 20)], close=True)
    return k.ring(0.35)


# ---- Fences and Walls / Gates and Posts ---------------------------------------------------------------------

@symbol("Fence Post", G_GATES, "Small square. A fence post at every vertex of a fence line.")
def _():
    return Sketch().box(1.2, 1.2)


# ---- Terrain and Breaklines ----------------------------------------------------------------------------------

@symbol("Ground Shot", G_POINTS, "Dot with a small plus. A surveyed ground point.")
def _():
    return Sketch().bead(0, 0, 0.22).plus(0.75)


@symbol("Spot Level", G_POINTS, "Plus through a ring a millimetre and a half across. A spot level on the ground or a structure.")
def _():
    return Sketch().plus(1.5).ring(0.75)


@symbol("Invert Level", G_POINTS, "Ring holding a downward triangle. The invert of a pipe or channel.")
def _():
    return Sketch().ring(1.0).path(triangle(1.1, False, 0, 0.0), close=True)


@symbol("Breakline Point", G_POINTS, "Dot between two bars, a dash-dot-dash. A ground point that lies on a breakline.")
def _():
    return Sketch().bead(0, 0, 0.26).line(0.65, 0, 1.5, 0).line(-0.65, 0, -1.5, 0)


@symbol("Obvert Level", G_POINTS, "Ring holding an arch over a bar. The obvert of a pipe, its highest inside point; the pair of Invert Level.")
def _():
    return Sketch().ring(1.1).arc(0.55, 0, 180, 0, -0.15).line(-0.8, -0.55, 0.8, -0.55)


@symbol("Water Level Mark", G_WATEREDGE, "Downward triangle standing on two wavy lines. The level of a water surface, or of a flood; the origin is the triangle's point.")
def _():
    k = Sketch().path([(0, 0), (-1.1, 1.9), (1.1, 1.9)], close=True)
    k.bow((-1.0, -0.35), (0, -0.35), 0.8, 1).bow((0, -0.35), (1.0, -0.35), 0.8, -1)
    k.bow((-1.0, -0.8), (0, -0.8), 0.8, 1).bow((0, -0.8), (1.0, -0.8), 0.8, -1)
    return k


@symbol("Swamp", G_WATEREDGE, "Two broken dashes with three reed ticks standing on the upper one. A swamp or marsh marked by a point.")
def _():
    k = Sketch().line(-1.8, -0.8, -0.4, -0.8).line(0.4, -0.8, 1.8, -0.8).line(-1.0, 0, 1.0, 0)
    for x in (-0.7, 0, 0.7):
        k.line(x, 0, x, 1.1)
    return k


# ---- Survey Control and Annotation ------------------------------------------------------------------------------

@symbol("Trigonometric Station", G_CONTROL, "Large triangle holding a ring and a dot. A primary control station; the origin is its dot.")
def _():
    return Sketch().path(triangle(5.6, True, 0, 0.35), close=True).ring(1.1).bead(0, 0, 0.26)


@symbol("Control Mark", G_CONTROL, "Ring with long cross-hairs and a dot. A secondary or traverse control mark.")
def _():
    return Sketch().ring(1.5).plus(2.3).bead(0, 0, 0.26)


@symbol("Level Benchmark", G_CONTROL, "Ring with a bar across it and an upward arrow above the bar. A levelling benchmark.")
def _():
    return (Sketch().ring(1.6).line(-1.6, 0, 1.6, 0).line(0, 0.3, 0, 1.2)
            .path([(-0.4, 0.8), (0, 1.2), (0.4, 0.8)]))


@symbol("Fixed Survey Mark", G_CONTROL, "Diamond with a centre dot. A permanent survey mark.")
def _():
    return Sketch().path([(0, 1.8), (1.8, 0), (0, -1.8), (-1.8, 0)], close=True).bead(0, 0, 0.3)


@symbol("Temporary Control", G_CONTROL, "Open diamond with a plus and no dot. A temporary control point.")
def _():
    return Sketch().path([(0, 1.8), (1.8, 0), (0, -1.8), (-1.8, 0)], close=True).plus(0.8)


@symbol("Alignment Pin", G_CONTROL, "Ring with a centre dot between two bars that run along the alignment. A pin set on a design alignment for setting out.")
def _():
    return Sketch().ring(0.85).bead(0, 0, 0.26).line(1.15, 0, 2.15, 0).line(-1.15, 0, -2.15, 0)


@symbol("Photo Centre", G_CONTROL, "Ring holding a square frame with a small plus at its middle. The centre of an aerial photograph.")
def _():
    return Sketch().ring(1.8).box(2.0, 2.0).plus(0.7)


@symbol("Borehole", G_INVEST, "Ring split into quadrants, two opposite ones evenly hatched. A borehole.")
def _():
    k = Sketch().ring(1.7).plus(1.7)
    for c in (0.7, 1.4, 2.1):        # the hatch lines x + y = c, a third of a millimetre or so apart
        if c < 1.7:
            a, b = (c, 0.0), (0.0, c)
        else:                        # past the axes' ends the line stops on the ring
            d = math.sqrt(2 * 1.7 * 1.7 - c * c)
            a, b = ((c + d) / 2, (c - d) / 2), ((c - d) / 2, (c + d) / 2)
        k.line(a[0], a[1], b[0], b[1])
        k.line(-a[0], -a[1], -b[0], -b[1])
    return k


@symbol("Test Pit", G_INVEST, "Square with a diamond joining the middles of its sides. A test pit or trench.")
def _():
    return Sketch().box(3.4, 3.4).path([(0, 1.7), (1.7, 0), (0, -1.7), (-1.7, 0)], close=True)


@symbol("Pothole", G_INVEST, "Ring of eight dashes holding P. A pothole dug to expose a service.")
def _():
    return Sketch().dashed_ring(1.8, 8, 0.6, 7.5).letter("P", 0, 0)


@symbol("Seismic Shot Point", G_INVEST, "Dot with two arcs on each side, the pulse leaving it. A seismic test shot.")
def _():
    k = Sketch().bead(0, 0, 0.3)
    for r in (0.85, 1.4):
        k.arc(r, -38, 38).arc(r, 142, 218)
    return k


# ---- Miscellaneous --------------------------------------------------------------------------------------------

@symbol("Hazard Point", G_HAZARD, "Warning triangle holding an exclamation mark. Any hazard the crew flags: unstable ground, asbestos, a live line.")
def _():
    return Sketch().path(triangle(3.8, True, 0, 0), close=True).letter("!", 0, -0.35)


@symbol("Heritage Item", G_HAZARD, "Five-point star outline. A protected heritage item such as a marker stone, a post or a plaque.", centre=True)
def _():
    return Sketch().path(star(5, 2.0, 0.8, 90), close=True)


@symbol("Headstone", G_HAZARD, "Upright slab with an arched top and a small cross cut on its face. A grave headstone, shot at the middle of its base.")
def _():
    return Sketch().path([(-0.9, 0.5), (-0.9, -1.4), (0.9, -1.4), (0.9, 0.5)]).arc(0.9, 0, 180, 0, 0.5).plus(0.35, 0, 0.2)


@symbol("Habitat Marker", G_HAZARD, "Ring holding a leaf. A habitat or protected-species marker.")
def _():
    a, b = (-0.85, -0.85), (0.85, 0.85)
    return Sketch().ring(1.7).bow(a, b, 1.5, 1).bow(a, b, 1.5, -1).line(-0.85, -0.85, -1.15, -1.15)


@symbol("Generic Point", G_GENERAL, "Plus. A point that no other symbol describes; the code and notes say what it is.")
def _():
    return Sketch().plus(1.0)


@symbol("Query Point", G_GENERAL, "Ring holding a question mark. A point whose identity is in doubt and must be checked in the field.")
def _():
    return Sketch().ring(1.7).letter("?", 0, 0)


# ---------------------------------------------------------------------------------------------------------
# Self-check and sheets
# ---------------------------------------------------------------------------------------------------------

SIZE_LIMITS = {"S": (1.0, 3.2), "M": (3.0, 4.8), "L": (5.0, 6.4)}
FORBIDDEN = re.compile("|".join(["1" + "2d", "ex" + "ds", "tf" + "nsw", "transport for n" + "sw", "n" + "sw"]), re.I)
ARITY = {"move": 3, "draw": 3, "arc": 4, "circle": 2, "dot": 2, "text": 2}
TEXT_KEYS = {"text", "height", "justify", "angle", "widthFactor"}
NAME_PATTERN = re.compile(r"^[A-Z][A-Za-z]*( [A-Z][A-Za-z]*| and| of| in)*$")


def check(contract=None):
    """Return (errors, warnings) for SYMBOLS: grammar, class sizes and centring; names, groups and classes
    against the contract when it is given."""
    errors, warnings = [], []
    want = {s["name"]: s for s in contract["symbols"]} if contract else {}
    names = [s["name"] for s in SYMBOLS]
    for n in sorted({n for n in names if names.count(n) > 1}):
        errors.append(f"{n}: defined twice")
    if contract:
        for n in want:
            if n not in names:
                errors.append(f"missing {n!r}")
        for n in names:
            if n not in want:
                errors.append(f"{n!r} is not in the contract")
    for s in SYMBOLS:
        n = s["name"]
        if set(s) - {"name", "group", "units", "atVertices", "description", "strokes"}:
            errors.append(f"{n}: unknown members {sorted(set(s) - {'name', 'group', 'units', 'atVertices', 'description', 'strokes'})}")
        if not NAME_PATTERN.match(n):
            errors.append(f"{n}: a name is Title Case words with no digits")
        if s["units"] != "paper" or s["atVertices"] is not True:
            errors.append(f"{n}: units paper and atVertices true")
        if len(s["group"].split("/")) != 2:
            errors.append(f"{n}: a group is Top/Sub")
        desc = s.get("description", "")
        if not desc or len(desc) > 160:
            errors.append(f"{n}: description must be 1..160 characters ({len(desc)})")
        for text in (n, s["group"], desc):
            if FORBIDDEN.search(text):
                errors.append(f"{n}: forbidden word in {text!r}")
        e = want.get(n)
        if e and s["group"] != e["group"]:
            errors.append(f"{n}: group {s['group']!r} should be {e['group']!r}")
        if e and e["units"] != "paper":
            errors.append(f"{n}: contract units are {e['units']!r}")
        strokes = s["strokes"]
        if not strokes or strokes[0][0] != "move":
            errors.append(f"{n}: the first stroke must be a move")
            continue
        if len(strokes) > 140:
            errors.append(f"{n}: {len(strokes)} strokes (limit 140)")
        elif len(strokes) > 70:
            warnings.append(f"{n}: {len(strokes)} strokes; keep symbols lean")
        for st in strokes:
            if not isinstance(st, list) or not st or st[0] not in ARITY:
                errors.append(f"{n}: unknown stroke {st!r} (no pen: the rule gives the colour)")
                continue
            if len(st) != ARITY[st[0]]:
                errors.append(f"{n}: {st[0]} takes {ARITY[st[0]] - 1} values")
                continue
            if st[0] == "text":
                t = st[1]
                if set(t) - TEXT_KEYS:
                    errors.append(f"{n}: text keys {sorted(set(t) - TEXT_KEYS)}")
                if t.get("height") != LABEL_HEIGHT or t.get("justify") != "middle-centre":
                    errors.append(f"{n}: a letter is 2.4 high and middle-centre")
                if not 1 <= len(t.get("text", "")) <= 3:
                    errors.append(f"{n}: a mark is one to three characters")
            else:
                vals = st[1:2] if st[0] == "arc" else st[1:]
                if not all(isinstance(v, (int, float)) and math.isfinite(v) and abs(v) <= 60 and round(v, 3) == v for v in vals):
                    errors.append(f"{n}: {st!r}: finite, at most 60, 3 decimals")
                if st[0] == "arc":
                    if st[1] <= 0 or not all(isinstance(v, (int, float)) and abs(v) <= 720 and round(v, 3) == v for v in st[2:]):
                        errors.append(f"{n}: {st!r}: bad arc")
                    elif abs(st[3] - st[2]) > 360:
                        errors.append(f"{n}: {st!r}: an arc sweeps under 360")
                if st[0] in ("circle", "dot") and st[1] < 0:
                    errors.append(f"{n}: negative radius")
                if st[0] == "dot" and st[1] > 0.6:
                    errors.append(f"{n}: dot radius above 0.6")
        x0, y0, x1, y1 = _bounds(strokes)
        ext = max(x1 - x0, y1 - y0)
        cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
        klass = e["size"] if e else ("S" if ext <= 3.2 and ext < 3.0 else "M" if ext <= 4.8 else "L")
        lo, hi = SIZE_LIMITS[klass]
        if not lo <= ext <= hi:
            errors.append(f"{n}: extent {ext:.2f} mm outside the {klass} class {lo}..{hi}")
        if klass == "S" and any(st[0] == "text" for st in strokes):
            errors.append(f"{n}: a symbol of the S class carries no letter")
        if abs(cx) > 0.6 or abs(cy) > 0.6:
            errors.append(f"{n}: centre ({cx:.2f}, {cy:.2f}) is more than 0.6 mm off the origin")
        elif abs(cx) > 0.35 or abs(cy) > 0.35:
            warnings.append(f"{n}: centre ({cx:.2f}, {cy:.2f}) is a little off the origin")
        if (abs(cx) > 0.25 or abs(cy) > 0.25) and "origin" not in desc.lower():
            errors.append(f"{n}: the box centre is ({cx:.2f}, {cy:.2f}) from the origin and the description does not say where the origin is")
    return errors, warnings


def _load_contract(path):
    import json
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def sheets(out_dir, contract, columns=8):
    """Four sheets of a quarter of the symbols each, split between groups, drawn in the service colours on the
    dark ground (needs Pillow and preview.py beside this file)."""
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import preview
    palette = {p["name"]: p["hex"] for p in contract["palette"]}
    colours = preview.colour_map(contract["codes"], palette)
    # four sheets, cut at the group boundaries nearest to the quarter marks
    bounds = [i for i in range(1, len(SYMBOLS)) if SYMBOLS[i]["group"] != SYMBOLS[i - 1]["group"]]
    cuts = []
    for q in (1, 2, 3):
        goal = q * len(SYMBOLS) / 4
        cuts.append(min((b for b in bounds if b not in cuts), key=lambda b: abs(b - goal)))
    edges = [0] + sorted(cuts) + [len(SYMBOLS)]
    chunks = [SYMBOLS[a:b] for a, b in zip(edges, edges[1:])]
    os.makedirs(out_dir, exist_ok=True)
    for i, chunk in enumerate(chunks, 1):
        preview.symbol_sheet(chunk, os.path.join(out_dir, f"katana-standard-symbols-{i}.png"), colours=colours,
                             columns=columns, title=f"Katana Standard symbols, sheet {i} of {len(chunks)}")
        print(f"sheet {i}: {len(chunk)} symbols")


def main(argv):
    contract = None
    if "--contract" in argv:
        contract = _load_contract(argv[argv.index("--contract") + 1])
    if "--sheets" in argv:
        if contract is None:
            print("--sheets needs --contract (the service colours come from it)")
            return 2
        sheets(argv[argv.index("--sheets") + 1], contract)
        return 0
    errors, warnings = check(contract)
    for w in warnings:
        print("warning:", w)
    for e in errors:
        print("ERROR:", e)
    print(f"symbols: {len(SYMBOLS)}, strokes: {sum(len(s['strokes']) for s in SYMBOLS)}, "
          f"{len(errors)} errors, {len(warnings)} warnings" + ("" if contract else " (names not checked: no --contract)"))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
