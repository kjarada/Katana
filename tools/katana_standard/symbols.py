"""Katana Standard: the symbol library (113 point symbols).

    python tools/katana_standard/symbols.py --check [--contract contract.json]
    python tools/katana_standard/symbols.py --sheets OUTDIR --contract contract.json

`SYMBOLS` is a list of 113 dicts in the customisation file's own member names (`name`, `group`, `units`,
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
           or an observation.  Hatching (at most four strokes) means "solid": a borehole's quadrants, a trap's
           grating.  A dashed ring means a temporary or former state: a dead tree, a pothole.  Rays mean a
           light (street light, bollard light, signal lamp) or, on a GNSS base, a signal.
Services.  One grammar for every service, learnt once; the service colour comes from the rule and the letter
           names the service (W water, S sewer, G gas, E electricity, C communications, R recycled, F fire,
           P fuel, I traffic systems, ? unknown).  Square = pit or box; single ring = round cover or pole;
           double ring = maintenance hole; bow-tie = valve; diamond = regulator or marker; ring with a bar =
           meter.  A letter is inside a pit, a ring or a hole, and BELOW a valve.
Trees.     Distinguished by canopy outline, all with a trunk dot: scalloped = broadleaf, spiked star = conifer,
           curved fronds = palm, irregular lobes with branches = native gum, dashed ring with a cross = dead,
           rings = stump; shrubs are the same scallop at a smaller size.
Control.   Distinguished by class: triangle with ring and dot = primary station; ring with long cross-hairs and
           dot = secondary mark; ring with a bar and arrow = benchmark; diamond with dot = fixed mark; open
           diamond with a plus = temporary; ring with a compass of rays and dot = GNSS base; triangle in a ring =
           instrument station.
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
G_SHRUBS = "Vegetation and Landscape/Shrubs and Plants"
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


@symbol("Drilled Hole", G_MARKS, "Ring with a small cross inside it. A hole drilled in rock or concrete, with the cross cut at its centre.")
def _():
    return Sketch().ring(1.1).cross(0.45)


@symbol("Chiselled Cross", G_MARKS, "Square with a cross whose arms run past the corners. A cross cut into a kerb, step or slab.")
def _():
    return Sketch().box(3.0, 3.0).cross(1.95)


@symbol("Mark Not Found", G_MARKS, "Ring struck through by a slash that runs past it. A mark that was searched for and not found.")
def _():
    return Sketch().ring(1.6).line(-1.5, -1.5, 1.5, 1.5)


# ---- Roads and Pavements -----------------------------------------------------------------------------------

@symbol("Kerb Ramp", G_ACCESS, "Trapezoid, wide at the kerb and narrowing away, with an arrow up the ramp. A pedestrian kerb ramp.")
def _():
    return (Sketch().path([(-1.8, -1.2), (1.8, -1.2), (1.0, 1.2), (-1.0, 1.2)], close=True)
            .line(0, -0.7, 0, 0.7).path([(-0.4, 0.3), (0, 0.7), (0.4, 0.3)]))


@symbol("Tactile Paving", G_ACCESS, "Square holding a three by three grid of dots. Tactile ground surface indicators.")
def _():
    k = Sketch().box(3.2, 3.2)
    for i in (-0.9, 0, 0.9):
        for j in (-0.9, 0, 0.9):
            k.dot(i, j, 0.2)
    return k


@symbol("Road Arrow Marking", G_MARKINGS, "Block arrow pointing +x, drawn as an outline. A painted lane arrow; turn it by drawing the line the other way.")
def _():
    return Sketch().path([(-3, -0.45), (-0.3, -0.45), (-0.3, -1.3), (3, 0), (-0.3, 1.3), (-0.3, 0.45), (-3, 0.45)], close=True)


# ---- Street Furniture and Signs ---------------------------------------------------------------------------

@symbol("Regulatory Sign", G_SIGNS, "Ring crossed edge to edge by a backslash bar. A regulatory sign face such as a prohibition or a limit.")
def _():
    return Sketch().ring(1.7).line(-1.2, 1.2, 1.2, -1.2)


@symbol("Warning Sign", G_SIGNS, "Triangle, apex up. A warning sign face (hazard ahead, give way, signals).")
def _():
    return Sketch().path(triangle(3.6, True, 0, 0), close=True)


@symbol("Guide Sign", G_SIGNS, "Wide rectangle. A direction or guide sign face, such as a street name or destination.")
def _():
    return Sketch().box(3.6, 2.2)


@symbol("Information Sign", G_SIGNS, "Round-cornered square holding a lower-case i. An information or tourist sign face; the round corners tell it from a pit.")
def _():
    return Sketch().rounded_box(3.2, 3.2, 0.7).letter("i", 0, 0)


@symbol("Street Light", G_LIGHTING, "Ring with a centre dot and eight rays. A street light pole and its lantern.")
def _():
    return Sketch().ring(1.0).bead(0, 0, 0.2).ticks(1.3, 2.2, range(0, 360, 45))


@symbol("Bollard Light", G_LIGHTING, "Small ring with four short rays. A light built into a bollard.")
def _():
    return Sketch().ring(0.7).ticks(0.95, 1.55, [0, 90, 180, 270])


@symbol("Bollard", G_POSTS, "Ring inside a ring. A fixed bollard.")
def _():
    return Sketch().ring(1.0).ring(0.45)


@symbol("Removable Bollard", G_POSTS, "Ring with a bar across its middle. A bollard that lifts out or lowers.")
def _():
    return Sketch().ring(0.95).line(-0.75, 0, 0.75, 0)


@symbol("Delineator Post", G_POSTS, "Small square with one diagonal. A roadside delineator or marker post.")
def _():
    return Sketch().box(1.8, 1.8).line(-0.9, -0.9, 0.9, 0.9)


@symbol("Bench", G_AMENITIES, "Long rectangle with two slat lines. A seat or bench.")
def _():
    return Sketch().box(4.0, 1.6).line(-2.0, 0.267, 2.0, 0.267).line(-2.0, -0.267, 2.0, -0.267)


@symbol("Litter Bin", G_AMENITIES, "Hexagon with a centre dot. A litter or waste bin.")
def _():
    return Sketch().path(ngon(6, 1.3, 90), close=True).bead(0, 0, BEAD_SMALL)


@symbol("Drinking Fountain", G_AMENITIES, "Ring holding a teardrop. A drinking fountain or bubbler.")
def _():
    k = Sketch().ring(1.7)
    cy, r, apex = -0.35, 0.58, 0.95
    theta = math.degrees(math.acos(r / (apex - cy)))
    p0, p1 = polar(r, 90 + theta, 0, cy), polar(r, 90 - theta, 0, cy)
    k.path([p0, (0, apex), p1])
    k.arc(r, 90 + theta, 90 - theta + 360, 0, cy)
    return k


@symbol("Bus Stop Shelter", G_AMENITIES, "Rectangle holding a bench divided into seats. A bus shelter; the large box is its roof outline.")
def _():
    k = Sketch().box(5.4, 3.6).box(3.6, 1.0, 0, -0.8)
    for x in (-0.9, 0, 0.9):
        k.line(x, -1.3, x, -0.3)
    return k


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


# ---- Kerbs and Drainage / Pits and Structures --------------------------------------------------------------

@symbol("Gully Pit", G_PITS, "Square with three grate bars. A stormwater gully pit with its grating.")
def _():
    k = Sketch().box(3.2, 3.2)
    for y in (-0.8, 0, 0.8):
        k.line(-1.6, y, 1.6, y)
    return k


@symbol("Kerb Inlet Pit", G_PITS, "Rectangle with a V notch cut into its left end. A side-entry pit; the notch faces the kerb.")
def _():
    return Sketch().path([(-2.0, 1.2), (2.0, 1.2), (2.0, -1.2), (-2.0, -1.2), (-2.0, -0.5), (-1.2, 0), (-2.0, 0.5)], close=True)


@symbol("Junction Pit", G_PITS, "Square with both diagonals. A stormwater junction pit.")
def _():
    return Sketch().box(3.2, 3.2).cross(1.6)


@symbol("Stormwater Manhole", G_PITS, "Double ring holding the letter D. A stormwater maintenance hole (D for drainage).")
def _():
    return Sketch().ring(1.7).ring(1.3).letter("D", 0, 0)


@symbol("Gross Pollutant Trap", G_PITS, "Rectangle crossed by four hatch lines. A gross pollutant trap or litter trap.")
def _():
    return (Sketch().box(5.4, 3.2).line(-2.5, -1.6, 0.7, 1.6).line(-0.7, -1.6, 2.5, 1.6)
            .line(-0.7, 1.6, 2.5, -1.6).line(-2.5, 1.6, 0.7, -1.6))


@symbol("Headwall", G_PITS, "Three sides of a box, open to -x, with wings flaring outward. A pipe headwall; the pipe arrives from the open side.")
def _():
    return (Sketch().path([(-2.2, 1.2), (2.4, 1.2), (2.4, -1.2), (-2.2, -1.2)])
            .line(-2.2, 1.2, -2.9, 1.9).line(-2.2, -1.2, -2.9, -1.9))


@symbol("Culvert End", G_PITS, "T shape with turned-in ends. A culvert end: the bar is its headwall, the stem the pipe running away from it.")
def _():
    return Sketch().line(-2.0, 1.0, 2.0, 1.0).line(0, 1.0, 0, -1.0).line(-2.0, 1.0, -2.0, 0.3).line(2.0, 1.0, 2.0, 0.3)


@symbol("Subsoil Inspection Point", G_PITS, "Small ring holding a smaller square, the cap of the riser. An inspection point on a subsoil drain.")
def _():
    return Sketch().ring(1.1).box(0.8, 0.8)


# ---- Rail and Transit ---------------------------------------------------------------------------------------

@symbol("Turnout Marker", G_TRACK, "Y shape forking toward +x. The toe of a turnout (points).")
def _():
    legs = 12.5
    a, b = polar(2.4, legs, 0, 0), polar(2.4, -legs, 0, 0)
    return Sketch().line(-2.15, 0, 0, 0).line(0, 0, a[0], a[1]).line(0, 0, b[0], b[1])


@symbol("Buffer Stop", G_TRACK, "Two rails ending against a block. The end of a track.")
def _():
    return Sketch().line(-1.9, 0.45, 1.1, 0.45).line(-1.9, -0.45, 1.1, -0.45).box(0.8, 2.6, 1.5, 0)


@symbol("Signal Mast", G_SIGNALS, "Pole ring with a dot, joined to a lamp ring with rays. A railway signal on its mast.")
def _():
    return (Sketch().ring(0.8, -1.3, 0).bead(-1.3, 0, 0.16).line(-0.5, 0, 0.45, 0).ring(0.7, 1.15, 0)
            .ticks(1.0, 1.45, [0, 40, -40], 1.15, 0))


@symbol("Overhead Mast", G_SIGNALS, "Square with a centre dot and two arms. A mast carrying overhead wires.")
def _():
    return Sketch().box(2.4, 2.4).bead(0, 0, BEAD_SMALL).line(-2.2, 0.6, -1.2, 0.6).line(1.2, 0.6, 2.2, 0.6)


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


@symbol("Water Pit", G_WATER, "Square holding W. A water meter or valve pit.")
def _():
    return _pit("W")


@symbol("Water Flow Meter", G_WATER, "Ring holding W above a bar. A water meter on the line.")
def _():
    return _meter("W")


@symbol("Water Tank", G_WATER, "Ring inside a ring holding W. A water tank or reservoir seen from above.")
def _():
    return Sketch().ring(2.8).ring(2.0).letter("W", 0, 0)


@symbol("Sewer Maintenance Hole", G_SEWER, "Double ring holding S. A sewer maintenance hole.")
def _():
    return _hole("S")


@symbol("Sewer Vent", G_SEWER, "Ring holding S with a chevron above. A sewer vent shaft.")
def _():
    return Sketch().ring(1.4, 0, -0.55).letter("S", 0, -0.55).path([(-0.8, 1.45), (0, 2.1), (0.8, 1.45)])


@symbol("Septic System Tank", G_SEWER, "Two-cell box with S in the left cell. A septic or treatment tank.")
def _():
    return Sketch().box(5.6, 3.4).line(0, -1.7, 0, 1.7).letter("S", -1.4, 0)


@symbol("Gas Isolation Valve", G_GAS, "Bow-tie with the letter G below. A gas main valve; the origin is between valve and letter.")
def _():
    return _valve("G")


@symbol("Gas Pit", G_GAS, "Square holding G. A gas valve or meter pit.")
def _():
    return _pit("G")


@symbol("Gas Flow Meter", G_GAS, "Ring holding G above a bar. A gas meter.")
def _():
    return _meter("G")


@symbol("Gas Pressure Regulator", G_GAS, "Diamond holding G. A gas pressure regulating set.")
def _():
    return Sketch().path([(0, 1.9), (1.9, 0), (0, -1.9), (-1.9, 0)], close=True).letter("G", 0, 0)


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


@symbol("Communications Pit", G_COMMS, "Square holding C. A communications pit.")
def _():
    return _pit("C")


@symbol("Communications Maintenance Hole", G_COMMS, "Double ring holding C. A communications maintenance hole.")
def _():
    return _hole("C")


@symbol("Communications Pillar", G_COMMS, "Square holding C with a broken diagonal. A communications pillar or cabinet.")
def _():
    k = Sketch().box(3.2, 3.2).letter("C", 0, 0)
    for s in (1, -1):
        p, q = polar(1.2, 45, 0, 0), polar(2.26, 45, 0, 0)
        k.line(s * p[0], s * p[1], s * q[0], s * q[1])
    return k


@symbol("Recycled Isolation Valve", G_RECYCLED, "Bow-tie with the letter R below. A recycled water valve; the origin is between valve and letter.")
def _():
    return _valve("R")


@symbol("Recycled Water Pit", G_RECYCLED, "Square holding R. A recycled water pit.")
def _():
    return _pit("R")


@symbol("Fire Hydrant", G_FIRE, "Ring with a plus and a nozzle tick each side. A fire hydrant.")
def _():
    return Sketch().ring(1.5).plus(0.7).line(1.5, 0, 2.3, 0).line(-1.5, 0, -2.3, 0)


@symbol("Booster Connection", G_FIRE, "Square holding F with two nozzle ticks on its right. A fire booster connection; the origin is the middle of the square.")
def _():
    return Sketch().box(3.2, 3.2).letter("F", 0, 0).line(1.6, 0.7, 2.4, 0.7).line(1.6, -0.7, 2.4, -0.7)


@symbol("Fire Isolation Valve", G_FIRE, "Bow-tie with the letter F below. A fire service valve; the origin is between valve and letter.")
def _():
    return _valve("F")


@symbol("Fuel Isolation Valve", G_FUEL, "Bow-tie with the letter P below. A fuel line valve; the origin is between valve and letter.")
def _():
    return _valve("P")


@symbol("Loop Detector", G_ITS, "Square holding a three-peak zigzag. A traffic detector loop.")
def _():
    return (Sketch().box(3.4, 3.4).path([(-1.3, -0.6), (-0.9, 0.6), (-0.45, -0.6), (0, 0.6), (0.45, -0.6), (0.9, 0.6), (1.3, -0.6)]))


@symbol("Traffic Systems Pit", G_ITS, "Square holding I. A traffic systems or ITS pit.")
def _():
    return _pit("I")


@symbol("Unknown Pit", G_UNKNOWN, "Square holding a question mark. A pit whose service is not known.")
def _():
    return _pit("?")


@symbol("Unknown Maintenance Hole", G_UNKNOWN, "Double ring holding a question mark. A hole whose service is not known.")
def _():
    return _hole("?")


@symbol("Paint Mark", G_UNKNOWN, "Cross through a small ring. A spray-paint mark on the ground.")
def _():
    return Sketch().cross(0.85).ring(0.6)


@symbol("Pumping Station", G_ALL, "Square holding a ring with a flow arrow. A pumping station of any service.")
def _():
    return (Sketch().box(5.4, 5.4).ring(1.5).line(-0.8, 0, 0.8, 0).path([(0.35, 0.4), (0.8, 0), (0.35, -0.4)]))


@symbol("Marker Post", G_ALL, "Ring holding an upward triangle. A route marker post for any service.")
def _():
    return Sketch().ring(1.0).path(triangle(1.1, True, 0, -0.05), close=True)


# ---- Vegetation and Landscape (all trees keep a trunk dot) -------------------------------------------------

@symbol("Broadleaf Tree", G_TREES, "Scalloped canopy of ten round lobes with a trunk dot. A broadleaf or deciduous tree.")
def _():
    lobes = [polar(2.1, 90 + 36 * i) + (0.7,) for i in range(10)]
    return Sketch().scallops(lobes).bead(0, 0, BEAD_SMALL)


@symbol("Conifer Tree", G_TREES, "Eight-point spiked star with a trunk dot. A conifer or pine.")
def _():
    return Sketch().path(star(8, 2.9, 1.5, 90), close=True).bead(0, 0, BEAD_SMALL)


@symbol("Palm Tree", G_TREES, "Seven curved leaf fronds from a trunk dot. A palm or tree fern.")
def _():
    k = Sketch()
    for i in range(7):
        k.bow((0, 0), polar(2.8, 90 + 360.0 * i / 7), 2.4, 1)
    return k.bead(0, 0, 0.2)


@symbol("Native Gum Tree", G_TREES, "Irregular five-lobed canopy with two branches and a trunk dot. A native gum or other spreading tree.")
def _():
    lobes = [(polar(1.7, 80) + (1.3,)), (polar(1.65, 155) + (1.05,)), (polar(1.7, 230) + (1.3,)),
             (polar(1.6, 305) + (0.95,)), (polar(1.75, 15) + (1.2,))]
    return Sketch().scallops(lobes).line(-1.15, -0.5, 1.15, 0.55).line(-0.55, 0.95, 0.6, -0.9).bead(0, 0, BEAD_SMALL)


@symbol("Dead Tree", G_TREES, "Dashed ring with a cross inside. A dead or fallen tree still on site.")
def _():
    return Sketch().dashed_ring(2.0, 8, 0.6, 7.5).cross(0.95)


@symbol("Tree Stump", G_TREES, "Ring with two broken growth rings and a centre dot. A cut stump; the broken rings tell it from a bollard.")
def _():
    return Sketch().ring(1.0).arc(0.62, 25, 155).arc(0.62, 205, 335).bead(0, 0, 0.15)


@symbol("Shrub", G_SHRUBS, "Six-lobed scallop with a centre dot. A single shrub.")
def _():
    lobes = [polar(0.75, 90 + 60 * i) + (0.5,) for i in range(6)]
    return Sketch().scallops(lobes).bead(0, 0, 0.18)


def _union_lobes(centres, disc, count, lobe, depth):
    """Lobes along the outline of the union of discs about the origin (which every disc contains), so a cluster
    of plants is one scalloped outline and not three rings seen through each other."""
    lobes = []
    for i in range(count):
        a = math.radians(90 + 360.0 * i / count)
        d = (math.cos(a), math.sin(a))
        reach = 0.0
        for cx, cy in centres:
            b = d[0] * cx + d[1] * cy
            reach = max(reach, b + math.sqrt(max(disc * disc - (cx * cx + cy * cy) + b * b, 0.0)))
        lobes.append((d[0] * (reach - depth), d[1] * (reach - depth), lobe))
    return lobes


@symbol("Shrub Cluster", G_SHRUBS, "One scalloped outline round three plants, each with its stem dot. A clump or bed of shrubs.")
def _():
    centres = [polar(0.95, 90), polar(0.95, 210), polar(0.95, 330)]
    k = Sketch().scallops(_union_lobes(centres, 1.2, 18, 0.5, 0.38))
    for cx, cy in centres:
        k.dot(cx * 0.8, cy * 0.8, 0.2)
    return k


@symbol("Tussock", G_SHRUBS, "Fan of five curved blades. A grass tussock or sedge clump.", centre=True)
def _():
    k = Sketch()
    for a in (90 - 50, 90 - 25, 90, 90 + 25, 90 + 50):
        k.bow((0, -0.9), polar(1.9, a, 0, -0.9), 3.0, 1 if a >= 90 else -1)
    return k


# ---- Buildings and Structures ---------------------------------------------------------------------------

@symbol("Building Corner", G_BUILDINGS, "Right-angle bracket with its corner at the origin. A building corner; it opens to +x and -y.")
def _():
    return Sketch().path([(1.2, 0), (0, 0), (0, -1.2)])


@symbol("Doorway", G_BUILDINGS, "Wall ends, a door leaf at right angles and its swing arc. A door in a wall; the origin is on the wall centre line.")
def _():
    return (Sketch().line(-2.2, 0.5, -0.8, 0.5).line(-2.2, -0.5, -0.8, -0.5).line(-0.8, 0.5, -0.8, -0.5)
            .line(2.2, 0.5, 0.8, 0.5).line(2.2, -0.5, 0.8, -0.5).line(0.8, 0.5, 0.8, -0.5)
            .line(-0.8, 0, -0.8, 1.6).arc(1.6, 0, 90, -0.8, 0))


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


@symbol("Monument", G_STRUCTURES, "Square holding an eight-spoke star. A monument, memorial or statue base.")
def _():
    return Sketch().box(3.2, 3.2).plus(1.1).cross(0.78)


# ---- Fences and Walls / Gates and Posts ---------------------------------------------------------------------

@symbol("Fence Post", G_GATES, "Small square. A fence post at every vertex of a fence line.")
def _():
    return Sketch().box(1.2, 1.2)


@symbol("Strainer Post", G_GATES, "Small square with both diagonals. A braced end or corner post.")
def _():
    return Sketch().box(1.8, 1.8).cross(0.9)


@symbol("Pedestrian Gate", G_GATES, "Two posts, fence stubs and a half-circle swing about the hinge. A pedestrian gate that opens either way; the origin is its centre.")
def _():
    return (Sketch().line(-2.2, 0, -0.95, 0).line(0.95, 0, 2.2, 0).box(0.5, 0.5, -0.7, 0).box(0.5, 0.5, 0.7, 0)
            .line(-0.45, 0, 0.45, 0).arc(1.4, -90, 90, -0.7, 0))


@symbol("Vehicle Gate", G_GATES, "Two hinge posts and a half-circle swing about each. A double vehicle gate that opens either way; the origin is its centre.")
def _():
    return (Sketch().box(0.5, 0.5, -2.8, 0).box(0.5, 0.5, 2.8, 0)
            .line(-2.55, 0, -0.12, 0).line(2.55, 0, 0.12, 0)
            .arc(2.8, -90, 90, -2.8, 0).arc(2.8, 90, 270, 2.8, 0))


# ---- Terrain and Breaklines ----------------------------------------------------------------------------------

@symbol("Ground Shot", G_POINTS, "Dot with a small plus. A surveyed ground point.")
def _():
    return Sketch().bead(0, 0, 0.22).plus(0.75)


@symbol("Spot Level", G_POINTS, "Plus through a ring a millimetre and a half across. A spot level on the ground or a structure.")
def _():
    return Sketch().plus(1.5).ring(0.75)


@symbol("High Point", G_POINTS, "Triangle, apex up, with a centre dot. The top of a rise.")
def _():
    return Sketch().path(triangle(3.4, True, 0, 0.0), close=True).bead(0, -0.3, BEAD_SMALL)


@symbol("Low Point", G_POINTS, "Triangle, apex down, with a centre dot. The bottom of a dip.")
def _():
    return Sketch().path(triangle(3.4, False, 0, 0.0), close=True).bead(0, 0.3, BEAD_SMALL)


@symbol("Invert Level", G_POINTS, "Ring holding a downward triangle. The invert of a pipe or channel.")
def _():
    return Sketch().ring(1.0).path(triangle(1.1, False, 0, 0.0), close=True)


@symbol("Boulder", G_ROCK, "Irregular hexagon with a crack. A boulder or loose rock.")
def _():
    pts = [polar(r, a) for r, a in ((1.7, 100), (1.45, 160), (1.65, 225), (1.55, 285), (1.75, 345), (1.4, 40))]
    return Sketch().path(pts, close=True).path([(-0.55, 0.65), (-0.1, 0.1), (-0.3, -0.5)])


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


@symbol("GNSS Base", G_CONTROL, "Ring with a dot and a compass of eight rays, the cardinal ones longer. A GNSS base or reference station.")
def _():
    k = Sketch().ring(1.5).bead(0, 0, 0.3)
    k.ticks(1.8, 2.9, [0, 90, 180, 270])
    k.ticks(1.8, 2.4, [45, 135, 225, 315])
    return k


@symbol("Temporary Control", G_CONTROL, "Open diamond with a plus and no dot. A temporary control point.")
def _():
    return Sketch().path([(0, 1.8), (1.8, 0), (0, -1.8), (-1.8, 0)], close=True).plus(0.8)


@symbol("Instrument Station", G_CONTROL, "Triangle inside a ring. A total station or instrument set-up point.")
def _():
    return Sketch().ring(2.1).path(ngon(3, 1.75, 90), close=True)


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


@symbol("Leader Dot", G_PLAN, "Small ring with a dot. The end of a leader line.")
def _():
    return Sketch().ring(0.6).bead(0, 0, 0.16)


@symbol("North Arrow", G_PLAN, "Arrow with a split, half-shaded head and the letter N at its tail. Points to +y, the plan's north; the origin is midway along the shaft.")
def _():
    k = (Sketch().line(0, -3.0, 0, 1.4).path([(0, 3.0), (-1.0, 0.6), (0, 1.4), (1.0, 0.6)], close=True))
    for y0 in (1.85, 2.3, 2.75):               # hatch the right half parallel to its outer edge
        x = (y0 - 1.4) / 1.6
        k.line(0, y0, x, y0 - 2.4 * x)
    return k.letter("N", -1.3, -2.15)


# ---- Miscellaneous --------------------------------------------------------------------------------------------

@symbol("Hazard Point", G_HAZARD, "Warning triangle holding an exclamation mark. Any hazard the crew flags: unstable ground, asbestos, a live line.")
def _():
    return Sketch().path(triangle(3.8, True, 0, 0), close=True).letter("!", 0, -0.35)


@symbol("Heritage Item", G_HAZARD, "Five-point star outline. A protected heritage item such as a marker stone, a post or a plaque.", centre=True)
def _():
    return Sketch().path(star(5, 2.0, 0.8, 90), close=True)


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
        if not NAME_PATTERN.match(n) and n != "GNSS Base":
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
    """Four sheets of about 28 symbols, split between groups, drawn in the service colours on the dark
    ground (needs Pillow and preview.py beside this file)."""
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
