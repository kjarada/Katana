"""Katana Standard survey codes: the vocabulary, its grammar, and what every code expands to.

    python tools/katana_standard/codes.py --check     validate every code; exit 1 on a problem
    python tools/katana_standard/codes.py --summary   codes and rules by class
    python tools/katana_standard/codes.py --grammar   print this grammar

`SECTIONS` is the whole vocabulary as data: a class, a group and the codes in it. `CODES` is the same list flat,
one dict per code. A code is a KEY and the rules the key makes. The assembler (make_katana_standard.py) turns each
code into the rules of `expand_rules()` below and writes them, in this order, as the `codes` of the
customisation file. The rules are written with the format's own words (layer, colour, weight, linestyle,
symbol, text, pipe, attributes, surface) and `meaning` is the sentence that makes the library readable: what the
code is for. A rule's `comment` is the legend label.

THE GRAMMAR

1. THE KEYS ARE ANOTHER LIBRARY'S, AND NOTHING ELSE OF THEM IS. Katana has two built-in customisations, this one
   and the NSW one (resources/customisation/nsw.customisation.json). By the owner's decision they use the SAME
   survey code keys, byte for byte, so that a field file coded for either draws in both. The keys are all that
   is taken: every layer, colour, weight, linestyle, symbol, text style, group and legend label, and every
   sentence of the meanings and of this document, is Katana's own. No key is ours: the keys are the other
   library's identifiers, reused for field-data compatibility, and the licence notes say so (LICENSING.md).
2. A key is an exact code or a prefix. `PBMK` matches the code PBMK and no other; `AC*` matches every code that
   begins AC (AC, AC1, AC01, ACE); the bare `*` matches every code. A code meets every key that fits it, and
   for each field the most specific rule that says anything about it wins: an exact key before a prefix, a
   longer prefix before a shorter, `*` last (`SurveyMap::lookup`). So the keys `PT04` and `PT04*` are two parts of
   one code: the first gives a trunk its symbol and the second keeps it out of the ground surface; and `Z0*`
   is more specific than `Z*`. A key is matched as written, in capitals: `ac1` matches no rule (`CODE EXPLAIN
   ac1` says the key `AC*` differs only in letter case), so set the data collector to send capitals.
3. Keys are capital letters and digits. A digit that is part of a key belongs to it: `U1*` is a 225 mm stormwater
   pipe and `PT04` a 400 mm trunk. The STRING NUMBER follows the key, and the linework reads it: `WM1` and `WM2`
   are two water mains, both answered by `WM*`. The keys `1`, `2`, `3`, `4`, `5`, `7` and `8` are text sizes (1.5,
   2.5, 3.5, 5, 7 and 10 mm of paper, and the parcel label's 2.5), and `1*` to `8*` give that text its layer.
4. Control words follow the code, after a space: `ST` first point, `END` last point, `CL` last point and close,
   `BC` and `EC` begin and end a curve (three points fix an arc), `JPN n` also join to point number n, `RECT`
   the third point of a rectangle. These are Katana's default spellings; the library says nothing about them,
   so loading it never resets a colleague's own. The first token of a point's code is always the key, so a
   point coded `CL` is a string called CL, answered by the key `CL*`, even though four keys (`BC*`, `CL*`,
   `EC*` and `ST*`) are spelled like a default control word: a control word is read only after the key.
5. There is no left or right suffix: the rule engine matches a key exactly or by prefix and has nothing else.
   A handed pair is two keys (`CV*` and `CY*` chevrons, `DA*` and `DC*` dragons teeth, `LH*` and `LR*` broken
   barrier lines, `HE*` and `HR*` wall hatching). Otherwise handedness is by walking direction: ticks, barbs,
   triangles, scallops and chevrons fall to the LEFT of travel. Symbols are drawn upright and are not turned
   to a bearing (Katana reads a rule's rotation and does not yet apply it), so a symbol's position is its
   message.
6. The STRUCTURE of every key is the other library's: the same ordered kinds of rule (feature, symbol, text,
   pipe, vertexPipe, segmentPipe, attributes, vertexAttributes, surface), the same draw mode (line or point),
   the same hide and surface flags, text rules where it has text rules and pipe rules where it has pipe rules,
   so that one field file makes the same kinds of entity (points, lines, text, pipes, surface members) in
   either library. `tools/katana_standard/parity.py` compares the two files and the tests hold it. What a
   key LOOKS like is Katana's.
7. One clean rule set per key. The other library repeats rules and shadows them, names pen colours that
   resolve to nothing, and draws a symbol through a linestyle slot; this one writes one rule of each kind per
   key, resolves every name, and says a symbol in a symbol rule. Where the two differ for that reason the key
   is listed in `parity_exceptions.json` with the reason, and `parity.py` fails the day an entry explains
   nothing.
8. What a code sets, in this order. `feature`: layer, colour, draw (line or point, none for a layer-only key),
   linestyle (lines only), weight, group and comment. `symbol` for points and for lines that carry a symbol at
   every vertex. `text` for the text keys. `pipe`, `vertexPipe` and `segmentPipe` for the bare `*`. `attributes`
   and `vertexAttributes`. `surface`, written where the other library writes it, and false there every time:
   it keeps what is not the ground (a mark, a trunk, a design string, a pipe) out of a ground surface, and no
   key says it belongs in one. Layers are three levels, lower case
   (`class/group/family`), and a family is what one switches on and off together.
9. Attributes. An attribute with an EMPTY value is a PROMPT: the code asks for it. One with a value is applied.
   Names carry their unit (`Depth (m)`, `Diameter (mm)`). The other library's attribute rules are kept as
   they are (a model zone to check, a point code), and one more is added to a code that stands for a buried or
   strung service, last: `utility.type` set to the service word, then the prompts Owner, Depth (m) (Height (m)
   for a line strung overhead) and Condition; a pipe also Material and Diameter (mm), a box culvert Width (mm)
   and Height (mm). Where the code states the size (a 300 mm pipe, a culvert 1200 mm high) the size is applied.
   Those are the words Katana's utility tools read, so a coded string is already typed for UTILITY DRAW.
   Stormwater pits and pipes are services too, though they are catalogued with drainage. The prompts use the
   library's own names, not `utility.owner`, so an empty prompt never reads as "recorded". The added rule makes
   no entity and `parity.py` counts it apart.
10. `comment` is the LEGEND LABEL: a noun phrase in sentence case, 1 to 36 characters, which the plot legend
   prints in capitals. When several codes share one linestyle the legend takes the linestyle's name instead,
   unless every coded entity agrees on the label. The longer text is `meaning`.
11. Weights, in millimetres of pen: 0.13 hairline, 0.18 fine, 0.25 light, 0.35 medium, 0.50 strong (0.70 is
   reserved). The hairline is for hatching, joints, hidden detail, tree outlines, text and design strings;
   nothing in the planting or ground classes is lighter than 0.18, since those colours are the faintest on
   paper. Katana stores `weight` and does not yet apply it; the scale is still the documented intent.
12. What Katana applies today and what it only stores. `CODE` applies layer, colour, linestyle, symbol name and
   size, and the string attributes (a value starting with `$` is deferred, not written), and the legend
   prints the comment. It stores, lists and lints, and does not yet apply: weight, group, hide, surface, text
   rules, pipe rules (justify, shape, sizes taken from the attributes), rotation, offset and raise. The rules
   are written for the day it does.
13. Load the library alone, or with Replace. Merged onto another customisation, a broader prefix rule there can
   supply a field these rules leave unsaid (a symbol, a text), because the most specific rule wins field by
   field. Merged onto the other library it shares every key, so each key's rules are replaced or added to field
   by field and the result is neither.
14. The classes. The 13 classes and their groups are this library's own way of arranging the keys for a reader
   and for the layer tree: a class letter is a label in the catalogue and is not part of any key. The keys that
   the other library labels as design strings (road, lane, earthworks, kerb, bridge, drainage and barrier
   design, master strings) are class D, Design and Setting Out; the bare `*` belongs to no class.

A field-book example is not given for a code: a code is a key and a string number, `WM1 ST, WM1, WM1 END`.
"""
import re
import sys

GRAMMAR = (__doc__ or "").split("THE GRAMMAR", 1)[-1].strip()

# The six pen weights in millimetres: (text, role, what it is for).  Katana's own weight is TEXT.
WEIGHTS = [
    ("0.13", "hairline", "hatching, joints, hidden detail, tree outlines, design strings, text"),
    ("0.18", "fine", "vegetation, ground shots and levels, intermediate contours, markings, minor services"),
    ("0.25", "light", "everyday detail: fences, paths, branches, structures"),
    ("0.35", "medium", "primary detail: kerbs, building walls, service mains, index contours"),
    ("0.50", "strong", "title boundary, bridge decks, limit of survey"),
    ("0.70", "bold", "reserved; not used by a code in edition 1"),
]

# The classes and their groups: (letter, name, layer root, summary, [(group, layer sub, summary), ...]).  The
# names are the definition groups too ("Class/Group"), so a definition, a rule and a layer are found in one
# place. A class letter is a label for a reader; no key is built from it.
TAXONOMY = [
    ("C", "Boundaries and Cadastre", "cadastre",
     "Where one owner's land ends and the next begins, and the marks that prove it.", [
         ("Lot Boundaries", "lots", "Title, compiled, digital, proposed and stratum boundaries, and the lines that join them."),
         ("Easements and Reserves", "easements", "Land that carries another party's right or a public purpose."),
         ("Boundary Marks", "marks", "Pegs, pins, nails and the other marks found or placed at corners, and the marks that witness them."),
     ]),
    ("R", "Roads and Pavements", "roads",
     "The made surface: its edges, its paint and the paths beside it.", [
         ("Carriageway", "carriageway", "The centre, the edges and the offsets of the road itself, sealed or not."),
         ("Markings", "markings", "Painted lines, arrows, crossings and chevrons, and the raised markers set in the road."),
         ("Paths and Access", "paths", "Footpath and driveway edges, and the ramps beside them."),
     ]),
    ("S", "Street Furniture and Signs", "street",
     "What people read, sit on, light their way by or steer round.", [
         ("Signs", "signs", "Sign posts and sign outlines, and the lines that carry a speed limit."),
         ("Lighting", "lighting", "Street, garden and hanging lights, and the arms that reach out to them."),
         ("Posts and Barriers", "barriers", "Bollards, guard rails, and cable and concrete barriers."),
         ("Amenities", "amenities", "Bins, seats, tables, shelters, letterboxes, parking meters and fireplaces."),
         ("Traffic Control", "traffic", "Signal posts and controller cabinets, and enforcement cameras."),
     ]),
    ("K", "Kerbs and Drainage", "drainage",
     "Where the street meets the water: kerbs, pits, pipes and channels.", [
         ("Kerbs", "kerbs", "Kerb top and back, gutter lines, medians and dish drains."),
         ("Pits and Structures", "structures", "Gully, junction and subsoil pits, headwalls and wing walls, and the lines that outline them."),
         ("Pipes and Culverts", "pipes", "Stormwater pipes and box culverts by size, and the inverts and obverts shot in them."),
         ("Channels and Waterways", "channels", "Open drains, flumes, fords and watercourses."),
     ]),
    ("T", "Rail and Transit", "rail",
     "Track, the lineside fittings and the platforms beside it.", [
         ("Track", "track", "Rails, centreline, formation edge, and the boxes and troughs beside the track."),
         ("Overhead and Signals", "overhead", "Signals, overhead wire stanchions and signal cable troughs."),
         ("Platforms", "platforms", "Platform edges and ramps."),
     ]),
    ("U", "Utilities", "utilities",
     "Everything buried or strung that carries a service, one family per service.", [
         ("Water", "water", "Drinking water mains, connections, valves, hydrants, meters and pits."),
         ("Sewer", "sewer", "Gravity and pressure sewers, connections, maintenance holes, vents and pump stations."),
         ("Gas", "gas", "Gas mains, regulators, meters, valve pits, vents and markers."),
         ("Electricity", "electricity", "Cables, overhead lines, ducts, poles, transformers, pillars and pits."),
         ("Communications", "telecommunications", "Telephone, data and fibre cables, ducts, pits, pillars, poles and masts."),
         ("Recycled Water", "recycled-water", "Recycled water mains, valves, meters, hydrants and markers."),
         ("Fire Service", "fire-service", "Fire hydrants."),
         ("Fuel", "fuel", "Fuel pipelines, markers, dispensers and tanks."),
         ("Traffic Systems", "its", "Cables, detectors and junction boxes for signals and cameras."),
         ("Unknown Services", "unknown", "What is there and has not been named: lines, pits, covers and poles."),
     ]),
    ("V", "Vegetation and Landscape", "vegetation",
     "Trees and the edges of planted and natural ground cover.", [
         ("Trees", "trees", "Trunks by diameter, canopies by spread, and outlines of foliage."),
         ("Ground Cover", "ground", "The edges of timber, scrub, grass, crops, orchards and garden beds."),
     ]),
    ("B", "Buildings and Structures", "buildings",
     "What stands on the ground: buildings, bridges, masts, tanks and steps.", [
         ("Buildings", "outline", "Walls, eaves, awnings, doors, slabs and floor levels."),
         ("Structures", "structures", "Bridge parts, piers, tanks, silos, pools and other built outlines."),
         ("Steps and Access", "access", "Outside stairs."),
     ]),
    ("F", "Fences and Walls", "fences",
     "What divides land: fences, walls, and the posts and gates between.", [
         ("Fences", "fence", "Fences of every kind, general, security and safety."),
         ("Walls", "wall", "Retaining walls."),
         ("Gates and Posts", "gate", "Gate leaves and fence or guide markers."),
     ]),
    ("G", "Terrain and Breaklines", "terrain",
     "The shape of the ground: shots, breaklines, contours, water edges and rock.", [
         ("Ground Points", "points", "Natural ground shots and spot heights."),
         ("Breaklines", "breaklines", "Banks, cuttings, ridges, scarps and the soft lines that shape a surface."),
         ("Contours", "contours", "Index, regular and extra contours, and their depression forms."),
         ("Water Edges", "water", "Shorelines, bank edges, swamps and water levels."),
         ("Rock and Cliff", "rock", "Rock outcrops and escarpment crests."),
     ]),
    ("M", "Survey Control and Annotation", "control",
     "The survey's own marks, its investigations and the words on the plan.", [
         ("Control Marks", "marks", "Bench marks, survey marks, stations, pegs, bolts and the marks a job sets and uses."),
         ("Investigation Points", "investigation", "Boreholes, test pits, potholes, seismic shots and trenches."),
         ("Text and Notes", "annotation", "Seven text sizes and the layer that holds them."),
         ("Plan Marks", "plan", "The limits of a survey and of the detail taken from photographs."),
     ]),
    ("X", "Miscellaneous", "miscellaneous",
     "Hazards, heritage, land use and the catch-alls.", [
         ("Hazards and Heritage", "hazards", "Hazards, heritage items, habitat and threatened communities."),
         ("General", "general", "A point to attend to, a crossing of two lines, and the survey check line."),
         ("Land Use Areas", "land-use", "The outlines of land by its use: built-up areas, cemeteries, golf courses, parks, sporting arenas and stockpiles."),
     ]),
    ("D", "Design and Setting Out", "design",
     "Strings that carry a design, not a survey: set out from them, do not read the ground from them.", [
         ("Road Design Strings", "road-design-strings", "Design strings of a road that the file does not name for any purpose."),
         ("Lane Marking Design", "lane-marking-design", "The designed lines of lanes, edges and junctions, to be painted."),
         ("Earthworks Design", "earthworks-design", "Batters, formation, interfaces and the design edges of cut and fill."),
         ("Kerb and Carriageway Design", "kerb-and-carriageway-design", "The designed lines of kerbs, gutters, hinges and carriageway edges."),
         ("Bridge Design", "bridge-design", "The designed lines of abutments and piers."),
         ("Drainage Design", "drainage-design", "Designed pipes, culverts, channels, drains and structures."),
         ("Barrier and Fence Design", "barrier-and-fence-design", "Designed guard rails, fences and bollards."),
         ("Master Strings", "master-strings", "The master lines of a design that the others are built from."),
         ("Design General", "design-general", "Designed paths, lights, walls and hazards that fit no other design group."),
     ]),
]

_GROUP = {g[0]: top[1] + "/" + g[0] for top in TAXONOMY for g in top[4]}
_CLASS_OF = {top[0]: top for top in TAXONOMY}

# Attributes written as whole numbers; every other attribute is text (its unit is in its name).
_INTEGER = frozenset({"Check Zone", "Diameter (mm)", "Height (mm)", "Width (mm)"})


def _attribute(spec):
    """'Name' is a prompt; 'Name=value' is applied."""
    name, _, value = spec.partition("=")
    return {"type": "integer" if name in _INTEGER else "text", "name": name, "value": value}


# ---- the constructors of a code's rules ----------------------------------------------------------------------

def _feature(draw, layer, colour, weight, comment, linestyle):
    rule = {"sets": "feature", "layer": layer, "colour": "katana " + colour, "weight": weight, "comment": comment}
    if draw:
        rule["draw"] = draw
    if linestyle:
        rule["linestyle"] = linestyle
    return rule


def line(layer, colour, weight, comment, linestyle="continuous"):
    """A feature rule that joins the points into a line. `colour` is the palette word after 'katana '."""
    return _feature("line", layer, colour, weight, comment, linestyle)


def point(layer, colour, weight, comment):
    """A feature rule that leaves the points as points."""
    return _feature("point", layer, colour, weight, comment, None)


def layer_only(layer, colour, weight, comment):
    """A feature rule that says only where it goes: the layer of the text codes."""
    return _feature(None, layer, colour, weight, comment, None)


def symbol(name, hide=False):
    """A symbol rule. `hide` keeps the point's symbol from being drawn; the library says it where the other
    library's lookup does (PBDY, PBMZ, PCRM and PPSM) and Katana does not yet apply it."""
    rule = {"sets": "symbol", "symbol": name}
    if hide:
        rule["hide"] = True
    return rule


def text(size, colour="note", justify_x="left", justify_y="bottom"):
    """A text rule: Standard style, millimetres of paper."""
    return {"sets": "text", "style": "Standard", "colour": "katana " + colour, "units": "paper", "size": size,
            "justifyX": justify_x, "justifyY": justify_y}


def pipe(justify, shape, size1, size2=None, kind="pipe"):
    """A pipe rule (`kind` pipe, vertexPipe or segmentPipe); a size names an attribute with a leading $."""
    rule = {"sets": kind, "justify": justify, "shape": shape, "size1": size1, "active": True}
    if size2:
        rule["size2"] = size2
    return rule


def attrs(*specs, kind="attributes"):
    """An attributes rule (`kind` attributes or vertexAttributes) from specs 'Name' or 'Name=value'."""
    return {"sets": kind, "attributes": [_attribute(s) for s in specs]}


def surface(flag):
    return {"sets": "surface", "surface": flag}


def code(key, meaning, *rules):
    return {"key": key, "meaning": meaning, "rules": list(rules)}


# ---- the vocabulary ------------------------------------------------------------------------------------------

SECTIONS = [

    # ---- every code (1) ------------------------------------------------------------------------------------------
    ("-", "", [
        code("*", "Any string that carries a Diameter attribute is also drawn as a round pipe section, measured "
             "down from its top (the obvert).",
             pipe("Obvert", "diameter", "$Diameter (mm)"),
             pipe("Obvert", "diameter", "$Diameter (mm)", kind="vertexPipe"),
             pipe("Obvert", "diameter", "$Diameter (mm)", kind="segmentPipe")),
    ]),

    # ---- Boundaries and Cadastre / Lot Boundaries (8) ------------------------------------------------------------
    ("C", "Lot Boundaries", [
        code("BC*", "A cadastral boundary compiled from plans and positioned to about one metre.",
             line("cadastre/lots/compiled", "boundary", "0.25", "Compiled boundary", "Compiled Boundary")),
        code("BM*", "A boundary taken from the digital cadastral map, whose position can be anywhere from one to "
             "twenty metres out.",
             line("cadastre/lots/compiled", "boundary", "0.18", "Digital boundary", "Digital Boundary")),
        code("BN*", "A cadastral boundary whose position is good to about half a metre.",
             line("cadastre/lots/compiled", "boundary", "0.25", "Boundary good to 0.5 m", "Estimated Boundary")),
        code("BO*", "A cadastral boundary whose position is good to about a tenth of a metre.",
             line("cadastre/lots/title", "boundary", "0.35", "Boundary good to 0.1 m")),
        code("BT*", "A title boundary measured on the ground and good to about two centimetres.",
             line("cadastre/lots/title", "boundary", "0.50", "Title boundary", "Title Boundary")),
        code("BY*", "A proposed boundary that marks land to be acquired.",
             line("cadastre/lots/special", "boundary", "0.35", "Boundary to be acquired",
                  "Acquisition Boundary")),
        code("BZ*", "The limit of a stratum parcel, which is fixed by a height or depth and not only by a line on "
             "the ground.",
             line("cadastre/lots/special", "boundary", "0.25", "Parcel stratum limit", "Stratum Parcel Edge")),
        code("JL*", "A join line between two boundary points, giving a bearing and distance; it is not itself a "
             "boundary.",
             line("cadastre/lots/join", "boundary", "0.13", "Boundary join line", "Survey Join Line")),
    ]),

    # ---- Boundaries and Cadastre / Easements and Reserves (1) ----------------------------------------------------
    ("C", "Easements and Reserves", [
        code("BE*", "The limit of an easement, land that carries another party's right of use.",
             line("cadastre/easements/general", "boundary", "0.25", "Easement boundary", "Easement Boundary")),
    ]),

    # ---- Boundaries and Cadastre / Boundary Marks (21) ------------------------------------------------------------
    ("C", "Boundary Marks", [
        code("PBDY", "A boundary mark of any kind, shot as a point.",
             point("cadastre/marks/found", "boundary", "0.25", "Cadastral corner mark"),
             symbol("Iron Pin", hide=True)),
        code("PBPG", "A peg set at a boundary corner, shot at its top.",
             point("cadastre/marks/found", "boundary", "0.25", "Peg at a boundary corner"),
             symbol("Corner Peg")),
        code("PBPG*", "Keeps boundary pegs out of the ground surface when the code carries a string number.",
             surface(False)),
        code("PCRM", "A mark set to reference cadastral corners, so that the corners can be re-established if their "
             "own marks are lost.",
             point("cadastre/marks/found", "boundary", "0.25", "Survey reference mark"),
             symbol("Fixed Survey Mark", hide=True)),
        code("PDHL", "A hole drilled in rock or concrete, with a chiselled wing cut beside it to make it easy to "
             "find.",
             point("cadastre/marks/found", "boundary", "0.25", "Winged drill hole"),
             symbol("Winged Drill Hole")),
        code("PDHL*", "Keeps winged drill holes out of the ground surface when the code carries a string number.",
             surface(False)),
        code("PGIN", "A galvanised iron nail driven into a hard surface as a survey mark.",
             point("cadastre/marks/found", "boundary", "0.25", "Galvanised iron nail"),
             symbol("Survey Spike")),
        code("PGIN*", "Keeps galvanised nails out of the ground surface when the code carries a string number.",
             surface(False)),
        code("PGPI", "A galvanised iron pipe driven in as a survey mark.",
             point("cadastre/marks/found", "boundary", "0.25", "Galvanised iron pipe"),
             symbol("Iron Pipe")),
        code("PGPI*", "Keeps galvanised pipes out of the ground surface when the code carries a string number.",
             surface(False)),
        code("PMSM", "A survey mark of a kind no other code covers, shot at its centre.",
             point("cadastre/marks/found", "boundary", "0.25", "Other survey mark"),
             symbol("Fixed Survey Mark")),
        code("PMSM*", "Keeps miscellaneous survey marks out of the ground surface.",
             surface(False)),
        code("PNAL", "A nail driven into a hard surface as a survey mark; each shot also records its code.",
             point("cadastre/marks/found", "boundary", "0.25", "Survey nail"),
             symbol("Survey Spike"),
             attrs("Point Code=PNAL", kind="vertexAttributes")),
        code("POCP", "A post standing at a boundary corner, taken as the corner mark.",
             point("cadastre/marks/found", "boundary", "0.25", "Fence corner post")),
        code("PPMK", "A permanent mark set in the ground to last, which later surveys can rely on to find a position.",
             point("cadastre/marks/found", "boundary", "0.25", "Permanent ground mark"),
             symbol("Fixed Survey Mark")),
        code("PRMB", "A reference mark set in a concrete block, used to re-establish a survey mark if it is lost.",
             point("cadastre/marks/reference", "boundary", "0.25", "Concrete reference block"),
             symbol("Corner Peg")),
        code("PRMB*", "Keeps concrete reference blocks out of the ground surface, since they fix a position and not "
             "the ground.",
             surface(False)),
        code("PRTR", "A tree used as a witness to a survey mark or a boundary corner, shown as a plain point.",
             point("cadastre/marks/reference", "boundary", "0.25", "Witness tree")),
        code("PSKI", "A spike or nail driven into a hard surface and used as a survey mark.",
             point("cadastre/marks/found", "boundary", "0.25", "Survey spike"),
             symbol("Survey Spike")),
        code("PSTA", "A star picket, a steel picket with a star-shaped section, driven in as a survey mark.",
             point("cadastre/marks/found", "boundary", "0.25", "Star picket mark"),
             symbol("Star Picket")),
        code("PSTA*", "Keeps star pickets out of the ground surface, since they mark a position and not the ground.",
             surface(False)),
    ]),

    # ---- Roads and Pavements / Carriageway (9) -------------------------------------------------------------------
    ("R", "Carriageway", [
        code("EJ*", "Outer edge of the graded formation that carries a road or track, before any pavement is laid.",
             line("roads/carriageway/edge", "pavement", "0.25", "Formation edge")),
        code("EK*", "Edge of a vehicle track, worn or cut into the ground and not made up as a road.",
             line("roads/carriageway/edge", "pavement", "0.18", "Track edge", "Unsealed Road Edge")),
        code("EM*", "Edge of the central median that separates opposing lanes of traffic.",
             line("roads/carriageway/edge", "pavement", "0.35", "Edge of a median strip"),
             attrs("Check Zone=1")),
        code("EP*", "Outer edge of the sealed road surface, shot where the seal meets the shoulder or kerb.",
             line("roads/carriageway/edge", "pavement", "0.35", "Pavement edge")),
        code("OF*", "Crown of a carriageway shot where it is offset from the road centreline, checked in model zone "
             "1.",
             line("roads/carriageway/centreline", "pavement", "0.25", "Offset carriageway crown, zone 1",
                  "Road Centreline"),
             attrs("Check Zone=1")),
        code("RC*", "Centre of the road, shot along the middle of the carriageway.",
             line("roads/carriageway/centreline", "pavement", "0.18", "Carriageway centre", "Road Centreline")),
        code("SG*", "Grid of bars across a road or gateway that stops stock but lets vehicles through.",
             line("roads/carriageway/grid", "pavement", "0.25", "Cattle grid outline")),
        code("UR*", "Edge of the formed surface of an unsealed road.",
             line("roads/carriageway/edge", "pavement", "0.25", "Gravel road edge", "Unsealed Road Edge")),
        code("VT*", "Edge of a track that vehicles use, formed by wheels and not built.",
             line("roads/carriageway/track", "pavement", "0.18", "Vehicle track edge", "Unsealed Road Edge")),
    ]),

    # ---- Roads and Pavements / Markings (24) ----------------------------------------------------------------------
    ("R", "Markings", [
        code("AE*", "A painted pavement arrow for a lane that goes straight on or turns left, shot along its axis.",
             line("roads/markings/arrow", "pavement", "0.18", "Straight-and-left arrow",
                  "Straight and Left Arrow Marking"),
             surface(False)),
        code("AI*", "A painted pavement arrow for a lane that goes straight on or turns right, shot along its axis.",
             line("roads/markings/arrow", "pavement", "0.18", "Straight-and-right arrow",
                  "Straight and Right Arrow Marking"),
             surface(False)),
        code("AL*", "A painted pavement arrow for traffic that turns left, shot along its axis.",
             line("roads/markings/arrow", "pavement", "0.18", "Left-turn arrow", "Left Turn Arrow Marking"),
             surface(False)),
        code("AR*", "A painted pavement arrow for traffic that turns right, shot along its axis.",
             line("roads/markings/arrow", "pavement", "0.18", "Right-turn arrow", "Right Turn Arrow Marking"),
             surface(False)),
        code("AS*", "A painted pavement arrow for traffic that goes straight on, shot along its axis.",
             line("roads/markings/arrow", "pavement", "0.18", "Straight-ahead arrow",
                  "Straight Ahead Arrow Marking"),
             surface(False)),
        code("BS*", "A painted double line with an unbroken barrier line on one side and a broken separation line "
             "on the other.",
             line("roads/markings/line", "pavement", "0.18", "Barrier beside broken line",
                  "Left Broken Barrier Marking")),
        code("CA*", "The zigzag line painted along the approach to a pedestrian crossing.",
             line("roads/markings/crossing", "pavement", "0.18", "Zigzag approach marking",
                  "Crossing Approach Marking"),
             surface(False)),
        code("CC*", "A short-dash line that carries a lane through a junction, checked in check zone 1 (roads, "
             "bridges and drainage).",
             line("roads/markings/line", "pavement", "0.18", "Lane continuity line", "Lane Continuity Marking"),
             attrs("Check Zone=1")),
        code("CD*", "An unbroken painted line on the pavement.",
             line("roads/markings/line", "pavement", "0.18", "Solid painted line")),
        code("CG*", "The give-way line painted across a turning lane.",
             line("roads/markings/line", "pavement", "0.18", "Give-way line", "Give Way Marking")),
        code("CN*", "A lane separation line with dashes and gaps of equal length, checked in check zone 1 (roads, "
             "bridges and drainage).",
             line("roads/markings/line", "pavement", "0.18", "Lane separation line", "Equal Dash Marking"),
             attrs("Check Zone=1")),
        code("CV*", "A row of painted chevrons that point to the left of travel, used to fill a painted island.",
             line("roads/markings/line", "pavement", "0.18", "Left-hand chevrons", "Left Chevron Marking")),
        code("CW*", "The painted edge of a clearway, drawn as dashes and gaps of equal length.",
             line("roads/markings/line", "pavement", "0.18", "Clearway edge line", "Equal Dash Marking")),
        code("CX*", "The painted lines that bound a pedestrian crossing controlled by traffic signals.",
             line("roads/markings/crossing", "pavement", "0.18", "Signalised crossing",
                  "Signal Crossing Marking")),
        code("CY*", "A row of painted chevrons that point to the right of travel, used to fill a painted island.",
             line("roads/markings/line", "pavement", "0.18", "Right-hand chevrons", "Right Chevron Marking")),
        code("CZ*", "Painted zebra stripes where pedestrians have right of way across the carriageway.",
             line("roads/markings/line", "pavement", "0.18", "Zebra crossing", "Zebra Crossing Marking"),
             surface(False)),
        code("DA*", "Row of painted dragon's teeth with the points standing to the left of the way the line is shot.",
             line("roads/markings/line", "pavement", "0.18", "Teeth, points left", "Left Dragons Teeth Marking")),
        code("DB*", "Pair of unbroken painted lines down a road that forbids crossing in either direction.",
             line("roads/markings/line", "pavement", "0.18", "Double barrier line", "Double Barrier Marking"),
             attrs("Check Zone=1")),
        code("DC*", "Row of painted dragon's teeth with the points standing to the right of the way the line is "
             "shot.",
             line("roads/markings/line", "pavement", "0.18", "Teeth, points right",
                  "Right Dragons Teeth Marking")),
        code("LL*", "Lane line painted in 3 metre dashes with 9 metre gaps, shot as built.",
             line("roads/markings/line", "pavement", "0.18", "Lane line, long gap", "Dashed Lane Marking")),
        code("PTMX", "Raised domed marker fixed to the road, called a silent cop, that guides traffic.",
             point("roads/markings/stud", "pavement", "0.25", "Domed pavement marker"),
             symbol("Raised Pavement Marker")),
        code("PTMX*", "Keeps silent-cop road markers out of the ground surface.",
             surface(False)),
        code("SB*", "A painted double line with a broken separation line on one side and an unbroken barrier line "
             "on the other, the barrier on the right.",
             line("roads/markings/line", "pavement", "0.18", "Separation and barrier lines",
                  "Right Broken Barrier Marking")),
        code("TL*", "Raised bars painted across the edge of a lane that rattle a tyre and warn a driver who drifts.",
             line("roads/markings/tactile", "pavement", "0.18", "Rumble strip line", "Zebra Crossing Marking")),
    ]),

    # ---- Roads and Pavements / Paths and Access (3) --------------------------------------------------------------
    ("R", "Paths and Access", [
        code("DW*", "Edge of a driveway that gives a lot access across the footpath to the road.",
             line("roads/paths/edge", "pavement", "0.25", "Driveway edge")),
        code("FP*", "Edge of a footpath for pedestrians beside the road.",
             line("roads/paths/edge", "pavement", "0.25", "Footpath edge"),
             attrs("Check Zone=3")),
        code("KR*", "A pedestrian kerb ramp, or pram ramp, shot as a line along its edge.",
             line("roads/paths/ramp", "pavement", "0.25", "Pedestrian ramp outline")),
    ]),

    # ---- Street Furniture and Signs / Signs (12) ------------------------------------------------------------------
    ("S", "Signs", [
        code("PSDS", "A post that carries a sign face on each of its two sides.",
             point("street/signs/sign", "street", "0.25", "Double-faced sign post"),
             symbol("Double Sided Sign Post")),
        code("PSIN", "A post that carries a single sign face.",
             point("street/signs/sign", "street", "0.25", "Single-faced sign post"),
             symbol("Sign Post")),
        code("S1*", "Stretch of road signed with a speed limit of 100 km/h; the string runs along the zone.",
             line("street/signs/speed-zone", "street", "0.25", "Speed limit 100 km/h",
                  "Hundred Speed Zone Line"),
             surface(False)),
        code("S2*", "Stretch of road signed with a speed limit of 110 km/h; the string runs along the zone.",
             line("street/signs/speed-zone", "street", "0.25", "Speed limit 110 km/h",
                  "Hundred and Ten Speed Zone Line"),
             surface(False)),
        code("S4*", "Stretch of road signed with a speed limit of 40 km/h; the string runs along the zone.",
             line("street/signs/speed-zone", "street", "0.25", "Speed limit 40 km/h", "Forty Speed Zone Line"),
             surface(False)),
        code("S5*", "Stretch of road signed with a speed limit of 50 km/h; the string runs along the zone.",
             line("street/signs/speed-zone", "street", "0.25", "Speed limit 50 km/h", "Fifty Speed Zone Line"),
             surface(False)),
        code("S6*", "Stretch of road signed with a speed limit of 60 km/h; the string runs along the zone.",
             line("street/signs/speed-zone", "street", "0.25", "Speed limit 60 km/h", "Sixty Speed Zone Line"),
             surface(False)),
        code("S7*", "Stretch of road signed with a speed limit of 70 km/h; the string runs along the zone.",
             line("street/signs/speed-zone", "street", "0.25", "Speed limit 70 km/h", "Seventy Speed Zone Line"),
             surface(False)),
        code("S8*", "Stretch of road signed with a speed limit of 80 km/h; the string runs along the zone.",
             line("street/signs/speed-zone", "street", "0.25", "Speed limit 80 km/h", "Eighty Speed Zone Line"),
             surface(False)),
        code("S9*", "Stretch of road signed with a speed limit of 90 km/h; the string runs along the zone.",
             line("street/signs/speed-zone", "street", "0.25", "Speed limit 90 km/h", "Ninety Speed Zone Line"),
             surface(False)),
        code("SI*", "Plan outline of a large sign face, such as a gantry or hoarding.",
             line("street/signs/sign", "street", "0.25", "Large sign outline")),
        code("SX*", "Plan outline of a sign carried on an arm that reaches out over the road.",
             line("street/signs/sign", "street", "0.25", "Cantilever sign outline"),
             surface(False)),
    ]),

    # ---- Street Furniture and Signs / Lighting (4) ---------------------------------------------------------------
    ("S", "Lighting", [
        code("LI*", "Outreach arm of a street light, shot from the pole to the lantern; it is kept out of the "
             "ground surface.",
             line("street/lighting/light", "street", "0.18", "Light outreach arm"),
             surface(False)),
        code("PLGN", "A low light standing in a garden bed or on a lawn, shot at its base.",
             point("street/lighting/light", "street", "0.25", "Low garden light"),
             symbol("Bollard Light")),
        code("PLPL", "A pole that carries a light, shot at the centre of the pole at ground level.",
             point("street/lighting/light", "street", "0.25", "Light pole"),
             symbol("Street Light")),
        code("PLSU", "A light hung from an overhead wire or bracket, shot on the ground directly below it.",
             point("street/lighting/light", "street", "0.25", "Hanging light"),
             symbol("Hanging Light")),
    ]),

    # ---- Street Furniture and Signs / Posts and Barriers (5) -----------------------------------------------------
    ("S", "Posts and Barriers", [
        code("AC*", "A fixed bollard, shot as a single point and drawn with the bollard symbol.",
             point("street/barriers/post", "street", "0.25", "Fixed bollard"),
             symbol("Bollard")),
        code("FG*", "Steel beam barrier on posts that guards the edge of a road.",
             line("street/barriers/guard-rail", "street", "0.35", "Roadside guard fence", "Guard Rail Line")),
        code("FW*", "Safety barrier of tensioned steel cables on posts beside or between traffic lanes.",
             line("street/barriers/guard-rail", "street", "0.35", "Cable safety barrier",
                  "Wire Rope Barrier Line")),
        code("KJ*", "A concrete safety barrier of the F-shaped profile, shot as a line along its base.",
             line("street/barriers/guard-rail", "street", "0.35", "Concrete safety barrier",
                  "Concrete Barrier Line")),
        code("TG*", "Line along the top of a guard rail.",
             line("street/barriers/guard-rail", "street", "0.25", "Guard rail crest", "Guard Rail Line")),
    ]),

    # ---- Street Furniture and Signs / Amenities (9) --------------------------------------------------------------
    ("S", "Amenities", [
        code("BH*", "The roof outline of a bus shelter, shot round its edge.",
             line("street/amenities/furniture", "street", "0.25", "Bus shelter roof"),
             surface(False)),
        code("BI*", "A large waste bin such as a skip, shot round its outline and marked with the bin symbol.",
             line("street/amenities/furniture", "street", "0.25", "Bulk bin"),
             symbol("Litter Bin")),
        code("PBIN", "A rubbish bin, shot at its centre.",
             point("street/amenities/furniture", "street", "0.25", "Litter bin"),
             symbol("Litter Bin")),
        code("PBUS", "The position of a bus stop, shot at the foot of its sign post.",
             point("street/amenities/furniture", "street", "0.25", "Bus stop position"),
             symbol("Bus Stop Sign")),
        code("PFPL", "A built fireplace, such as a barbecue hearth, in a park, camp or picnic area.",
             point("street/amenities/furniture", "street", "0.25", "Outdoor fireplace"),
             symbol("Fireplace")),
        code("PKME", "A kerbside meter that collects the fee for a parking bay.",
             point("street/amenities/furniture", "street", "0.25", "Kerbside meter"),
             symbol("Parking Meter")),
        code("PMBX", "A letterbox at the property frontage, shot at the middle of its base.",
             point("street/amenities/furniture", "street", "0.25", "Letterbox"),
             symbol("Letterbox")),
        code("SE*", "Plan outline of a fixed seat or bench.",
             line("street/amenities/furniture", "street", "0.25", "Seat outline"),
             surface(False)),
        code("TA*", "Plan outline of a picnic table.",
             line("street/amenities/furniture", "street", "0.25", "Picnic table outline")),
    ]),

    # ---- Street Furniture and Signs / Traffic Control (6) --------------------------------------------------------
    ("S", "Traffic Control", [
        code("PCAM", "A speed, red-light or traffic enforcement camera, shot at its pole.",
             point("street/traffic/equipment", "street", "0.25", "Enforcement camera"),
             symbol("Roadside Camera")),
        code("PCFU", "The flash lamp that lights a vehicle for an enforcement camera, shot at its pole.",
             point("street/traffic/equipment", "street", "0.25", "Camera flash lamp"),
             symbol("Camera Strobe")),
        code("PCTV", "A closed-circuit television camera watching the road, shot at the pole that carries it.",
             point("street/traffic/equipment", "street", "0.25", "Closed-circuit camera"),
             symbol("Roadside Camera")),
        code("PSCL", "The cabinet that houses the controller of a set of traffic signals.",
             point("street/traffic/equipment", "street", "0.25", "Signal controller cabinet"),
             symbol("Signal Controller Cabinet")),
        code("PSGL", "A traffic signal post with its lanterns, at an intersection or a crossing.",
             point("street/traffic/equipment", "street", "0.25", "Traffic signal post"),
             symbol("Signal Pole")),
        code("TO*", "Plan outline of a signal pole together with the arm that reaches out over the road.",
             line("street/traffic/equipment", "street", "0.25", "Signal mast arm outline"),
             surface(False)),
    ]),

    # ---- Kerbs and Drainage / Kerbs (6) --------------------------------------------------------------------------
    ("K", "Kerbs", [
        code("BK*", "The back edge of a kerb, where it meets the verge or the footpath.",
             line("drainage/kerbs/back", "kerb", "0.18", "Kerb back", "Kerb Back Line")),
        code("DD*", "Shallow V-shaped concrete drain beside a road, shot along its lowest line.",
             line("drainage/kerbs/dish-drain", "kerb", "0.25", "V-shaped concrete drain", "Dish Drain Edge")),
        code("FI*", "Lowest line of the gutter beside a kerb, along which stormwater runs.",
             line("drainage/kerbs/gutter", "kerb", "0.25", "Gutter invert line", "Gutter Lip Line"),
             attrs("Check Zone=3")),
        code("KB*", "Top edge of a kerb, shot on the road side and checked in model zone 3.",
             line("drainage/kerbs/top", "kerb", "0.35", "Kerb top edge, zone 3"),
             attrs("Check Zone=3")),
        code("LP*", "Lip of the gutter beside a kerb, shot along its line and checked in model zone 1.",
             line("drainage/kerbs/gutter", "kerb", "0.25", "Gutter lip, zone 1", "Gutter Lip Line"),
             attrs("Check Zone=1")),
        code("TM*", "Top edge of a raised median or traffic island.",
             line("drainage/kerbs/median", "kerb", "0.35", "Median top edge")),
    ]),

    # ---- Kerbs and Drainage / Pits and Structures (17) ------------------------------------------------------------
    ("K", "Pits and Structures", [
        code("DP*", "Outline of a stormwater pit, drawn at its true size on the plan.",
             line("drainage/structures/pit", "stormwater", "0.25", "Drainage pit outline"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition")),
        code("DX*", "Outline of a rectangular box in the drainage system, such as a junction or inlet box.",
             line("drainage/structures/pit", "stormwater", "0.25", "Drainage box outline"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition")),
        code("HB*", "Line along the foot of a headwall where it meets the ground or the pipe invert.",
             line("drainage/structures/outlet", "stormwater", "0.18", "Headwall base"),
             attrs("utility.type=stormwater", "Owner", "Condition")),
        code("HW*", "Line along the top of a headwall that surrounds the end of a pipe or culvert.",
             line("drainage/structures/outlet", "stormwater", "0.25", "Headwall crest"),
             attrs("utility.type=stormwater", "Owner", "Condition")),
        code("IP*", "Outline of a stormwater gully pit shot as a closed line instead of a single point.",
             line("drainage/structures/pit", "stormwater", "0.25", "Gully pit outline"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition")),
        code("KI*", "A kerb inlet shot as a line along the kerb: the opening where gutter water enters the "
             "stormwater system.",
             line("drainage/structures/pit", "stormwater", "0.25", "Kerb inlet opening"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition")),
        code("PDJM", "A stormwater maintenance hole at the point where two or more drainage lines meet.",
             point("drainage/structures/manhole", "stormwater", "0.25", "Junction manhole"),
             symbol("Stormwater Manhole"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition")),
        code("PEWW", "The free end of a wing wall that flanks a culvert or a drain outlet.",
             point("drainage/structures/outlet", "stormwater", "0.25", "Wingwall end"),
             symbol("Wingwall End"),
             attrs("utility.type=stormwater", "Owner", "Condition")),
        code("PGUL", "A stormwater gully pit that takes surface runoff, shot at the middle of its grate.",
             point("drainage/structures/pit", "stormwater", "0.25", "Stormwater gully pit"),
             symbol("Gully Pit"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition")),
        code("PHWB", "The foot of a pipe headwall, shot where the wall meets the pipe or the ground.",
             point("drainage/structures/outlet", "stormwater", "0.25", "Headwall foot"),
             symbol("Headwall"),
             attrs("utility.type=stormwater", "Owner", "Condition")),
        code("PHWT", "The top edge of a pipe headwall, shot on its coping.",
             point("drainage/structures/outlet", "stormwater", "0.25", "Headwall crest point"),
             symbol("Headwall"),
             attrs("utility.type=stormwater", "Owner", "Condition")),
        code("PILT", "The point where drainage flows into a sump, shot at the mouth of the inlet.",
             point("drainage/structures/pit", "stormwater", "0.25", "Sump inlet"),
             symbol("Sump Inlet"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition")),
        code("PJBX", "The top of a concrete junction box on a stormwater line, where pipes meet.",
             point("drainage/structures/pit", "stormwater", "0.25", "Concrete junction box"),
             symbol("Junction Pit"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition")),
        code("PSDO", "The invert level where a subsoil drain discharges at its outlet.",
             point("drainage/structures/outlet", "stormwater", "0.25", "Subsoil drain outlet level"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition")),
        code("PSDO*", "Keeps subsoil drain outlet levels out of the ground surface, since an invert is not the ground.",
             surface(False)),
        code("PSFP", "The flushing point of a subsoil drain, a riser through which the pipe can be cleared.",
             point("drainage/structures/pit", "stormwater", "0.25", "Subsoil flushing point"),
             symbol("Subsoil Inspection Point"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition")),
        code("PSFP*", "Keeps subsoil flushing points out of the ground surface.",
             surface(False)),
    ]),

    # ---- Kerbs and Drainage / Pipes and Culverts (55) -------------------------------------------------------------
    ("K", "Pipes and Culverts", [
        code("B0*", "A box culvert 150 mm high, drawn as its outline in plan.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Box culvert, 150 mm", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=150")),
        code("B1*", "A box culvert 225 mm high, drawn as its outline in plan.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Box culvert, 225 mm", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=225")),
        code("B2*", "A box culvert 300 mm high, drawn as its outline in plan.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Box culvert, 300 mm", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=300")),
        code("B3*", "A box culvert 375 mm high, drawn as its outline in plan.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Box culvert, 375 mm", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=375")),
        code("B4*", "A box culvert 450 mm high, drawn as its outline in plan.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Box culvert, 450 mm", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=450")),
        code("B6*", "A box culvert 600 mm high, drawn as its outline in plan.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Box culvert, 600 mm", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=600")),
        code("B7*", "A box culvert 750 mm high, drawn as its outline in plan.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Box culvert, 750 mm", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=750")),
        code("B9*", "A box culvert 900 mm high, drawn as its outline in plan.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Box culvert, 900 mm", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=900")),
        code("BX*", "A box culvert of any height, drawn as its outline in plan.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Box culvert, other height",
                  "Culvert Outline"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)")),
        code("D0*", "Rectangular box culvert with a clear opening 1050 mm high.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, 1050 mm high", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=1050")),
        code("D1*", "Rectangular box culvert with a clear opening 1200 mm high.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, 1200 mm high", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=1200")),
        code("D2*", "Rectangular box culvert with a clear opening 1500 mm high.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, 1500 mm high", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=1500")),
        code("D3*", "Rectangular box culvert with a clear opening 1800 mm high.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, 1800 mm high", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=1800")),
        code("D4*", "Rectangular box culvert with a clear opening 2100 mm high.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, 2100 mm high", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=2100")),
        code("D5*", "Rectangular box culvert with a clear opening 2400 mm high.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, 2400 mm high", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=2400")),
        code("D6*", "Rectangular box culvert with a clear opening 2700 mm high.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, 2700 mm high", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=2700")),
        code("D7*", "Rectangular box culvert with a clear opening 3000 mm high.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, 3000 mm high", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=3000")),
        code("D8*", "Rectangular box culvert with a clear opening 3300 mm high.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, 3300 mm high", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=3300")),
        code("D9*", "Rectangular box culvert with a clear opening 3600 mm high.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, 3600 mm high", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)=3600")),
        code("DZ*", "Box culvert traced from imagery and not measured on the ground.",
             line("drainage/pipes/culvert", "stormwater", "0.25", "Digitised box culvert", "Culvert Outline"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)")),
        code("PI01", "The invert of a 225 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-small", "stormwater", "0.18", "Pipe invert, 225 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=225")),
        code("PI02", "The invert of a 300 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-small", "stormwater", "0.18", "Pipe invert, 300 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=300")),
        code("PI03", "The invert of a 375 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-small", "stormwater", "0.18", "Pipe invert, 375 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=375")),
        code("PI04", "The invert of a 450 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-small", "stormwater", "0.18", "Pipe invert, 450 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=450")),
        code("PI05", "The invert of a 525 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-small", "stormwater", "0.18", "Pipe invert, 525 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=525")),
        code("PI06", "The invert of a 600 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-small", "stormwater", "0.18", "Pipe invert, 600 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=600")),
        code("PI07", "The invert of a 750 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-large", "stormwater", "0.18", "Pipe invert, 750 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=750")),
        code("PI09", "The invert of a 900 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-large", "stormwater", "0.18", "Pipe invert, 900 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=900")),
        code("PI10", "The invert of a 1050 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-large", "stormwater", "0.18", "Pipe invert, 1050 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1050")),
        code("PI12", "The invert of a 1200 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-large", "stormwater", "0.18", "Pipe invert, 1200 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1200")),
        code("PI13", "The invert of a 1350 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-large", "stormwater", "0.18", "Pipe invert, 1350 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1350")),
        code("PI15", "The invert of a 1500 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-large", "stormwater", "0.18", "Pipe invert, 1500 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1500")),
        code("PI16", "The invert of a 1650 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-large", "stormwater", "0.18", "Pipe invert, 1650 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1650")),
        code("PI18", "The invert of a 1800 mm stormwater pipe, shot at its lowest inside point.",
             point("drainage/pipes/invert-large", "stormwater", "0.18", "Pipe invert, 1800 mm"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1800")),
        code("PINV", "The lowest inside point of a stormwater pipe, shot where it meets a pit; the diameter is asked "
             "for.",
             point("drainage/pipes/invert-small", "stormwater", "0.18", "Pipe invert"),
             symbol("Invert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("POBV", "The obvert of a stormwater pipe, the highest point on its inside, shot at the crown.",
             point("drainage/pipes/pipe", "stormwater", "0.18", "Pipe obvert"),
             symbol("Obvert Level"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("POBV*", "Keeps pipe obvert shots out of the ground surface, since they lie below it.",
             surface(False)),
        code("U1*", "Stormwater pipe 225 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-small", "stormwater", "0.25", "Stormwater pipe, 225 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=225")),
        code("U2*", "Stormwater pipe 300 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-small", "stormwater", "0.25", "Stormwater pipe, 300 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=300")),
        code("U3*", "Stormwater pipe 375 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-small", "stormwater", "0.25", "Stormwater pipe, 375 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=375")),
        code("U4*", "Stormwater pipe 450 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-small", "stormwater", "0.25", "Stormwater pipe, 450 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=450")),
        code("U5*", "Stormwater pipe 525 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-small", "stormwater", "0.35", "Stormwater pipe, 525 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=525")),
        code("U6*", "Stormwater pipe 600 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-small", "stormwater", "0.35", "Stormwater pipe, 600 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=600")),
        code("U7*", "Stormwater pipe 750 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-large", "stormwater", "0.35", "Stormwater pipe, 750 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=750")),
        code("U9*", "Stormwater pipe 900 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-large", "stormwater", "0.35", "Stormwater pipe, 900 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=900")),
        code("UB*", "Rectangular box culvert whose height was not measured.",
             line("drainage/pipes/culvert", "stormwater", "0.35", "Culvert, height unstated", "Culvert Outline"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Width (mm)",
                   "Height (mm)")),
        code("UU*", "Stormwater pipe whose diameter was not measured.",
             line("drainage/pipes/pipe", "stormwater", "0.25", "Stormwater pipe, size unstated",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("UX*", "Stormwater pipe of any diameter, for a pipe whose size is given in its attributes.",
             line("drainage/pipes/pipe", "stormwater", "0.25", "Stormwater pipe, any size",
                  "Stormwater Pipe Line"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("UZ*", "Stormwater pipe traced from imagery or plans and not located on the ground.",
             line("drainage/pipes/pipe", "stormwater", "0.18", "Digitised stormwater pipe",
                  "Stormwater Pipe Line"),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("V1*", "Stormwater pipe 1050 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-large", "stormwater", "0.35", "Stormwater pipe, 1050 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1050")),
        code("V2*", "Stormwater pipe 1200 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-large", "stormwater", "0.35", "Stormwater pipe, 1200 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1200")),
        code("V3*", "Stormwater pipe 1350 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-large", "stormwater", "0.35", "Stormwater pipe, 1350 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1350")),
        code("V5*", "Stormwater pipe 1500 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-large", "stormwater", "0.35", "Stormwater pipe, 1500 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1500")),
        code("V6*", "Stormwater pipe 1650 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-large", "stormwater", "0.35", "Stormwater pipe, 1650 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1650")),
        code("V8*", "Stormwater pipe 1800 mm across, shot as a line along its route.",
             line("drainage/pipes/pipe-large", "stormwater", "0.35", "Stormwater pipe, 1800 mm",
                  "Stormwater Pipe Line"),
             surface(False),
             attrs("utility.type=stormwater", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)=1800")),
    ]),

    # ---- Kerbs and Drainage / Channels and Waterways (4) ---------------------------------------------------------
    ("K", "Channels and Waterways", [
        code("DF*", "Galvanised iron flume that carries water down a batter, shot along its floor.",
             line("drainage/channels/drain", "waterway", "0.25", "Batter flume", "Open Drain Invert")),
        code("DT*", "Broad shallow channel beside the road shoulder that collects runoff, shot along its floor.",
             line("drainage/channels/drain", "waterway", "0.25", "Roadside table drain", "Open Drain Invert")),
        code("FO*", "Place where a road or track runs through a watercourse without a bridge or culvert.",
             line("drainage/channels/creek", "waterway", "0.25", "Ford crossing")),
        code("WC*", "Centre of a natural watercourse.",
             line("drainage/channels/creek", "waterway", "0.25", "Watercourse centre", "Creek Centreline")),
    ]),

    # ---- Rail and Transit / Track (8) ----------------------------------------------------------------------------
    ("T", "Track", [
        code("PRAL", "A level taken on the top of the head of a running rail.",
             point("rail/track/rail", "rail", "0.35", "Rail head level"),
             symbol("Spot Level")),
        code("PRAL*", "Keeps rail head levels out of the ground surface, so the track does not distort the terrain "
             "model.",
             surface(False)),
        code("PRBX", "A box beside the track that houses railway control equipment.",
             point("rail/track/lineside", "rail", "0.25", "Lineside control box"),
             symbol("Rail Control Box")),
        code("PRCC", "A trough beside the track that carries signalling cables, taken as a single point.",
             point("rail/track/lineside", "rail", "0.25", "Signalling cable trough"),
             symbol("Rail Cable Trough")),
        code("PRTM", "A survey monument set beside the track as a lasting reference for its alignment.",
             point("rail/track/lineside", "rail", "0.25", "Track survey monument"),
             symbol("Fixed Survey Mark")),
        code("RA*", "Top of a running rail, shot along the middle of the rail head.",
             line("rail/track/rail", "rail", "0.35", "Rail head", "Running Rail")),
        code("RF*", "Edge of the railway formation, the shaped earth platform that carries the ballast.",
             line("rail/track/formation", "rail", "0.25", "Track formation edge", "Track Formation Edge")),
        code("RT*", "Centre of a railway track, midway between its two rails.",
             line("rail/track/centreline", "rail", "0.18", "Track centreline", "Track Centreline")),
    ]),

    # ---- Rail and Transit / Overhead and Signals (3) -------------------------------------------------------------
    ("T", "Overhead and Signals", [
        code("PRSG", "A railway signal on its mast or post beside the track.",
             point("rail/overhead/mast", "rail", "0.18", "Lineside signal"),
             symbol("Signal Mast")),
        code("PSTR", "A steel stanchion beside the track that carries the overhead wires or a signal.",
             point("rail/overhead/mast", "rail", "0.18", "Overhead wire stanchion"),
             symbol("Overhead Mast")),
        code("RS*", "Trough beside the track that carries signalling cable.",
             line("rail/overhead/trough", "rail", "0.18", "Signal cable trough", "Buried Duct")),
    ]),

    # ---- Rail and Transit / Platforms (2) ------------------------------------------------------------------------
    ("T", "Platforms", [
        code("RP*", "Top edge of a railway platform, on the side nearest the track.",
             line("rail/platforms/edge", "rail", "0.35", "Platform top edge", "Platform Edge Line")),
        code("RR*", "Edge of a ramp that rises to a railway platform.",
             line("rail/platforms/edge", "rail", "0.25", "Platform ramp edge")),
    ]),

    # ---- Utilities / Water (16) -----------------------------------------------------------------------------------
    ("U", "Water", [
        code("PWAV", "Valve at a high point of a water main that lets trapped air escape.",
             point("utilities/water/fitting", "water", "0.25", "Air release valve"),
             symbol("Air Release Valve"),
             attrs("utility.type=water", "Owner", "Depth (m)", "Condition")),
        code("PWCP", "Fitting on a water main where pressure or electrical readings are taken.",
             point("utilities/water/fitting", "water", "0.25", "Main test point"),
             symbol("Test Point"),
             attrs("utility.type=water", "Owner", "Depth (m)", "Condition")),
        code("PWEP", "Pump that lifts or boosts water, shot as a point.",
             point("utilities/water/structure", "water", "0.25", "Water pump"),
             symbol("Pumping Station"),
             attrs("utility.type=water", "Owner", "Condition")),
        code("PWET", "Terminal that connects a water main to an electrical earth.",
             point("utilities/water/fitting", "water", "0.25", "Main earthing point"),
             symbol("Earthing Point"),
             attrs("utility.type=water", "Owner", "Depth (m)", "Condition")),
        code("PWHY", "Hydrant on a drinking water main, used for flushing and for fire fighting.",
             point("utilities/water/fitting", "water", "0.25", "Mains water hydrant"),
             symbol("Fire Hydrant"),
             attrs("utility.type=water", "Owner", "Condition")),
        code("PWMM", "Post or plate that shows the route of a water main.",
             point("utilities/water/fitting", "water", "0.25", "Water main marker post"),
             symbol("Marker Post"),
             attrs("utility.type=water", "Owner", "Condition")),
        code("PWMR", "Meter that records the water passing through a connection.",
             point("utilities/water/fitting", "water", "0.25", "Connection meter"),
             symbol("Water Flow Meter"),
             attrs("utility.type=water", "Owner", "Depth (m)", "Condition")),
        code("PWSR", "Valve at a low point of a main that empties it for cleaning or repair.",
             point("utilities/water/fitting", "water", "0.25", "Drain-off valve"),
             symbol("Scour Valve"),
             attrs("utility.type=water", "Owner", "Depth (m)", "Condition")),
        code("PWSV", "Valve that shuts off the flow in a water main.",
             point("utilities/water/fitting", "water", "0.25", "Isolating valve"),
             symbol("Water Isolation Valve"),
             attrs("utility.type=water", "Owner", "Depth (m)", "Condition")),
        code("PWTP", "Tap on a water supply, such as a standpipe or a garden tap.",
             point("utilities/water/fitting", "water", "0.25", "Water tap"),
             symbol("Tap"),
             attrs("utility.type=water", "Owner", "Condition")),
        code("RE*", "Outline of a water storage reservoir, traced round its walls.",
             line("utilities/water/structure", "water", "0.25", "Reservoir outline"),
             attrs("utility.type=water", "Owner", "Condition")),
        code("UO*", "Water pipeline that runs above ground on supports.",
             line("utilities/water/overhead", "water", "0.25", "Raised water pipeline", "Water Supply Pipe"),
             surface(False),
             attrs("utility.type=water", "Owner", "Height (m)", "Condition", "Material", "Diameter (mm)")),
        code("WI*", "Pipe that carries water for irrigation, shot as a line along its route.",
             line("utilities/water/irrigation", "water", "0.18", "Irrigation pipe", "Water Supply Pipe"),
             attrs("utility.type=water", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("WM*", "Water main, shot as a line along its route.",
             line("utilities/water/main", "water", "0.35", "Drinking water main", "Water Supply Pipe"),
             attrs("utility.type=water", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("WY*", "Small pipe that joins a property to the water main, shot as a line along its route.",
             line("utilities/water/connection", "water", "0.18", "Water property connection",
                  "Service Connection Line"),
             attrs("utility.type=water", "Owner", "Depth (m)", "Condition")),
        code("WZ*", "Water main traced from imagery or plans and not located on the ground.",
             line("utilities/water/main", "water", "0.18", "Digitised water main", "Water Supply Pipe"),
             attrs("utility.type=water", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
    ]),

    # ---- Utilities / Sewer (13) -----------------------------------------------------------------------------------
    ("U", "Sewer", [
        code("PSLH", "A lamphole: a small opening in a sewer for looking along the pipe with a lamp.",
             point("utilities/sewer/structure", "sewer", "0.25", "Sewer inspection lamphole"),
             symbol("Sewer Inspection Opening"),
             attrs("utility.type=sewer", "Owner", "Depth (m)", "Condition")),
        code("PSMH", "The cover of a sewer maintenance hole, taken at the centre of the lid.",
             point("utilities/sewer/structure", "sewer", "0.25", "Sewer maintenance hole"),
             symbol("Sewer Maintenance Hole"),
             attrs("utility.type=sewer", "Owner", "Depth (m)", "Condition")),
        code("PSMM", "A marker that shows where a sewer main runs.",
             point("utilities/sewer/fitting", "sewer", "0.25", "Sewer route marker"),
             symbol("Marker Post"),
             attrs("utility.type=sewer", "Owner", "Depth (m)", "Condition")),
        code("PSPS", "A pumping station on a sewer, taken at the lid of the wet well.",
             point("utilities/sewer/structure", "sewer", "0.25", "Sewage pumping station"),
             symbol("Pumping Station"),
             attrs("utility.type=sewer", "Owner", "Depth (m)", "Condition")),
        code("PSSV", "A stop valve that shuts off flow in a sewer line, taken at its cover.",
             point("utilities/sewer/fitting", "sewer", "0.25", "Sewer isolation valve"),
             symbol("Sewer Isolation Valve"),
             attrs("utility.type=sewer", "Owner", "Depth (m)", "Condition")),
        code("PSVP", "A vent pipe that lets air into a sewer and gas out of it.",
             point("utilities/sewer/fitting", "sewer", "0.25", "Sewer vent stack"),
             symbol("Sewer Vent"),
             attrs("utility.type=sewer", "Owner", "Depth (m)", "Condition")),
        code("SM*", "Gravity sewer main, shot as a line along its route.",
             line("utilities/sewer/main", "sewer", "0.35", "Gravity sewer main", "Sewer Gravity Pipe"),
             attrs("utility.type=sewer", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("SP*", "Outline of a sewage pond or lagoon.",
             line("utilities/sewer/structure", "sewer", "0.25", "Sewage pond outline"),
             attrs("utility.type=sewer", "Owner", "Condition")),
        code("SQ*", "Outline of a station that pumps sewage or water out of a low point.",
             line("utilities/sewer/structure", "sewer", "0.25", "Pump-out station outline"),
             attrs("utility.type=sewer", "Owner", "Condition")),
        code("SR*", "Pressure sewer, the rising main that carries pumped flow uphill, shot as a line along its "
             "route.",
             line("utilities/sewer/main", "sewer", "0.35", "Pumped sewer line", "Sewer Pressure Pipe"),
             attrs("utility.type=sewer", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("ST*", "Outline of a septic tank.",
             line("utilities/sewer/structure", "sewer", "0.25", "Septic tank outline"),
             attrs("utility.type=sewer", "Owner", "Condition")),
        code("SY*", "Small branch that joins a property to the sewer main, shot as a line along its route.",
             line("utilities/sewer/connection", "sewer", "0.18", "Sewer property connection",
                  "Service Connection Line"),
             attrs("utility.type=sewer", "Owner", "Depth (m)", "Condition")),
        code("SZ*", "Gravity sewer traced from imagery or plans and not located on the ground.",
             line("utilities/sewer/main", "sewer", "0.18", "Digitised sewer main", "Sewer Gravity Pipe"),
             attrs("utility.type=sewer", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
    ]),

    # ---- Utilities / Gas (15) -------------------------------------------------------------------------------------
    ("U", "Gas", [
        code("DG*", "Small gas pipe that runs from the street main to a property.",
             line("utilities/gas/main", "gas", "0.18", "Gas property connection", "Service Connection Line"),
             attrs("utility.type=gas", "Owner", "Depth (m)", "Condition")),
        code("HA*", "Buried pipeline that carries ethane.",
             line("utilities/gas/main", "gas", "0.35", "Ethane main", "Gas Supply Pipe"),
             attrs("utility.type=gas", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("HG*", "Gas main that works at high pressure.",
             line("utilities/gas/main", "gas", "0.35", "High pressure gas main", "Gas Supply Pipe"),
             attrs("utility.type=gas", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("LG*", "A buried gas main that works at low pressure, shot as a line along its route.",
             line("utilities/gas/main", "gas", "0.35", "Low-pressure gas main", "Gas Supply Pipe"),
             attrs("utility.type=gas", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("NG*", "A buried gas main laid in nylon pipe, shot as a line along its route.",
             line("utilities/gas/main", "gas", "0.35", "Nylon gas main", "Gas Supply Pipe"),
             attrs("utility.type=gas", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("PGAS", "The box over a gas valve, shot at its lid.",
             point("utilities/gas/structure", "gas", "0.25", "Gas valve pit"),
             symbol("Gas Pit"),
             attrs("utility.type=gas", "Owner", "Depth (m)", "Condition")),
        code("PGHL", "A manhole giving access to a gas main, shot at its cover.",
             point("utilities/gas/structure", "gas", "0.25", "Gas maintenance hole"),
             symbol("Gas Maintenance Hole"),
             attrs("utility.type=gas", "Owner", "Depth (m)", "Condition")),
        code("PGHM", "A marker on the surface showing the route of a high-pressure gas main.",
             point("utilities/gas/fitting", "gas", "0.25", "High-pressure gas marker"),
             symbol("High Pressure Marker Post"),
             attrs("utility.type=gas", "Owner", "Condition")),
        code("PGMR", "A gas meter, shot at its centre.",
             point("utilities/gas/fitting", "gas", "0.25", "Gas flow meter"),
             symbol("Gas Flow Meter"),
             attrs("utility.type=gas", "Owner", "Depth (m)", "Condition")),
        code("PGPM", "A marker on the surface showing the route of a buried gas main.",
             point("utilities/gas/fitting", "gas", "0.25", "Gas line marker"),
             symbol("Marker Post"),
             attrs("utility.type=gas", "Owner", "Condition")),
        code("PGRB", "The cabinet that houses a gas pressure regulator, shot at the middle of the box.",
             point("utilities/gas/fitting", "gas", "0.25", "Gas regulator cabinet"),
             symbol("Gas Pressure Regulator"),
             attrs("utility.type=gas", "Owner", "Condition")),
        code("PGTP", "A surface fitting on a gas pipeline where its protection or pressure can be tested.",
             point("utilities/gas/fitting", "gas", "0.25", "Gas test terminal"),
             symbol("Test Point"),
             attrs("utility.type=gas", "Owner", "Condition")),
        code("PGVP", "A vent riser on a gas main or casing that lets gas escape safely to the open air.",
             point("utilities/gas/fitting", "gas", "0.25", "Gas vent riser"),
             symbol("Gas Vent"),
             attrs("utility.type=gas", "Owner", "Condition")),
        code("YG*", "Polyethylene gas main, shot as a line along its route.",
             line("utilities/gas/main", "gas", "0.25", "Plastic gas main", "Gas Supply Pipe"),
             attrs("utility.type=gas", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("ZG*", "Gas main traced from imagery or plans and not located on the ground.",
             line("utilities/gas/main", "gas", "0.18", "Digitised gas main", "Gas Supply Pipe"),
             attrs("utility.type=gas", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
    ]),

    # ---- Utilities / Electricity (23) -----------------------------------------------------------------------------
    ("U", "Electricity", [
        code("EA*", "Buried conductor that bonds electrical equipment to the earth.",
             line("utilities/electricity/main", "electricity", "0.18", "Earthing conductor",
                  "Underground Cable"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("EC*", "Plan outline of the cabinet that houses a transformer.",
             line("utilities/electricity/structure", "electricity", "0.25", "Transformer cabinet outline"),
             attrs("utility.type=electricity", "Owner", "Condition")),
        code("ED*", "Buried conduit that carries electrical cable.",
             line("utilities/electricity/main", "electricity", "0.25", "Electrical conduit", "Buried Duct"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("EL*", "Outline of the footing under one leg of a transmission pylon.",
             line("utilities/electricity/structure", "electricity", "0.25", "Pylon leg footing"),
             attrs("utility.type=electricity", "Owner", "Condition")),
        code("EN*", "Outline of a sump that collects water or oil at an electricity installation.",
             line("utilities/electricity/structure", "electricity", "0.25", "Electrical sump outline"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("EU*", "Buried electricity line, located by locator or from records.",
             line("utilities/electricity/main", "electricity", "0.35", "Buried power line", "Underground Cable"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("EY*", "Small electricity cable from the street network to a property.",
             line("utilities/electricity/main", "electricity", "0.18", "Electrical property connection",
                  "Service Connection Line"),
             surface(False),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("EZ*", "Electricity line traced from imagery or plans and not located on the ground.",
             line("utilities/electricity/main", "electricity", "0.18", "Digitised power line",
                  "Underground Cable"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("PECM", "A marker on the surface showing the route of a buried power cable.",
             point("utilities/electricity/fitting", "electricity", "0.25", "Power cable marker"),
             symbol("Marker Post"),
             attrs("utility.type=electricity", "Owner", "Condition")),
        code("PEFP", "A point where a fuse protects a distribution cable, on a pole or in a cabinet.",
             point("utilities/electricity/fitting", "electricity", "0.25", "Distribution fuse"),
             symbol("Electrical Fuse"),
             attrs("utility.type=electricity", "Owner", "Condition")),
        code("PEJB", "A box in which electricity cables are joined, shot at its lid.",
             point("utilities/electricity/structure", "electricity", "0.25", "Electrical junction box"),
             symbol("Electrical Pit"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("PEMH", "A manhole giving access to underground electricity cables, shot at its cover.",
             point("utilities/electricity/structure", "electricity", "0.25", "Power cable manhole"),
             symbol("Electrical Maintenance Hole"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("PETC", "The centre of a ground-mounted cabinet that houses an electricity transformer.",
             point("utilities/electricity/structure", "electricity", "0.25", "Ground transformer"),
             symbol("Transformer"),
             attrs("utility.type=electricity", "Owner", "Condition")),
        code("PEUP", "A pillar above ground where a customer is supplied from the underground power cable.",
             point("utilities/electricity/structure", "electricity", "0.25", "Power service pillar"),
             symbol("Electrical Pillar"),
             attrs("utility.type=electricity", "Owner", "Condition")),
        code("PHTT", "A steel lattice tower that carries high-voltage lines, shot at the centre of its footprint.",
             point("utilities/electricity/structure", "electricity", "0.35", "Transmission pylon"),
             symbol("Transmission Pylon"),
             attrs("utility.type=electricity", "Owner", "Condition")),
        code("PPLP", "A pole that carries power lines and also a street light.",
             point("utilities/electricity/structure", "electricity", "0.25", "Power pole with light"),
             symbol("Power and Light Pole"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("PPPL", "A pole that carries power lines and nothing else.",
             point("utilities/electricity/structure", "electricity", "0.25", "Power pole"),
             symbol("Power Pole"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("PPTR", "A pole that carries power lines and has a transformer mounted on it.",
             point("utilities/electricity/structure", "electricity", "0.25", "Power pole with transformer"),
             symbol("Transformer"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("PSAP", "The ground anchor of the stay wire that braces a pole.",
             point("utilities/electricity/fitting", "electricity", "0.25", "Pole stay anchor"),
             symbol("Guy Anchor"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("PSPL", "A short pole that carries the stay wire bracing another pole.",
             point("utilities/electricity/structure", "electricity", "0.25", "Short bracing pole"),
             symbol("Stay Pole"),
             attrs("utility.type=electricity", "Owner", "Depth (m)", "Condition")),
        code("SC*", "Stay wire that braces a pole or mast, drawn from the pole to its ground anchor.",
             line("utilities/electricity/overhead", "electricity", "0.18", "Pole stay wire", "Overhead Line"),
             surface(False),
             attrs("utility.type=electricity", "Owner", "Condition")),
        code("UE*", "Main overhead power line that carries bulk supply between substations.",
             line("utilities/electricity/overhead", "electricity", "0.35", "High voltage overhead line",
                  "Overhead Line"),
             surface(False),
             attrs("utility.type=electricity", "Owner", "Height (m)", "Condition")),
        code("UL*", "Overhead power line of the local network, below the major transmission lines.",
             line("utilities/electricity/overhead", "electricity", "0.25", "Local overhead line",
                  "Overhead Line"),
             surface(False),
             attrs("utility.type=electricity", "Owner", "Height (m)", "Condition")),
    ]),

    # ---- Utilities / Communications (23) --------------------------------------------------------------------------
    ("U", "Communications", [
        code("OA*", "Optical fibre cable strung above ground on poles, shot as a line along its route.",
             line("utilities/telecommunications/fibre", "telecom", "0.18", "Overhead optical fibre",
                  "Overhead Line"),
             surface(False),
             attrs("utility.type=telecommunications", "Owner", "Height (m)", "Condition")),
        code("OD*", "A conduit that carries optical fibre cable, shot as a line along its route.",
             line("utilities/telecommunications/fibre", "telecom", "0.25", "Fibre duct", "Buried Duct"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)")),
        code("OU*", "Optical fibre cable buried in the ground, shot as a line along its route.",
             line("utilities/telecommunications/fibre", "telecom", "0.35", "Buried optical fibre",
                  "Communications Cable"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
        code("OZ*", "An optical fibre route digitised from records and not located on site.",
             line("utilities/telecommunications/fibre", "telecom", "0.25", "Digitised optical fibre",
                  "Communications Cable"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
        code("PMPT", "A transmitter mast or tower of the mobile telephone network, shot at its base.",
             point("utilities/telecommunications/structure", "telecom", "0.25", "Mobile phone mast"),
             symbol("Mast"),
             attrs("utility.type=telecommunications", "Owner", "Condition")),
        code("POFJ", "A junction box on an optical fibre route where cables are joined or split.",
             point("utilities/telecommunications/structure", "telecom", "0.25", "Fibre junction box"),
             symbol("Communications Pillar"),
             attrs("utility.type=telecommunications", "Owner", "Condition")),
        code("POFM", "A marker that shows where an optical fibre cable runs underground.",
             point("utilities/telecommunications/structure", "telecom", "0.25", "Fibre route marker"),
             symbol("Marker Post"),
             attrs("utility.type=telecommunications", "Owner", "Condition")),
        code("POFP", "A pit on an optical fibre route, shot at the middle of its lid.",
             point("utilities/telecommunications/structure", "telecom", "0.25", "Fibre cable pit"),
             symbol("Communications Pit"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
        code("PT3P", "Concrete telephone pit with three lids in a row, where cable is joined and drawn in.",
             point("utilities/telecommunications/structure", "telecom", "0.25", "Triple communications pit"),
             symbol("Communications Triple Pit"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
        code("PTBX", "Public telephone box or booth standing on the footpath.",
             point("utilities/telecommunications/structure", "telecom", "0.25", "Telephone booth"),
             symbol("Telephone Box"),
             attrs("utility.type=telecommunications", "Owner", "Condition")),
        code("PTCM", "Marker that shows the route of a buried telephone cable.",
             point("utilities/telecommunications/fitting", "telecom", "0.25", "Communications cable marker"),
             symbol("Marker Post"),
             attrs("utility.type=telecommunications", "Owner", "Condition")),
        code("PTDP", "Street cabinet from which telephone cable is distributed to nearby customers.",
             point("utilities/telecommunications/structure", "telecom", "0.25", "Cable distribution pillar"),
             symbol("Communications Pillar"),
             attrs("utility.type=telecommunications", "Owner", "Condition")),
        code("PTJP", "Post standing above ground at a joint in a telephone cable.",
             point("utilities/telecommunications/fitting", "telecom", "0.25", "Cable joint post"),
             symbol("Marker Post"),
             attrs("utility.type=telecommunications", "Owner", "Condition")),
        code("PTMP", "Large main pit, about 1.1 m square, at a major telephone cable junction.",
             point("utilities/telecommunications/structure", "telecom", "0.25", "Communications main pit"),
             symbol("Communications Main Pit"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
        code("PTPL", "Pole that carries telephone cable overhead.",
             point("utilities/telecommunications/overhead", "telecom", "0.25", "Communications pole"),
             symbol("Power Pole"),
             attrs("utility.type=telecommunications", "Owner", "Condition")),
        code("PTSP", "Concrete telephone pit with a single lid.",
             point("utilities/telecommunications/structure", "telecom", "0.25", "Single communications pit"),
             symbol("Communications Pit"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
        code("PTTP", "Concrete telephone pit with two lids side by side.",
             point("utilities/telecommunications/structure", "telecom", "0.25", "Twin communications pit"),
             symbol("Communications Twin Pit"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
        code("TD*", "Conduit that carries telephone cable, shot as a line along its route.",
             line("utilities/telecommunications/duct", "telecom", "0.25", "Phone cable duct", "Buried Duct"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
        code("TN*", "Buried telephone line, shot as a line along its route.",
             line("utilities/telecommunications/copper", "telecom", "0.25", "Buried phone line",
                  "Communications Cable"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
        code("TS*", "Outline of a telephone sump, checked in the third model zone (road side).",
             line("utilities/telecommunications/structure", "telecom", "0.25", "Telephone sump, zone 3"),
             attrs("Check Zone=3")),
        code("TX*", "Plan outline of a public telephone box.",
             line("utilities/telecommunications/structure", "telecom", "0.25", "Telephone box outline"),
             attrs("utility.type=telecommunications", "Owner", "Condition")),
        code("TY*", "Small cable that joins a property to the telephone network, shot as a line along its route.",
             line("utilities/telecommunications/connection", "telecom", "0.18", "Telephone property connection",
                  "Service Connection Line"),
             surface(False),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
        code("TZ*", "Telephone line traced from imagery or plans and not located on the ground.",
             line("utilities/telecommunications/copper", "telecom", "0.18", "Digitised telephone line",
                  "Communications Cable"),
             attrs("utility.type=telecommunications", "Owner", "Depth (m)", "Condition")),
    ]),

    # ---- Utilities / Recycled Water (8) --------------------------------------------------------------------------
    ("U", "Recycled Water", [
        code("PRAV", "An air valve on a recycled water main, which lets trapped air out at a high point.",
             point("utilities/recycled-water/fitting", "recycled", "0.25", "Recycled air valve"),
             symbol("Air Release Valve"),
             attrs("utility.type=recycled-water", "Owner", "Depth (m)", "Condition")),
        code("PRET", "An earth terminal on a recycled water main, the point where the main is bonded to earth.",
             point("utilities/recycled-water/fitting", "recycled", "0.25", "Recycled earth terminal"),
             symbol("Earthing Point"),
             attrs("utility.type=recycled-water", "Owner", "Depth (m)", "Condition")),
        code("PRHY", "A hydrant on a recycled water main.",
             point("utilities/recycled-water/fitting", "recycled", "0.25", "Recycled hydrant"),
             symbol("Fire Hydrant"),
             attrs("utility.type=recycled-water", "Owner", "Depth (m)", "Condition")),
        code("PRMM", "A marker that shows where a recycled water main runs.",
             point("utilities/recycled-water/fitting", "recycled", "0.25", "Recycled main marker"),
             symbol("Marker Post"),
             attrs("utility.type=recycled-water", "Owner", "Depth (m)", "Condition")),
        code("PRMR", "A meter on a recycled water supply.",
             point("utilities/recycled-water/fitting", "recycled", "0.25", "Recycled water meter"),
             symbol("Recycled Flow Meter"),
             attrs("utility.type=recycled-water", "Owner", "Depth (m)", "Condition")),
        code("PRSV", "A stop valve that shuts off a length of recycled water main.",
             point("utilities/recycled-water/fitting", "recycled", "0.25", "Recycled stop valve"),
             symbol("Recycled Isolation Valve"),
             attrs("utility.type=recycled-water", "Owner", "Depth (m)", "Condition")),
        code("PRTP", "A tap on a recycled water supply, such as an outlet or a standpipe.",
             point("utilities/recycled-water/fitting", "recycled", "0.25", "Recycled tap"),
             symbol("Tap"),
             attrs("utility.type=recycled-water", "Owner", "Depth (m)", "Condition")),
        code("RM*", "Main that carries recycled water, shot along its centre.",
             line("utilities/recycled-water/main", "recycled", "0.35", "Recycled water main",
                  "Recycled Water Pipe"),
             attrs("utility.type=recycled-water", "Owner", "Depth (m)", "Condition", "Material",
                   "Diameter (mm)")),
    ]),

    # ---- Utilities / Fire Service (1) ----------------------------------------------------------------------------
    ("U", "Fire Service", [
        code("PWFB", "Hydrant on a fire service main, for the fire brigade's hoses.",
             point("utilities/fire-service/fitting", "fire", "0.25", "Fire service hydrant"),
             symbol("Fire Hydrant"),
             attrs("utility.type=fire-service", "Owner", "Condition")),
    ]),

    # ---- Utilities / Fuel (7) ------------------------------------------------------------------------------------
    ("U", "Fuel", [
        code("FU*", "Buried pipeline for a fuel that has no code of its own.",
             line("utilities/fuel/main", "fuel", "0.35", "Other fuel pipeline", "Fuel Supply Pipe"),
             attrs("utility.type=fuel", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("FZ*", "Fuel pipeline of an unlisted kind, traced from imagery and not located on the ground.",
             line("utilities/fuel/main", "fuel", "0.18", "Digitised fuel pipeline", "Fuel Supply Pipe"),
             attrs("utility.type=fuel", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("HO*", "Pipeline that carries oil at high pressure.",
             line("utilities/fuel/main", "fuel", "0.35", "Pressurised oil line", "Fuel Supply Pipe"),
             attrs("utility.type=fuel", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("HZ*", "High pressure oil pipeline traced from imagery and not located on the ground.",
             line("utilities/fuel/main", "fuel", "0.18", "Digitised oil line", "Fuel Supply Pipe"),
             attrs("utility.type=fuel", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("PFPM", "A marker on the surface showing the route of a buried fuel pipeline.",
             point("utilities/fuel/fitting", "fuel", "0.25", "Fuel line marker"),
             symbol("Marker Post"),
             attrs("utility.type=fuel", "Owner", "Condition")),
        code("POHM", "A sign or post that marks the route of a high-pressure oil pipeline.",
             point("utilities/fuel/fitting", "fuel", "0.25", "Oil pipeline marker"),
             symbol("Marker Post"),
             attrs("utility.type=fuel", "Owner", "Condition")),
        code("PPET", "A petrol pump at a service station, shot at the middle of its base.",
             point("utilities/fuel/structure", "fuel", "0.25", "Fuel dispenser"),
             symbol("Petrol Pump"),
             attrs("utility.type=fuel", "Owner", "Condition")),
    ]),

    # ---- Utilities / Traffic Systems (4) -------------------------------------------------------------------------
    ("U", "Traffic Systems", [
        code("IT*", "A cable of a road traffic management system, such as signals or cameras, shot as a line along "
             "its route.",
             line("utilities/its/main", "its", "0.25", "Traffic systems cable", "Traffic Systems Cable"),
             attrs("utility.type=its", "Owner", "Depth (m)", "Condition")),
        code("PSDR", "A detector for a set of traffic signals, such as a vehicle loop in the road.",
             point("utilities/its/fitting", "its", "0.25", "Signal vehicle detector"),
             symbol("Loop Detector"),
             attrs("utility.type=its", "Owner", "Depth (m)", "Condition")),
        code("PSJX", "A junction box in the ground where traffic signal cables are joined.",
             point("utilities/its/structure", "its", "0.25", "Signal junction box"),
             symbol("Traffic Systems Pit"),
             attrs("utility.type=its", "Owner", "Depth (m)", "Condition")),
        code("SD*", "Loop cut into the road surface that detects vehicles for a traffic signal, drawn along the "
             "loop.",
             line("utilities/its/loop", "its", "0.18", "Detector loop", "Traffic Systems Cable"),
             attrs("utility.type=its", "Owner", "Depth (m)", "Condition")),
    ]),

    # ---- Utilities / Unknown Services (8) ------------------------------------------------------------------------
    ("U", "Unknown Services", [
        code("CB*", "The bottom outline of an underground utility chamber, drawn as a hidden edge.",
             line("utilities/unknown/structure", "unknown", "0.13", "Chamber base edge", "Hidden Outline"),
             attrs("utility.type=unknown", "Owner", "Depth (m)", "Condition")),
        code("CT*", "The top outline of an underground utility chamber.",
             line("utilities/unknown/structure", "unknown", "0.25", "Chamber top edge"),
             attrs("utility.type=unknown", "Owner", "Depth (m)", "Condition")),
        code("PGAT", "The lid of a recessed access cover that serves a service nobody has identified.",
             point("utilities/unknown/structure", "unknown", "0.25", "Cover lid, service unknown"),
             symbol("Unknown Pit"),
             attrs("utility.type=unknown", "Owner", "Depth (m)", "Condition")),
        code("PPOL", "A pole that has not been identified as power, telephone or lighting.",
             point("utilities/unknown/structure", "unknown", "0.25", "Pole of unknown use"),
             symbol("Query Point"),
             attrs("utility.type=unknown", "Owner", "Depth (m)", "Condition")),
        code("PUSR", "Surface sign of a service that has not been identified, such as an unmarked cover or stub.",
             point("utilities/unknown/structure", "unknown", "0.25", "Unknown service point"),
             symbol("Query Point"),
             attrs("utility.type=unknown", "Owner", "Depth (m)", "Condition")),
        code("PUTP", "Buried tank whose contents are not known, shot at its lid or filler.",
             point("utilities/unknown/structure", "unknown", "0.25", "Buried tank, contents unknown"),
             symbol("Storage Tank"),
             attrs("utility.type=unknown", "Owner", "Depth (m)", "Condition")),
        code("UP*", "Pipeline whose service has not been identified.",
             line("utilities/unknown/line", "unknown", "0.25", "Pipeline, service unknown",
                  "Unknown Service Line"),
             attrs("utility.type=unknown", "Owner", "Depth (m)", "Condition", "Material", "Diameter (mm)")),
        code("US*", "Plan outline of a box where service lines are joined, whose service is not known.",
             line("utilities/unknown/structure", "unknown", "0.25", "Junction box outline"),
             attrs("utility.type=unknown", "Owner", "Condition")),
    ]),

    # ---- Vegetation and Landscape / Trees (51) --------------------------------------------------------------------
    ("V", "Trees", [
        code("PF01", "A tree whose crown spreads about one metre across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 1 m spread"),
             symbol("Tiny Tree Canopy")),
        code("PF01*", "Keeps the one-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF02", "A tree whose crown spreads about two metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 2 m spread"),
             symbol("Tiny Tree Canopy")),
        code("PF02*", "Keeps the two-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF03", "A tree whose crown spreads about three metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 3 m spread"),
             symbol("Small Tree Canopy")),
        code("PF03*", "Keeps the three-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF04", "A tree whose crown spreads about four metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 4 m spread"),
             symbol("Small Tree Canopy")),
        code("PF04*", "Keeps the four-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF05", "A tree whose crown spreads about five metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 5 m spread"),
             symbol("Medium Tree Canopy")),
        code("PF05*", "Keeps the five-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF06", "A tree whose crown spreads about six metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 6 m spread"),
             symbol("Medium Tree Canopy")),
        code("PF06*", "Keeps the six-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF07", "A tree whose crown spreads about seven metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 7 m spread"),
             symbol("Large Tree Canopy")),
        code("PF07*", "Keeps the seven-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF08", "A tree whose crown spreads about eight metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 8 m spread"),
             symbol("Large Tree Canopy")),
        code("PF08*", "Keeps the eight-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF09", "A tree whose crown spreads about nine metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 9 m spread"),
             symbol("Broad Tree Canopy")),
        code("PF09*", "Keeps the nine-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF10", "A tree whose crown spreads about ten metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 10 m spread"),
             symbol("Broad Tree Canopy")),
        code("PF10*", "Keeps the ten-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF12", "A tree whose crown spreads about twelve metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 12 m spread"),
             symbol("Giant Tree Canopy")),
        code("PF12*", "Keeps the twelve-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PF15", "A tree whose crown spreads about fifteen metres across, drawn as a canopy ring of that size.",
             point("vegetation/trees/canopy", "tree", "0.18", "Canopy, 15 m spread"),
             symbol("Giant Tree Canopy")),
        code("PF15*", "Keeps the fifteen-metre canopy marks out of the ground surface when the code carries a string "
             "number.",
             surface(False)),
        code("PT01", "A tree whose trunk is about 100 mm across, taken at the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 100 mm across"),
             symbol("Hundred Millimetre Tree Trunk")),
        code("PT01*", "Keeps 100 mm tree trunks out of the ground surface, since a trunk is not the ground.",
             surface(False)),
        code("PT02", "A tree whose trunk is about 200 mm across, taken at the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 200 mm across"),
             symbol("Two Hundred Millimetre Tree Trunk")),
        code("PT02*", "Keeps 200 mm tree trunks out of the ground surface, since a trunk is not the ground.",
             surface(False)),
        code("PT03", "A tree whose trunk is about 300 mm across, taken at the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 300 mm across"),
             symbol("Three Hundred Millimetre Tree Trunk")),
        code("PT03*", "Keeps 300 mm tree trunks out of the ground surface, since a trunk is not the ground.",
             surface(False)),
        code("PT04", "Tree trunk about 400 mm across, shot at the middle of the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 400 mm across"),
             symbol("Narrow Tree Trunk")),
        code("PT04*", "Keeps trunk points of 400 mm trees out of the ground surface.",
             surface(False)),
        code("PT05", "Tree trunk about 500 mm across, shot at the middle of the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 500 mm across"),
             symbol("Narrow Tree Trunk")),
        code("PT05*", "Keeps trunk points of 500 mm trees out of the ground surface.",
             surface(False)),
        code("PT06", "Tree trunk about 600 mm across, shot at the middle of the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 600 mm across"),
             symbol("Narrow Tree Trunk")),
        code("PT06*", "Keeps trunk points of 600 mm trees out of the ground surface.",
             surface(False)),
        code("PT07", "Tree trunk about 700 mm across, shot at the middle of the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 700 mm across"),
             symbol("Medium Tree Trunk")),
        code("PT07*", "Keeps trunk points of 700 mm trees out of the ground surface.",
             surface(False)),
        code("PT08", "Tree trunk about 800 mm across, shot at the middle of the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 800 mm across"),
             symbol("Medium Tree Trunk")),
        code("PT08*", "Keeps trunk points of 800 mm trees out of the ground surface.",
             surface(False)),
        code("PT09", "Tree trunk about 900 mm across, shot at the middle of the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 900 mm across"),
             symbol("Medium Tree Trunk")),
        code("PT09*", "Keeps trunk points of 900 mm trees out of the ground surface.",
             surface(False)),
        code("PT10", "Tree trunk about 1000 mm across, shot at the middle of the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 1000 mm across"),
             symbol("Wide Tree Trunk")),
        code("PT10*", "Keeps trunk points of 1000 mm trees out of the ground surface.",
             surface(False)),
        code("PT12", "Tree trunk about 1200 mm across, shot at the middle of the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 1200 mm across"),
             symbol("Wide Tree Trunk")),
        code("PT12*", "Keeps trunk points of 1200 mm trees out of the ground surface.",
             surface(False)),
        code("PT15", "Tree trunk about 1500 mm across, shot at the middle of the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk 1500 mm across"),
             symbol("Wide Tree Trunk")),
        code("PT15*", "Keeps trunk points of 1500 mm trees out of the ground surface.",
             surface(False)),
        code("PTRE", "Tree trunk whose diameter was not measured, shot at the middle of the trunk.",
             point("vegetation/trees/trunk", "tree", "0.18", "Trunk of unrecorded size"),
             symbol("Unmeasured Tree Trunk")),
        code("PTRE*", "Keeps trunk points of unmeasured trees out of the ground surface.",
             surface(False)),
        code("TR*", "Outline of the foliage of a tree or a stand of trees, drawn along its drip line.",
             line("vegetation/trees/foliage", "tree", "0.13", "Tree foliage outline")),
    ]),

    # ---- Vegetation and Landscape / Ground Cover (7) -------------------------------------------------------------
    ("V", "Ground Cover", [
        code("EG*", "Edge of a garden bed where planting meets lawn or paving.",
             line("vegetation/ground/edge", "planting", "0.18", "Garden bed boundary")),
        code("JC*", "Edge of cultivated land or pasture, shot as a line round the paddock.",
             line("vegetation/ground/edge", "planting", "0.18", "Cultivated land or pasture")),
        code("JD*", "Edge of dense timber or scrub, shot along the line of the outermost trunks.",
             line("vegetation/ground/edge", "planting", "0.18", "Dense timber or scrub edge", "Bush Edge Line")),
        code("JG*", "Edge of grassland, shot as a line round the grassed area.",
             line("vegetation/ground/edge", "planting", "0.18", "Grassland edge", "Grass Edge")),
        code("JM*", "Edge of medium-density timber or scrub, where the trees stand more widely spaced than in dense "
             "bush.",
             line("vegetation/ground/edge", "planting", "0.18", "Medium timber or scrub edge",
                  "Medium Bush Edge")),
        code("JO*", "Outline of an orchard, shot round the outer rows of trees.",
             line("vegetation/ground/edge", "planting", "0.18", "Orchard outline")),
        code("JS*", "Edge of scattered timber or scrub, where single trees stand well apart.",
             line("vegetation/ground/edge", "planting", "0.18", "Scattered timber or scrub edge",
                  "Scattered Bush Edge")),
    ]),

    # ---- Buildings and Structures / Buildings (14) ----------------------------------------------------------------
    ("B", "Buildings", [
        code("AW*", "The outer edge of an awning that projects over a footpath or a frontage.",
             line("buildings/outline/roof", "building", "0.18", "Awning edge", "Eave Line"),
             surface(False)),
        code("BU*", "The outer face of a building wall.",
             line("buildings/outline/wall", "building", "0.35", "Building wall face")),
        code("BV*", "The edge of a roof where it overhangs the wall.",
             line("buildings/outline/roof", "building", "0.18", "Eave line", "Eave Line"),
             surface(False)),
        code("BW*", "The foot of a wall where it meets the ground, drawn as a hidden edge.",
             line("buildings/outline/wall", "building", "0.13", "Wall base", "Hidden Outline")),
        code("DO*", "Opening of a door in a building wall, drawn as a short line across the wall.",
             line("buildings/outline/detail", "building", "0.25", "Door opening")),
        code("LB*", "Edge of a loading bay or loading dock, shot as a line along the raised platform.",
             line("buildings/outline/detail", "building", "0.25", "Loading dock edge", "Platform Edge Line")),
        code("OR*", "Outline of a ruin, the remains of a building that is no longer whole.",
             line("buildings/outline/hidden", "building", "0.13", "Ruin outline", "Hidden Outline")),
        code("PAWN", "A survey point on the edge of an awning.",
             point("buildings/outline/roof", "building", "0.25", "Point on an awning"),
             symbol("Awning Point")),
        code("PAWN*", "Awning points that carry a string number, which are kept out of the surface model.",
             surface(False)),
        code("PCBU", "A building corner shot at natural surface level, where the wall meets the ground.",
             point("buildings/outline/detail", "building", "0.35", "Building corner at ground"),
             symbol("Building Corner")),
        code("PFLR", "A spot level taken on the finished floor of a building.",
             point("buildings/outline/detail", "building", "0.18", "Finished floor level"),
             symbol("Spot Level")),
        code("PFLR*", "Keeps floor levels out of the ground surface when the code carries a string number.",
             surface(False)),
        code("TW*", "Line along the top of a wall.",
             line("buildings/outline/wall", "building", "0.25", "Wall top edge")),
        code("VE*", "Edge of a concrete slab laid flat on the ground.",
             line("buildings/outline/slab", "building", "0.25", "Ground slab edge")),
    ]),

    # ---- Buildings and Structures / Structures (20) ---------------------------------------------------------------
    ("B", "Structures", [
        code("AB*", "Foot of a bridge abutment, where its face meets the ground.",
             line("buildings/structures/bridge", "building", "0.35", "Abutment foot", "Abutment Foot Line")),
        code("AT*", "The upper edge of a bridge abutment, shot along its crest.",
             line("buildings/structures/bridge", "building", "0.35", "Abutment crest")),
        code("BP*", "The plan outline of a bridge pier or column.",
             line("buildings/structures/bridge", "building", "0.35", "Pier or column outline")),
        code("BR*", "The edge of a bridge deck.",
             line("buildings/structures/bridge", "building", "0.50", "Deck edge")),
        code("HS*", "Beam across the top of a bridge pier or pile that carries the girders.",
             line("buildings/structures/bridge", "building", "0.35", "Pier headstock")),
        code("OM*", "Outline of a structure that no other code names.",
             line("buildings/structures/structure", "building", "0.25", "Other structure outline")),
        code("OS*", "Outline of a silo or tank, seen from above.",
             line("buildings/structures/structure", "building", "0.25", "Silo or tank outline")),
        code("OT*", "Outline of a swimming pool.",
             line("buildings/structures/structure", "waterway", "0.25", "Swimming pool outline")),
        code("OW*", "Outline of a windmill, shot round its base.",
             line("buildings/structures/structure", "building", "0.25", "Windmill outline")),
        code("PABB", "Survey point on the bottom of a bridge abutment.",
             point("buildings/structures/bridge", "building", "0.25", "Abutment base point"),
             symbol("Bottom Abutment Point")),
        code("PABT", "Survey point on the top of a bridge abutment.",
             point("buildings/structures/bridge", "building", "0.25", "Point on top of abutment"),
             symbol("Top Abutment Point")),
        code("PBCP", "Point where a bridge pile meets its cap.",
             point("buildings/structures/bridge", "building", "0.25", "Pile cap point"),
             symbol("Pile Cap")),
        code("PBPI", "The centre of a bridge pier or column, shot where it meets the ground.",
             point("buildings/structures/bridge", "building", "0.35", "Bridge pier or column"),
             symbol("Bridge Column")),
        code("PFLG", "A flagpole, shot at its base.",
             point("buildings/structures/structure", "building", "0.35", "Flag mast"),
             symbol("Flagpole")),
        code("PWML", "Windmill, usually one that pumps water, shot at the middle of its tower.",
             point("buildings/structures/structure", "building", "0.25", "Windmill tower"),
             symbol("Windmill")),
        code("UC*", "Line on the ground under a bridge that marks the limit of its clearance overhead.",
             line("buildings/structures/bridge", "building", "0.13", "Bridge clearance limit", "Hidden Outline"),
             surface(False)),
        code("UT*", "Plan outline of a tank buried in the ground.",
             line("buildings/structures/tank", "building", "0.25", "Buried tank outline", "Hidden Outline")),
        code("WW*", "Wall that flares out from the end of a bridge abutment to hold back the embankment.",
             line("buildings/structures/bridge", "building", "0.35", "Abutment wing wall")),
        code("XJ*", "Line of a joint that lets a bridge deck expand.",
             line("buildings/structures/joint", "building", "0.13", "Deck joint line")),
        code("XP*", "Edge of a steel plate that covers an expansion joint.",
             line("buildings/structures/joint", "building", "0.25", "Joint cover plate")),
    ]),

    # ---- Buildings and Structures / Steps and Access (1) ---------------------------------------------------------
    ("B", "Steps and Access", [
        code("SO*", "Outside stairs of a building, shot along the nosing of the treads.",
             line("buildings/access/steps", "building", "0.25", "External stair nosing", "Steps Edge"),
             surface(False)),
    ]),

    # ---- Fences and Walls / Fences (5) ---------------------------------------------------------------------------
    ("F", "Fences", [
        code("FC*", "Fence of a construction that no other fence code covers.",
             line("fences/fence/general", "fence", "0.25", "Other fence")),
        code("FE*", "Ordinary property or paddock fence of wire on posts.",
             line("fences/fence/wire", "fence", "0.25", "General fence", "Post and Wire Fence"),
             attrs("Check Zone=2")),
        code("FL*", "Fence of unrecorded construction, shot as a plain line.",
             line("fences/fence/general", "fence", "0.18", "Fence, type unstated")),
        code("FM*", "Fence built to keep people out, too high or too close in its mesh to climb.",
             line("fences/fence/palisade", "fence", "0.25", "Security fence", "Metal Palisade Fence")),
        code("SF*", "Fence put up to keep people away from a drop, a pool or another danger.",
             line("fences/fence/general", "fence", "0.25", "Fence guarding a hazard", "Metal Palisade Fence")),
    ]),

    # ---- Fences and Walls / Walls (1) ----------------------------------------------------------------------------
    ("F", "Walls", [
        code("RW*", "Wall built to hold back earth, shot along its base.",
             line("fences/wall/retaining", "wall", "0.35", "Earth retaining wall", "Earth Retaining Wall")),
    ]),

    # ---- Fences and Walls / Gates and Posts (2) ------------------------------------------------------------------
    ("F", "Gates and Posts", [
        code("AG*", "A gate leaf between two posts, shot as a line from its hinge to its latch.",
             line("fences/gate/gate", "fence", "0.25", "Gate leaf", "Gate Leaf Outline")),
        code("POST", "A single fence post or roadside guide post, shot at its centre.",
             point("fences/gate/post", "fence", "0.25", "Fence or guide marker"),
             symbol("Fence Post")),
    ]),

    # ---- Terrain and Breaklines / Ground Points (2) --------------------------------------------------------------
    ("G", "Ground Points", [
        code("PNSS", "A shot on the natural ground surface, used as a point of the terrain model.",
             point("terrain/points/ground", "ground", "0.18", "Natural ground shot"),
             symbol("Ground Shot")),
        code("PSHT", "A spot height: a single level taken at a point and kept visible on the plan.",
             point("terrain/points/spot", "ground", "0.18", "Spot level on ground"),
             symbol("Spot Level")),
    ]),

    # ---- Terrain and Breaklines / Breaklines (7) -----------------------------------------------------------------
    ("G", "Breaklines", [
        code("BB*", "The bottom edge of a bank, where the slope meets the flat ground below.",
             line("terrain/breaklines/bank", "breakline", "0.25", "Bank toe", "Bank Toe Edge")),
        code("CU*", "The top edge of a cutting, where the ground begins to fall into the cut.",
             line("terrain/breaklines/bank", "breakline", "0.25", "Cutting crest", "Bank Top Edge")),
        code("NB*", "A breakline or ridge, shot as a line that the surface model must follow.",
             line("terrain/breaklines/ridge-gully", "breakline", "0.18", "Ridge or crest line", "Ridge Line")),
        code("NS*", "A line of ground shots along the natural surface where there is no sharper break, checked in "
             "model zone 2.",
             line("terrain/breaklines/general", "ground", "0.18", "Natural ground line, zone 2",
                  "Soft Breakline"),
             attrs("Check Zone=2")),
        code("PBRK", "A ground point lying on a breakline, shot where the grade changes so that the surface model "
             "can join it to its neighbours.",
             point("terrain/breaklines/general", "breakline", "0.18", "Breakline point"),
             symbol("Breakline Point")),
        code("TC*", "Top edge of a bank or batter, with the ticks pointing downhill.",
             line("terrain/breaklines/bank", "breakline", "0.25", "Bank crest", "Bank Top Edge")),
        code("VL*", "Scarp left by a landslide or by water eating into a bank.",
             line("terrain/breaklines/erosion", "breakline", "0.25", "Slip scarp", "Bank Top Edge")),
    ]),

    # ---- Terrain and Breaklines / Contours (7) -------------------------------------------------------------------
    ("G", "Contours", [
        code("X0*", "Contour at the regular interval between index contours.",
             line("terrain/contours/intermediate", "contour", "0.18", "Regular contour")),
        code("XQ*", "Contour round a hollow at the regular interval, with the ticks pointing downhill.",
             line("terrain/contours/depression", "contour", "0.18", "Regular depression contour",
                  "Depression Contour")),
        code("Y0*", "Extra contour drawn at each whole metre between the regular ones.",
             line("terrain/contours/auxiliary", "contour", "0.13", "Whole-metre extra contour")),
        code("YQ*", "Extra contour drawn at each tenth of a metre, for flat ground.",
             line("terrain/contours/auxiliary", "contour", "0.13", "Tenth-metre extra contour")),
        code("Z*", "Index contour, the heavier line drawn at every fifth contour.",
             line("terrain/contours/index", "contour", "0.35", "Heavy contour", "Index Contour")),
        code("Z0*", "Index contour at the regular index interval.",
             line("terrain/contours/index", "contour", "0.35", "Index contour, regular", "Index Contour")),
        code("ZQ*", "Index contour round a hollow, with the ticks pointing downhill.",
             line("terrain/contours/depression", "contour", "0.35", "Index depression contour",
                  "Depression Contour")),
    ]),

    # ---- Terrain and Breaklines / Water Edges (7) ----------------------------------------------------------------
    ("G", "Water Edges", [
        code("JW*", "Outline of a swamp or marsh, shot along the edge of the wet ground.",
             line("terrain/water/edge", "waterway", "0.18", "Marsh boundary", "Marsh Edge")),
        code("PFHT", "The level that a flood is known to have reached, shot at the mark it left.",
             point("terrain/water/edge", "waterway", "0.25", "Flood level mark"),
             symbol("Water Level Mark")),
        code("PSWA", "A swamp or marsh: a patch of wet, reedy ground marked by a point.",
             point("terrain/water/wetland", "waterway", "0.18", "Swamp or marsh"),
             symbol("Swamp")),
        code("PWLP", "Point where the level of a water surface was read.",
             point("terrain/water/level", "waterway", "0.18", "Water surface level"),
             symbol("Water Level Mark")),
        code("WE*", "Edge of water, with a level taken on it.",
             line("terrain/water/edge", "waterway", "0.25", "Waterline with level", "Water Edge")),
        code("WL*", "Shoreline of a lake.",
             line("terrain/water/lake", "waterway", "0.25", "Lake shoreline", "Water Edge")),
        code("WR*", "Bank edge of a river or creek.",
             line("terrain/water/river", "waterway", "0.25", "River bank edge", "Water Edge")),
    ]),

    # ---- Terrain and Breaklines / Rock and Cliff (2) -------------------------------------------------------------
    ("G", "Rock and Cliff", [
        code("JR*", "Outline of an area of exposed rock, shot along its edge.",
             line("terrain/rock/edge", "contour", "0.18", "Rock area outline", "Rock Outcrop Edge")),
        code("VC*", "Top edge of a cliff or escarpment, with the ticks pointing over the drop.",
             line("terrain/rock/edge", "contour", "0.25", "Escarpment crest", "Cliff Edge")),
    ]),

    # ---- Survey Control and Annotation / Control Marks (28) -------------------------------------------------------
    ("M", "Control Marks", [
        code("PAPN", "An alignment pin: a pin set in the ground to mark a design alignment for setting out.",
             point("control/marks/control", "control", "0.25", "Pin on a design alignment"),
             symbol("Alignment Pin")),
        code("PAPN*", "Alignment pins that carry a string number, which are kept out of the surface model.",
             surface(False)),
        code("PBLT", "A bolt set in concrete or rock as a survey mark, shot at the centre of its head.",
             point("control/marks/control", "control", "0.25", "Survey bolt"),
             symbol("Survey Spike")),
        code("PBLT*", "Survey bolts that carry a string number, which are kept out of the surface model.",
             surface(False)),
        code("PBMK", "A bench mark: a mark of known height that levelling is run from.",
             point("control/marks/level", "control", "0.25", "Levelling bench mark"),
             symbol("Level Benchmark")),
        code("PBMZ", "A bench mark kept under its own separate code so it can be listed and checked on its own.",
             point("control/marks/level", "control", "0.25", "Bench mark, separate list"),
             symbol("Level Benchmark", hide=True)),
        code("PCMK", "A permanent control mark of the road authority, used to tie road work into the survey network.",
             point("control/marks/control", "control", "0.25", "Road authority control mark"),
             symbol("Control Mark")),
        code("PCMK*", "Keeps the road authority's control marks out of the ground surface when the code carries a "
             "string number.",
             surface(False)),
        code("PDPY", "A temporary peg driven into the ground to carry a height for a dumpy level.",
             point("control/marks/temporary", "control", "0.25", "Levelling peg"),
             symbol("Temporary Control")),
        code("PDPY*", "Keeps levelling pegs out of the ground surface when the code carries a string number.",
             surface(False)),
        code("PDSM", "A permanent survey mark placed by the water authority and used as a height or position "
             "reference.",
             point("control/marks/control", "control", "0.25", "Water authority mark"),
             symbol("Fixed Survey Mark")),
        code("PDSM*", "Keeps the water authority's survey marks out of the ground surface when the code carries a "
             "string number.",
             surface(False)),
        code("PHCP", "A control point held in plan only, with no level, used to control photogrammetry.",
             point("control/marks/control", "control", "0.25", "Plan-only control mark"),
             symbol("Control Mark")),
        code("PMCP", "A minor control point set to support the main control network.",
             point("control/marks/control", "control", "0.25", "Minor control mark"),
             symbol("Control Mark")),
        code("PPCN", "The ground point at the centre of an aerial photograph, used to position and orient the image.",
             point("control/marks/control", "control", "0.25", "Photo centre mark"),
             symbol("Photo Centre")),
        code("PPSM", "A permanent survey mark with coordinates, which the survey can be tied to.",
             point("control/marks/control", "control", "0.25", "Coordinated survey mark"),
             symbol("Fixed Survey Mark", hide=True)),
        code("PSS*", "Keeps every station mark out of the ground surface: control marks fix positions and are not "
             "terrain.",
             surface(False)),
        code("PSSA", "A survey mark of unspecified type, used when the kind of mark is not given.",
             point("control/marks/control", "control", "0.25", "Survey mark, type unstated"),
             symbol("Control Mark")),
        code("PSSB", "The broad arrow cut in stone or a kerb to mark a bench level.",
             point("control/marks/level", "control", "0.25", "Broad arrow bench mark"),
             symbol("Level Benchmark")),
        code("PSSC", "A peg driven in as a station for the job.",
             point("control/marks/temporary", "control", "0.25", "Station peg"),
             symbol("Temporary Control")),
        code("PSSD", "A station marked by a plug, a drill hole, an iron pipe or a reference block.",
             point("control/marks/control", "control", "0.25", "Plug, pipe or block station"),
             symbol("Fixed Survey Mark")),
        code("PSSE", "A station marked by a bolt, a nail, a spike or a similar small metal fixing.",
             point("control/marks/temporary", "control", "0.25", "Bolt, nail or spike station"),
             symbol("Survey Spike")),
        code("PSSF", "A trigonometric station, a primary control mark of the geodetic network.",
             point("control/marks/control", "control", "0.25", "Trigonometric station"),
             symbol("Trigonometric Station")),
        code("PSSG", "A mark of the permanent, state or project control networks, used as a survey station.",
             point("control/marks/control", "control", "0.25", "Network control mark"),
             symbol("Control Mark")),
        code("PSSM", "A mark of the state geodetic network, with published coordinates and heights.",
             point("control/marks/control", "control", "0.25", "State geodetic mark"),
             symbol("Control Mark")),
        code("PTSS", "Trigonometric station built as a concrete pillar.",
             point("control/marks/control", "control", "0.25", "Concrete survey pillar"),
             symbol("Trigonometric Station")),
        code("PTSS*", "Keeps concrete trig pillar points out of the ground surface.",
             surface(False)),
        code("PVCP", "Point whose height alone is held as control, such as a target for aerial photo mapping.",
             point("control/marks/level", "control", "0.25", "Height control point"),
             symbol("Level Benchmark")),
    ]),

    # ---- Survey Control and Annotation / Investigation Points (9) ------------------------------------------------
    ("M", "Investigation Points", [
        code("PBHX", "A borehole, shot at the centre of its collar.",
             point("control/investigation/ground", "control", "0.25", "Borehole point"),
             symbol("Borehole")),
        code("PGEO", "A pit excavated to inspect the ground or to expose a buried service, shot at its centre.",
             point("control/investigation/ground", "control", "0.25", "Geotechnical test pit"),
             symbol("Test Pit")),
        code("PHNL", "A pothole dug to expose a service, recorded without a level on the service.",
             point("control/investigation/pothole", "control", "0.25", "Pothole, no level"),
             symbol("Pothole")),
        code("PHNL*", "Keeps pothole points without a level out of the ground surface.",
             surface(False)),
        code("PHRL", "A pothole dug to expose a service, with the level of the exposed service recorded.",
             point("control/investigation/pothole", "control", "0.25", "Pothole with level"),
             symbol("Pothole")),
        code("PSTS", "A seismic test shot: the point where a charge was fired for a seismic survey.",
             point("control/investigation/ground", "control", "0.25", "Seismic shot point"),
             symbol("Seismic Shot Point")),
        code("SL*", "Line followed by a seismic survey, along which shots and receivers are set.",
             line("control/investigation/geophysics", "control", "0.18", "Seismic survey line",
                  "Hidden Outline")),
        code("XB*", "Bottom edge of a trench dug to expose buried services.",
             line("control/investigation/trench", "control", "0.18", "Trench floor edge", "Hidden Outline")),
        code("XT*", "Top edge of a trench dug to expose buried services.",
             line("control/investigation/trench", "control", "0.25", "Trench top edge")),
    ]),

    # ---- Survey Control and Annotation / Text and Notes (14) ------------------------------------------------------
    ("M", "Text and Notes", [
        code("1", "Plan text 1.5 mm high on paper, the finest size, for crowded detail.",
             text(1.5)),
        code("1*", "Layer and pen for 1.5 mm plan text, so it can be switched off with the other notes.",
             layer_only("control/annotation/text", "note", "0.13", "Finest text")),
        code("2", "Plan text 2.5 mm high on paper, the everyday size for notes and labels.",
             text(2.5)),
        code("2*", "Layer and pen for 2.5 mm plan text, so it can be switched off with the other notes.",
             layer_only("control/annotation/text", "note", "0.13", "Small text")),
        code("3", "Plan text 3.5 mm high on paper, for prominent notes and sub-headings.",
             text(3.5)),
        code("3*", "Layer and pen for 3.5 mm plan text, so it can be switched off with the other notes.",
             layer_only("control/annotation/text", "note", "0.13", "Medium text")),
        code("4", "Plan text 5 mm high on paper, large enough for headings.",
             text(5)),
        code("4*", "Layer and pen for 5.0 mm plan text, so it can be switched off with the other notes.",
             layer_only("control/annotation/text", "note", "0.13", "Large text")),
        code("5", "Plan text 7 mm high on paper, for titles and major headings.",
             text(7)),
        code("5*", "Layer and pen for 7.0 mm plan text, so it can be switched off with the other notes.",
             layer_only("control/annotation/text", "note", "0.13", "Extra large text")),
        code("7", "Plan text 10 mm high on paper, the largest size, for sheet titles.",
             text(10)),
        code("7*", "Layer and pen for 10.0 mm plan text, so it can be switched off with the other notes.",
             layer_only("control/annotation/text", "note", "0.13", "Display text")),
        code("8", "Parcel label text 2.5 mm high on paper: the lot number and the deposited plan number.",
             text(2.5)),
        code("8*", "Layer and pen for parcel label text, so lot and plan numbers can be switched off together.",
             layer_only("control/annotation/text", "note", "0.13", "Lot and plan labels")),
    ]),

    # ---- Survey Control and Annotation / Plan Marks (3) ----------------------------------------------------------
    ("M", "Plan Marks", [
        code("DE*", "Outer limit of the area digitised from aerial photography, where detail comes from the photo "
             "and not the field.",
             line("control/plan/limit", "note", "0.35", "Photo digitising limit", "Photo Detail Boundary")),
        code("FS*", "Outer limit of the ground that was surveyed in the field, drawn over a photo base.",
             line("control/plan/limit", "note", "0.50", "Field survey limit", "Limit of Survey Line")),
        code("NF*", "Limit of an area where photo-based mapping was completed or checked by survey in the field.",
             line("control/plan/limit", "note", "0.25", "Photo completion limit", "Limit of Survey Line")),
    ]),

    # ---- Miscellaneous / Hazards and Heritage (12) ----------------------------------------------------------------
    ("X", "Hazards and Heritage", [
        code("PHAZ", "A site that endangers people or later works, such as contaminated or unstable ground.",
             point("miscellaneous/hazards/hazard", "hazard", "0.25", "Site that endangers people"),
             symbol("Hazard Point")),
        code("PHAZ*", "Keeps hazardous-site points out of the ground surface.",
             surface(False)),
        code("PHEC", "A point inside a plant or animal community recorded as endangered, to be avoided by works.",
             point("miscellaneous/hazards/habitat", "tree", "0.25", "Threatened community"),
             symbol("Habitat Marker")),
        code("PHEC*", "Keeps endangered-community points out of the ground surface.",
             surface(False)),
        code("PHEE", "A point inside a vegetation community listed by law as an endangered ecological community.",
             point("miscellaneous/hazards/habitat", "tree", "0.25", "Listed ecological community"),
             symbol("Habitat Marker")),
        code("PHEE*", "Keeps listed ecological community points out of the ground surface.",
             surface(False)),
        code("PHIA", "A place of Indigenous cultural heritage, marked so that works can keep clear of it.",
             point("miscellaneous/hazards/heritage", "note", "0.25", "Indigenous cultural site"),
             symbol("Heritage Item")),
        code("PHIA*", "Keeps Indigenous heritage points out of the ground surface.",
             surface(False)),
        code("PHIS", "A point of historic interest, such as a plaque, a marker stone or an old relic.",
             point("miscellaneous/hazards/heritage", "note", "0.25", "Historic interest point"),
             symbol("Heritage Item")),
        code("PHNI", "A heritage site without Indigenous significance, such as a historic ruin, relic or building "
             "remnant.",
             point("miscellaneous/hazards/heritage", "note", "0.25", "Other heritage site"),
             symbol("Heritage Item")),
        code("PHNI*", "Keeps non-Indigenous heritage points out of the ground surface.",
             surface(False)),
        code("PHST", "A grave headstone in a cemetery, shot at the middle of its base.",
             point("miscellaneous/hazards/heritage", "note", "0.25", "Grave headstone"),
             symbol("Headstone")),
    ]),

    # ---- Miscellaneous / General (3) -----------------------------------------------------------------------------
    ("X", "General", [
        code("HP*", "Outline of an area singled out for attention; the code does not say why.",
             line("miscellaneous/general/generic", "note", "0.25", "Area for attention"),
             surface(False)),
        code("PVID", "Point where two surveyed lines meet, usually worked out by calculation.",
             point("miscellaneous/general/generic", "note", "0.25", "Crossing of two lines"),
             symbol("Generic Point")),
        code("QQ*", "Line of check shots used to test the survey or the model, kept out of the ground surface.",
             line("miscellaneous/general/check", "control", "0.13", "Survey check line"),
             surface(False)),
    ]),

    # ---- Miscellaneous / Land Use Areas (6) ----------------------------------------------------------------------
    ("X", "Land Use Areas", [
        code("OB*", "General outline of a built-up area, drawn as one extent and not building by building.",
             line("miscellaneous/land-use/area", "building", "0.13", "Built-up area")),
        code("OC*", "Outline of a cemetery or burial ground.",
             line("miscellaneous/land-use/area", "note", "0.18", "Cemetery outline")),
        code("OG*", "Outline of a golf course.",
             line("miscellaneous/land-use/area", "planting", "0.18", "Golf course outline", "Grass Edge")),
        code("OH*", "Outline of a stockpile of loose material, shot round its toe.",
             line("miscellaneous/land-use/area", "breakline", "0.18", "Stockpile outline", "Bank Toe Edge")),
        code("OO*", "Outline of a sporting arena, such as a stadium, an oval or a court.",
             line("miscellaneous/land-use/area", "building", "0.18", "Sporting arena outline")),
        code("OP*", "Outline of a park or other open space.",
             line("miscellaneous/land-use/area", "planting", "0.18", "Park or open space outline", "Grass Edge")),
    ]),

    # ---- Design and Setting Out / Road Design Strings (27) --------------------------------------------------------
    ("D", "Road Design Strings", [
        code("BD*", "A road design string with no named role, left free for a design package to give its own "
             "meaning.",
             line("design/road-design-strings/unclassified", "pavement", "0.18", "Spare road string, first")),
        code("C9*", "A road design string with no named role, left free for a design package to give its own "
             "meaning.",
             line("design/road-design-strings/unclassified", "pavement", "0.18", "Spare road string, second")),
        code("CL*", "A road design string with no named role, left free for a design package to give its own "
             "meaning.",
             line("design/road-design-strings/unclassified", "pavement", "0.18", "Spare road string, third")),
        code("CM*", "A road design string with no named role, left free for a design package to give its own "
             "meaning.",
             line("design/road-design-strings/unclassified", "pavement", "0.18", "Spare road string, fourth")),
        code("CR*", "A road design string with no named role, left free for a design package to give its own "
             "meaning.",
             line("design/road-design-strings/unclassified", "pavement", "0.18", "Spare road string, fifth")),
        code("CS*", "A road design string with no named role, left free for a design package to give its own "
             "meaning.",
             line("design/road-design-strings/unclassified", "pavement", "0.18", "Spare road string, sixth")),
        code("E0*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-e", "pavement", "0.18", "Unclassified road string")),
        code("E3*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-e", "pavement", "0.18", "Unclassified road string")),
        code("E4*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-e", "pavement", "0.18", "Unclassified road string")),
        code("E5*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-e", "pavement", "0.18", "Unclassified road string")),
        code("E6*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-e", "pavement", "0.18", "Unclassified road string")),
        code("E7*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-e", "pavement", "0.18", "Unclassified road string")),
        code("EB*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-e", "pavement", "0.18", "Unclassified road string")),
        code("EF*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-e", "pavement", "0.18", "Unclassified road string")),
        code("ER*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-e", "pavement", "0.18", "Unclassified road string")),
        code("ET*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-e", "pavement", "0.18", "Unclassified road string")),
        code("I0*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-i", "pavement", "0.18", "Unclassified road string")),
        code("I1*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-i", "pavement", "0.18", "Unclassified road string")),
        code("I2*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-i", "pavement", "0.18", "Unclassified road string")),
        code("I3*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-i", "pavement", "0.18", "Unclassified road string")),
        code("I4*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-i", "pavement", "0.18", "Unclassified road string")),
        code("I5*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-i", "pavement", "0.18", "Unclassified road string")),
        code("I6*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-i", "pavement", "0.18", "Unclassified road string")),
        code("I7*", "Road design string of a kind that has no recorded purpose, kept so that a design file still "
             "plots.",
             line("design/road-design-strings/unclassified-i", "pavement", "0.18", "Unclassified road string")),
        code("I8*", "A road design string with no role assigned yet; the first of two spare classes held for the "
             "designer's own use.",
             line("design/road-design-strings/unclassified", "pavement", "0.18",
                  "Unassigned road string, first")),
        code("I9*", "A road design string with no role assigned yet; the second of two spare classes held for the "
             "designer's own use.",
             line("design/road-design-strings/unclassified", "pavement", "0.18",
                  "Unassigned road string, second")),
        code("R*", "Road design string starting with R that no more specific code claims.",
             line("design/road-design-strings/unclassified", "pavement", "0.25", "Unclassified road string")),
    ]),

    # ---- Design and Setting Out / Lane Marking Design (25) --------------------------------------------------------
    ("D", "Lane Marking Design", [
        code("L0*", "Separation line along a bicycle path, design type S4.",
             line("design/lane-marking-design/dividing", "pavement", "0.18", "Bike path separation, type 4",
                  "Bicycle Lane Marking")),
        code("L2*", "Dividing line between opposing lanes of a multi-lane road, design type S2.",
             line("design/lane-marking-design/dividing", "pavement", "0.25", "Multi-lane dividing line",
                  "Dividing Line Marking")),
        code("L3*", "Lane line between lanes travelling the same way on a multi-lane road, design type L1.",
             line("design/lane-marking-design/lane-lines", "pavement", "0.18", "Multi-lane lane line, type 1",
                  "Dashed Lane Marking")),
        code("L4*", "Lane line between the lanes of a multi-lane roundabout, design type L4.",
             line("design/lane-marking-design/lane-lines", "pavement", "0.18", "Roundabout lane line",
                  "Dashed Lane Marking")),
        code("L5*", "Separation line along a bicycle path, design type S5.",
             line("design/lane-marking-design/dividing", "pavement", "0.18", "Bike path separation, type 5")),
        code("L6*", "Lane line beside a bus lane, design type L6.",
             line("design/lane-marking-design/lane-lines", "pavement", "0.18", "Bus lane line",
                  "Dashed Lane Marking")),
        code("L7*", "Lane line beside a bicycle lane, design type L7.",
             line("design/lane-marking-design/lane-lines", "pavement", "0.18", "Bike lane line",
                  "Bicycle Lane Marking")),
        code("L8*", "Dividing line between opposing lanes of a two-lane road, design type S1.",
             line("design/lane-marking-design/dividing", "pavement", "0.25", "Two-lane dividing line, type 1",
                  "Dividing Line Marking")),
        code("L9*", "Dividing line between opposing lanes of a two-lane road, design type S3.",
             line("design/lane-marking-design/dividing", "pavement", "0.25", "Two-lane dividing line, type 3",
                  "Dividing Line Marking")),
        code("LA*", "Continuity line, design type C1, that carries a lane edge across a junction.",
             line("design/lane-marking-design/intersection", "pavement", "0.18", "Continuity line, type 1",
                  "Lane Continuity Marking")),
        code("LD*", "Double barrier line, design type BB: two unbroken lines that forbid crossing in either "
             "direction.",
             line("design/lane-marking-design/dividing", "pavement", "0.25", "Designed double barrier",
                  "Double Barrier Marking")),
        code("LE*", "Edge line along the side of the carriageway, in its general form.",
             line("design/lane-marking-design/edge-lines", "pavement", "0.18", "General edge line")),
        code("LF*", "Continuity line, design type C4, that carries a bicycle lane across a junction.",
             line("design/lane-marking-design/intersection", "pavement", "0.18", "Bike lane continuity line",
                  "Lane Continuity Marking")),
        code("LH*", "Combined barrier and separation line, design type BS1: an unbroken line beside a broken one.",
             line("design/lane-marking-design/dividing", "pavement", "0.25", "Barrier and separation, type 1",
                  "Left Broken Barrier Marking")),
        code("LJ*", "Kerbside line marking a clearway along the kerb, design type C2.",
             line("design/lane-marking-design/edge-lines", "pavement", "0.18", "Clearway kerbside line",
                  "Lane Continuity Marking")),
        code("LK*", "Kerbside line marking a no-stopping length of kerb, design type C3.",
             line("design/lane-marking-design/edge-lines", "pavement", "0.25", "No-stopping kerbside line")),
        code("LM*", "Edge line round a painted median, design type E5.",
             line("design/lane-marking-design/edge-lines", "pavement", "0.18", "Painted median edge line")),
        code("LN*", "Lane line between lanes travelling the same way on a multi-lane road, design type L3.",
             line("design/lane-marking-design/lane-lines", "pavement", "0.18", "Multi-lane lane line, type 3",
                  "Dashed Lane Marking")),
        code("LR*", "Combined barrier and separation line, design type BS2, the mirror form of BS1.",
             line("design/lane-marking-design/dividing", "pavement", "0.25", "Barrier and separation, type 2",
                  "Right Broken Barrier Marking")),
        code("LS*", "Stop line at traffic signals, design type TF.",
             line("design/lane-marking-design/intersection", "pavement", "0.35", "Signal stop line")),
        code("LT*", "Turn line, design type T1, that guides turning traffic through a junction.",
             line("design/lane-marking-design/intersection", "pavement", "0.18", "Turn guide line",
                  "Lane Continuity Marking")),
        code("LU*", "A plain unbroken painted line that fits none of the named types.",
             line("design/lane-marking-design/edge-lines", "pavement", "0.18", "Plain unbroken line")),
        code("LW*", "Give way line, design type TB, painted across the approach to a junction.",
             line("design/lane-marking-design/intersection", "pavement", "0.18", "Give way line",
                  "Lane Continuity Marking")),
        code("LY*", "A bicycle crossing painted across the carriageway.",
             line("design/lane-marking-design/intersection", "pavement", "0.18", "Bicycle crossing",
                  "Bicycle Crossing Marking")),
        code("LZ*", "A zebra pedestrian crossing painted across the carriageway.",
             line("design/lane-marking-design/intersection", "pavement", "0.18", "Designed zebra crossing",
                  "Zebra Crossing Marking")),
    ]),

    # ---- Design and Setting Out / Earthworks Design (14) ----------------------------------------------------------
    ("D", "Earthworks Design", [
        code("DJ*", "Design edge of a table drain on the side furthest from the road.",
             line("design/earthworks-design/table-drain", "waterway", "0.25", "Table drain, far edge")),
        code("DK*", "Design edge of a table drain on the side nearest the road.",
             line("design/earthworks-design/table-drain", "waterway", "0.25", "Table drain, near edge")),
        code("DM*", "Line along a median designed to sit lower than the carriageways beside it.",
             line("design/earthworks-design/formation", "breakline", "0.25", "Sunken median")),
        code("E1*", "Outer, downslope edge of a bench cut into a slope in the earthworks design.",
             line("design/earthworks-design/batter", "breakline", "0.25", "Bench outer edge")),
        code("E2*", "Inner edge of a bench, where it meets the rising slope behind.",
             line("design/earthworks-design/batter", "breakline", "0.25", "Bench inner edge")),
        code("EH*", "Line where the formation turns into the batter slope, the hinge of the cross-section.",
             line("design/earthworks-design/batter", "breakline", "0.18", "Batter hinge line")),
        code("EV*", "Outer edge of the verge as designed, on the side furthest from the road.",
             line("design/earthworks-design/formation", "breakline", "0.25", "Verge outer edge")),
        code("EW*", "Edge of a designed footpath on the side nearest the road.",
             line("design/earthworks-design/formation", "pavement", "0.25", "Footpath near edge")),
        code("EX*", "Edge of a designed footway on the side furthest from the road.",
             line("design/earthworks-design/formation", "pavement", "0.25", "Footway far edge")),
        code("IA*", "Design line where an earthworks batter meets the existing ground, the catch point of a cut or "
             "a fill.",
             line("design/earthworks-design/interface", "breakline", "0.18", "Batter meets ground",
                  "Bank Toe Edge")),
        code("IB*", "Design line along the front edge of a bench cut into an earthworks batter.",
             line("design/earthworks-design/interface", "breakline", "0.18", "Bench front edge",
                  "Bank Top Edge")),
        code("IC*", "A second design string for the front edge of an earthworks bench, drawn as a plain line.",
             line("design/earthworks-design/interface", "breakline", "0.18", "Bench front edge, plain")),
        code("ID*", "Design line along the front edge of a table drain, the side nearest the road, where it meets "
             "the earthworks.",
             line("design/earthworks-design/interface", "breakline", "0.18", "Table drain, road side",
                  "Dish Drain Edge")),
        code("IE*", "Design line along the back edge of a table drain, on the side away from the road.",
             line("design/earthworks-design/interface", "breakline", "0.18", "Table drain, far side")),
    ]),

    # ---- Design and Setting Out / Kerb and Carriageway Design (15) ------------------------------------------------
    ("D", "Kerb and Carriageway Design", [
        code("C0*", "Design line for the edge of a median.",
             line("design/kerb-and-carriageway-design/carriageway", "pavement", "0.25", "Median edge design")),
        code("C1*", "Design line for the first base line of a kerb, at its foot.",
             line("design/kerb-and-carriageway-design/kerb", "kerb", "0.25", "Kerb base, first")),
        code("C2*", "Design line for the second base line of a kerb, at its foot.",
             line("design/kerb-and-carriageway-design/kerb", "kerb", "0.25", "Kerb base, second")),
        code("C3*", "Design line for the first hinge of a kerb, a break in its cross-section.",
             line("design/kerb-and-carriageway-design/kerb", "kerb", "0.18", "Kerb hinge, first")),
        code("C4*", "Design line for the second hinge of a kerb, a break in its cross-section.",
             line("design/kerb-and-carriageway-design/kerb", "kerb", "0.18", "Kerb hinge, second")),
        code("C5*", "Design line for the third hinge of a kerb, a break in its cross-section.",
             line("design/kerb-and-carriageway-design/kerb", "kerb", "0.18", "Kerb hinge, third")),
        code("C6*", "Design line for the fourth hinge of a kerb, a break in its cross-section.",
             line("design/kerb-and-carriageway-design/kerb", "kerb", "0.18", "Kerb hinge, fourth")),
        code("C7*", "Design line for the fifth hinge of a kerb, a break in its cross-section.",
             line("design/kerb-and-carriageway-design/kerb", "kerb", "0.18", "Kerb hinge, fifth")),
        code("C8*", "Design line for the sixth hinge of a kerb, a break in its cross-section.",
             line("design/kerb-and-carriageway-design/kerb", "kerb", "0.18", "Kerb hinge, sixth")),
        code("CE*", "Design line for the edge of the sealed carriageway surface.",
             line("design/kerb-and-carriageway-design/carriageway", "pavement", "0.25",
                  "Bituminous edge design")),
        code("CF*", "Design line along the flow line of a gutter.",
             line("design/kerb-and-carriageway-design/carriageway", "kerb", "0.18", "Gutter flow line design")),
        code("CH*", "Design line along the lip of the gutter at the edge of the through lane.",
             line("design/kerb-and-carriageway-design/carriageway", "kerb", "0.18", "Gutter lip design")),
        code("ES*", "Outer edge of the road shoulder as designed, on the side away from the traffic lanes.",
             line("design/kerb-and-carriageway-design/carriageway", "pavement", "0.25", "Shoulder outer edge")),
        code("NJ*", "Design line along the joint between two pavement areas, such as new work meeting existing.",
             line("design/kerb-and-carriageway-design/carriageway", "pavement", "0.13",
                  "Pavement interface joint")),
        code("NL*", "Design line for the crown of a carriageway that is offset from the road's master alignment.",
             line("design/kerb-and-carriageway-design/carriageway", "pavement", "0.25",
                  "Offset carriageway crown", "Road Centreline")),
    ]),

    # ---- Design and Setting Out / Bridge Design (6) --------------------------------------------------------------
    ("D", "Bridge Design", [
        code("AU*", "Design line for the first hinge of a bridge abutment, a break in its cross-section.",
             line("design/bridge-design/substructure", "building", "0.25", "Abutment hinge, first")),
        code("AV*", "Design line for the second hinge of a bridge abutment, a break in its cross-section.",
             line("design/bridge-design/substructure", "building", "0.25", "Abutment hinge, second")),
        code("AY*", "Design line for the third hinge of a bridge abutment, a break in its cross-section.",
             line("design/bridge-design/substructure", "building", "0.25", "Abutment hinge, third")),
        code("AZ*", "Design line for the fourth hinge of a bridge abutment, a break in its cross-section.",
             line("design/bridge-design/substructure", "building", "0.25", "Abutment hinge, fourth")),
        code("BA*", "Design line along the face of a bridge abutment.",
             line("design/bridge-design/substructure", "building", "0.35", "Abutment design line")),
        code("BQ*", "Design line for the outline of a bridge pier.",
             line("design/bridge-design/substructure", "building", "0.35", "Pier design line")),
    ]),

    # ---- Design and Setting Out / Drainage Design (6) ------------------------------------------------------------
    ("D", "Drainage Design", [
        code("DH*", "Design line for the wall that surrounds the end of a drain or culvert.",
             line("design/drainage-design/structure", "stormwater", "0.25", "Designed headwall")),
        code("DI*", "Design line for a stormwater drainage pipe.",
             line("design/drainage-design/pipe", "stormwater", "0.35", "Designed drain pipe",
                  "Stormwater Pipe Line")),
        code("DL*", "Design line for a concrete or otherwise lined drain that catches runoff above a cutting.",
             line("design/drainage-design/channel", "waterway", "0.35", "Lined catch drain",
                  "Open Drain Invert")),
        code("DU*", "Design line for an earth drain with no lining that catches runoff above a cutting.",
             line("design/drainage-design/channel", "waterway", "0.18", "Unlined catch drain",
                  "Open Drain Invert")),
        code("DV*", "Design line for a culvert that carries water under a road or embankment.",
             line("design/drainage-design/pipe", "stormwater", "0.35", "Designed culvert", "Culvert Outline")),
        code("SH*", "Design line along the edge of a subsoil drain.",
             line("design/drainage-design/subsoil", "stormwater", "0.18", "Designed subsoil drain edge",
                  "Subsoil Drain Line")),
    ]),

    # ---- Design and Setting Out / Barrier and Fence Design (5) ---------------------------------------------------
    ("D", "Barrier and Fence Design", [
        code("AD*", "Intended position of a bollard on a design, set out as a point and drawn with the bollard "
             "symbol.",
             point("design/barrier-and-fence-design/barrier", "street", "0.25", "Designed bollard"),
             symbol("Bollard")),
        code("BF*", "Design line for a W-beam or thrie-beam guard rail on the left-hand side of the alignment.",
             line("design/barrier-and-fence-design/barrier", "street", "0.35", "Left guard rail design",
                  "Guard Rail Line")),
        code("BG*", "Design line for a W-beam or thrie-beam guard rail on the right-hand side of the alignment.",
             line("design/barrier-and-fence-design/barrier", "street", "0.35", "Right guard rail design",
                  "Guard Rail Line")),
        code("FN*", "Design line for a fence along a property boundary.",
             line("design/barrier-and-fence-design/fence", "fence", "0.25", "Designed boundary fence",
                  "Chain Mesh Fence")),
        code("SA*", "Design line for the rail of a pedestrian fence along a footpath or a bridge.",
             line("design/barrier-and-fence-design/fence", "fence", "0.25", "Designed pedestrian rail",
                  "Post and Rail Fence")),
    ]),

    # ---- Design and Setting Out / Master Strings (10) -------------------------------------------------------------
    ("D", "Master Strings", [
        code("MA*", "Master string for the edge of a median, one of the strings that control the road model.",
             line("design/master-strings/carriageway", "pavement", "0.25", "Median edge master string")),
        code("MC*", "Master control string, the main alignment from which the other road model strings are built.",
             line("design/master-strings/carriageway", "pavement", "0.35", "Master control string")),
        code("MD*", "Master string for the edge of a driveway.",
             line("design/master-strings/carriageway", "pavement", "0.25", "Driveway master string")),
        code("ME*", "Master string for the earthworks of the road model, such as batters and benching.",
             line("design/master-strings/works", "breakline", "0.25", "Earthworks master string")),
        code("MK*", "Master string for the edge of the sealed bitumen surface.",
             line("design/master-strings/carriageway", "pavement", "0.25", "Sealed edge master string")),
        code("MP*", "Master string for the edge of an off-road path.",
             line("design/master-strings/carriageway", "pavement", "0.25", "Off-road path master string")),
        code("MS*", "Master string for a safety barrier along the road.",
             line("design/master-strings/works", "street", "0.25", "Safety barrier master string",
                  "Guard Rail Line")),
        code("MT*", "Master string for the edge of a traffic island or a turning head.",
             line("design/master-strings/carriageway", "pavement", "0.25", "Island or turn head string")),
        code("MW*", "Master string for a retaining wall beside the road.",
             line("design/master-strings/works", "wall", "0.25", "Retaining wall master string",
                  "Earth Retaining Wall")),
        code("MX*", "Master string for temporary works that the finished road does not keep.",
             line("design/master-strings/works", "hazard", "0.25", "Temporary works master string",
                  "Temporary Works Line")),
    ]),

    # ---- Design and Setting Out / Design General (7) -------------------------------------------------------------
    ("D", "Design General", [
        code("DY*", "Design edge of a driveway.",
             line("design/design-general/path", "pavement", "0.25", "Designed driveway")),
        code("E8*", "Edge of a designed path on the side nearest the road.",
             line("design/design-general/path", "pavement", "0.25", "Path near edge")),
        code("E9*", "Edge of a designed path on the side furthest from the road.",
             line("design/design-general/path", "pavement", "0.25", "Path far edge")),
        code("HE*", "Hatch ticks on the left of the line, showing which side of a designed wall is its face.",
             line("design/design-general/wall-hatch", "wall", "0.13", "Left wall face hatching",
                  "Left Wall Face Hatch Line")),
        code("HL*", "Design line that marks the edge of a hazard area.",
             line("design/design-general/hazard", "hazard", "0.25", "Designed hazard edge", "Hazard Boundary")),
        code("HR*", "Hatch ticks on the right of the line, showing which side of a designed wall is its face.",
             line("design/design-general/wall-hatch", "wall", "0.13", "Right wall face hatching",
                  "Right Wall Face Hatch Line")),
        code("PLIG*", "The designed position of a road lighting pole, for setting out.",
             point("design/design-general/lighting", "street", "0.25", "Designed lighting pole")),
    ]),
]


CODES = []
for _class, _group, _codes in SECTIONS:
    for _c in _codes:
        CODES.append(dict(_c, **{"class": _class, "group": _GROUP[_group] if _group else ""}))
del _class, _group, _codes, _c


def feature_of(c):
    """The code's feature rule, or None when it has none (a surface-only key, a text key)."""
    return next((r for r in c["rules"] if r["sets"] == "feature"), None)


def kind_of(c):
    """What a code draws, in one word: line, point, both (a line with a symbol at every vertex), text, layer (a
    layer-only key), pipe (the bare `*`) or surface (a key that only sets the surface flag)."""
    sets = [r["sets"] for r in c["rules"]]
    feature = feature_of(c)
    if "pipe" in sets:
        return "pipe"
    if "text" in sets:
        return "text"
    if feature is None:
        return "surface" if sets == ["surface"] else "other"
    if feature.get("draw") == "line":
        return "both" if "symbol" in sets else "line"
    if feature.get("draw") == "point":
        return "point"
    return "layer"


def styling(c):
    """How a code looks, in one dict: `colour`, `layer`, `weight`, `comment`, `linestyle` and `symbol`, each None
    where the code has no such thing. The catalogue and the picture scripts read this and not the rules."""
    feature = feature_of(c)
    symbol_rule = next((r for r in c["rules"] if r["sets"] == "symbol"), None)
    text_rule = next((r for r in c["rules"] if r["sets"] == "text"), None)
    linestyle = feature.get("linestyle") if feature else None
    return {
        "colour": feature["colour"] if feature else (text_rule["colour"] if text_rule else None),
        "layer": feature["layer"] if feature else None,
        "weight": feature["weight"] if feature else None,
        "comment": feature["comment"] if feature else None,
        "linestyle": linestyle,
        "symbol": symbol_rule["symbol"] if symbol_rule else None,
        "hide": bool(symbol_rule and symbol_rule.get("hide")),
    }


def expand_rules(c):
    """The rules one code expands to, in the order they are written: the order of the code's own rules. Every
    rule is keyed by the code's key. The assembler copies this expansion and the self-check lints what it makes."""
    key = c["key"]
    feature = feature_of(c)
    rules = []
    for r in c["rules"]:
        sets = r["sets"]
        out = {"key": key, "sets": sets}
        if sets == "feature":
            for member in ("layer", "colour", "draw", "linestyle", "weight"):
                if member in r:
                    out[member] = r[member]
            out["group"] = c["group"]
            out["comment"] = r["comment"]
        elif sets == "symbol":
            out["symbol"] = {"name": r["symbol"], "colour": feature["colour"]}
            if r.get("hide"):
                out["hide"] = True
        elif sets == "text":
            out["text"] = {m: r[m] for m in ("style", "colour", "units", "size", "justifyX", "justifyY")}
        elif sets in ("pipe", "vertexPipe", "segmentPipe"):
            out[sets] = {m: r[m] for m in ("justify", "shape", "size1", "size2", "active") if m in r}
        elif sets in ("attributes", "vertexAttributes"):
            out[sets] = [dict(a) for a in r["attributes"]]
        elif sets == "surface":
            out["surface"] = r["surface"]
        else:
            raise ValueError("unknown kind of rule %r in %s" % (sets, key))
        rules.append(out)
    return rules


def definitions_used():
    """(linestyle names, symbol names) the codes name, without the plain `continuous` line."""
    linestyles = {r["linestyle"] for c in CODES for r in c["rules"] if r["sets"] == "feature" and r.get("linestyle")}
    symbols = {r["symbol"] for c in CODES for r in c["rules"] if r["sets"] == "symbol"}
    return linestyles - {"continuous"}, symbols


# ---- the self-check ------------------------------------------------------------------------------------------

CONTROL_WORDS = ("ST", "END", "CL", "BC", "EC", "JPN", "RECT")
# The words CLAUDE.md section 9 keeps out of new text, assembled from pieces so this file does not hold them. The
# bare name of the other built-in is a word of this document and not of the data, so it is barred from the data
# only: a data string names no library.
FORBIDDEN = re.compile("|".join(["1" + "2d", "ex" + "ds", "tf" + "nsw", "transport for n" + "sw", r"\bn" + "sw\\b"]), re.I)
RULE_KINDS = ("feature", "symbol", "text", "pipe", "vertexPipe", "segmentPipe", "attributes", "vertexAttributes",
              "surface")
MEMBERS = {
    "feature": {"sets", "layer", "colour", "draw", "linestyle", "weight", "comment"},
    "symbol": {"sets", "symbol", "hide"},
    "text": {"sets", "style", "colour", "units", "size", "justifyX", "justifyY"},
    "pipe": {"sets", "justify", "shape", "size1", "size2", "active"},
    "attributes": {"sets", "attributes"},
    "surface": {"sets", "surface"},
}
MEMBERS["vertexPipe"] = MEMBERS["segmentPipe"] = MEMBERS["pipe"]
MEMBERS["vertexAttributes"] = MEMBERS["attributes"]
# The members each kind of rule may carry in the file (docs/customisation.md): anything else is "field outside section".
RULE_MEMBERS = {
    "feature": {"key", "sets", "layer", "colour", "draw", "linestyle", "weight", "group", "comment"},
    "symbol": {"key", "sets", "symbol", "hide"},
    "text": {"key", "sets", "text"},
    "pipe": {"key", "sets", "pipe"},
    "vertexPipe": {"key", "sets", "vertexPipe"},
    "segmentPipe": {"key", "sets", "segmentPipe"},
    "attributes": {"key", "sets", "attributes"},
    "vertexAttributes": {"key", "sets", "vertexAttributes"},
    "surface": {"key", "sets", "surface"},
}
KEY_PATTERN = re.compile(r"\*|[A-Z0-9]+\*?")
# Words that name a measured quantity: an attribute that has one carries its unit, as `Depth (m)`.
QUANTITY = ("depth", "diameter", "height", "width", "capacity", "level", "voltage", "rating", "size")
# The one utility code whose attribute list the other library fixes, so it cannot also say its service.
NOT_TYPED = {"TS*": "its only attributes rule is the model zone to check, and the entries of that rule are fixed"}


def _sibling(name):
    """The module `name` beside this file, or None when there is none yet."""
    import importlib.util
    import os
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), name + ".py")
    if not os.path.exists(path):
        return None
    spec = importlib.util.spec_from_file_location("ks_" + name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules["ks_" + name] = module
    spec.loader.exec_module(module)
    return module


def _strings(obj):
    if isinstance(obj, str):
        yield obj
    elif isinstance(obj, dict):
        for k, v in obj.items():
            yield from _strings(k)
            yield from _strings(v)
    elif isinstance(obj, (list, tuple)):
        for v in obj:
            yield from _strings(v)


def check():
    """Validate CODES; return (errors, warnings), two lists of sentences. The names of colours, linestyles and
    symbols are checked against the sibling modules colours.py, linestyles.py and symbols.py."""
    errors, warnings = [], []
    err, warn = errors.append, warnings.append

    colours = _sibling("colours")
    palette = set(colours.COLOURS) if colours else set()
    ls_module, sy_module = _sibling("linestyles"), _sibling("symbols")
    linestyles = {d["name"] for d in ls_module.LINESTYLES} | {"continuous"} if ls_module else None
    symbols = {d["name"] for d in sy_module.SYMBOLS} if sy_module else None
    if not (palette and linestyles and symbols):
        warn("colour, linestyle or symbol names are not checked: colours.py, linestyles.py or symbols.py is "
             "not beside this file")

    classes = {top[0] for top in TAXONOMY}
    groups = set(_GROUP.values())
    weights = {w[0] for w in WEIGHTS}
    seen_keys, seen_comments = {}, {}
    used_colours = set()
    for c in CODES:
        k = c.get("key", "?")
        where = k
        if set(c) != {"key", "meaning", "rules", "class", "group"}:
            err("%s: members differ (%s)" % (where, sorted(set(c) ^ {"key", "meaning", "rules", "class", "group"})))
            continue
        if not KEY_PATTERN.fullmatch(k):
            err("%s: a key is capital letters and digits, with a * at the end for a prefix, or a bare *" % where)
        if k in seen_keys:
            err("%s: key repeated" % where)
        seen_keys[k] = c
        kinds = [r.get("sets") for r in c["rules"]]
        if not kinds:
            err("%s: a code has at least one rule" % where)
        if len(kinds) != len(set(kinds)):
            err("%s: two rules of one kind: only the first would apply to the field it sets" % where)
        for r in c["rules"]:
            if r.get("sets") not in RULE_KINDS:
                err("%s: unknown kind of rule %r" % (where, r.get("sets")))
            elif set(r) - MEMBERS[r["sets"]]:
                err("%s (%s): members outside the kind: %s" % (where, r["sets"], sorted(set(r) - MEMBERS[r["sets"]])))
        feature = feature_of(c)
        # class and group
        if k == "*":
            if c["class"] != "-" or c["group"]:
                err("*: the bare * belongs to no class and no group")
        else:
            if c["class"] not in classes:
                err("%s: class %r is not in the taxonomy" % (where, c["class"]))
            elif c["group"] not in groups or c["group"].split("/")[0] != _CLASS_OF[c["class"]][1]:
                err("%s: group %r is not a group of class %s" % (where, c["group"], c["class"]))
        # the feature rule
        if feature is not None:
            layer = feature["layer"]
            if not re.fullmatch(r"[a-z]+(-[a-z]+)*(/[a-z]+(-[a-z]+)*){2}", layer):
                err("%s: layer %r is three lower-case levels, words joined by hyphens" % (where, layer))
            elif c["group"] in _LAYER_OF_GROUP and not layer.startswith(_LAYER_OF_GROUP[c["group"]] + "/"):
                err("%s: layer %r should begin %s/" % (where, layer, _LAYER_OF_GROUP[c["group"]]))
            if feature["weight"] not in weights:
                err("%s: weight %r is not on the scale" % (where, feature["weight"]))
            elif feature["weight"] == "0.70":
                warn("%s: 0.70 is reserved in edition 1" % where)
            if palette and feature["colour"] not in palette:
                err("%s: colour %r is not in the palette" % (where, feature["colour"]))
            used_colours.add(feature["colour"])
            draw = feature.get("draw")
            if draw not in (None, "line", "point"):
                err("%s: draw is line, point or absent" % where)
            if draw == "line" and not feature.get("linestyle"):
                err("%s: a line has a linestyle (a definition or continuous)" % where)
            if draw != "line" and feature.get("linestyle"):
                err("%s: only a line has a linestyle" % where)
            if linestyles is not None and feature.get("linestyle") and feature["linestyle"] not in linestyles:
                err("%s: linestyle %r is not defined" % (where, feature["linestyle"]))
            comment = feature["comment"]
            if not comment or len(comment) > 36:
                err("%s: comment must be 1 to 36 characters (it is the legend label): %r" % (where, comment))
            else:
                rest = " ".join(w for w in comment[1:].split() if not (len(w) > 1 and w.isupper()))
                if comment[0] != comment[0].upper() or rest != rest.lower():
                    if not re.search(r"\d|[A-Z]{2}", comment):
                        warn("%s: comment %r is not sentence case" % (where, comment))
                if k.rstrip("*") in comment.upper().split():
                    err("%s: the comment must not be the key" % where)
                seen_comments.setdefault(comment.lower(), []).append(k)
        for r in c["rules"]:
            if r.get("sets") == "symbol":
                if symbols is not None and r["symbol"] not in symbols:
                    err("%s: symbol %r is not defined" % (where, r["symbol"]))
                if feature is None or feature.get("draw") is None:
                    err("%s: a symbol rule follows a feature rule that draws" % where)
            if r.get("sets") == "text":
                if r["style"] != "Standard" or r["units"] != "paper" or not 0 < r["size"] <= 10:
                    err("%s: text is style Standard, units paper, a size up to 10 mm" % where)
                if palette and r["colour"] not in palette:
                    err("%s: text colour %r is not in the palette" % (where, r["colour"]))
                used_colours.add(r["colour"])
            if r.get("sets") in ("pipe", "vertexPipe", "segmentPipe"):
                if r["justify"] not in ("Centre", "Invert", "Obvert") or r["shape"] not in ("diameter", "culvert"):
                    err("%s: pipe justify %r or shape %r is not one the library uses" % (where, r["justify"], r["shape"]))
                for size in (r.get("size1"), r.get("size2")):
                    if size and not size.startswith("$"):
                        err("%s: pipe size %r names an attribute with a leading $" % (where, size))
            if r.get("sets") == "surface" and not isinstance(r["surface"], bool):
                err("%s: surface is true or false" % where)

        # attributes
        for r in c["rules"]:
            if r.get("sets") not in ("attributes", "vertexAttributes"):
                continue
            names = []
            for a in r["attributes"]:
                if set(a) != {"type", "name", "value"} or a["type"] not in ("text", "integer"):
                    err("%s: attribute %r is {type text|integer, name, value}" % (where, a))
                    continue
                names.append(a["name"])
                if (a["type"] == "integer") != (a["name"] in _INTEGER):
                    err("%s: attribute %r is an integer exactly when its name is in _INTEGER" % (where, a["name"]))
                if a["value"] and not (a["name"].startswith("utility.") or a["name"] == "Check Zone" or
                                       a["type"] == "integer" or a["value"] == k.rstrip("*")):
                    err("%s: attribute %r applies a value; only utility.*, a zone, a stated size or the code does"
                        % (where, a["name"]))
                lower = a["name"].lower()
                if not a["name"].startswith("utility.") and any(w in lower for w in QUANTITY) \
                        and not re.search(r"\([^)]+\)$", a["name"]) and a["name"] != "Check Zone":
                    err("%s: attribute %r names a quantity and must carry its unit, as 'Depth (m)'" % (where, a["name"]))
            if len(names) != len(set(names)):
                err("%s: an attribute is listed twice" % where)
        typed = [a["value"] for r in c["rules"] if r.get("sets") == "attributes" for a in r["attributes"]
                 if a["name"] == "utility.type"]
        statused = [a["value"] for r in c["rules"] if r.get("sets") == "attributes" for a in r["attributes"]
                    if a["name"] == "utility.status"]
        if statused:
            err("%s: utility.status is for a disused service and this library has no disused code" % where)
        if typed:
            if c["class"] not in ("U", "K"):
                err("%s: only the utility codes and the stormwater codes of class K set utility.type" % where)
            elif c["class"] == "U":
                service = feature["layer"].split("/")[1] if feature and feature["layer"].startswith("utilities/") else None
                if typed != [service]:
                    err("%s: a utility code sets utility.type to the service of its layer, %r; it sets %s"
                        % (where, service, typed))
            elif typed != ["stormwater"]:
                err("%s: a stormwater code sets utility.type to 'stormwater', it sets %s" % (where, typed))
        elif c["class"] == "U" and k in NOT_TYPED:
            pass
        elif c["class"] == "U" and feature is not None and feature["layer"].startswith("utilities/"):
            warn("%s: a utility code that says no service" % where)

        # prose
        meaning = c["meaning"]
        if not 15 <= len(meaning) <= 160 or "\n" in meaning:
            err("%s: meaning is one line of 15 to 160 characters (%d)" % (where, len(meaning)))
        elif meaning[0] != meaning[0].upper() or not meaning.endswith("."):
            warn("%s: meaning should be a sentence with a capital and a full stop" % where)
        for s in _strings(c):
            if FORBIDDEN.search(s):
                err("%s: forbidden word in %r" % (where, s))
            if not s.isascii():
                warn("%s: non-ASCII text in %r" % (where, s))

    for comment, keys in seen_comments.items():
        if len(keys) > 1 and comment not in SHARED_COMMENTS:
            warn("comment %r is the legend label of %s" % (comment, ", ".join(keys)))
    for top in TAXONOMY:
        for g in top[4]:
            if not any(c["group"] == top[1] + "/" + g[0] for c in CODES):
                warn("group %s/%s has no code" % (top[1], g[0]))

    # The rules the codes expand to: legal members, no duplicate, no shadowed rule.
    rules = [r for c in CODES for r in expand_rules(c)]
    once = set()
    for r in rules:
        if set(r) - RULE_MEMBERS[r["sets"]]:
            err("%s (%s): members outside the section: %s" % (r["key"], r["sets"], sorted(set(r) - RULE_MEMBERS[r["sets"]])))
        if (r["key"], r["sets"]) in once:
            err("%s (%s): rule repeated" % (r["key"], r["sets"]))
        once.add((r["key"], r["sets"]))

    # Every definition is named by a code, and every colour of the palette is used.
    used_ls, used_sy = definitions_used()
    if ls_module:
        for d in ls_module.LINESTYLES:
            if d["name"] not in used_ls:
                err("linestyle %r is defined and no code names it" % d["name"])
    if sy_module:
        for d in sy_module.SYMBOLS:
            if d["name"] not in used_sy:
                err("symbol %r is defined and no code names it" % d["name"])
    if palette:
        for name in sorted(palette - used_colours):
            err("colour %r is in the palette and no code uses it" % name)
    return errors, warnings


# Legend labels that are shared on purpose: the unclassified design strings have no purpose to tell them apart by.
SHARED_COMMENTS = frozenset({"unclassified road string"})

_LAYER_OF_GROUP = {top[1] + "/" + g[0]: top[2] + "/" + g[1] for top in TAXONOMY for g in top[4]}


def summary():
    lines = []
    for top in TAXONOMY:
        in_class = [c for c in CODES if c["class"] == top[0]]
        kinds = {}
        for c in in_class:
            kinds[kind_of(c)] = kinds.get(kind_of(c), 0) + 1
        shown = ", ".join("%d %s" % (n, kind) for kind, n in sorted(kinds.items()))
        lines.append("%s  %-32s%4d codes  (%s)" % (top[0], top[1], len(in_class), shown))
    other = [c for c in CODES if c["class"] == "-"]
    lines.append("-  %-32s%4d codes" % ("(no class)", len(other)))
    lines.append("   %-32s%4d codes, %d rules" % ("all", len(CODES), sum(len(expand_rules(c)) for c in CODES)))
    return "\n".join(lines)


if __name__ == "__main__":
    if "--grammar" in sys.argv:
        print(GRAMMAR)
    if "--summary" in sys.argv:
        print(summary())
    if "--check" in sys.argv:
        problems, advice = check()
        for sentence in advice[:80]:
            print("warning:", sentence)
        for sentence in problems[:200]:
            print("ERROR:", sentence)
        print("codes: %d codes, %d rules, %d errors, %d warnings" % (
            len(CODES), sum(len(expand_rules(c)) for c in CODES), len(problems), len(advice)))
        sys.exit(1 if problems else 0)
