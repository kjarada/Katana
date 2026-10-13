#!/usr/bin/env python3
"""What do two Katana customisation files share besides survey code keys? Counts, never names.

    python tools/katana_standard/originality.py REFERENCE.json CANDIDATE.json [--limit N]

Katana Standard reuses the survey code KEYS of another library, with what each key stands for and the kinds of
rule it has, on the owner's decision, so that a field file coded for either draws in both
(docs/katana_standard.md, "What is shared and what is not"; parity.py holds that side). Everything it NAMES, DRAWS
and WORDS is meant to be Katana's own, and this script is the scan that holds that to account. It reads both
files with the standard `json` module, shares no code with Katana, and compares what a person could copy:

  definition names    a linestyle or symbol name EQUAL to one of the reference's: the same letters and digits in
                      the same order, whatever the case, the blanks and the punctuation (`Stone Line`, `stone-line`
                      and `Stoneline` are one name). A name is compared three ways, each counted apart: with the
                      reference's definition names whole; with those names less their LEAD-IN, the word of
                      capitals a reference may put before a name, with or without a single capital before that
                      (`E WALL Stone Line` and `WALL Stone Line` are both also `Stone Line`); and with the
                      reference's legend labels, since a symbol named after the other library's label for the
                      thing is that library's name for it as much as one named after its symbol
  group paths         a definition's group, and a rule's group, equal in the same sense
  layer paths         a rule's layer, likewise
  legend labels       a rule's comment, equal in the same sense, with the reference's own "[KEY]" and "E XXXX"
                      lead-ins taken off; and, counted apart, a comment that is one of the reference's definition
                      names, whole or less its lead-in
  colour names        a name in the colour table, likewise
  colour values       a colour VALUE equal to one the reference draws in: a value of its table, or the value of a
                      standard colour name one of its rules uses (`white`, `red`: the 27 names a Katana file may
                      use without a table). One value is excused and counted apart: pure white, when it is the
                      candidate's communications colour (the colour of the codes whose attributes say
                      `utility.type=telecommunications`), since AS 5488 names white for communications and every
                      drawing system has it. A second white, or a white that is some other colour of the
                      candidate, is a colour value in common like any other
  attribute names     the name of an attribute a rule applies or asks for
  text styles         the style a text rule names
  shapes              a definition that DRAWS what one of the reference's draws. A definition is read as the
                      marks it makes where Katana's renderer puts them (`flattenDefinition`): every point is the
                      written one less the definition's `origin` and times its `factor`, a draw is a straight
                      piece between two points, and a circle, an arc, a dot and a text are where the pen stands
                      (a draw that goes nowhere marks a point, and is a dot). What a pen would not show is then
                      taken out, which leaves the DRAWING: pieces that lie on one straight line and touch or
                      overlap are one line, however many draws wrote it; a mark written twice is one mark; and
                      a dot that lies on a line, a ring or an arc of its own definition is no mark, since Katana
                      draws a dot as one point of the pen's width and the pen has been there. Two definitions
                      have one shape when every mark of each drawing is a mark of the other, so the
                      comparison is made
                        without regard to the ORDER the strokes are written in, to how they are chained (a square
                          as one path, or as four lines), or to the pieces a line is cut into;
                        without regard to the DIRECTION a line or an arc is drawn in;
                        under the EIGHT SYMMETRIES of the square: the four quarter turns, and each of them
                          mirrored. A definition turned or mirrored is the same drawing;
                        without regard to the pen (a colour is no shape) or to a dot's radius (Katana draws a dot
                          as one point of the pen's width, whatever radius the file gives it).
                      Compared three ways, each counted apart: as drawn; moved, so that the lower left corner of
                      the box round its points is the origin; and scaled, which is moved and then sized so that
                      the larger side of that box is 1 (radii and text heights are sized with it; a shape that
                      is all about one point is sized by its largest radius). Two numbers are the same when
                      they are less than 0.001 apart as drawn and moved, and less than 0.01 apart in the unit
                      box: the third decimal, which is as fine as this library writes a number, and the second.
                      An arc is its centre, its radius and the three points where it starts, is half way and
                      ends, so an angle is held as closely as a length and no closer. A text is its
                      characters, its place and its height as written (the renderer does not size a text by the
                      factor). Every definition of the reference takes part. One of the candidate's is left
                      out when five strokes or fewer DRAW it (a dash, a dash and a letter, a plus, a square, a
                      ring: any library draws those). The strokes counted are those of the plain list that
                      makes the drawing, not of the list as it was written: a move and a stroke for each ring,
                      arc, dot and text, a draw for each line, and a move for each path of lines drawn end to
                      end, in as few paths as draw every line once. So a dash written as ten pieces is a dash
                      and is left out, and a ring with two chords is six strokes however it was written
  near names          a definition name at least 0.9 alike (difflib) to a reference definition name, to one less
                      its lead-in or to a reference legend label, and a legend label as near to any of those,
                      without being equal to it; listed to --limit
  same-key labels     how many of the candidate's legend labels are at least 0.85 like the reference's label for
                      the SAME key. Two libraries that name one real thing (a give way line, a sewage pumping
                      station) use the trade's words for it, so this is a figure to read and not a failure

The exit status is 0 when every exact count is 0 and 1 otherwise; 2 when a file cannot be read. The reference's
own words appear in no output: only the candidate's, and only to say which of ITS names to look at. Tests:
tools/katana_standard/test_originality.py.
"""
import difflib
import json
import math
import re
import sys


TRIVIAL = 5   # strokes that draw it (`strokes_to_draw`): a dash, a plus, a square, a ring, which any library draws
WHITE = "#ffffff"
SERVICE_WORD = "telecommunications"
ATTRIBUTE_SCOPES = ("attributes", "vertexAttributes", "segmentAttributes")
# The 27 standard colour names and their values (src/katana_entity/colour_names.cpp; docs/customisation.md, "Colour
# names"): a rule may name one with no table, so a library that draws in `white` draws in this value.
STANDARD_COLOURS = {
    "red": "#ff0000", "green": "#00ff00", "blue": "#0000ff", "yellow": "#ffff00", "cyan": "#00ffff",
    "magenta": "#ff00ff", "white": "#ffffff", "black": "#000000", "grey": "#808080", "orange": "#ffa500",
    "brown": "#a52a2a", "purple": "#800080", "pink": "#ffc0cb", "violet": "#ee82ee", "dark red": "#8b0000",
    "dark green": "#006400", "dark blue": "#00008b", "dark cyan": "#008b8b", "dark magenta": "#8b008b",
    "dark orange": "#ff8c00", "dark grey": "#404040", "light grey": "#c0c0c0", "light blue": "#add8e6",
    "light green": "#90ee90", "light cyan": "#e0ffff", "light yellow": "#ffffe0", "light pink": "#ffb6c1"}


def load(path):
    with open(path, encoding="utf-8-sig") as handle:
        return json.load(handle)


def fold(text):
    """The words of a name: lower case, and every run of anything but a letter or a digit one blank. What the
    likeness of two names (difflib) is measured on."""
    return re.sub(r"[^a-z0-9]+", " ", text.lower()).strip()


def squeeze(text):
    """The letters and digits of a name and nothing else: two names are EQUAL when these are, so a blank, a
    hyphen or a comma more or less does not make a name another name."""
    return re.sub(r"[^a-z0-9]+", "", text.lower())


def fold_colour(name):
    """A colour name as Katana compares it: lower case, `_` and `-` as blanks, `gray` as `grey` at the end."""
    folded = name.lower().replace("_", " ").replace("-", " ").strip()
    return folded[:-4] + "grey" if folded == "gray" or folded.endswith(" gray") else folded


def clean_label(text):
    """A reference legend label less its lead-ins: a "[KEY]" in brackets, then a single capital and a word of two
    to eight capitals and digits ("[AB] E WALLS Stone wall face" is "Stone wall face")."""
    text = re.sub(r"^\[[^\]]*\]\s*", "", text.strip())
    return re.sub(r"^[A-Z]\s+[A-Z0-9]{2,8}\s+", "", text)


LEAD_IN = re.compile(r"^(?:[A-Z]\s+)?[A-Z0-9]{2,}\s+(?=\S)")


def without_lead_ins(name):
    """Every shorter form of a reference definition name that taking a lead-in off its front gives: a word of
    two or more capitals and digits, with or without a single capital before it, when more follows. "E WALL Stone
    Line" gives ["Stone Line"], "AB CD Peg" gives ["CD Peg", "Peg"], and "Stone Line" gives nothing."""
    out, rest = [], name.strip()
    while True:
        shorter = LEAD_IN.sub("", rest, count=1)
        if shorter == rest:
            return out
        out.append(shorter)
        rest = shorter


def definitions(document):
    return document.get("linestyles", []) + document.get("symbols", [])


def rules(document):
    return document.get("codes", [])


def colour_values(document):
    """Every colour VALUE a document draws in: its table's, and the standard value of each standard name a rule
    of it uses that its table does not define."""
    table = {fold_colour(name): value.lower() for name, value in document.get("colours", {}).items()}
    values = set(table.values())
    for r in rules(document):
        for name in (r.get("colour"), (r.get("symbol") or {}).get("colour"), (r.get("text") or {}).get("colour")):
            folded = fold_colour(name) if name else ""
            if folded and folded not in table and folded in STANDARD_COLOURS:
                values.add(STANDARD_COLOURS[folded])
    return values


def communications_colours(document):
    """The colour names of the feature rules whose key says `utility.type=telecommunications` in an attributes
    rule: the document's communications colour, found from the document alone."""
    typed = {r["key"] for r in rules(document) for a in r.get("attributes", [])
             if a.get("name") == "utility.type" and a.get("value") == SERVICE_WORD}
    return {r["colour"] for r in rules(document)
            if r["key"] in typed and r.get("sets") == "feature" and r.get("colour")}


def attribute_names(document):
    return {a["name"] for r in rules(document) for scope in ATTRIBUTE_SCOPES for a in r.get(scope, [])
            if a.get("name")}


def text_styles(document):
    return {r["text"]["style"] for r in rules(document) if isinstance(r.get("text"), dict) and r["text"].get("style")}


def strokes_of(definition):
    return [list(s) if not isinstance(s, dict) else s for s in definition.get("strokes", [])]


# ---- shapes ----------------------------------------------------------------------------------------------------
# A stroke list says how a drawing is made, and a shape is the drawing. `marks_of` reads a definition the way
# Katana's renderer does (cad::flattenDefinition: every point is the written one less the definition's `origin`
# and times its `factor`; a move lifts the pen, a draw is a straight piece from where the pen stands, and a
# circle, an arc, a dot and a text are about the pen and do not move it). `drawing_of` then takes out what a pen
# would not show: the cuts in a line that was written in pieces, a mark written twice, a dot on a line. What is
# left no longer says in which order, in which direction, in how many pieces or in which colour it was made.

DRAWN, MOVED, SCALED = "as drawn", "moved", "scaled"
# Two numbers are the same when they are less than this apart: the third decimal as drawn and moved (a file of
# this library has no finer number, so two of its numbers that differ are apart), and the second in the unit box.
STEPS = {DRAWN: 0.001, MOVED: 0.001, SCALED: 0.01}
GUARD = 1e-9   # 1.001 - 1 is a hair under 0.001 in binary, and a whole step apart is apart
# A point is ON a line, a ring or an arc when it is nearer than this to it, and two pieces are on ONE straight
# line when their straight lines pass this near each other and point the same way to POINTING radians. Both are
# room for the arithmetic and no tolerance: what a file writes on a line is on it.
ON = 1e-6
POINTING = 1e-9
# The eight symmetries of the square: (quarter turns counter-clockwise, mirrored in the y axis first).
SYMMETRIES = [(turns, mirrored) for mirrored in (False, True) for turns in range(4)]


def marks_of(definition):
    """The marks a definition's stroke list draws, each a tuple of its kind, its point and what else it has:
         ("line", x0, y0, x1, y1)              a straight piece; which end comes first says nothing
         ("circle", x, y, radius)              an arc of a whole turn or more is one too
         ("arc", x, y, radius, start, sweep)   counter-clockwise from `start` (0 to 360) through `sweep` degrees,
                                               whichever way round it was written
         ("dot", x, y)                         whatever its radius; a draw that goes nowhere is one too
         ("text", x, y, height, characters)
    A point is where the renderer puts it: the written point less the definition's `origin`, times its `factor`.
    A radius is a length whatever its sign, times the factor too. A text's height is the written one: the
    renderer sizes a text by where the definition is put and not by its factor. A pen stroke makes no mark."""
    origin = [float(v) for v in definition.get("origin", (0.0, 0.0))]
    factor = float(definition.get("factor", 1.0))

    def place(x, y):
        return ((float(x) - origin[0]) * factor, (float(y) - origin[1]) * factor)

    out = []
    pen = place(0.0, 0.0)
    for s in strokes_of(definition):
        kind = s[0]
        if kind == "move":
            pen = place(s[1], s[2])
        elif kind == "draw":
            to = place(s[1], s[2])
            out.append(("dot",) + pen if to == pen else ("line",) + pen + to)
            pen = to
        elif kind == "circle":
            out.append(("circle",) + pen + (abs(float(s[1])) * factor,))
        elif kind == "arc":
            low, high = sorted((float(s[2]), float(s[3])))
            if high - low >= 360.0:
                out.append(("circle",) + pen + (abs(float(s[1])) * factor,))
            else:
                out.append(("arc",) + pen + (abs(float(s[1])) * factor, low % 360.0, high - low))
        elif kind == "dot":
            out.append(("dot",) + pen)
        elif kind == "text":
            out.append(("text",) + pen + (float(s[1].get("height", 0)), s[1].get("text", "")))
    return out


def gathered(items, key, room):
    """The items in groups: put in the order of `key`, and cut wherever two neighbours are `room` or more apart."""
    out = []
    for item in sorted(items, key=key):
        if out and key(item) - key(out[-1][-1]) < room:
            out[-1].append(item)
        else:
            out.append([item])
    return out


def joined(lines):
    """Lines, each (x0, y0, x1, y1), with every set of them that lies on one straight line and touches or
    overlaps as ONE line: a straight run chained from pieces is the run, and a line drawn over a part of itself
    is the line. Pieces with a gap between them stay two lines: a gap is no ink."""
    pieces = []
    for line in lines:
        (x0, y0), (x1, y1) = sorted((line[:2], line[2:]))   # from its lower left end: pieces of one line point one way
        length = math.hypot(x1 - x0, y1 - y0)
        ux, uy = (x1 - x0) / length, (y1 - y0) / length
        # The way it points, how far its straight line passes from the origin, where along that it starts and
        # ends, and its two ends as they are written.
        pieces.append((math.atan2(uy, ux), x0 * uy - y0 * ux, x0 * ux + y0 * uy, x1 * ux + y1 * uy, (x0, y0), (x1, y1)))
    out = []
    for one_way in gathered(pieces, lambda piece: piece[0], POINTING):
        for one_straight_line in gathered(one_way, lambda piece: piece[1], ON):
            run = None   # (where it ends, its first end, its last end) of the pieces joined so far
            for _, _, starts, ends, first, last in sorted(one_straight_line, key=lambda piece: piece[2]):
                if run is not None and starts < run[0] + ON:   # it starts before the run ends
                    if ends > run[0]:
                        run = (ends, run[1], last)
                    continue
                if run is not None:
                    out.append(run[1] + run[2])
                run = (ends, first, last)
            out.append(run[1] + run[2])
    return out


def inked(point, marks):
    """Whether a point lies on a line, a ring or an arc among the marks: where a pen has already been."""
    px, py = point
    for m in marks:
        if m[0] == "line":
            dx, dy = m[3] - m[1], m[4] - m[2]
            length = math.hypot(dx, dy)
            off = ((px - m[1]) * dy - (py - m[2]) * dx) / length
            along = ((px - m[1]) * dx + (py - m[2]) * dy) / length
            if abs(off) < ON and -ON < along < length + ON:
                return True
        elif m[0] in ("circle", "arc") and m[3] > ON and abs(math.hypot(px - m[1], py - m[2]) - m[3]) < ON:
            if m[0] == "circle":
                return True
            # How far round from the arc's start the point is, with the room ON is at this radius.
            turn = (math.degrees(math.atan2(py - m[2], px - m[1])) - m[4]) % 360.0
            room = math.degrees(ON / m[3])
            if turn < m[5] + room or turn > 360.0 - room:
                return True
    return False


def drawing_of(definition):
    """What a definition draws: its marks, less what makes no difference to the drawing. Lines that lie on one
    straight line and touch or overlap are one line; a mark written twice is one mark; and a dot that lies on a
    line, a ring or an arc of its own definition is no mark, since Katana draws a dot as one point of the pen's
    width and the pen has been there."""
    marks = marks_of(definition)
    out = [("line",) + line for line in joined([m[1:] for m in marks if m[0] == "line"])]
    rest = []
    for m in marks:
        if m[0] != "line" and m not in rest:
            rest.append(m)
    out += [m for m in rest if m[0] in ("circle", "arc")]
    return out + [m for m in rest if m[0] == "text" or (m[0] == "dot" and not inked(m[1:3], out))]


def strokes_to_draw(drawing):
    """How many strokes the plain list has that draws a drawing: a move and a stroke for each ring, arc, dot and
    text; a draw for each line; and a move for each PATH of lines, a path being lines drawn end to end without
    lifting the pen, in as few paths as draw every line once. Lines that meet at their ends hang together, and
    a set that hangs together takes one path for every two ends where an odd number of its lines meet, and one
    when there is no such end (it closes on itself). A dash is 2, a plus 4, a square 5, a ring 2; a square
    with both diagonals is 8, three lines meeting at each of its four corners."""
    ends, meeting, group = [], [], []   # each point lines end at; how many end there; what it hangs together with

    def end(x, y):
        for at, known in enumerate(ends):
            if abs(known[0] - x) < ON and abs(known[1] - y) < ON:
                return at
        ends.append((x, y))
        meeting.append(0)
        group.append(len(group))
        return len(ends) - 1

    def root(at):
        while group[at] != at:
            at = group[at]
        return at

    lines = [m for m in drawing if m[0] == "line"]
    for m in lines:
        a, b = end(m[1], m[2]), end(m[3], m[4])
        meeting[a] += 1
        meeting[b] += 1
        group[root(a)] = root(b)
    odd = {}
    for at, count in enumerate(meeting):
        odd[root(at)] = odd.get(root(at), 0) + count % 2
    paths = sum(max(1, count // 2) for count in odd.values())
    return 2 * (len(drawing) - len(lines)) + len(lines) + paths


def compared(document):
    """[(definition, its drawing)] for the definitions of a document whose shapes are compared: those that more
    than TRIVIAL strokes draw. The count is of the drawing and not of the list, so a dash written as ten pieces
    is left out as a dash is, and a ring with two chords is compared however few strokes wrote it."""
    drawn = [(d, drawing_of(d)) for d in definitions(document)]
    return [(d, drawing) for d, drawing in drawn if strokes_to_draw(drawing) > TRIVIAL]


def turned(marks, quarter_turns, mirrored):
    """The marks under one of the eight symmetries of the square: mirrored in the y axis when asked, then turned
    counter-clockwise by `quarter_turns` quarter turns. A text moves with its point and keeps its characters."""
    def point(x, y):
        if mirrored:
            x = -x
        for _ in range(quarter_turns):
            x, y = -y, x
        return (x, y)

    out = []
    for m in marks:
        if m[0] == "line":
            out.append(("line",) + point(m[1], m[2]) + point(m[3], m[4]))
        elif m[0] == "arc":
            # Mirrored, an arc runs the other way round, so it starts where it ended.
            start = 180.0 - m[4] - m[5] if mirrored else m[4]
            out.append(("arc",) + point(m[1], m[2]) + (m[3], (start + 90.0 * quarter_turns) % 360.0, m[5]))
        else:
            out.append((m[0],) + point(m[1], m[2]) + m[3:])
    return out


def placed(marks, how):
    """The marks as they are compared `how`, as (the box round their points, {kind: [marks without the kind]}):
    DRAWN as they are; MOVED so that the lower left corner of the box is the origin; SCALED moved, and then
    sized so that the larger side of the box is 1. A shape whose marks are all about one point has a box of no
    size, and is sized by its largest radius or text height instead. An arc is given as its centre, its radius
    and the three points where it starts, is half way and ends, so that its angles are held as closely as its
    lengths are and no closer: two angles are the same when no point of the arc is a step apart for them."""
    points = [(m[1], m[2]) for m in marks] + [(m[3], m[4]) for m in marks if m[0] == "line"]
    if not points:
        return (0.0, 0.0, 0.0, 0.0), {}
    low_x, low_y = min(p[0] for p in points), min(p[1] for p in points)
    high_x, high_y = max(p[0] for p in points), max(p[1] for p in points)
    dx, dy = (0.0, 0.0) if how == DRAWN else (low_x, low_y)
    size = 1.0
    if how == SCALED:
        size = (max(high_x - low_x, high_y - low_y)
                or max((m[3] for m in marks if m[0] in ("circle", "arc", "text")), default=0.0) or 1.0)

    def at(x, y):
        return ((x - dx) / size, (y - dy) / size)

    by_kind = {}
    for m in marks:
        if m[0] == "line":
            mark = at(m[1], m[2]) + at(m[3], m[4])
        elif m[0] == "dot":
            mark = at(m[1], m[2])
        elif m[0] == "arc":
            mark = at(m[1], m[2]) + (m[3] / size,)
            for part in (0.0, 0.5, 1.0):
                angle = math.radians(m[4] + m[5] * part)
                mark += at(m[1] + m[3] * math.cos(angle), m[2] + m[3] * math.sin(angle))
        else:   # a circle's radius and a text's height are sized with the shape
            mark = at(m[1], m[2]) + (m[3] / size,) + m[4:]
        by_kind.setdefault(m[0], []).append(mark)
    return at(low_x, low_y) + at(high_x, high_y), by_kind


def alike(kind, a, b, step):
    """Whether two placed marks of one kind are one mark: every number less than a step apart, a line's ends
    either way round, a text's characters the same."""
    def near(p, q):
        return all(abs(u - v) < step - GUARD for u, v in zip(p, q))

    if kind == "line":
        return near(a, b) or near(a, b[2:] + b[:2])
    if kind == "text":
        return a[3] == b[3] and near(a[:3], b[:3])
    return near(a, b)


def one_shape(ours, theirs, step):
    """Whether two placed drawings are one shape: every mark of each is a mark of the other. The boxes are
    compared first, which decides most pairs: where the marks pair off, so do their points."""
    (our_box, our_marks), (their_box, their_marks) = ours, theirs
    if not our_marks or our_marks.keys() != their_marks.keys():
        return False
    if not all(abs(u - v) < step - GUARD for u, v in zip(our_box, their_box)):
        return False
    return (all(any(alike(kind, m, n, step) for n in their_marks[kind])
                for kind in our_marks for m in our_marks[kind])
            and all(any(alike(kind, n, m, step) for m in our_marks[kind])
                    for kind in their_marks for n in their_marks[kind]))


def shapes_in_common(reference, candidate):
    """{DRAWN: [names], MOVED: [names], SCALED: [names]}: the candidate's definitions that draw, under one of
    the eight symmetries, what a definition of the reference draws. Every definition of the reference takes
    part, with those of the candidate that `compared` gives: a drawing of the reference that is one of those
    takes as many strokes as it, however few wrote it."""
    their_drawings = [drawing_of(d) for d in definitions(reference)]
    # Each of the candidate's under the eight symmetries; the reference's are compared as they are.
    ours = [(d["name"], [turned(drawing, turns, mirrored) for turns, mirrored in SYMMETRIES])
            for d, drawing in compared(candidate)]
    found = {}
    for how in (DRAWN, MOVED, SCALED):
        theirs = {}   # by the kinds of mark a shape has: two shapes of other kinds are never one
        for drawing in their_drawings:
            shape = placed(drawing, how)
            theirs.setdefault(frozenset(shape[1]), []).append(shape)
        found[how] = []
        for name, eight in ours:
            views = [placed(drawing, how) for drawing in eight]
            if any(one_shape(view, shape, STEPS[how])
                   for shape in theirs.get(frozenset(views[0][1]), ()) for view in views):
                found[how].append(name)
    return found


def scan(reference, candidate):
    """The whole comparison of two documents, as a dict: `exact` maps each thing compared to the candidate's own
    names that are also the reference's (every list is to be empty); `excused` is the candidate's one white;
    `near_names`, `near_labels` and `same_key_labels` are figures to read; `failing` is the sum of `exact`."""
    exact = {}
    # What the reference calls things: its definition names whole, those names less a lead-in, and its legend
    # labels. A name of the candidate is compared with all three, and so is a label of it.
    ref_names = {squeeze(d["name"]) for d in definitions(reference)} - {""}
    ref_tails = {squeeze(t) for d in definitions(reference) for t in without_lead_ins(d["name"])} - ref_names - {""}
    ref_labels = {squeeze(clean_label(r["comment"])) for r in rules(reference) if r.get("comment")} - {""}
    cand_names = [d["name"] for d in definitions(candidate)]
    cand_labels = sorted({r["comment"] for r in rules(candidate) if r.get("comment")})
    exact["definition names"] = [n for n in cand_names if squeeze(n) in ref_names]
    exact["definition names and reference names less their lead-in"] = [n for n in cand_names
                                                                        if squeeze(n) in ref_tails]
    exact["definition names and reference legend labels"] = [n for n in cand_names if squeeze(n) in ref_labels]

    ref_groups = ({squeeze(d.get("group", "")) for d in definitions(reference)}
                  | {squeeze(r.get("group", "")) for r in rules(reference)})
    cand_groups = {d.get("group", "") for d in definitions(candidate)} | {r.get("group", "") for r in rules(candidate)}
    exact["group paths"] = sorted(g for g in cand_groups if g and squeeze(g) in ref_groups)

    ref_layers = {squeeze(r["layer"]) for r in rules(reference) if r.get("layer")}
    cand_layers = {r["layer"] for r in rules(candidate) if r.get("layer")}
    exact["layer paths"] = sorted(layer for layer in cand_layers if squeeze(layer) in ref_layers)

    exact["legend labels"] = [c for c in cand_labels if squeeze(c) in ref_labels]
    exact["legend labels and reference definition names"] = [c for c in cand_labels
                                                             if squeeze(c) in ref_names or squeeze(c) in ref_tails]

    ref_colour_names = {squeeze(n) for n in reference.get("colours", {})}
    exact["colour names"] = sorted(n for n in candidate.get("colours", {}) if squeeze(n) in ref_colour_names)

    # Pure white is excused for ONE colour of the candidate, its communications colour, and for no other.
    ref_values = colour_values(reference)
    communications = communications_colours(candidate)
    excused = [n for n, v in candidate.get("colours", {}).items() if v.lower() == WHITE and communications == {n}]
    exact["colour values (but the one white of communications)"] = [
        n for n, v in candidate.get("colours", {}).items() if v.lower() in ref_values and n not in excused]

    exact["attribute names"] = sorted(attribute_names(candidate) & attribute_names(reference))
    exact["text styles"] = sorted(text_styles(candidate) & text_styles(reference))

    for how, names in shapes_in_common(reference, candidate).items():
        exact["shapes, " + how] = names

    # Near and not equal: against everything the reference calls a thing, as words.
    said = ({fold(d["name"]) for d in definitions(reference)}
            | {fold(t) for d in definitions(reference) for t in without_lead_ins(d["name"])}
            | {fold(clean_label(r["comment"])) for r in rules(reference) if r.get("comment")}) - {""}
    said_list, said_squeezed = sorted(said), {squeeze(text) for text in said}

    def near(text):
        return (squeeze(text) not in said_squeezed
                and bool(difflib.get_close_matches(fold(text), said_list, n=1, cutoff=0.9)))

    near_names = [n for n in cand_names if near(n)]
    near_labels = [c for c in cand_labels if near(c)]

    # The candidate's labels that are near the reference's label for the same key: read, not failed (see above).
    ref_label_of = {}
    for r in rules(reference):
        if r.get("comment"):
            ref_label_of.setdefault(r["key"], set()).add(fold(clean_label(r["comment"])))
    same_key = sorted({r["comment"] for r in rules(candidate) if r.get("comment")
                       for label in ref_label_of.get(r["key"], ())
                       if difflib.SequenceMatcher(None, label, fold(r["comment"])).ratio() >= 0.85})

    ref_keys = {r["key"] for r in rules(reference)}
    cand_keys = {r["key"] for r in rules(candidate)}
    return {"exact": exact, "excused": excused, "near_names": near_names, "near_labels": near_labels,
            "same_key_labels": same_key, "failing": sum(len(items) for items in exact.values()),
            "keys": (len(ref_keys & cand_keys), len(ref_keys), len(cand_keys)),
            "definitions": (len(definitions(reference)), len(definitions(candidate)),
                            len(definitions(candidate)) - len(compared(candidate)))}


def render(result, limit):
    out = ["reference definitions=%d candidate definitions=%d (%d of the candidate's are drawn by %d strokes or "
           "fewer and are left out of the comparison of shapes)" % (result["definitions"] + (TRIVIAL,)),
           "survey code keys in common=%d (reference %d, candidate %d)" % result["keys"],
           "white excused for the communications colour=%d" % len(result["excused"])]

    def listed(title, items):
        out.append("%s=%d" % (title, len(items)))
        out.extend("  " + item for item in items[:limit])

    for what, items in result["exact"].items():
        listed("%s in common" % what, items)
    listed("near definition names (>=0.9)", result["near_names"])
    listed("near legend labels (>=0.9)", result["near_labels"])
    listed("legend labels near the reference's for the same key (>=0.85, not a failure)", result["same_key_labels"])
    out.append("originality=%s" % ("PASS" if result["failing"] == 0 else "FAIL"))
    return "\n".join(out)


def main(argv):
    limit = 10
    if "--limit" in argv:
        at = argv.index("--limit")
        limit = int(argv[at + 1])
        argv = argv[:at] + argv[at + 2:]
    paths = [a for a in argv if not a.startswith("--")]
    if len(paths) != 2:
        sys.stderr.write(__doc__)
        return 2
    try:
        reference, candidate = load(paths[0]), load(paths[1])
    except (OSError, ValueError) as error:
        sys.stderr.write("originality: %s\n" % error)
        return 2
    result = scan(reference, candidate)
    print(render(result, limit))
    return 0 if result["failing"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
