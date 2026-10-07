"""Katana Standard: the 71 linestyles.

Every linestyle is plain data in LINESTYLES, a list of dicts with the customisation file's own members
(`name`, `group`, `units`, `length`, `strokes`) and one extra, `description`: a line saying what it looks
like and when to use it.  Nothing but the standard library is imported.

    python tools/katana_standard/linestyles.py --check [--contract contract.json]
    python tools/katana_standard/linestyles.py --list
    python tools/katana_standard/linestyles.py --json

`--check` exits 1 on an error.  It always checks the stroke grammar, the names and the geometry below; when
a contract.json is beside this file, or named by --contract or by KATANA_STANDARD_CONTRACT, it also compares
every name, group, unit and period with it.

The house rules, which the check enforces:

* Units are paper millimetres, so a pattern keeps its size at any plan scale.  +x runs along the line and
  +y is to its LEFT.  Instance k starts at k times `length`; every stroke lies inside 0..length in x and
  inside plus or minus 3.4 mm in y, so a pattern closes on itself and nothing overshoots a join.
* A cell is laid out from x = 0.  A dash starts it, so a line opens with a full dash, and every dash is one
  stroke inside its cell: nothing abuts across the join, which would show as a faint seam where two
  antialiased ends meet.  Marks stand at the exact middle of a gap or of the cell.  Waves start on the line.
* The repeating pattern is mirror-symmetric along the line, about the middle of one of its dashes or marks,
  so read from either end it has the same marks in the same places: only where a mark stands to one side
  (a tick, a hump) does the digitising direction show, always as the LEFT.  The few patterns with a direction
  of their own (the flow chevrons, the dot-dot-dash drain) or an irregular tuft are named in SYMMETRY with
  "none", and the lightning bolt, which leans, with "point"; the check proves the symmetry of the rest.
* Katana lays a definition by giving every stroke POINT its own distance along the line, so a long straight
  stroke is a chord and cuts a corner.  Every straight run along the line is drawn in chords of at most one
  millimetre (`_Pen.run`), which keeps a dash or a baseline within about 0.35 mm of a right-angle corner.
* A dot is a 0.6 mm dash.  Katana draws the `dot` stroke as one point the width of the pen: a speck a third
  of the line's weight on a plot and a single pixel on the screen, so that a dash-dot line reads as dashes.
* Posts stand ON the line, which runs through them: guard rails, fence posts, chain mesh.  Beads are
  THREADED on it, the line stopping at each side of the bead and the inside left open: the contour diamond,
  the insulator ring, the exclusion square.  The two kinds of mark are then told apart at a glance even where
  their outlines match.
* Letters are 2.4 mm and `middle-centre`, in a gap of 5 mm or more.  Numbers carry at most three decimals.
  No stroke names a pen: the code's rule gives the colour.
"""
import json
import math
import os
import sys

G_LOT = "Boundaries and Cadastre/Lot Boundaries"
G_EASE = "Boundaries and Cadastre/Easements and Reserves"
G_CARR = "Roads and Pavements/Carriageway"
G_MARK = "Roads and Pavements/Markings"
G_PATH = "Roads and Pavements/Paths and Access"
G_POST = "Street Furniture and Signs/Posts and Barriers"
G_KERB = "Kerbs and Drainage/Kerbs"
G_PIPE = "Kerbs and Drainage/Pipes and Culverts"
G_CHAN = "Kerbs and Drainage/Channels and Waterways"
G_TRACK = "Rail and Transit/Track"
G_OVER = "Rail and Transit/Overhead and Signals"
G_PLAT = "Rail and Transit/Platforms"
G_WATER = "Utilities/Water"
G_SEWER = "Utilities/Sewer"
G_GAS = "Utilities/Gas"
G_ELEC = "Utilities/Electricity"
G_COMM = "Utilities/Communications"
G_RECY = "Utilities/Recycled Water"
G_FIRE = "Utilities/Fire Service"
G_FUEL = "Utilities/Fuel"
G_UNKN = "Utilities/Unknown Services"
G_ALL = "Utilities/All Services"
G_SHRUB = "Vegetation and Landscape/Shrubs and Plants"
G_COVER = "Vegetation and Landscape/Ground Cover"
G_BUILD = "Buildings and Structures/Buildings"
G_STEPS = "Buildings and Structures/Steps and Access"
G_FENCE = "Fences and Walls/Fences"
G_WALL = "Fences and Walls/Walls"
G_BREAK = "Terrain and Breaklines/Breaklines"
G_CONT = "Terrain and Breaklines/Contours"
G_WEDGE = "Terrain and Breaklines/Water Edges"
G_ROCK = "Terrain and Breaklines/Rock and Cliff"
G_PLAN = "Survey Control and Annotation/Plan Marks"
G_HAZ = "Miscellaneous/Hazards and Heritage"
G_TEMP = "Miscellaneous/Temporary Works"

CHORD = 1.0  # the longest straight chord of any run of ink, in mm
DOT = 0.6    # a dot is a dash this long, in mm
LETTER_HEIGHT = 2.4


def _n(value):
    """A clean number: three decimals at most, no negative zero, an int when whole."""
    value = round(float(value), 3)
    if value == 0:
        return 0
    return int(value) if value == int(value) else value


class _Pen:
    """Writes the stroke list.  Every primitive opens with its own `move`, because `arc`, `circle`, `dot`
    and `text` never move the pen."""

    def __init__(self):
        self.strokes = []

    def _move(self, x, y=0.0):
        self.strokes.append(["move", _n(x), _n(y)])

    def _draw(self, x, y=0.0):
        self.strokes.append(["draw", _n(x), _n(y)])

    def run(self, x0, x1, y=0.0):
        """A straight run from x0 to x1 at height y, in equal chords no longer than CHORD."""
        parts = max(1, math.ceil((x1 - x0) / CHORD - 1e-9))
        self._move(x0, y)
        for i in range(1, parts + 1):
            self._draw(x1 if i == parts else x0 + (x1 - x0) * i / parts, y)

    def path(self, points, close=False):
        """A polyline through `points`; `close` returns to the first point."""
        self._move(*points[0])
        for point in points[1:]:
            self._draw(*point)
        if close:
            self._draw(*points[0])

    def tick(self, x, y0, y1):
        self.path([(x, y0), (x, y1)])

    def arc(self, cx, cy, radius, a0, a1):
        self._move(cx, cy)
        self.strokes.append(["arc", _n(radius), _n(a0), _n(a1)])

    def circle(self, cx, cy, radius):
        self._move(cx, cy)
        self.strokes.append(["circle", _n(radius)])

    def dot(self, x, y=0.0):
        """A dot centred on x: a 0.6 mm dash (see the module notes)."""
        self.run(x - DOT / 2.0, x + DOT / 2.0, y)

    def text(self, x, y, label):
        self._move(x, y)
        self.strokes.append(["text", {"text": label, "height": 2.4, "justify": "middle-centre"}])

    def square(self, cx, cy, side):
        h = side / 2.0
        self.path([(cx - h, cy - h), (cx + h, cy - h), (cx + h, cy + h), (cx - h, cy + h)], close=True)

    def diamond(self, cx, cy, half):
        self.path([(cx - half, cy), (cx, cy + half), (cx + half, cy), (cx, cy - half)], close=True)

    def cross(self, cx, cy, half):
        self.path([(cx - half, cy - half), (cx + half, cy + half)])
        self.path([(cx - half, cy + half), (cx + half, cy - half)])

    def thread(self, length, cx, half):
        """The baseline of a cell with a bead of half-width `half` centred on cx threaded on it."""
        self.run(0, cx - half)
        self.run(cx + half, length)


LINESTYLES = []
SYMMETRY = {}  # name -> "point" or "none"; every other linestyle is mirror-symmetric about x = length / 2


def _style(name, group, length, description, symmetry="mirror"):
    """Register the function below as the definition of one linestyle."""
    def register(build):
        pen = _Pen()
        build(pen)
        LINESTYLES.append({"name": name, "group": group, "units": "paper", "length": _n(length),
                           "strokes": pen.strokes, "description": description})
        if symmetry != "mirror":
            SYMMETRY[name] = symmetry
        return build
    return register


def _gap_pipe(pen, length, gap, label):
    """A dash, then a gap of `gap` mm holding `label`: the line of a service."""
    pen.run(0, length - gap)
    pen.text(length - gap / 2.0, 0, label)


def _dash(pen, length, dash, y=0.0):
    """One dash from the cell start, then a gap.  Returns the middle of the gap, where a mark stands."""
    pen.run(0, dash, y)
    return (dash + length) / 2.0


# ---------------------------------------------------------------------------------------------------------
# Boundaries and Cadastre
# ---------------------------------------------------------------------------------------------------------

@_style("Title Boundary", G_LOT, 25,
        "Unbroken line crossed by a short tick every 25 mm: the registered title boundary, the firmest line in the cadastre.")
def _(p):
    p.run(0, 25)
    p.tick(12.5, -1.6, 1.6)


@_style("Occupation Boundary", G_LOT, 10,
        "Even dashes, 7 on and 3 off: a boundary as occupied on the ground (a fence line, a wall face) and not as titled.")
def _(p):
    _dash(p, 10, 7)


@_style("Compiled Boundary", G_LOT, 14,
        "Long dash, then one dot: a boundary compiled from plans and not measured in the field.")
def _(p):
    _dash(p, 14, 8)
    p.dot(11)


@_style("Reserve Boundary", G_LOT, 18,
        "Long dash, then two dots: the limit of a public road reserve or of a park or reserve.")
def _(p):
    _dash(p, 18, 9)
    p.dot(11.9)
    p.dot(15.1)


@_style("Municipal Boundary", G_LOT, 20,
        "Long dash and a short dash in turn: the boundary between two local government areas.")
def _(p):
    p.run(0, 11)
    p.run(14, 17)


@_style("Easement Boundary", G_EASE, 6,
        "Short even dashes, 4 on and 2 off: the edge of an easement or covenant area; the code says what it carries.")
def _(p):
    _dash(p, 6, 4)


# ---------------------------------------------------------------------------------------------------------
# Roads and Pavements
# ---------------------------------------------------------------------------------------------------------

@_style("Unsealed Road Edge", G_CARR, 2.5,
        "A fine stipple of short specks, like gravel thrown on the edge: the edge of an unsealed road.")
def _(p):
    p.run(0, 0.8)


@_style("Shoulder Edge", G_CARR, 8,
        "Dashes, 6 on and 2 off: the outer edge of the road shoulder.")
def _(p):
    _dash(p, 8, 6)


@_style("Road Centreline", G_CARR, 14,
        "Long dashes with a cross tick standing in each gap: the centre of the carriageway, shot at the crown.")
def _(p):
    p.tick(_dash(p, 14, 10), -1.5, 1.5)


@_style("Dashed Lane Marking", G_MARK, 12,
        "Short painted dashes with long gaps, 4 on and 8 off: the dashed line between traffic lanes.")
def _(p):
    _dash(p, 12, 4)


@_style("Double Barrier Marking", G_MARK, 10,
        "Two close unbroken lines, 1 mm apart: the double continuous barrier line painted on a road.")
def _(p):
    p.run(0, 10, 0.5)
    p.run(0, 10, -0.5)


@_style("Zebra Crossing Marking", G_MARK, 3,
        "A row of bars across the line with no baseline, like the stripes of a zebra crossing; shoot it along the crossing's width.")
def _(p):
    p.tick(1.5, -1.8, 1.8)


@_style("Cycleway Edge", G_PATH, 10,
        "Dashes with a small bead in each gap: the edge of a cycleway or a shared path.")
def _(p):
    p.circle(_dash(p, 10, 6), 0, 0.5)


# ---------------------------------------------------------------------------------------------------------
# Street Furniture
# ---------------------------------------------------------------------------------------------------------

@_style("Guard Rail Line", G_POST, 12,
        "Unbroken line with a small square post every 12 mm, the rail running through it: a roadside guard rail or crash barrier.")
def _(p):
    p.run(0, 12)
    p.square(6, 0, 1.4)


# ---------------------------------------------------------------------------------------------------------
# Kerbs and Drainage
# ---------------------------------------------------------------------------------------------------------

@_style("Mountable Kerb Edge", G_KERB, 6,
        "Unbroken line with a rounded hump to the left every 6 mm: a low kerb a vehicle can drive over.")
def _(p):
    p.run(0, 6)
    p.arc(3, 0, 1.5, 0, 180)


@_style("Dish Drain Edge", G_KERB, 8,
        "Unbroken line with a shallow V standing to its left every 8 mm: a V-shaped concrete dish drain.")
def _(p):
    p.run(0, 8)
    p.path([(2.5, 1.4), (4, 0), (5.5, 1.4)])


@_style("Stormwater Pipe Line", G_PIPE, 16,
        "A line broken by the letter D every 16 mm: a stormwater pipe, shot at the surface or inside the pits.")
def _(p):
    _gap_pipe(p, 16, 5, "D")


@_style("Subsoil Drain Line", G_PIPE, 9,
        "Two dots and a dash in turn, written in the digitising direction: a perforated subsoil drain.", "none")
def _(p):
    p.run(0, DOT)
    p.run(1.5, 1.5 + DOT)
    p.run(3, 6)


@_style("Culvert Outline", G_PIPE, 12,
        "Two parallel lines 1.8 mm apart joined by a tie every 12 mm, like a ladder: a rectangular culvert.")
def _(p):
    p.run(0, 12, 0.9)
    p.run(0, 12, -0.9)
    p.tick(6, -0.9, 0.9)


@_style("Open Drain Invert", G_CHAN, 14,
        "Unbroken line with an open chevron that points the way you shot, downstream: an open drain, a swale or a flow path.", "none")
def _(p):
    p.run(0, 14)
    p.path([(5.8, 1.4), (8.2, 0), (5.8, -1.4)])


@_style("Creek Centreline", G_CHAN, 12,
        "A smooth continuous wave 1.8 mm high and 6 mm long: the centre of a creek or river bed.")
def _(p):
    p.path([(0.5 * i, 0.9 * math.sin(2 * math.pi * 0.5 * i / 6.0)) for i in range(25)])


# ---------------------------------------------------------------------------------------------------------
# Rail and Transit
# ---------------------------------------------------------------------------------------------------------

@_style("Running Rail", G_TRACK, 4,
        "Unbroken line crossed by a tie every 4 mm, like a ladder: the head of a running rail.")
def _(p):
    p.run(0, 4)
    p.tick(2, -1.4, 1.4)


@_style("Track Centreline", G_TRACK, 14,
        "Long dashes with a small square standing in each gap: the centre of the track between the rails.")
def _(p):
    p.square(_dash(p, 14, 10), 0, 1.0)


@_style("Ballast Edge", G_TRACK, 3,
        "Two rows of tiny dashes, one 1 mm above the other and staggered, like stones: the shoulder of the ballast.")
def _(p):
    p.run(0.55, 0.95, 0.5)
    p.run(2.05, 2.45, -0.5)


@_style("Contact Wire Line", G_OVER, 12,
        "Unbroken line with a tick to the left, then a tick to the right: the overhead contact wire, shot at each mast.")
def _(p):
    p.run(0, 12)
    p.tick(3, 0, 1.6)
    p.tick(9, 0, -1.6)


@_style("Platform Edge Line", G_PLAT, 5,
        "Unbroken line with a short tick to the left every 5 mm: the edge of a platform.")
def _(p):
    p.run(0, 5)
    p.tick(2.5, 0, 1.2)


# ---------------------------------------------------------------------------------------------------------
# Utilities
# ---------------------------------------------------------------------------------------------------------

@_style("Water Supply Pipe", G_WATER, 16,
        "A line broken by the letter W every 16 mm: a pressure main that carries drinking water.")
def _(p):
    _gap_pipe(p, 16, 5, "W")


@_style("Sewer Gravity Pipe", G_SEWER, 20,
        "A line broken by the letter S and a small flow chevron: a gravity sewer. The chevron points the way you shot, so shoot downhill.", "none")
def _(p):
    p.run(0, 12)
    p.text(14.7, 0, "S")
    p.path([(16.9, 0.7), (18.1, 0), (16.9, -0.7)])


@_style("Sewer Pressure Pipe", G_SEWER, 20,
        "A line broken by the letters SR every 20 mm: a pressure sewer that carries flow uphill.")
def _(p):
    _gap_pipe(p, 20, 7, "SR")


@_style("Gas Supply Pipe", G_GAS, 16,
        "A line broken by the letter G every 16 mm: a pipe that carries natural gas.")
def _(p):
    _gap_pipe(p, 16, 5, "G")


@_style("Underground Cable", G_ELEC, 12,
        "A line broken by the letter E every 12 mm: a buried electricity cable.")
def _(p):
    _gap_pipe(p, 12, 5, "E")


@_style("Overhead Line", G_ELEC, 14,
        "Unbroken line with a small ring, an insulator, threaded on it every 14 mm: an overhead power or communications line.")
def _(p):
    p.thread(14, 7, 0.7)
    p.circle(7, 0, 0.7)


@_style("Communications Cable", G_COMM, 12,
        "A line broken by the letter C every 12 mm: a buried telephone, data or fibre cable.")
def _(p):
    _gap_pipe(p, 12, 5, "C")


@_style("Recycled Water Pipe", G_RECY, 16,
        "A line broken by the letter R every 16 mm: a main that carries recycled water.")
def _(p):
    _gap_pipe(p, 16, 5, "R")


@_style("Fire Service Pipe", G_FIRE, 16,
        "A line broken by the letter F every 16 mm: a main that serves hydrants and sprinklers.")
def _(p):
    _gap_pipe(p, 16, 5, "F")


@_style("Fuel Supply Pipe", G_FUEL, 16,
        "A line broken by the letter P every 16 mm: a pipeline for fuel or oil.")
def _(p):
    _gap_pipe(p, 16, 5, "P")


@_style("Unknown Service Line", G_UNKN, 12,
        "A line broken by a question mark every 12 mm: a buried line whose service is not yet known.")
def _(p):
    _gap_pipe(p, 12, 5, "?")


@_style("Buried Duct", G_ALL, 10,
        "Two close parallel dashed lines, 1.2 mm apart: a duct or conduit that carries cable, whatever the service.")
def _(p):
    _dash(p, 10, 7, 0.6)
    _dash(p, 10, 7, -0.6)


@_style("Service Connection Line", G_ALL, 6,
        "Fine even dashes, 3 on and 3 off: the small property connection between a main and a lot.")
def _(p):
    _dash(p, 6, 3)


@_style("Disused Service Line", G_ALL, 8,
        "Dashes with a small cross in each gap: a service that is no longer in use, whatever its kind.")
def _(p):
    p.cross(_dash(p, 8, 4), 0, 0.9)


# ---------------------------------------------------------------------------------------------------------
# Vegetation
# ---------------------------------------------------------------------------------------------------------

@_style("Hedge Line", G_SHRUB, 2.4,
        "A row of small round scallops to the left with no baseline: a clipped hedge, shot along its face.")
def _(p):
    p.arc(1.2, 0, 1.2, 0, 180)


@_style("Bush Edge Line", G_COVER, 7,
        "Unbroken line with three uneven ticks to the left, like a tuft: the edge of scrub or bush.", "none")
def _(p):
    p.run(0, 7)
    p.tick(1.5, 0, 0.8)
    p.tick(3.5, 0, 1.6)
    p.tick(5.5, 0, 1.0)


@_style("Grass Edge", G_COVER, 6,
        "Unbroken line with a small fan of three blades to the left every 6 mm: the edge of mown grass.")
def _(p):
    p.run(0, 6)
    for angle in (55, 90, 125):
        p.path([(3, 0), (3 + 1.5 * math.cos(math.radians(angle)), 1.5 * math.sin(math.radians(angle)))])


# ---------------------------------------------------------------------------------------------------------
# Buildings
# ---------------------------------------------------------------------------------------------------------

@_style("Eave Line", G_BUILD, 10.5,
        "A long dash and a short dash in turn, 1.5 mm apart: the edge of a roof overhang, a verandah or an awning.")
def _(p):
    p.run(0, 6)
    p.run(7.5, 9)


@_style("Hidden Outline", G_BUILD, 3,
        "Fine short dashes, 1 on and 2 off: a building edge seen only on a plan, or a building not yet finished.")
def _(p):
    _dash(p, 3, 1)


@_style("Steps Edge", G_STEPS, 2.5,
        "Unbroken line crossed by a close row of treads 4 mm long: the nosing of a flight of steps.")
def _(p):
    p.run(0, 2.5)
    p.tick(1.25, -2, 2)


# ---------------------------------------------------------------------------------------------------------
# Fences
# ---------------------------------------------------------------------------------------------------------

@_style("Chain Mesh Fence", G_FENCE, 8,
        "Unbroken line with a small cross every 8 mm: a wire mesh fence on steel posts.")
def _(p):
    p.run(0, 8)
    p.cross(4, 0, 0.8)


@_style("Paling Fence", G_FENCE, 2,
        "Unbroken line with a close comb of ticks to the left, one every 2 mm: a timber paling fence.")
def _(p):
    p.run(0, 2)
    p.tick(1, 0, 1.6)


@_style("Post and Rail Fence", G_FENCE, 12,
        "Two parallel rails 1 mm apart with a square post every 12 mm: a timber or steel post and rail fence.")
def _(p):
    p.run(0, 12, 0.5)
    p.run(0, 12, -0.5)
    p.square(6, 0, 1.4)


@_style("Post and Wire Fence", G_FENCE, 10,
        "Unbroken line with a small ring for a post every 10 mm: a rural fence of plain wire on posts.")
def _(p):
    p.run(0, 10)
    p.circle(5, 0, 0.5)


@_style("Barbed Wire Fence", G_FENCE, 6,
        "Unbroken line with a small V barb above the line, then one below it: a rural fence with barbed wire.")
def _(p):
    p.run(0, 6)
    p.path([(0.9, 1.2), (1.5, 0), (2.1, 1.2)])
    p.path([(3.9, -1.2), (4.5, 0), (5.1, -1.2)])


@_style("Electric Fence", G_FENCE, 12,
        "Unbroken line with a lightning bolt every 12 mm: a fence with an electrified wire.", "point")
def _(p):
    p.run(0, 12)
    p.path([(6.8, 1.5), (5.4, 0.15), (6.6, -0.15), (5.2, -1.5)])


@_style("Metal Palisade Fence", G_FENCE, 2,
        "Unbroken line crossed by a close row of short bars, one every 2 mm: a steel palisade or pool fence.")
def _(p):
    p.run(0, 2)
    p.tick(1, -0.9, 0.9)


# ---------------------------------------------------------------------------------------------------------
# Walls
# ---------------------------------------------------------------------------------------------------------

@_style("Brick Wall", G_WALL, 5,
        "Two parallel lines 1 mm apart with a joint across them every 5 mm: a brick or block wall, shot on its lower face.")
def _(p):
    p.run(0, 5, 0.5)
    p.run(0, 5, -0.5)
    p.tick(2.5, -0.5, 0.5)


@_style("Stone Wall", G_WALL, 2,
        "A chain of touching rings, 2 mm across, centred on the line: a dry or mortared stone wall.")
def _(p):
    p.circle(1, 0, 1)


@_style("Earth Retaining Wall", G_WALL, 4,
        "Unbroken line with a small triangle to the left every 4 mm: a wall that holds back earth; the barbs point to the lower side.")
def _(p):
    p.run(0, 4)
    p.path([(1, 0), (2, 1.8), (3, 0)])


# ---------------------------------------------------------------------------------------------------------
# Terrain and Breaklines
# ---------------------------------------------------------------------------------------------------------

@_style("Bank Top Edge", G_BREAK, 6,
        "Unbroken line with a long tick to the left every 6 mm: the top edge of a slope, the ticks pointing downhill.")
def _(p):
    p.run(0, 6)
    p.tick(3, 0, 2.4)


@_style("Bank Toe Edge", G_BREAK, 3,
        "Unbroken line with a short tick to the left every 3 mm: the bottom edge of a slope, the ticks standing away from the slope.")
def _(p):
    p.run(0, 3)
    p.tick(1.5, 0, 1.0)


@_style("Ridge Line", G_BREAK, 10,
        "Dashes with a small caret pointing to the left in each gap: a line along a crest.")
def _(p):
    x = _dash(p, 10, 6)
    p.path([(x - 0.8, -0.6), (x, 0.6), (x + 0.8, -0.6)])


@_style("Gully Line", G_BREAK, 10,
        "Dashes with a small caret pointing to the right in each gap: a line along the bottom of a gully.")
def _(p):
    x = _dash(p, 10, 6)
    p.path([(x - 0.8, 0.6), (x, -0.6), (x + 0.8, 0.6)])


@_style("Soft Breakline", G_BREAK, 4,
        "Fine short dashes, 2.5 on and 1.5 off: a gentle change in grade that a surface should still follow.")
def _(p):
    _dash(p, 4, 2.5)


@_style("Exclusion Boundary", G_BREAK, 8,
        "Unbroken line with a small hollow square threaded on it every 8 mm: an area a surface must leave out, such as a building or a lake.")
def _(p):
    p.thread(8, 4, 0.8)
    p.square(4, 0, 1.6)


@_style("Index Contour", G_CONT, 20,
        "Unbroken line with a small diamond bead threaded on it every 20 mm: every fifth contour, marked so it can be followed.")
def _(p):
    p.thread(20, 10, 0.6)
    p.diamond(10, 0, 0.6)


@_style("Depression Contour", G_CONT, 8,
        "Unbroken line with a short tick to the left every 8 mm: a contour round a hollow, the ticks pointing downhill.")
def _(p):
    p.run(0, 8)
    p.tick(4, 0, 1.0)


@_style("Water Edge", G_WEDGE, 10,
        "Dashes with a small ripple arc to the left in each gap: the edge of water, or the line of the highest recent water.")
def _(p):
    p.arc(_dash(p, 10, 6), 0, 1.2, 0, 180)


@_style("Rock Outcrop Edge", G_ROCK, 5,
        "A sharp zigzag 1.6 mm high with no baseline: the edge of exposed rock.")
def _(p):
    p.path([(0, 0), (0.625, 0.8), (1.875, -0.8), (3.125, 0.8), (4.375, -0.8), (5, 0)])


@_style("Cliff Edge", G_ROCK, 4,
        "Unbroken line with ticks to the left, a long one and a short one in turn: the top edge of a cliff, pointing downhill.")
def _(p):
    p.run(0, 4)
    p.tick(1, 0, 2.6)
    p.tick(3, 0, 1.2)


# ---------------------------------------------------------------------------------------------------------
# Plan marks, hazards, temporary works
# ---------------------------------------------------------------------------------------------------------

@_style("Limit of Survey Line", G_PLAN, 24,
        "Long dashes with an open triangle in each gap: the edge of the surveyed area; nothing beyond it was measured.")
def _(p):
    x = _dash(p, 24, 14)
    p.path([(x - 1.2, -1.04), (x + 1.2, -1.04), (x, 1.04)], close=True)


@_style("Revision Cloud Edge", G_PLAN, 6,
        "A row of large round scallops to the left with no baseline: a cloud drawn round an area that has changed.")
def _(p):
    p.arc(3, 0, 3, 0, 180)


@_style("Hazard Boundary", G_HAZ, 10,
        "Dashes with an exclamation mark in each gap: the boundary of a hazardous area.")
def _(p):
    p.text(_dash(p, 10, 6), 0, "!")


@_style("Temporary Works Line", G_TEMP, 10,
        "Dashes with the letter T in each gap: a temporary fence, a hoarding or the edge of a work zone.")
def _(p):
    p.text(_dash(p, 10, 6), 0, "T")


# ---------------------------------------------------------------------------------------------------------
# The self-check
# ---------------------------------------------------------------------------------------------------------

ARITY = {"move": 3, "draw": 3, "arc": 4, "circle": 2, "dot": 2, "text": 2}
MEMBERS = {"name", "group", "units", "length", "strokes", "description"}
CELL = 0.05  # the raster of the symmetry test, in mm


def _geometry(defn):
    """The definition as polylines [(x, y), ...] and texts [(x, y, label)], arcs and circles in 24 chords
    a turn as Katana draws them."""
    polys, texts, line, x, y = [], [], [], 0.0, 0.0

    def flush():
        nonlocal line
        if len(line) > 1:
            polys.append(line)
        line = []

    for stroke in defn["strokes"]:
        kind = stroke[0]
        if kind == "move":
            flush()
            x, y = stroke[1], stroke[2]
        elif kind == "draw":
            if not line:
                line = [(x, y)]
            x, y = stroke[1], stroke[2]
            line.append((x, y))
        elif kind in ("arc", "circle"):
            flush()
            a0, a1 = (0.0, 360.0) if kind == "circle" else (stroke[2], stroke[3])
            radius = abs(stroke[1])
            steps = max(2, math.ceil(abs(a1 - a0) / 360.0 * 24))
            polys.append([(x + radius * math.cos(math.radians(a0 + (a1 - a0) * i / steps)),
                           y + radius * math.sin(math.radians(a0 + (a1 - a0) * i / steps))) for i in range(steps + 1)])
        elif kind == "text":
            flush()
            texts.append((x, y, stroke[1]["text"]))
    flush()
    return polys, texts


def _extent(defn):
    """Bounding box (x0, y0, x1, y1) of the ink; a text is a box of 0.62 of its height a character."""
    polys, texts = _geometry(defn)
    xs = [p[0] for poly in polys for p in poly]
    ys = [p[1] for poly in polys for p in poly]
    for x, y, label in texts:
        half_w = 0.62 * LETTER_HEIGHT * len(label) / 2.0
        xs += [x - half_w, x + half_w]
        ys += [y - LETTER_HEIGHT / 2.0, y + LETTER_HEIGHT / 2.0]
    return min(xs), min(ys), max(xs), max(ys)


def _ink(polys):
    """The set of raster cells touched by the polylines, sampled every half cell."""
    cells = set()
    for poly in polys:
        for (xa, ya), (xb, yb) in zip(poly, poly[1:]):
            steps = max(1, math.ceil(math.hypot(xb - xa, yb - ya) / (CELL / 2.0)))
            for i in range(steps + 1):
                cells.add((round((xa + (xb - xa) * i / steps) / CELL), round((ya + (yb - ya) * i / steps) / CELL)))
    return cells


def _symmetric(defn, mode):
    """Whether the repeating pattern equals itself reflected in a vertical axis (mode "mirror") or turned
    half a turn about a point of the line (mode "point"), within a cell of the raster."""
    length = defn["length"]
    polys, texts = _geometry(defn)
    ink = _ink(polys)
    period = round(length / CELL)
    near = set()
    for shift in (-period, 0, period):
        for cx, cy in ink:
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    near.add((cx + shift + dx, cy + dy))
    # an axis lies on a vertex or halfway between two, and repeats every half period; c2 is twice its
    # position in cells, so that a cell cx reflects to c2 - cx
    vertex = sorted({round(x / CELL) for poly in polys for x, _ in poly} | {round(x / CELL) for x, _, _ in texts})
    candidates = {2 * v for v in vertex} | {a + b for a, b in zip(vertex, vertex[1:])}
    sign = 1 if mode == "mirror" else -1
    for c2 in sorted({c % period for c in candidates}):
        if not all((c2 - cx, sign * cy) in near for cx, cy in ink):
            continue
        if all(any(tl == label and abs(ty - sign * y) < 1e-6 and
                   min((c2 * CELL - x - tx) % length, length - (c2 * CELL - x - tx) % length) < 0.06
                   for tx, ty, tl in texts) for x, y, label in texts):
            return True
    return False


def _title_case(name):
    words = name.split(" ")
    return all(w in ("and", "of") or (w[:1].isupper() and w[1:].islower() and w.isalpha()) for w in words)


def _contract_path(argv):
    if "--contract" in argv:
        return argv[argv.index("--contract") + 1]
    if os.environ.get("KATANA_STANDARD_CONTRACT"):
        return os.environ["KATANA_STANDARD_CONTRACT"]
    beside = os.path.join(os.path.dirname(os.path.abspath(__file__)), "contract.json")
    return beside if os.path.exists(beside) else None


def check(contract_path=None):
    """Returns (errors, warnings), lists of messages."""
    errors, warnings = [], []
    names = [d["name"] for d in LINESTYLES]
    if len(names) != len(set(names)):
        errors.append("duplicate names: " + ", ".join(sorted({n for n in names if names.count(n) > 1})))
    for d in LINESTYLES:
        n = d["name"]
        if set(d) - MEMBERS:
            errors.append(f"{n}: unknown members {sorted(set(d) - MEMBERS)}")
        if not _title_case(n):
            errors.append(f"{n}: a name is Title Case words only: no digits, slash or stray blank")
        if d["units"] != "paper":
            errors.append(f"{n}: units must be paper")
        if d["group"].count("/") != 1:
            errors.append(f"{n}: group {d['group']!r} must have two levels")
        description = d.get("description", "")
        if not description or len(description) > 160:
            errors.append(f"{n}: the description is one line of 1 to 160 characters (it is {len(description)})")
        strokes = d["strokes"]
        if not strokes or strokes[0][0] != "move":
            errors.append(f"{n}: the first stroke must be a move")
        if len(strokes) > 200:
            errors.append(f"{n}: {len(strokes)} strokes is over the hard limit of 200")
        elif len(strokes) > 90:
            warnings.append(f"{n}: {len(strokes)} strokes; keep a definition lean")
        grammar_ok = True
        for s in strokes:
            if not isinstance(s, list) or not s or s[0] not in ARITY or len(s) != ARITY[s[0]]:
                errors.append(f"{n}: bad stroke {s!r}")
                grammar_ok = False
                continue
            if s[0] == "text":
                t = s[1]
                if set(t) - {"text", "height", "justify"} or t.get("height") != LETTER_HEIGHT or t.get("justify") != "middle-centre":
                    errors.append(f"{n}: a label is 2.4 mm and middle-centre: {t!r}")
                if not 1 <= len(t.get("text", "")) <= 3:
                    errors.append(f"{n}: a label is one to three characters")
                continue
            limit = [60] + [720] * 2 if s[0] == "arc" else [60] * (len(s) - 1)
            if not all(isinstance(v, (int, float)) and math.isfinite(v) and abs(v) <= m and round(v, 3) == v
                       for v, m in zip(s[1:], limit)):
                errors.append(f"{n}: {s!r}: numbers are finite, to 3 decimals, a length at most 60 and an angle at most 720")
                grammar_ok = False
            elif s[0] == "arc" and (s[1] <= 0 or abs(s[3] - s[2]) > 360):
                errors.append(f"{n}: {s!r}: an arc has a positive radius and sweeps at most one turn")
            elif s[0] == "circle" and s[1] <= 0:
                errors.append(f"{n}: {s!r}: a circle has a positive radius")
            elif s[0] == "dot":
                errors.append(f"{n}: a dot is drawn as a short dash (see the module notes), not a `dot` stroke")
        if not grammar_ok:
            continue
        length = d["length"]
        if not (isinstance(length, (int, float)) and length > 0):
            errors.append(f"{n}: length must be above 0")
            continue
        x0, y0, x1, y1 = _extent(d)
        if x0 < -0.001 or x1 > length + 0.001:
            errors.append(f"{n}: ink runs outside one period (x {x0:.3f}..{x1:.3f}, period {length})")
        if max(abs(y0), abs(y1)) > 3.4:
            errors.append(f"{n}: |y| reaches {max(abs(y0), abs(y1)):.2f} mm (limit 3.4)")
        polys, _ = _geometry(d)
        for poly in polys:
            for (xa, ya), (xb, yb) in zip(poly, poly[1:]):
                if ya == yb and abs(xb - xa) > 2.5 + 1e-6:  # a mark's own side may be longer than a chord
                    warnings.append(f"{n}: a straight run of {abs(xb - xa):.2f} mm would cut a corner (chords are at most {CHORD})")
        mode = SYMMETRY.get(n, "mirror")
        if mode != "none" and not _symmetric(d, mode):
            errors.append(f"{n}: the pattern is not {mode}-symmetric (name it in SYMMETRY if that is intended)")
    for n in SYMMETRY:
        if n not in names:
            errors.append(f"SYMMETRY names {n!r}, which is not a linestyle")
    if contract_path:
        wanted = json.load(open(contract_path, encoding="utf-8"))["linestyles"]
        by_name = {w["name"]: w for w in wanted}
        for w in wanted:
            if w["name"] not in names:
                errors.append(f"missing {w['name']!r}")
        for d in LINESTYLES:
            w = by_name.get(d["name"])
            if w is None:
                errors.append(f"{d['name']}: not in the contract")
                continue
            if d["group"] != w["group"]:
                errors.append(f"{d['name']}: group {d['group']!r} should be {w['group']!r}")
            if d["units"] != w["units"]:
                errors.append(f"{d['name']}: units {d['units']!r} should be {w['units']!r}")
            if d["length"] != w["periodMm"]:
                errors.append(f"{d['name']}: length {d['length']} should be {w['periodMm']}")
        if names != [w["name"] for w in wanted]:
            warnings.append("the order of LINESTYLES differs from the contract's")
    return errors, warnings


def main(argv):
    if "--json" in argv:
        json.dump(LINESTYLES, sys.stdout, indent=1)
        print()
        return 0
    if "--list" in argv:
        for d in LINESTYLES:
            print(f"{d['name']:<26} {d['length']:>5} mm  {len(d['strokes']):>3} strokes  {d['group']}")
        print(f"{len(LINESTYLES)} linestyles, {sum(len(d['strokes']) for d in LINESTYLES)} strokes")
        return 0
    if "--check" in argv:
        contract = _contract_path(argv)
        errors, warnings = check(contract)
        for message in warnings:
            print("warning:", message)
        for message in errors:
            print("ERROR:", message)
        compared = (f"compared with {contract}" if contract
                    else "no contract.json found: names, groups and periods were not compared")
        print(f"linestyles: {len(LINESTYLES)} definitions, {len(errors)} errors, {len(warnings)} warnings; {compared}")
        return 1 if errors else 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
