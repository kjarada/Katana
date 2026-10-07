"""Katana Standard survey codes: the vocabulary, its grammar, and what every code expands to.

    python tools/katana_standard/codes.py --check [--contract FILE]    validate every code; exit 1 on a problem
    python tools/katana_standard/codes.py --summary                    codes and rules by class
    python tools/katana_standard/codes.py --grammar                    print this grammar

`CODES` is the whole vocabulary as data, one dict per code. The assembler (make_katana_standard.py) turns each
into the rules of `expand_rules()` below and writes them, in this order, as the `codes` of the customisation
file. The dict members are the format's own words where the format has them (layer, colour, weight, linestyle,
symbol, attributes, surface, hide, pipe, text) plus six that are ours: `key`, `group`, `name`, `kind` (line,
point, both or text), and the prose that makes the library readable: `comment` (the legend label), `meaning`
(what the code is, one line), `how` (the field-book instruction) and an optional `example`.

THE GRAMMAR

1. A key is THREE capital letters: class, subgroup, item. `UWM` is Utilities, Water, Main; `KKT` is Kerbs and
   Drainage, Kerbs, Top edge. The first two letters are in TAXONOMY and fix the group and the first two levels
   of the layer; the third names the item inside its subgroup.
2. Utility items share their third letter across services, so it is learnt once: M main, B branch or
   connection, D disused main, V isolation valve, X meter, P pit, H maintenance hole, K marker post or pillar,
   T tank or transformer, C cable, N duct, O overhead line. Where the plain letter was taken the code uses the
   next natural one: UWU and USP (pump stations), UEP (power pole), UEJ (electrical pit), UEY (pole guy), UFH
   (hydrant), UFB (booster), USR and UGR (pressure sewer, regulator), UXM (paint mark). The service letters are
   those of the one-letter asset types of AS 5488 where one exists (C E F G I P S W); R (recycled water) and X
   (unknown) are ours.
3. DIGITS ARE NEVER PART OF A KEY. `UWM`, `UWM1` and `UWM01` are one code; the digits are the STRING NUMBER,
   which the linework reads: `UWM1` and `UWM2` are two water mains. A key written with a digit before its `*`
   would match only numbered names.
4. Control words follow the code, after a space: `ST` first point, `END` last point, `CL` last point and close,
   `BC` and `EC` begin and end a curve (three points fix an arc), `JPN n` also join to point number n, `RECT`
   the third point of a rectangle. These are Katana's default spellings; the library says nothing about them,
   so loading it never resets a colleague's own. No key is a control word.
5. There is no left or right suffix: the rule engine matches a key exactly or by prefix and has nothing else.
   Handedness is by walking direction. Ticks, barbs, triangles, scallops and chevrons always fall to the LEFT of
   travel, and each code's `how` says which way to walk. Symbols are drawn upright and are not turned to a
   bearing (Katana reads a rule's rotation and does not yet apply it), so a symbol's position is its message.
6. Every code owns a `KEY*` rule, so a typo (`UWQ`) is reported as "no rule" and is never swallowed by a family
   rule. All keys are three letters, so no key is a prefix of another; there is no bare `*` rule, and no rule
   is shadowed or repeated.
7. What a code sets, in this order. `feature`: layer, colour, draw (line or point), linestyle (lines only),
   weight, group and comment. `symbol` for points and for lines that carry a symbol at every vertex (`both`:
   fences, tree rows, leaders). `text` for the six text codes. `pipe` for the nine pipe codes. `attributes`.
   `surface`, written for EVERY code, true or false: true for ground shots and levels, kerb, carriageway and
   path edges, banks, ridges, gullies, hard and soft breaklines, water edges, the cliff edge, drain inverts,
   and ballast and platform edges; false for everything else, including the exclusion boundary.
8. Attributes. An attribute with an EMPTY value is a PROMPT: the code asks for it. One with a value is applied.
   Names carry their unit (`Depth (m)`, `Diameter (mm)`). Every buried utility asks Owner, Depth (m) and
   Condition, and a pipe also Material and Diameter (mm). Every utility code sets `utility.type` to its
   service word, and a disused code sets `utility.status` to `disused`: those are the keys and words Katana's
   utility tools read, so a coded string is already typed for UTILITY DRAW. The prompts use the library's own
   names, not `utility.owner`, so an empty prompt never reads as "recorded".
9. `comment` is the LEGEND LABEL: a noun phrase in sentence case, 1 to 36 characters, which the plot legend
   prints in capitals. When several codes share one linestyle the legend takes the linestyle's name instead,
   unless every coded entity agrees on the label. Longer text is `meaning` and `how`.
10. Weights, in millimetres of pen: 0.13 hairline, 0.18 fine, 0.25 light, 0.35 medium, 0.50 strong (0.70 is
   reserved). Katana stores `weight` and does not yet apply it; the scale is still the documented intent.
11. What Katana applies today and what it only stores. `CODE` applies layer, colour, linestyle, symbol name and
   size, and the string attributes (a value starting with `$` is deferred, not written), and the legend
   prints the comment. It stores, lists and lints, and does not yet apply: weight, group, hide, surface, text
   rules, pipe rules (justify, shape, sizes taken from the attributes), rotation, offset and raise. The rules
   are written for the day it does.
12. Load the library alone, or with Replace. Merged onto another customisation, a broader prefix rule there can
   supply a field these rules leave unsaid (a symbol, a text), because the most specific rule wins field by
   field.

A field-book `example` is the code field of consecutive points, separated by commas: `UWM1 ST, UWM1, UWM1 END`.
"""
import re
import sys

GRAMMAR = (__doc__ or "").split("THE GRAMMAR", 1)[-1].strip()

# The six pen weights in millimetres: (text, role, what it is for).  Katana's own weight is TEXT.
WEIGHTS = [
    ("0.13", "hairline", "hatching, hidden detail, leaders, text"),
    ("0.18", "fine", "vegetation, intermediate contours, markings, minor services"),
    ("0.25", "light", "everyday detail: fences, paths, branches"),
    ("0.35", "medium", "primary detail: kerbs, building walls, service mains, index contours"),
    ("0.50", "strong", "title boundary, bridge decks, limit of survey"),
    ("0.70", "bold", "reserved; not used by a code in edition 1"),
]

# The six looks a text code can have: size in millimetres of paper, justification, slant and weight.
TEXT_LOOKS = {
    "label": {"size": 2.0, "justifyX": "left", "justifyY": "bottom", "italic": False, "weight": "Normal"},
    "note": {"size": 2.5, "justifyX": "left", "justifyY": "bottom", "italic": False, "weight": "Normal"},
    "heading": {"size": 5.0, "justifyX": "left", "justifyY": "bottom", "italic": False, "weight": "Bold"},
    "level": {"size": 2.0, "justifyX": "left", "justifyY": "middle", "italic": True, "weight": "Normal"},
    "road": {"size": 3.0, "justifyX": "centre", "justifyY": "middle", "italic": True, "weight": "Normal"},
    "contour": {"size": 1.8, "justifyX": "centre", "justifyY": "middle", "italic": True, "weight": "Normal"},
}

# The classes and subgroups: (letter, name, layer, summary, [(letter, name, layer, summary), ...]).  The names
# are the definition groups too ("Top/Sub"), so a definition, a rule and a layer are found in one place.
TAXONOMY = [
    ("C", "Boundaries and Cadastre", "cadastre",
     "Where one owner's land ends and the next begins, and the marks that prove it.", [
         ("B", "Lot Boundaries", "lots", "The lines of title: registered, occupied, compiled, reserve and municipal."),
         ("E", "Easements and Reserves", "easements", "Land that carries another party's right or a public purpose."),
         ("M", "Boundary Marks", "marks", "Pegs, pins, spikes and crosses found or placed at corners."),
     ]),
    ("R", "Roads and Pavements", "roads",
     "The made surface: its edges, its paint and the paths beside it.", [
         ("C", "Carriageway", "carriageway", "Edges, shoulders, centreline and islands of the road itself."),
         ("M", "Markings", "markings", "Painted lines, crossings, hatching and arrows."),
         ("P", "Paths and Access", "paths", "Footpaths, cycleways, driveways, kerb ramps and tactile paving."),
     ]),
    ("S", "Street Furniture and Signs", "street",
     "What people read, sit on, light their way by or steer round.", [
         ("S", "Signs", "signs", "Regulatory, warning, guide and information signs."),
         ("L", "Lighting", "lighting", "Street lights and bollard lights."),
         ("B", "Posts and Barriers", "barriers", "Bollards, delineators and guard rail."),
         ("A", "Amenities", "amenities", "Benches, bins, fountains and shelters."),
         ("T", "Traffic Control", "traffic", "Signal poles, cameras and controller cabinets."),
     ]),
    ("K", "Kerbs and Drainage", "drainage",
     "Where the street meets the water: kerbs, pits, pipes and channels.", [
         ("K", "Kerbs", "kerbs", "Kerb top, gutter, back and return, and the mountable and dish forms."),
         ("P", "Pits and Structures", "structures", "Pits, manholes, traps, headwalls and culvert ends."),
         ("C", "Pipes and Culverts", "pipes", "Stormwater pipes, box culverts and subsoil drains."),
         ("W", "Channels and Waterways", "channels", "Open drains, swales, flow paths and creeks."),
     ]),
    ("T", "Rail and Transit", "rail",
     "Track, the wires above it and the platforms beside it.", [
         ("T", "Track", "track", "Running rails, centreline, ballast, crossings and track fittings."),
         ("O", "Overhead and Signals", "overhead", "Contact wire, masts and signals."),
         ("P", "Platforms", "platforms", "Platform edges."),
     ]),
    ("U", "Utilities", "utilities",
     "Everything buried or strung that carries a service, one family per service.", [
         ("W", "Water", "water", "Drinking water mains, connections, valves, pits and tanks."),
         ("S", "Sewer", "sewer", "Gravity and pressure sewers, branches, maintenance holes and pump stations."),
         ("G", "Gas", "gas", "Gas mains, services, valves, regulators and meters."),
         ("E", "Electricity", "electricity", "Cables, overhead lines, ducts, poles, transformers and pillars."),
         ("C", "Communications", "telecommunications", "Telephone, data and fibre cables, ducts, pits and pillars."),
         ("R", "Recycled Water", "recycled-water", "Recycled water mains, connections, valves and pits."),
         ("F", "Fire Service", "fire-service", "Fire mains, hydrants, boosters and valves."),
         ("P", "Fuel", "fuel", "Fuel pipelines, valves, markers and tanks."),
         ("I", "Traffic Systems", "its", "Conduits, pits and detector loops for signals and cameras."),
         ("X", "Unknown Services", "unknown", "What is there and has not been named: lines, pits, covers and paint."),
     ]),
    ("V", "Vegetation and Landscape", "vegetation",
     "Trees, shrubs and the edges of planted and natural ground cover.", [
         ("T", "Trees", "trees", "Trees by kind, stumps, tree rows and canopy outlines."),
         ("S", "Shrubs and Plants", "shrubs", "Shrubs, clusters, tussocks and hedges."),
         ("G", "Ground Cover", "ground", "The edges of lawn, bush, garden beds and crops."),
     ]),
    ("B", "Buildings and Structures", "buildings",
     "What stands on the ground: buildings, bridges, masts, tanks and steps.", [
         ("B", "Buildings", "outline", "Walls, eaves, verandahs, hidden edges and doors."),
         ("S", "Structures", "structures", "Bridges, masts, tanks, flagpoles and monuments."),
         ("A", "Steps and Access", "access", "Steps and handrails."),
     ]),
    ("F", "Fences and Walls", "fences",
     "What divides land: fences, walls, and the posts and gates between.", [
         ("F", "Fences", "fence", "Chain mesh, paling, rail, wire, barbed, electric and palisade."),
         ("W", "Walls", "wall", "Masonry, concrete and retaining walls."),
         ("G", "Gates and Posts", "gate", "Gates, strainer posts and lone fence posts."),
     ]),
    ("G", "Terrain and Breaklines", "terrain",
     "The shape of the ground: shots, breaklines, contours, water edges and rock.", [
         ("P", "Ground Points", "points", "Ground shots and levels, highs, lows and inverts."),
         ("B", "Breaklines", "breaklines", "Banks, ridges, gullies and the hard and soft lines that shape a surface."),
         ("C", "Contours", "contours", "Index, intermediate and depression contours."),
         ("W", "Water Edges", "water", "Water's edge and the high water mark."),
         ("R", "Rock and Cliff", "rock", "Rock outcrops, cliff edges and boulders."),
     ]),
    ("M", "Survey Control and Annotation", "control",
     "The survey's own marks, its investigations and the words on the plan.", [
         ("C", "Control Marks", "marks", "Trigonometric stations, benchmarks and the marks a job sets and uses."),
         ("I", "Investigation Points", "investigation", "Boreholes, test pits and potholes."),
         ("T", "Text and Notes", "annotation", "Six looks of text: label, note, heading, level, road name and contour value."),
         ("M", "Plan Marks", "plan", "North arrow, leader, limit of survey and revision cloud."),
     ]),
    ("X", "Miscellaneous", "miscellaneous",
     "Hazards, heritage, temporary works and the catch-alls.", [
         ("H", "Hazards and Heritage", "hazards", "Hazards, heritage items and habitat markers."),
         ("G", "General", "general", "A generic point and line, and the query point."),
         ("T", "Temporary Works", "temporary", "Temporary fences and work zones."),
     ]),
]

_GROUP = {top[0] + sub[0]: top[1] + "/" + sub[1] for top in TAXONOMY for sub in top[4]}
_LAYER = {top[0] + sub[0]: top[2] + "/" + sub[2] for top in TAXONOMY for sub in top[4]}

# Attributes written as whole numbers; every other attribute is text (its unit is in its name).
_INTEGER = frozenset({"Diameter (mm)", "Ducts", "Floors", "Height (mm)", "Lid Size (mm)",
                      "Trunk Diameter (mm)", "Voltage (V)", "Width (mm)"})


def _attribute(spec):
    """'Name' is a prompt; 'Name=value' is applied."""
    name, _, value = spec.partition("=")
    return {"type": "integer" if name in _INTEGER else "text", "name": name, "value": value}


def code(key, name, kind, layer, colour, weight, *, label, meaning, how, example=None, style=None, symbol=None,
         attrs=(), pipe=None, surface=False, hide=None, look=None):
    """One code as a dict. `colour` is the palette word after 'katana '; `style` the linestyle of a line;
    `symbol` the symbol of a point or of the vertices of a `both` line; `look` the TEXT_LOOKS entry of a text
    code; `pipe` is (justify, shape, size1, size2)."""
    return {
        "key": key, "group": _GROUP[key[:2]], "name": name, "kind": kind, "layer": layer,
        "colour": "katana " + colour, "weight": weight, "linestyle": style, "symbol": symbol,
        "attributes": [_attribute(a) for a in attrs], "surface": surface, "hide": hide,
        "pipe": list(pipe) if pipe else None,
        "text": dict(TEXT_LOOKS[look], style="Standard", units="paper") if look else None,
        "comment": label, "meaning": meaning, "how": how, "example": example,
    }


CODES = [
    # ---- CB Boundaries and Cadastre / Lot Boundaries -------------------------------------------------------------
    code("CBT", "Title Boundary", "line", "cadastre/lots/title", "boundary", "0.50",
         style="Title Boundary",
         attrs=["Plan Reference"],
         label="Title boundary",
         meaning="Lot boundary as registered on the title.",
         how=("Shoot each boundary at the mark you found, or at the computed corner where a mark is "
              "missing (code that point CMN). One string per lot; close it with CL. Fill Plan Reference "
              "from the deposited plan."),
         example="CBT1 ST, CBT1, CBT1, CBT1 CL"),
    code("CBO", "Occupation Boundary", "line", "cadastre/lots/occupation", "boundary", "0.35",
         style="Occupation Boundary",
         label="Occupation boundary",
         meaning="Boundary as occupied on the ground: a fence, a wall face or a hedge.",
         how=("Shoot the line the owners treat as the boundary. Where it parts from the title line (CBT) "
              "code both: the gap between them is the encroachment."),
         example="CBO1 ST, CBO1, CBO1 END"),
    code("CBC", "Compiled Boundary", "line", "cadastre/lots/compiled", "boundary", "0.25",
         style="Compiled Boundary",
         attrs=["Plan Reference"],
         label="Compiled boundary",
         meaning="Boundary taken from plans and not measured on this survey.",
         how=("Use where you drew the line from deposited plans without measuring it, and shoot only the "
              "corners you hold. Plan Reference names the plan you compiled from. Recode as CBT once the "
              "marks are found."),
         example="CBC1 ST, CBC1, CBC1 END"),
    code("CBR", "Road Reserve Boundary", "line", "cadastre/lots/road-reserve", "boundary", "0.35",
         style="Reserve Boundary",
         label="Road reserve boundary",
         meaning="Limit of a public road reserve.",
         how=("Shoot along the reserve limit from the title or the road plan, at each change of "
              "direction. Park and drainage reserves are CER, not this code."),
         example="CBR1 ST, CBR1, CBR1 END"),
    code("CBM", "Municipal Boundary", "line", "cadastre/lots/municipal", "boundary", "0.50",
         style="Municipal Boundary",
         label="Municipal boundary",
         meaning="Boundary between two local government areas.",
         how=("Shoot only where the boundary is marked or the gazetted plan gives coordinates; do not "
              "draw it from a map. It is a long, quiet line: one point at each change of direction is "
              "enough."),
         example="CBM1 ST, CBM1, CBM1 END"),

# ---- CE Boundaries and Cadastre / Easements and Reserves -----------------------------------------------------
    code("CEA", "Access Easement", "line", "cadastre/easements/access", "boundary", "0.25",
         style="Easement Boundary",
         attrs=["Plan Reference"],
         label="Access easement",
         meaning="Right of way over a lot for access.",
         how=("Shoot both edges of the easement from the registered plan, one string per side. Close "
              "with CL when it is a block, and END when it runs off the lot. Plan Reference is the plan "
              "that created it."),
         example="CEA1 ST, CEA1, CEA1 END"),
    code("CED", "Drainage Easement", "line", "cadastre/easements/drainage", "boundary", "0.25",
         style="Easement Boundary",
         attrs=["Plan Reference"],
         label="Drainage easement",
         meaning="Easement that carries a drain or an overland flow path.",
         how=("Shoot both edges from the plan. Pipes and channels inside it are shot with their own "
              "codes (KCP, KWD); this code only says where the easement lies."),
         example="CED1 ST, CED1, CED1 END"),
    code("CES", "Services Easement", "line", "cadastre/easements/services", "boundary", "0.25",
         style="Easement Boundary",
         attrs=["Plan Reference", "Owner"],
         label="Services easement",
         meaning="Easement for pipes or cables, including pipeline easements.",
         how=("Shoot both edges from the plan. Owner is the authority the easement benefits and Plan "
              "Reference the plan that created it. The buried service itself gets its own U code."),
         example="CES1 ST, CES1, CES1 END"),
    code("CEG", "General Easement", "line", "cadastre/easements/general", "boundary", "0.25",
         style="Easement Boundary",
         attrs=["Plan Reference"],
         label="General easement",
         meaning="Easement whose purpose is not recorded.",
         how=("Use only when the plan shows an easement and gives no purpose. Say what you know in Plan "
              "Reference and flag it for the drafter; replace it with CEA, CED or CES once the purpose "
              "is found."),
         example="CEG1 ST, CEG1, CEG1 END"),
    code("CER", "Public Reserve Boundary", "line", "cadastre/easements/reserve", "boundary", "0.35",
         style="Reserve Boundary",
         label="Public reserve boundary",
         meaning="Limit of a park, a reserve or other public land.",
         how=("Shoot along the reserve limit from the title, at each change of direction. Road reserves "
              "are CBR."),
         example="CER1 ST, CER1, CER1 END"),
    code("CEV", "Covenant Area", "line", "cadastre/easements/general", "boundary", "0.25",
         style="Easement Boundary",
         attrs=["Plan Reference"],
         label="Covenant area",
         meaning="Area restricted by a covenant or a restriction on use.",
         how=("Shoot the outline of the restricted area from the plan or the covenant text and close "
              "with CL. Plan Reference points to the instrument."),
         example="CEV1 ST, CEV1, CEV1, CEV1 CL"),

# ---- CM Boundaries and Cadastre / Boundary Marks -------------------------------------------------------------
    code("CMP", "Corner Peg", "point", "cadastre/marks/found", "boundary", "0.25",
         symbol="Corner Peg",
         attrs=["Mark Number", "Condition"],
         label="Corner peg",
         meaning="Peg found or placed at a boundary corner.",
         how=("Shoot the top centre of the peg. Mark Number is the plan's number for it; Condition says "
              "sound, leaning or disturbed. A nail or spike in hard ground is CMS.")),
    code("CMI", "Iron Pin", "point", "cadastre/marks/found", "boundary", "0.25",
         symbol="Iron Pin",
         attrs=["Mark Number", "Condition"],
         label="Iron pin",
         meaning="Iron pin found or placed at a boundary corner.",
         how=("Shoot the top centre of the pin. Condition records whether it stands proud, is flush, "
              "bent or rusted.")),
    code("CMT", "Iron Pipe", "point", "cadastre/marks/found", "boundary", "0.25",
         symbol="Iron Pipe",
         attrs=["Mark Number", "Condition"],
         label="Iron pipe",
         meaning="Iron pipe found or placed at a boundary corner.",
         how="Shoot the centre of the pipe's mouth, not its rim. Mark Number and Condition as for any mark."),
    code("CMS", "Survey Spike", "point", "cadastre/marks/found", "boundary", "0.25",
         symbol="Survey Spike",
         attrs=["Mark Number", "Condition"],
         label="Survey spike",
         meaning="Spike or nail in a road, a kerb or concrete.",
         how=("Shoot the centre of the head, or of the washer when there is one. A drilled hole is CMD "
              "and a cut cross is CMX.")),
    code("CMD", "Drilled Hole", "point", "cadastre/marks/found", "boundary", "0.25",
         symbol="Drilled Hole",
         attrs=["Mark Number", "Condition"],
         label="Drilled hole",
         meaning="Hole drilled in rock or concrete.",
         how=("Shoot the centre of the hole at the surface. When it holds a lead plug or a pin, the plug "
              "centre is the mark.")),
    code("CMX", "Chiselled Cross", "point", "cadastre/marks/found", "boundary", "0.25",
         symbol="Chiselled Cross",
         attrs=["Mark Number", "Condition"],
         label="Chiselled cross",
         meaning="Cross cut in rock, a kerb or concrete.",
         how=("Shoot the intersection of the two arms, not the end of either. Note in Condition if the "
              "arms are worn or the cross is not square.")),
    code("CMN", "Mark Not Found", "point", "cadastre/marks/not-found", "boundary", "0.25",
         symbol="Mark Not Found",
         attrs=["Mark Number", "Search Note"],
         label="Mark not found",
         meaning="Boundary mark searched for and not found.",
         how=("Shoot the computed position of the missing mark, never a guess. Search Note says how long "
              "you looked and what you dug. It is the evidence behind any mark you later reinstate.")),

# ---- RC Roads and Pavements / Carriageway --------------------------------------------------------------------
    code("RCE", "Sealed Road Edge", "line", "roads/carriageway/edge", "pavement", "0.35",
         style="continuous", surface=True,
         label="Sealed road edge",
         meaning="Outer edge of a sealed carriageway that has no kerb.",
         how=("Shoot the edge of the seal at each change of direction, and every 10 m on a curve, with a "
              "level at each point. A kerbed street is shot as kerb (KKT, KKL) and this code is left "
              "out."),
         example="RCE1 ST, RCE1, RCE1 END"),
    code("RCU", "Unsealed Road Edge", "line", "roads/carriageway/edge", "pavement", "0.25",
         style="Unsealed Road Edge", surface=True,
         label="Unsealed road edge",
         meaning="Edge of the formed gravel or earth running surface.",
         how=("Shoot the edge of the formed surface, not the edge of the verge. Where the edge wanders "
              "or is soft, take a point every 5 m."),
         example="RCU1 ST, RCU1, RCU1 END"),
    code("RCS", "Shoulder Edge", "line", "roads/carriageway/edge", "pavement", "0.25",
         style="Shoulder Edge", surface=True,
         label="Shoulder edge",
         meaning="Outer edge of the road shoulder, where it meets the verge.",
         how=("Shoot where the shoulder ends and the verge begins. The edge of the lane is RCE and is "
              "shot as its own string."),
         example="RCS1 ST, RCS1, RCS1 END"),
    code("RCL", "Road Centreline", "line", "roads/carriageway/centreline", "pavement", "0.25",
         style="Road Centreline",
         label="Road centreline",
         meaning="Centre of the carriageway, shot at the crown.",
         how=("Shoot the crown of the road, not the painted line, with a level at each point: alignments "
              "and long sections are cut from this string. Walk in the direction of chainage."),
         example="RCL1 ST, RCL1, RCL1 END"),
    code("RCJ", "Pavement Joint", "line", "roads/carriageway/joint", "pavement", "0.13",
         style="continuous",
         label="Pavement joint",
         meaning="Construction or expansion joint in a concrete pavement.",
         how=("Shoot both ends of the joint and any bend in it, one string per joint. Needed only on "
              "concrete pavements where the jointing pattern matters."),
         example="RCJ1 ST, RCJ1 END"),
    code("RCI", "Traffic Island Edge", "line", "roads/carriageway/edge", "pavement", "0.35",
         style="continuous", surface=True,
         label="Traffic island edge",
         meaning="Edge of a raised or painted traffic island.",
         how=("Shoot the top front edge of a raised island, or the painted outline of a flush one, and "
              "close with CL."),
         example="RCI1 ST, RCI1, RCI1, RCI1 CL"),

# ---- RM Roads and Pavements / Markings -----------------------------------------------------------------------
    code("RMD", "Dashed Lane Line", "line", "roads/markings/line", "pavement", "0.18",
         style="Dashed Lane Marking",
         label="Dashed lane line",
         meaning="Dashed painted line between traffic lanes.",
         how=("Shoot along the middle of the dashes at each change of direction, and every 10 m on a "
              "curve. The dash pattern is drawn for you, so do not shoot each dash."),
         example="RMD1 ST, RMD1, RMD1 END"),
    code("RMB", "Barrier Line", "line", "roads/markings/line", "pavement", "0.18",
         style="Double Barrier Marking",
         label="Barrier line",
         meaning="Double continuous painted line, as at a no-overtaking section.",
         how="Shoot along the middle of the pair of lines. The second line is drawn beside the first for you.",
         example="RMB1 ST, RMB1, RMB1 END"),
    code("RMS", "Stop Line", "line", "roads/markings/line", "pavement", "0.25",
         style="continuous",
         label="Stop line",
         meaning="Painted bar where traffic must stop.",
         how=("Shoot the two ends of the bar on its leading edge, the side traffic reaches first. Two "
              "points only."),
         example="RMS1 ST, RMS1 END"),
    code("RMZ", "Zebra Crossing", "line", "roads/markings/line", "pavement", "0.18",
         style="Zebra Crossing Marking",
         label="Zebra crossing",
         meaning="Zebra crossing, shown as a row of stripes.",
         how=("Shoot the middle of the outer stripe at each end of the crossing, walking across the "
              "road. The stripes are drawn along the traffic direction for you."),
         example="RMZ1 ST, RMZ1 END"),
    code("RMH", "Painted Hatch Area", "line", "roads/markings/line", "pavement", "0.13",
         style="continuous",
         label="Painted hatch area",
         meaning="Outline of a painted hatched or chevron area.",
         how="Shoot each corner of the painted area and close with CL. The hatching itself is not shot.",
         example="RMH1 ST, RMH1, RMH1, RMH1 CL"),
    code("RME", "Edge Line", "line", "roads/markings/line", "pavement", "0.18",
         style="continuous",
         label="Edge line",
         meaning="Painted line along the edge of the carriageway.",
         how=("Shoot along the middle of the painted line. It is rarely the same line as the edge of the "
              "seal (RCE), so shoot both when both matter."),
         example="RME1 ST, RME1, RME1 END"),
    code("RMP", "Parking Bay Line", "line", "roads/markings/line", "pavement", "0.13",
         style="continuous",
         label="Parking bay line",
         meaning="Painted line that marks out a parking bay.",
         how="Shoot the two ends of each painted line, or one closed string round a single marked bay.",
         example="RMP1 ST, RMP1 END"),
    code("RMA", "Road Arrow", "point", "roads/markings/arrow", "pavement", "0.18",
         symbol="Road Arrow Marking",
         label="Road arrow",
         meaning="Painted direction arrow on the carriageway.",
         how=("Shoot the middle of the arrow. The symbol is drawn pointing east and Katana does not yet "
              "turn symbols, so write the real direction in a drafting note.")),

# ---- RP Roads and Pavements / Paths and Access ---------------------------------------------------------------
    code("RPF", "Footpath Edge", "line", "roads/paths/edge", "pavement", "0.25",
         style="continuous", surface=True,
         label="Footpath edge",
         meaning="Edge of a paved footpath.",
         how=("Shoot each edge where the paving meets the kerb, the verge or the property, with a level "
              "at each point. One string per side."),
         example="RPF1 ST, RPF1, RPF1 END"),
    code("RPC", "Cycleway Edge", "line", "roads/paths/edge", "pavement", "0.25",
         style="Cycleway Edge", surface=True,
         label="Cycleway edge",
         meaning="Edge of a cycleway or a shared path.",
         how=("Shoot each edge of the path. The bead in the gap tells a cycleway from a footpath on the "
              "plan at a glance."),
         example="RPC1 ST, RPC1, RPC1 END"),
    code("RPD", "Driveway Edge", "line", "roads/paths/edge", "pavement", "0.25",
         style="continuous", surface=True,
         label="Driveway edge",
         meaning="Edge of a driveway, from the gutter to the boundary.",
         how=("Shoot each edge of the driveway from the gutter crossing to the property boundary, one "
              "string per side."),
         example="RPD1 ST, RPD1, RPD1 END"),
    code("RPR", "Kerb Ramp", "point", "roads/paths/ramp", "pavement", "0.25",
         symbol="Kerb Ramp",
         label="Kerb ramp",
         meaning="Pedestrian ramp cut down to the road at a kerb.",
         how=("Shoot the middle of the ramp where it meets the gutter. The footpath edges either side "
              "are shot as RPF.")),
    code("RPT", "Tactile Paving", "point", "roads/paths/ramp", "pavement", "0.25",
         symbol="Tactile Paving",
         label="Tactile paving",
         meaning="Patch of tactile ground surface indicators.",
         how="Shoot the middle of each patch, one point per patch. A long strip is a point at each end."),

# ---- SS Street Furniture and Signs / Signs -------------------------------------------------------------------
    code("SSR", "Regulatory Sign", "point", "street/signs/sign", "street", "0.25",
         symbol="Regulatory Sign",
         attrs=["Sign Code", "Face Text"],
         label="Regulatory sign",
         meaning="Sign that gives an order, such as a speed limit or a stop.",
         how=("Shoot the foot of the post, or the middle of the face when it is fixed to another "
              "structure. Sign Code is the code printed on the sign's standard; Face Text is the wording "
              "as it reads.")),
    code("SSW", "Warning Sign", "point", "street/signs/sign", "street", "0.25",
         symbol="Warning Sign",
         attrs=["Sign Code", "Face Text"],
         label="Warning sign",
         meaning="Sign that warns of a hazard ahead.",
         how=("Shoot the foot of the post. Sign Code and Face Text as for a regulatory sign; a "
              "supplementary plate is noted in Face Text.")),
    code("SSG", "Guide Sign", "point", "street/signs/sign", "street", "0.25",
         symbol="Guide Sign",
         attrs=["Sign Code", "Face Text"],
         label="Guide sign",
         meaning="Sign that gives a direction, a distance or a street name.",
         how=("Shoot the foot of the post. Put the wording in Face Text, one line per plate, so the sign "
              "can be checked without a site visit.")),
    code("SSI", "Information Sign", "point", "street/signs/sign", "street", "0.25",
         symbol="Information Sign",
         attrs=["Sign Code", "Face Text"],
         label="Information sign",
         meaning="Sign that informs, such as parking or a facility.",
         how="Shoot the foot of the post. Face Text carries the message or the restriction times."),

# ---- SL Street Furniture and Signs / Lighting ----------------------------------------------------------------
    code("SLS", "Street Light", "point", "street/lighting/light", "street", "0.25",
         symbol="Street Light",
         attrs=["Owner", "Material", "Height (m)", "Condition"],
         label="Street light",
         meaning="Street light on its own pole, apart from the power network.",
         how=("Shoot the middle of the pole at ground level. Height (m) is to the lamp, Material is the "
              "pole's, and Owner is the lighting authority. A light on a power pole is coded UEP, not "
              "this.")),
    code("SLB", "Bollard Light", "point", "street/lighting/light", "street", "0.25",
         symbol="Bollard Light",
         attrs=["Owner", "Condition"],
         label="Bollard light",
         meaning="Low light on a bollard or a short post.",
         how="Shoot the middle of the base. A bollard with no light is SBB."),

# ---- SB Street Furniture and Signs / Posts and Barriers ------------------------------------------------------
    code("SBB", "Bollard", "point", "street/barriers/post", "street", "0.25",
         symbol="Bollard",
         label="Bollard",
         meaning="Fixed bollard that stops vehicles and cannot be moved.",
         how=("Shoot the middle of the top of the bollard. For a closely spaced row shoot every bollard, "
              "not the ends.")),
    code("SBR", "Removable Bollard", "point", "street/barriers/post", "street", "0.25",
         symbol="Removable Bollard",
         label="Removable bollard",
         meaning="Bollard that lifts out of a socket to let vehicles through.",
         how="Shoot the middle of the socket cover; if the bollard is standing, the middle of its top."),
    code("SBG", "Delineator Post", "point", "street/barriers/post", "street", "0.25",
         symbol="Delineator Post",
         label="Delineator post",
         meaning="Roadside delineator: a reflective marker post at the edge of the road.",
         how="Shoot the middle of the post at ground level. Shoot every post of a run, not the first and last."),
    code("SBL", "Guard Rail", "line", "street/barriers/guard-rail", "street", "0.35",
         style="Guard Rail Line",
         label="Guard rail",
         meaning="Roadside guard rail or crash barrier.",
         how=("Shoot the front face of the rail at each end and at each change of direction. The posts "
              "are drawn along the line at their usual spacing, so you do not shoot them."),
         example="SBL1 ST, SBL1, SBL1 END"),

# ---- SA Street Furniture and Signs / Amenities ---------------------------------------------------------------
    code("SAB", "Bench", "point", "street/amenities/furniture", "street", "0.25",
         symbol="Bench",
         label="Bench",
         meaning="Park or street bench, fixed or loose.",
         how="Shoot the middle of the seat. A long bench is one point at its middle, not one at each end."),
    code("SAR", "Litter Bin", "point", "street/amenities/furniture", "street", "0.25",
         symbol="Litter Bin",
         label="Litter bin",
         meaning="Litter bin, free-standing or fixed to a post.",
         how="Shoot the middle of the bin. A bin fixed to a post is shot at the post."),
    code("SAF", "Drinking Fountain", "point", "street/amenities/furniture", "street", "0.25",
         symbol="Drinking Fountain",
         label="Drinking fountain",
         meaning="Public drinking fountain.",
         how="Shoot the middle of the fountain's base. Its feed is a water service and is shot as UWB."),
    code("SAS", "Bus Stop Shelter", "point", "street/amenities/furniture", "street", "0.25",
         symbol="Bus Stop Shelter",
         label="Bus stop shelter",
         meaning="Bus shelter or other roofed waiting area.",
         how=("Shoot the middle of the roof outline. A shelter that needs its exact footprint is "
              "outlined with BBW.")),

# ---- ST Street Furniture and Signs / Traffic Control ---------------------------------------------------------
    code("STS", "Signal Pole", "point", "street/traffic/equipment", "street", "0.25",
         symbol="Signal Pole",
         attrs=["Owner", "Material", "Height (m)", "Condition"],
         label="Signal pole",
         meaning="Traffic signal pole that carries the lanterns, with or without a mast arm.",
         how=("Shoot the middle of the pole at ground level. Height (m) is to the top of the mast arm; "
              "Owner is the road authority.")),
    code("STC", "Roadside Camera", "point", "street/traffic/equipment", "street", "0.25",
         symbol="Roadside Camera",
         label="Roadside camera",
         meaning="Traffic or enforcement camera on its own pole or on a signal pole.",
         how="Shoot the middle of the pole's base, not the camera. The pole's other attachments are not coded."),
    code("STK", "Signal Controller Cabinet", "point", "street/traffic/equipment", "street", "0.25",
         symbol="Signal Controller Cabinet",
         attrs=["Owner", "Condition"],
         label="Signal controller cabinet",
         meaning="Cabinet that houses a signal controller.",
         how=("Shoot the middle of the cabinet's footprint. Owner is the road authority; Condition notes "
              "damage or a missing door.")),

# ---- KK Kerbs and Drainage / Kerbs ---------------------------------------------------------------------------
    code("KKT", "Kerb Top Edge", "line", "drainage/kerbs/top", "kerb", "0.35",
         style="continuous", surface=True,
         label="Kerb top edge",
         meaning="Top front edge of a kerb, where the face meets the top.",
         how=("Shoot the top front arris with a level at every point: the street's design levels come "
              "from this string. Take a point at each change of direction and every 10 m on a curve."),
         example="KKT1 ST, KKT1, KKT1 END"),
    code("KKL", "Gutter Lip", "line", "drainage/kerbs/gutter", "kerb", "0.25",
         style="continuous", surface=True,
         label="Gutter lip",
         meaning="Flow line of the gutter at the foot of the kerb face.",
         how=("Shoot the line water runs on, where the gutter turns up into the kerb face. Level it "
              "carefully: gutter falls are checked against it."),
         example="KKL1 ST, KKL1, KKL1 END"),
    code("KKB", "Kerb Back Edge", "line", "drainage/kerbs/back", "kerb", "0.18",
         style="continuous", surface=True,
         label="Kerb back edge",
         meaning="Back edge of the kerb top.",
         how=("Shoot where the kerb top meets the footpath or the verge. It is needed only when the "
              "width of the kerb matters; KKT alone is enough otherwise."),
         example="KKB1 ST, KKB1, KKB1 END"),
    code("KKR", "Kerb Return", "line", "drainage/kerbs/top", "kerb", "0.35",
         style="continuous", surface=True,
         label="Kerb return",
         meaning="Kerb round a street corner, shot as a curve.",
         how=("Shoot the straight kerb up to the curve, then BC at its first point, one point near the "
              "middle, and EC at its last. Three points fix the arc, so make the middle one a true "
              "midpoint."),
         example="KKR1 ST, KKR1 BC, KKR1, KKR1 EC, KKR1 END"),
    code("KKM", "Mountable Kerb", "line", "drainage/kerbs/top", "kerb", "0.35",
         style="Mountable Kerb Edge", surface=True,
         label="Mountable kerb",
         meaning="Low kerb that a vehicle can drive over.",
         how=("Shoot the top front edge. Walk with the carriageway on your left, so the bump in the line "
              "falls on the road side."),
         example="KKM1 ST, KKM1, KKM1 END"),
    code("KKD", "Shallow Dish Drain", "line", "drainage/kerbs/dish-drain", "kerb", "0.25",
         style="Dish Drain Edge", surface=True,
         label="Shallow dish drain",
         meaning="Shallow V-shaped concrete drain.",
         how=("Shoot the invert, the lowest line of the dish, with a level at each point. The V falls on "
              "the left of travel."),
         example="KKD1 ST, KKD1, KKD1 END"),

# ---- KP Kerbs and Drainage / Pits and Structures -------------------------------------------------------------
    code("KPG", "Grated Gully Pit", "point", "drainage/structures/pit", "stormwater", "0.25",
         symbol="Gully Pit",
         attrs=["Pit Size (mm)", "Depth (m)", "Condition"],
         label="Grated gully pit",
         meaning="Grated pit that takes surface water.",
         how=("Shoot the middle of the grate. Pit Size (mm) is the clear opening of the pit; Depth (m) "
              "is from the grate to the invert. Level the invert separately as GPI.")),
    code("KPK", "Kerb Inlet Pit", "point", "drainage/structures/pit", "stormwater", "0.25",
         symbol="Kerb Inlet Pit",
         attrs=["Pit Size (mm)", "Depth (m)", "Condition"],
         label="Kerb inlet pit",
         meaning="Pit with its opening in the kerb face.",
         how=("Shoot the middle of the kerb opening on the kerb line. The notch is drawn on the symbol's "
              "left and is not turned to follow the kerb.")),
    code("KPJ", "Junction Pit", "point", "drainage/structures/pit", "stormwater", "0.25",
         symbol="Junction Pit",
         attrs=["Pit Size (mm)", "Depth (m)", "Condition"],
         label="Junction pit",
         meaning="Pit where pipes meet, with a solid lid.",
         how=("Shoot the middle of the lid. Depth (m) is from the lid to the lowest invert, which is "
              "levelled as GPI.")),
    code("KPH", "Stormwater Manhole", "point", "drainage/structures/manhole", "stormwater", "0.25",
         symbol="Stormwater Manhole",
         attrs=["Lid Size (mm)", "Depth (m)", "Condition"],
         label="Stormwater manhole",
         meaning="Round access cover over a stormwater pipe.",
         how=("Shoot the middle of the cover. Lid Size (mm) is the clear diameter of the opening. Sewer "
              "covers are USH, not this.")),
    code("KPT", "Gross Pollutant Trap", "point", "drainage/structures/outlet", "stormwater", "0.25",
         symbol="Gross Pollutant Trap",
         attrs=["Owner", "Condition"],
         label="Gross pollutant trap",
         meaning="Structure that traps litter and sediment before an outfall.",
         how=("Shoot the middle of the access lid. A trap large enough to need an outline is outlined "
              "with a generic line (XGL) and a note.")),
    code("KPW", "Headwall", "point", "drainage/structures/outlet", "stormwater", "0.25",
         symbol="Headwall",
         attrs=["Owner", "Condition"],
         label="Headwall",
         meaning="Concrete wall round the end of a pipe or culvert.",
         how=("Shoot the middle of the top of the wall, on the face where the pipe passes through, then "
              "code the pipe itself. The symbol is drawn open to the west and is not turned.")),
    code("KPC", "Culvert End", "point", "drainage/structures/outlet", "stormwater", "0.25",
         symbol="Culvert End",
         attrs=["Owner", "Condition"],
         label="Culvert end",
         meaning="Open end of a culvert barrel.",
         how=("Shoot the middle of the mouth at the invert. Shoot both ends of the culvert so its length "
              "and fall are known.")),
    code("KPS", "Subsoil Inspection Point", "point", "drainage/structures/pit", "stormwater", "0.25",
         symbol="Subsoil Inspection Point",
         label="Subsoil inspection point",
         meaning="Inspection opening on a subsoil drain.",
         how="Shoot the middle of the cap on the inspection riser."),

# ---- KC Kerbs and Drainage / Pipes and Culverts --------------------------------------------------------------
    code("KCP", "Stormwater Pipe", "line", "drainage/pipes/pipe", "stormwater", "0.35",
         style="Stormwater Pipe Line",
         pipe=("Invert", "diameter", "$Diameter (mm)", ""),
         attrs=["Material", "Diameter (mm)", "Depth (m)", "Owner", "Condition"],
         label="Stormwater pipe",
         meaning="Stormwater pipe, shot pit to pit.",
         how=("Shoot the invert at every pit and manhole it passes through: the rule reads the level you "
              "shoot as the bottom of the pipe. Give Diameter (mm) and Material."),
         example="KCP1 ST, KCP1, KCP1 END"),
    code("KCB", "Box Culvert", "line", "drainage/pipes/culvert", "stormwater", "0.35",
         style="Culvert Outline",
         pipe=("Invert", "culvert", "$Width (mm)", "$Height (mm)"),
         attrs=["Material", "Width (mm)", "Height (mm)", "Depth (m)", "Owner", "Condition"],
         label="Box culvert",
         meaning="Rectangular culvert, drawn as a pair of walls.",
         how=("Shoot the invert along the middle of the culvert and at both ends. Width (mm) and Height "
              "(mm) are the clear opening, not the outside size."),
         example="KCB1 ST, KCB1 END"),
    code("KCS", "Subsoil Drain", "line", "drainage/pipes/subsoil", "stormwater", "0.18",
         style="Subsoil Drain Line",
         attrs=["Depth (m)", "Condition"],
         label="Subsoil drain",
         meaning="Perforated drain buried beside a road or a wall.",
         how=("Shoot along the drain at each inspection point and bend. Depth (m) is to the invert, and "
              "Condition says whether it was found blocked."),
         example="KCS1 ST, KCS1, KCS1 END"),

# ---- KW Kerbs and Drainage / Channels and Waterways ----------------------------------------------------------
    code("KWD", "Open Drain Invert", "line", "drainage/channels/drain", "waterway", "0.25",
         style="Open Drain Invert", surface=True,
         label="Open drain invert",
         meaning="Lowest line of an open drain or a lined channel.",
         how=("Shoot the invert from upstream to downstream, with a level on every point. The chevrons "
              "point the way you walked, so walking with the water keeps them honest."),
         example="KWD1 ST, KWD1, KWD1 END"),
    code("KWC", "Creek Centreline", "line", "drainage/channels/creek", "waterway", "0.25",
         style="Creek Centreline",
         label="Creek centreline",
         meaning="Middle of a creek or river bed, drawn as a wave.",
         how=("Shoot the middle of the bed from upstream to downstream, a point at every bend and every "
              "riffle. The banks are GBT and GBB, and the water's edge is GWE."),
         example="KWC1 ST, KWC1, KWC1 END"),
    code("KWS", "Swale Centreline", "line", "drainage/channels/flow", "waterway", "0.18",
         style="Open Drain Invert",
         label="Swale centreline",
         meaning="Lowest line of a grassed swale.",
         how=("Shoot the lowest line of the swale from upstream to downstream. The top edges of the "
              "swale are shot as banks (GBT), not with this code."),
         example="KWS1 ST, KWS1, KWS1 END"),
    code("KWF", "Overland Flow Path", "line", "drainage/channels/flow", "waterway", "0.13",
         style="Open Drain Invert",
         label="Overland flow path",
         meaning="Route that water takes over the ground in a storm.",
         how=("Shoot the route from upstream to downstream, taking it from the survey or the flood "
              "study. It is a line of intent rather than a feature, so say so in the drafting note."),
         example="KWF1 ST, KWF1, KWF1 END"),

# ---- TT Rail and Transit / Track -----------------------------------------------------------------------------
    code("TTR", "Running Rail", "line", "rail/track/rail", "rail", "0.35",
         style="Running Rail",
         label="Running rail",
         meaning="Head of a running rail, drawn as a ladder of sleepers.",
         how=("Shoot the top centre of the rail head every 10 m on the straight and every 5 m on a "
              "curve. One string per rail, and give the two rails separate numbers (TTR1, TTR2)."),
         example="TTR1 ST, TTR1, TTR1 END"),
    code("TTC", "Track Centreline", "line", "rail/track/centreline", "rail", "0.18",
         style="Track Centreline",
         label="Track centreline",
         meaning="Centre of the track, halfway between the rails.",
         how=("Shoot the point halfway between the running rails at the same stations as the rails. The "
              "rails are the evidence and this is the computed alignment."),
         example="TTC1 ST, TTC1, TTC1 END"),
    code("TTB", "Ballast Edge", "line", "rail/track/ballast", "rail", "0.35",
         style="Ballast Edge", surface=True,
         label="Ballast edge",
         meaning="Shoulder of the ballast, where the stone meets the formation.",
         how="Shoot the shoulder of the ballast at each change of direction, one string per side.",
         example="TTB1 ST, TTB1, TTB1 END"),
    code("TTL", "Level Crossing", "line", "rail/track/crossing", "rail", "0.35",
         style="continuous",
         label="Level crossing",
         meaning="Outline of a road and rail level crossing.",
         how="Shoot each corner of the crossing deck and close with CL.",
         example="TTL1 ST, TTL1, TTL1, TTL1 CL"),
    code("TTX", "Turnout Marker", "point", "rail/track/fitting", "rail", "0.35",
         symbol="Turnout Marker",
         label="Turnout marker",
         meaning="Toe of a set of points, where the switch blade starts.",
         how=("Shoot the point where the switch blade begins. The heel and the crossing are shot as "
              "running rail.")),
    code("TTE", "Buffer Stop", "point", "rail/track/fitting", "rail", "0.35",
         symbol="Buffer Stop",
         label="Buffer stop",
         meaning="End of track, where a buffer stop stands.",
         how="Shoot the middle of the buffer face, between the rails."),

# ---- TO Rail and Transit / Overhead and Signals --------------------------------------------------------------
    code("TOW", "Contact Wire", "line", "rail/overhead/wire", "rail", "0.18",
         style="Contact Wire Line",
         label="Contact wire",
         meaning="Overhead contact wire that powers electric trains or trams.",
         how=("Shoot the wire's position under each mast and at each change of direction. Its height "
              "above the rail is measured separately and written on the plan."),
         example="TOW1 ST, TOW1, TOW1 END"),
    code("TOM", "Overhead Mast", "point", "rail/overhead/mast", "rail", "0.18",
         symbol="Overhead Mast",
         attrs=["Owner", "Material", "Height (m)", "Condition"],
         label="Overhead mast",
         meaning="Mast that carries the overhead wire.",
         how=("Shoot the middle of the mast base. Height (m) is to the top, Material is steel or "
              "concrete, Owner is the rail operator.")),
    code("TOS", "Signal Mast", "point", "rail/overhead/mast", "rail", "0.18",
         symbol="Signal Mast",
         label="Signal mast",
         meaning="Signal carried on a mast.",
         how="Shoot the middle of the mast base. A signal fixed to a gantry is shot at the gantry leg."),

# ---- TP Rail and Transit / Platforms -------------------------------------------------------------------------
    code("TPE", "Platform Edge", "line", "rail/platforms/edge", "rail", "0.35",
         style="Platform Edge Line", surface=True,
         label="Platform edge",
         meaning="Coping edge of a platform, nearest the track.",
         how=("Shoot the coping edge nearest the track. Walk with the platform on your left, so the tick "
              "falls on the platform side."),
         example="TPE1 ST, TPE1, TPE1 END"),

# ---- UW Utilities / Water ------------------------------------------------------------------------------------
    code("UWM", "Water Supply Pipe", "line", "utilities/water/main", "water", "0.35",
         style="Water Supply Pipe",
         pipe=("Centre", "diameter", "$Diameter (mm)", ""),
         attrs=["utility.type=water", "Material", "Diameter (mm)", "Depth (m)", "Owner", "Condition"],
         label="Water supply pipe",
         meaning="Pressure main that carries drinking water.",
         how=("Shoot the centre of the main at every fitting and bend, and where a locator or a pothole "
              "places it. Diameter (mm), Material and Depth (m) to the top of the pipe are the three "
              "that matter."),
         example="UWM1 ST, UWM1, UWM1 END"),
    code("UWB", "Water Service Connection", "line", "utilities/water/main", "water", "0.18",
         style="Service Connection Line",
         attrs=["utility.type=water", "Owner", "Depth (m)", "Condition"],
         label="Water service connection",
         meaning="Small pipe from a main to a property meter.",
         how=("Shoot from the tee on the main to the meter at the boundary. The string ends at the "
              "meter, which is coded UWX."),
         example="UWB1 ST, UWB1 END"),
    code("UWD", "Disused Water Main", "line", "utilities/water/disused", "water", "0.25",
         style="Disused Service Line",
         attrs=["utility.type=water", "utility.status=disused", "Material", "Diameter (mm)", "Depth (m)",
                "Owner", "Condition"],
         label="Disused water main",
         meaning="Water main that is no longer in service.",
         how=("Shoot it as carefully as a live main: it is still in the ground. Disused status is set "
              "for you. If it is cut and capped, say where in Condition."),
         example="UWD1 ST, UWD1, UWD1 END"),
    code("UWV", "Water Isolation Valve", "point", "utilities/water/fitting", "water", "0.25",
         symbol="Water Isolation Valve",
         attrs=["utility.type=water", "Owner", "Depth (m)", "Condition"],
         label="Water isolation valve",
         meaning="Isolating valve on a water main.",
         how=("Shoot the middle of the valve box lid. Depth (m) is from the lid to the top of the main "
              "it controls.")),
    code("UWX", "Water Flow Meter", "point", "utilities/water/fitting", "water", "0.25",
         symbol="Water Flow Meter",
         attrs=["utility.type=water", "Owner", "Depth (m)", "Condition"],
         label="Water flow meter",
         meaning="Meter that records water supplied.",
         how=("Shoot the middle of the meter lid. A meter in a pit is shot at the lid; the pit is not "
              "coded again.")),
    code("UWP", "Water Pit", "point", "utilities/water/structure", "water", "0.25",
         symbol="Water Pit",
         attrs=["utility.type=water", "Owner", "Lid Size (mm)", "Depth (m)", "Condition"],
         label="Water pit",
         meaning="Pit or chamber on a water service.",
         how=("Shoot the middle of the lid. Lid Size (mm) is the clear opening; Depth (m) is from the "
              "lid to the floor.")),
    code("UWT", "Water Tank", "point", "utilities/water/structure", "water", "0.25",
         symbol="Water Tank",
         attrs=["utility.type=water", "Owner", "Capacity (kL)", "Condition"],
         label="Water tank",
         meaning="Tank or reservoir that stores water.",
         how="Shoot the middle of the tank's footprint. Capacity (kL) is the rated volume in kilolitres."),
    code("UWU", "Water Pumping Station", "point", "utilities/water/structure", "water", "0.25",
         symbol="Pumping Station",
         attrs=["utility.type=water", "Owner", "Condition"],
         label="Water pumping station",
         meaning="Pump station on a water main.",
         how="Shoot the middle of the pump house, or the wet well lid when there is no building."),

# ---- US Utilities / Sewer ------------------------------------------------------------------------------------
    code("USM", "Sewer Gravity Pipe", "line", "utilities/sewer/main", "sewer", "0.35",
         style="Sewer Gravity Pipe",
         pipe=("Invert", "diameter", "$Diameter (mm)", ""),
         attrs=["utility.type=sewer", "Material", "Diameter (mm)", "Depth (m)", "Owner", "Condition"],
         label="Sewer gravity pipe",
         meaning="Gravity sewer; the level that matters is the invert.",
         how=("Shoot the invert at every maintenance hole, walking downstream: the chevron points the "
              "way you shot. Diameter (mm) and Material are read at the hole."),
         example="USM1 ST, USM1, USM1 END"),
    code("USR", "Sewer Pressure Pipe", "line", "utilities/sewer/main", "sewer", "0.35",
         style="Sewer Pressure Pipe",
         pipe=("Centre", "diameter", "$Diameter (mm)", ""),
         attrs=["utility.type=sewer", "Material", "Diameter (mm)", "Depth (m)", "Owner", "Condition"],
         label="Sewer pressure pipe",
         meaning="Pressure sewer that carries flow under pump.",
         how=("Shoot the centre of the pipe from the pump toward the discharge. Unlike a gravity sewer "
              "it need not fall, so depth and diameter are the data."),
         example="USR1 ST, USR1, USR1 END"),
    code("USB", "Sewer Branch", "line", "utilities/sewer/main", "sewer", "0.18",
         style="Service Connection Line",
         attrs=["utility.type=sewer", "Owner", "Depth (m)", "Condition"],
         label="Sewer branch",
         meaning="Property connection to a sewer main.",
         how=("Shoot from the junction on the main to the property inspection opening. Depth (m) at the "
              "junction is the useful one."),
         example="USB1 ST, USB1 END"),
    code("USD", "Disused Sewer Main", "line", "utilities/sewer/disused", "sewer", "0.25",
         style="Disused Service Line",
         attrs=["utility.type=sewer", "utility.status=disused", "Material", "Diameter (mm)", "Depth (m)",
                "Owner", "Condition"],
         label="Disused sewer main",
         meaning="Sewer that is no longer in service.",
         how=("Shoot as a live sewer, at each maintenance hole and bend. Disused status is set for you; "
              "say in Condition if it is sealed or flowing."),
         example="USD1 ST, USD1, USD1 END"),
    code("USH", "Sewer Maintenance Hole", "point", "utilities/sewer/structure", "sewer", "0.25",
         symbol="Sewer Maintenance Hole",
         attrs=["utility.type=sewer", "Owner", "Lid Size (mm)", "Depth (m)", "Invert Level (m)", "Condition"],
         label="Sewer maintenance hole",
         meaning="Man-entry chamber on a sewer.",
         how=("Shoot the middle of the cover. Depth (m) is cover to lowest invert, Invert Level (m) is "
              "the outlet pipe's, Lid Size (mm) is the clear opening. Stormwater manholes are KPH.")),
    code("USE", "Sewer Vent", "point", "utilities/sewer/fitting", "sewer", "0.25",
         symbol="Sewer Vent",
         attrs=["utility.type=sewer", "Owner", "Height (m)", "Condition"],
         label="Sewer vent",
         meaning="Vent stack on a sewer that lets gas out and air in.",
         how="Shoot the middle of the stack at ground level. Height (m) is to the top of the vent."),
    code("UST", "Septic System Tank", "point", "utilities/sewer/structure", "sewer", "0.25",
         symbol="Septic System Tank",
         attrs=["utility.type=sewer", "Owner", "Condition"],
         label="Septic system tank",
         meaning="Septic or other on-site treatment tank.",
         how="Shoot the middle of the tank's access lid. Several lids on one tank are shot at the largest."),
    code("USP", "Sewage Pumping Station", "point", "utilities/sewer/structure", "sewer", "0.25",
         symbol="Pumping Station",
         attrs=["utility.type=sewer", "Owner", "Condition"],
         label="Sewage pumping station",
         meaning="Pump station on a sewer that lifts flow to the next gravity run.",
         how="Shoot the middle of the wet well lid. The control cabinet is not shot separately."),

# ---- UG Utilities / Gas --------------------------------------------------------------------------------------
    code("UGM", "Gas Supply Pipe", "line", "utilities/gas/main", "gas", "0.35",
         style="Gas Supply Pipe",
         pipe=("Centre", "diameter", "$Diameter (mm)", ""),
         attrs=["utility.type=gas", "Material", "Diameter (mm)", "Depth (m)", "Owner", "Condition", "Pressure"],
         label="Gas supply pipe",
         meaning="Pipe that carries natural gas.",
         how=("Shoot the centre of the pipe at every fitting and bend. Pressure is the class the owner "
              "states (low, medium or high), never a guess. Diameter (mm), Material and Depth (m) as for "
              "any main."),
         example="UGM1 ST, UGM1, UGM1 END"),
    code("UGB", "Gas Service", "line", "utilities/gas/main", "gas", "0.18",
         style="Service Connection Line",
         attrs=["utility.type=gas", "Owner", "Depth (m)", "Condition"],
         label="Gas service",
         meaning="Property connection to a gas main.",
         how="Shoot from the tee on the main to the meter. The string ends at the meter, which is coded UGX.",
         example="UGB1 ST, UGB1 END"),
    code("UGD", "Disused Gas Main", "line", "utilities/gas/disused", "gas", "0.25",
         style="Disused Service Line",
         attrs=["utility.type=gas", "utility.status=disused", "Material", "Diameter (mm)", "Depth (m)",
                "Owner", "Condition"],
         label="Disused gas main",
         meaning="Gas main that is no longer in service.",
         how=("Shoot as a live main. A disused gas main may still hold gas, so say in Condition if it is "
              "purged or capped. Disused status is set for you."),
         example="UGD1 ST, UGD1, UGD1 END"),
    code("UGV", "Gas Isolation Valve", "point", "utilities/gas/fitting", "gas", "0.25",
         symbol="Gas Isolation Valve",
         attrs=["utility.type=gas", "Owner", "Depth (m)", "Condition"],
         label="Gas isolation valve",
         meaning="Isolating valve on a gas main.",
         how="Shoot the middle of the valve box lid. Depth (m) is from the lid to the top of the main."),
    code("UGR", "Gas Pressure Regulator", "point", "utilities/gas/fitting", "gas", "0.25",
         symbol="Gas Pressure Regulator",
         attrs=["utility.type=gas", "Owner", "Depth (m)", "Condition"],
         label="Gas pressure regulator",
         meaning="Pressure regulator, where gas pressure steps down.",
         how="Shoot the middle of the regulator, in its pit or on its pad."),
    code("UGX", "Gas Flow Meter", "point", "utilities/gas/fitting", "gas", "0.25",
         symbol="Gas Flow Meter",
         attrs=["utility.type=gas", "Owner", "Depth (m)", "Condition"],
         label="Gas flow meter",
         meaning="Meter that records gas supplied.",
         how=("Shoot the middle of the meter housing or its lid. Meters usually sit at the property "
              "boundary, where the gas service (UGB) ends.")),
    code("UGP", "Gas Pit", "point", "utilities/gas/structure", "gas", "0.25",
         symbol="Gas Pit",
         attrs=["utility.type=gas", "Owner", "Lid Size (mm)", "Depth (m)", "Condition"],
         label="Gas pit",
         meaning="Pit or valve box on a gas service.",
         how=("Shoot the middle of the lid. Lid Size (mm) is the clear opening; Depth (m) is from the "
              "lid to the floor.")),
    code("UGK", "Gas Marker Post", "point", "utilities/gas/fitting", "gas", "0.25",
         symbol="Marker Post",
         attrs=["utility.type=gas", "Owner", "Condition"],
         label="Gas marker post",
         meaning="Post that marks the route of a gas pipeline.",
         how=("Shoot the middle of the post at ground level. The pipeline is not necessarily directly "
              "beneath it.")),

# ---- UE Utilities / Electricity ------------------------------------------------------------------------------
    code("UEC", "Underground Cable", "line", "utilities/electricity/main", "electricity", "0.35",
         style="Underground Cable",
         attrs=["utility.type=electricity", "Owner", "Depth (m)", "Condition", "Ducts", "Voltage (V)"],
         label="Underground cable",
         meaning="Buried electricity cable.",
         how=("Shoot the centre of the cable, or of the duct bank that holds it. Voltage (V) is in whole "
              "volts and Ducts is the number of ducts. Assume every unlabelled cable is live."),
         example="UEC1 ST, UEC1, UEC1 END"),
    code("UEO", "Overhead Line", "line", "utilities/electricity/overhead", "electricity", "0.25",
         style="Overhead Line",
         attrs=["utility.type=electricity", "Owner", "Voltage (V)", "Height (m)", "Condition"],
         label="Overhead line",
         meaning="Overhead power line, shot under each pole.",
         how=("Shoot the ground under the line at each pole and at each change of direction. Height (m) "
              "is the lowest wire above ground at the middle of a span."),
         example="UEO1 ST, UEO1, UEO1 END"),
    code("UED", "Disused Cable", "line", "utilities/electricity/disused", "electricity", "0.25",
         style="Disused Service Line",
         attrs=["utility.type=electricity", "utility.status=disused", "Owner", "Depth (m)", "Condition",
                "Ducts"],
         label="Disused cable",
         meaning="Cable that is no longer in service.",
         how=("Shoot as a live cable. A disused cable may still be live, so say what you know in "
              "Condition. Disused status is set for you."),
         example="UED1 ST, UED1, UED1 END"),
    code("UEN", "Electrical Duct", "line", "utilities/electricity/main", "electricity", "0.25",
         style="Buried Duct",
         attrs=["utility.type=electricity", "Owner", "Depth (m)", "Condition", "Ducts"],
         label="Electrical duct",
         meaning="Duct or conduit that carries electricity cable.",
         how="Shoot the centre of the duct at each pit and bend. Ducts is the number of ducts in the bank.",
         example="UEN1 ST, UEN1, UEN1 END"),
    code("UEP", "Power Pole", "point", "utilities/electricity/structure", "electricity", "0.25",
         symbol="Power Pole",
         attrs=["utility.type=electricity", "Owner", "Material", "Height (m)", "Condition"],
         label="Power pole",
         meaning="Pole that carries power lines.",
         how=("Shoot the middle of the pole at ground level. Height (m) is the pole's height above "
              "ground, and Material is timber, concrete or steel. A street light on its own pole is SLS.")),
    code("UEY", "Pole Guy", "point", "utilities/electricity/fitting", "electricity", "0.25",
         symbol="Guy Anchor",
         attrs=["utility.type=electricity", "Owner", "Condition"],
         label="Pole guy",
         meaning="Anchor of a stay wire that braces a pole.",
         how=("Shoot the anchor where it enters the ground. The wire itself is not shot. The symbol is "
              "drawn pointing west and is not turned.")),
    code("UET", "Transformer", "point", "utilities/electricity/structure", "electricity", "0.25",
         symbol="Transformer",
         attrs=["utility.type=electricity", "Owner", "Rating (kVA)", "Condition"],
         label="Transformer",
         meaning="Transformer on a pole or on the ground.",
         how=("Shoot the middle of a ground unit, or the pole that carries a pole-mounted unit. Rating "
              "(kVA) is read from the nameplate.")),
    code("UEK", "Electrical Pillar", "point", "utilities/electricity/structure", "electricity", "0.25",
         symbol="Electrical Pillar",
         attrs=["utility.type=electricity", "Owner", "Condition"],
         label="Electrical pillar",
         meaning="Street pillar or cabinet of the electricity network.",
         how="Shoot the middle of the pillar's footprint. Condition notes damage or a missing door."),
    code("UEJ", "Electrical Pit", "point", "utilities/electricity/structure", "electricity", "0.25",
         symbol="Electrical Pit",
         attrs=["utility.type=electricity", "Owner", "Lid Size (mm)", "Depth (m)", "Condition"],
         label="Electrical pit",
         meaning="Pit or chamber on an electricity cable.",
         how=("Shoot the middle of the lid. Lid Size (mm) is the clear opening; Depth (m) is from the "
              "lid to the floor.")),

# ---- UC Utilities / Communications ---------------------------------------------------------------------------
    code("UCC", "Communications Cable", "line", "utilities/telecommunications/main", "telecom", "0.35",
         style="Communications Cable",
         attrs=["utility.type=telecommunications", "Owner", "Depth (m)", "Condition", "Ducts"],
         label="Communications cable",
         meaning="Buried telephone, data or fibre cable.",
         how=("Shoot the centre of the cable, or of the duct it runs in, at each pit and bend. Ducts is "
              "the number of ducts. Owner matters most here: several carriers share one route."),
         example="UCC1 ST, UCC1, UCC1 END"),
    code("UCN", "Communications Duct", "line", "utilities/telecommunications/main", "telecom", "0.25",
         style="Buried Duct",
         attrs=["utility.type=telecommunications", "Owner", "Depth (m)", "Condition", "Ducts"],
         label="Communications duct",
         meaning="Duct or conduit that carries communications cable.",
         how="Shoot the centre of the duct at each pit and bend. Ducts is the number of ducts in the bank.",
         example="UCN1 ST, UCN1, UCN1 END"),
    code("UCO", "Overhead Communications Cable", "line", "utilities/telecommunications/overhead", "telecom", "0.18",
         style="Overhead Line",
         attrs=["utility.type=telecommunications", "Owner", "Height (m)", "Condition"],
         label="Overhead communications cable",
         meaning="Aerial communications cable.",
         how=("Shoot the ground under the cable at each pole. Height (m) is the lowest point of the span "
              "above ground."),
         example="UCO1 ST, UCO1, UCO1 END"),
    code("UCD", "Disused Communications Cable", "line", "utilities/telecommunications/disused", "telecom", "0.25",
         style="Disused Service Line",
         attrs=["utility.type=telecommunications", "utility.status=disused", "Owner", "Depth (m)",
                "Condition", "Ducts"],
         label="Disused communications cable",
         meaning="Communications cable that is no longer in service.",
         how="Shoot as a live cable; owners often leave old cable in place. Disused status is set for you.",
         example="UCD1 ST, UCD1, UCD1 END"),
    code("UCP", "Communications Pit", "point", "utilities/telecommunications/structure", "telecom", "0.25",
         symbol="Communications Pit",
         attrs=["utility.type=telecommunications", "Owner", "Lid Size (mm)", "Depth (m)", "Condition"],
         label="Communications pit",
         meaning="Pit on a communications route.",
         how=("Shoot the middle of the lid. Lid Size (mm) is the clear opening; Depth (m) is from the "
              "lid to the floor.")),
    code("UCH", "Communications Maintenance Hole", "point", "utilities/telecommunications/structure", "telecom", "0.25",
         symbol="Communications Maintenance Hole",
         attrs=["utility.type=telecommunications", "Owner", "Lid Size (mm)", "Depth (m)", "Condition"],
         label="Communications maintenance hole",
         meaning="Man-entry chamber on a communications route.",
         how=("Shoot the middle of the cover. Lid Size (mm) is the clear opening; Depth (m) is from the "
              "cover to the floor.")),
    code("UCK", "Communications Pillar", "point", "utilities/telecommunications/structure", "telecom", "0.25",
         symbol="Communications Pillar",
         attrs=["utility.type=telecommunications", "Owner", "Condition"],
         label="Communications pillar",
         meaning="Street pillar or cabinet of the communications network.",
         how="Shoot the middle of the pillar's footprint. Owner is the carrier named on the door."),

# ---- UR Utilities / Recycled Water ---------------------------------------------------------------------------
    code("URM", "Recycled Water Pipe", "line", "utilities/recycled-water/main", "recycled", "0.35",
         style="Recycled Water Pipe",
         pipe=("Centre", "diameter", "$Diameter (mm)", ""),
         attrs=["utility.type=recycled-water", "Material", "Diameter (mm)", "Depth (m)", "Owner", "Condition"],
         label="Recycled water pipe",
         meaning="Main that carries recycled water.",
         how=("Shoot the centre of the main at every fitting and bend. Never assume it is drinking "
              "water: purple pipe and a purple lid are the clues. Diameter (mm), Material and Depth (m) "
              "as for any main."),
         example="URM1 ST, URM1, URM1 END"),
    code("URB", "Recycled Water Connection", "line", "utilities/recycled-water/main", "recycled", "0.18",
         style="Service Connection Line",
         attrs=["utility.type=recycled-water", "Owner", "Depth (m)", "Condition"],
         label="Recycled water connection",
         meaning="Property connection to a recycled water main.",
         how="Shoot from the tee on the main to the property meter. The string ends at the meter.",
         example="URB1 ST, URB1 END"),
    code("URD", "Disused Recycled Main", "line", "utilities/recycled-water/disused", "recycled", "0.25",
         style="Disused Service Line",
         attrs=["utility.type=recycled-water", "utility.status=disused", "Material", "Diameter (mm)",
                "Depth (m)", "Owner", "Condition"],
         label="Disused recycled main",
         meaning="Recycled water main that is no longer in service.",
         how=("Shoot as a live main: it is still in the ground and still in the way. Disused status is "
              "set for you."),
         example="URD1 ST, URD1, URD1 END"),
    code("URV", "Recycled Isolation Valve", "point", "utilities/recycled-water/fitting", "recycled", "0.25",
         symbol="Recycled Isolation Valve",
         attrs=["utility.type=recycled-water", "Owner", "Depth (m)", "Condition"],
         label="Recycled isolation valve",
         meaning="Isolating valve on a recycled water main.",
         how="Shoot the middle of the valve box lid. Depth (m) is from the lid to the top of the main."),
    code("URP", "Recycled Water Pit", "point", "utilities/recycled-water/structure", "recycled", "0.25",
         symbol="Recycled Water Pit",
         attrs=["utility.type=recycled-water", "Owner", "Lid Size (mm)", "Depth (m)", "Condition"],
         label="Recycled water pit",
         meaning="Pit or chamber on a recycled water service.",
         how=("Shoot the middle of the lid. Lid Size (mm) is the clear opening; Depth (m) is from the "
              "lid to the floor.")),

# ---- UF Utilities / Fire Service -----------------------------------------------------------------------------
    code("UFM", "Fire Service Pipe", "line", "utilities/fire-service/main", "fire", "0.35",
         style="Fire Service Pipe",
         pipe=("Centre", "diameter", "$Diameter (mm)", ""),
         attrs=["utility.type=fire-service", "Material", "Diameter (mm)", "Depth (m)", "Owner", "Condition"],
         label="Fire service pipe",
         meaning="Main that feeds hydrants and sprinkler systems.",
         how=("Shoot the centre of the main at every fitting and bend. A fire main inside a building "
              "boundary is usually private: say so in Owner."),
         example="UFM1 ST, UFM1, UFM1 END"),
    code("UFH", "Fire Hydrant", "point", "utilities/fire-service/fitting", "fire", "0.25",
         symbol="Fire Hydrant",
         attrs=["utility.type=fire-service", "Owner", "Hydrant Type", "Condition"],
         label="Fire hydrant",
         meaning="Fire hydrant, standing or underground.",
         how=("Shoot the middle of the hydrant, or of the lid of an underground one. Hydrant Type says "
              "pillar or underground.")),
    code("UFB", "Booster Connection", "point", "utilities/fire-service/fitting", "fire", "0.25",
         symbol="Booster Connection",
         attrs=["utility.type=fire-service", "Owner", "Condition"],
         label="Booster connection",
         meaning="Connection where the fire brigade boosts a building's supply.",
         how="Shoot the middle of the cabinet or the pad that carries the booster outlets."),
    code("UFV", "Fire Isolation Valve", "point", "utilities/fire-service/fitting", "fire", "0.25",
         symbol="Fire Isolation Valve",
         attrs=["utility.type=fire-service", "Owner", "Depth (m)", "Condition"],
         label="Fire isolation valve",
         meaning="Isolating valve on a fire service.",
         how="Shoot the middle of the valve box lid. Depth (m) is from the lid to the top of the pipe."),

# ---- UP Utilities / Fuel -------------------------------------------------------------------------------------
    code("UPM", "Fuel Supply Pipe", "line", "utilities/fuel/main", "fuel", "0.35",
         style="Fuel Supply Pipe",
         pipe=("Centre", "diameter", "$Diameter (mm)", ""),
         attrs=["utility.type=fuel", "Material", "Diameter (mm)", "Depth (m)", "Owner", "Condition", "Product"],
         label="Fuel supply pipe",
         meaning="Pipeline for fuel or oil.",
         how=("Shoot the centre of the pipe at every fitting and bend. Product is what it carries "
              "(petrol, diesel, jet fuel, oil). Treat an unlabelled steel line in a fuel precinct as "
              "fuel until proved otherwise."),
         example="UPM1 ST, UPM1, UPM1 END"),
    code("UPV", "Fuel Isolation Valve", "point", "utilities/fuel/fitting", "fuel", "0.25",
         symbol="Fuel Isolation Valve",
         attrs=["utility.type=fuel", "Owner", "Depth (m)", "Condition"],
         label="Fuel isolation valve",
         meaning="Isolating valve on a fuel pipeline.",
         how="Shoot the middle of the valve box lid. Depth (m) is from the lid to the top of the pipe."),
    code("UPK", "Fuel Marker Post", "point", "utilities/fuel/fitting", "fuel", "0.25",
         symbol="Marker Post",
         attrs=["utility.type=fuel", "Owner", "Condition"],
         label="Fuel marker post",
         meaning="Post that marks the route of a fuel pipeline.",
         how=("Shoot the middle of the post at ground level. The pipeline is not necessarily directly "
              "beneath it.")),
    code("UPT", "Fuel Tank", "point", "utilities/fuel/structure", "fuel", "0.25",
         symbol="Storage Tank",
         attrs=["utility.type=fuel", "Owner", "Capacity (kL)", "Product"],
         label="Fuel tank",
         meaning="Underground or surface fuel tank.",
         how=("Shoot the middle of the tank, or of the fill point of an underground one. Capacity (kL) "
              "and Product from the label or the owner.")),

# ---- UI Utilities / Traffic Systems --------------------------------------------------------------------------
    code("UIC", "Traffic Systems Conduit", "line", "utilities/its/main", "its", "0.25",
         style="Buried Duct",
         attrs=["utility.type=its", "Owner", "Depth (m)", "Condition", "Ducts"],
         label="Traffic systems conduit",
         meaning="Conduit for signal, camera or sensor cable.",
         how="Shoot the centre of the conduit at each pit and bend. Ducts is the number of ducts.",
         example="UIC1 ST, UIC1, UIC1 END"),
    code("UIP", "Traffic Systems Pit", "point", "utilities/its/structure", "its", "0.25",
         symbol="Traffic Systems Pit",
         attrs=["utility.type=its", "Owner", "Lid Size (mm)", "Depth (m)", "Condition"],
         label="Traffic systems pit",
         meaning="Pit on a traffic systems route.",
         how=("Shoot the middle of the lid. Lid Size (mm) is the clear opening; Depth (m) is from the "
              "lid to the floor.")),
    code("UIL", "Loop Detector", "point", "utilities/its/fitting", "its", "0.25",
         symbol="Loop Detector",
         attrs=["utility.type=its", "Owner", "Condition"],
         label="Loop detector",
         meaning="Vehicle detector loop cut into the road.",
         how="Shoot the middle of the loop, or of its saw cut. Shoot its lead-in cable with UIC."),

# ---- UX Utilities / Unknown Services -------------------------------------------------------------------------
    code("UXL", "Unknown Service", "line", "utilities/unknown/main", "unknown", "0.25",
         style="Unknown Service Line",
         attrs=["utility.type=unknown", "Depth (m)", "Locate Method"],
         label="Unknown service",
         meaning="Buried line whose service has not been established.",
         how=("Shoot a line you can see or detect but cannot name. Locate Method says how you found it "
              "(locator, radar, potholed). Recode it as soon as the owner replies; an unknown line is a "
              "question, not a fact."),
         example="UXL1 ST, UXL1, UXL1 END"),
    code("UXP", "Unknown Pit", "point", "utilities/unknown/structure", "unknown", "0.25",
         symbol="Unknown Pit",
         attrs=["utility.type=unknown", "Depth (m)", "Lid Size (mm)"],
         label="Unknown pit",
         meaning="Pit whose service has not been established.",
         how=("Shoot the middle of the lid. Lid Size (mm) is the clear opening. Lift the lid if it is "
              "safe to, and recode the pit if it says what it is.")),
    code("UXH", "Unknown Maintenance Hole", "point", "utilities/unknown/structure", "unknown", "0.25",
         symbol="Unknown Maintenance Hole",
         attrs=["utility.type=unknown", "Depth (m)", "Lid Size (mm)"],
         label="Unknown maintenance hole",
         meaning="Man-entry chamber whose service has not been established.",
         how=("Shoot the middle of the cover. Lid Size (mm) is the clear opening. Do not open an unknown "
              "cover without a gas test.")),
    code("UXM", "Surface Paint Mark", "point", "utilities/unknown/marking", "unknown", "0.25",
         symbol="Paint Mark",
         attrs=["utility.type=unknown", "Paint Colour", "Locate Method"],
         label="Surface paint mark",
         meaning="Spray-paint mark that locates a service from the surface.",
         how=("Shoot the middle of the mark. Paint Colour is the colour you see, which names the service "
              "when the locator used the usual convention. Locate Method is who painted it and how.")),

# ---- VT Vegetation and Landscape / Trees ---------------------------------------------------------------------
    code("VTB", "Broadleaf Tree", "point", "vegetation/trees/tree", "tree", "0.18",
         symbol="Broadleaf Tree",
         attrs=["Species", "Height (m)", "Canopy Diameter (m)", "Trunk Diameter (mm)"],
         label="Broadleaf tree",
         meaning="Tree with a broad, leafy canopy.",
         how=("Shoot the middle of the trunk at ground level. Species is the common name; Height (m) and "
              "Canopy Diameter (m) are estimated to the nearest half metre; Trunk Diameter (mm) is taken "
              "1.4 m above ground.")),
    code("VTC", "Conifer Tree", "point", "vegetation/trees/tree", "tree", "0.18",
         symbol="Conifer Tree",
         attrs=["Species", "Height (m)", "Canopy Diameter (m)", "Trunk Diameter (mm)"],
         label="Conifer tree",
         meaning="Pine, cypress or other cone-bearing tree.",
         how=("Shoot the middle of the trunk at ground level. Species, Height (m), Canopy Diameter (m) "
              "and Trunk Diameter (mm) as for any tree.")),
    code("VTP", "Palm Tree", "point", "vegetation/trees/tree", "tree", "0.18",
         symbol="Palm Tree",
         attrs=["Species", "Height (m)", "Canopy Diameter (m)", "Trunk Diameter (mm)"],
         label="Palm tree",
         meaning="Palm: a single unbranched trunk with a crown of fronds.",
         how=("Shoot the middle of the trunk at ground level. A palm's canopy is its frond spread, taken "
              "as Canopy Diameter (m).")),
    code("VTG", "Native Gum Tree", "point", "vegetation/trees/tree", "tree", "0.18",
         symbol="Native Gum Tree",
         attrs=["Species", "Height (m)", "Canopy Diameter (m)", "Trunk Diameter (mm)"],
         label="Native gum tree",
         meaning="Eucalypt or other native gum.",
         how=("Shoot the middle of the trunk at ground level. Gums often fork low: measure Trunk "
              "Diameter (mm) below the fork and say so in the species note.")),
    code("VTD", "Dead Tree", "point", "vegetation/trees/dead", "tree", "0.18",
         symbol="Dead Tree",
         attrs=["Species", "Height (m)", "Canopy Diameter (m)", "Trunk Diameter (mm)"],
         label="Dead tree",
         meaning="Standing dead tree: bare, but still a hazard and often a habitat.",
         how=("Shoot the middle of the trunk at ground level. A dead tree with hollows is a habitat: add "
              "an XHH point beside it.")),
    code("VTS", "Tree Stump", "point", "vegetation/trees/dead", "tree", "0.18",
         symbol="Tree Stump",
         attrs=["Trunk Diameter (mm)"],
         label="Tree stump",
         meaning="Stump of a felled tree, cut at or near ground level.",
         how="Shoot the middle of the stump. Trunk Diameter (mm) is taken across the cut face."),
    code("VTR", "Tree Row", "both", "vegetation/trees/line", "tree", "0.13",
         style="continuous", symbol="Broadleaf Tree",
         attrs=["Species"],
         label="Tree row",
         meaning="Line of trees, with a tree symbol at every point.",
         how=("Shoot the middle of each trunk in order. A tree is drawn at every point and the line is "
              "drawn through them, so a row of 30 trees is 30 points. Species applies to the whole row."),
         example="VTR1 ST, VTR1, VTR1 END"),
    code("VTK", "Tree Canopy Outline", "line", "vegetation/trees/line", "tree", "0.13",
         style="continuous",
         label="Tree canopy outline",
         meaning="Drip line of a large canopy, drawn as an outline.",
         how=("Shoot the edge of the canopy at a point every 2 m round it, and close with CL. The "
              "outline helps where the canopy matters more than the trunk, as in a tree protection zone."),
         example="VTK1 ST, VTK1, VTK1, VTK1 CL"),

# ---- VS Vegetation and Landscape / Shrubs and Plants ---------------------------------------------------------
    code("VSS", "Shrub", "point", "vegetation/shrubs/plant", "planting", "0.18",
         symbol="Shrub",
         label="Shrub",
         meaning="Single shrub: a woody plant with no main trunk, big enough to matter.",
         how=("Shoot the middle of the plant. Use for shrubs big enough to matter; low planting is a "
              "garden bed edge (VGM).")),
    code("VSC", "Shrub Cluster", "point", "vegetation/shrubs/plant", "planting", "0.18",
         symbol="Shrub Cluster",
         label="Shrub cluster",
         meaning="Group of shrubs too close together to shoot one by one.",
         how="Shoot the middle of the group. For a large group, outline it with VGB instead."),
    code("VST", "Tussock", "point", "vegetation/shrubs/plant", "planting", "0.18",
         symbol="Tussock",
         label="Tussock",
         meaning="Tussock or clump of grass.",
         how=("Shoot the middle of the clump. Use it only for tussocks big enough to matter, such as a "
              "pampas or lomandra clump; ordinary grass is not coded.")),
    code("VSH", "Hedge", "line", "vegetation/shrubs/hedge", "planting", "0.18",
         style="Hedge Line",
         attrs=["Height (m)", "Species"],
         label="Hedge",
         meaning="Clipped hedge, shot along its face.",
         how=("Shoot along the face of the hedge nearest you. Walk with the hedge on your left, so the "
              "bumps fall on the hedge side. Height (m) and Species apply to the whole length."),
         example="VSH1 ST, VSH1, VSH1 END"),

# ---- VG Vegetation and Landscape / Ground Cover --------------------------------------------------------------
    code("VGL", "Lawn Edge", "line", "vegetation/ground/edge", "planting", "0.13",
         style="Grass Edge",
         label="Lawn edge",
         meaning="Edge of mown grass, where lawn meets paving, garden or bush.",
         how=("Shoot where the lawn meets paving, garden or bush. Walk with the lawn on your left so the "
              "fan falls on the grass."),
         example="VGL1 ST, VGL1, VGL1 END"),
    code("VGB", "Bush Edge", "line", "vegetation/ground/edge", "planting", "0.18",
         style="Bush Edge Line",
         label="Bush edge",
         meaning="Edge of scrub or bush: the outer limit of dense growth.",
         how=("Shoot the outer edge of the bush at each change of direction. Walk with the bush on your "
              "left so the tufts fall on the bush side."),
         example="VGB1 ST, VGB1, VGB1 END"),
    code("VGM", "Garden Bed Edge", "line", "vegetation/ground/edge", "planting", "0.13",
         style="continuous",
         label="Garden bed edge",
         meaning="Edge of a garden bed, where planting meets lawn or paving.",
         how="Shoot the edge of the bed where it meets lawn or paving. Close with CL for an island bed.",
         example="VGM1 ST, VGM1, VGM1, VGM1 CL"),
    code("VGC", "Crop Edge", "line", "vegetation/ground/edge", "planting", "0.13",
         style="continuous",
         label="Crop edge",
         meaning="Edge of a cropped paddock.",
         how=("Shoot the edge of the crop at each corner and close with CL. Cropped land changes season "
              "by season, so date it in the job notes."),
         example="VGC1 ST, VGC1, VGC1, VGC1 CL"),

# ---- BB Buildings and Structures / Buildings -----------------------------------------------------------------
    code("BBW", "Building Wall Face", "line", "buildings/outline/wall", "building", "0.35",
         style="continuous",
         attrs=["Name", "Floors", "Roof Material"],
         label="Building wall face",
         meaning="Outer face of a building wall at ground level.",
         how=("Shoot the outside face corner to corner. Close with CL, or finish a rectangular building "
              "with RECT on its third corner and Katana builds the fourth. Name, Floors and Roof "
              "Material go with the string."),
         example="BBW1 ST, BBW1, BBW1 RECT"),
    code("BBE", "Eave Line", "line", "buildings/outline/roof", "building", "0.18",
         style="Eave Line",
         label="Eave line",
         meaning="Edge of the roof overhang, seen from above.",
         how=("Shoot the outer edge of the eave or gutter, as the roof looks on a plan. Close with CL "
              "where it goes round the building."),
         example="BBE1 ST, BBE1, BBE1, BBE1 CL"),
    code("BBV", "Verandah Line", "line", "buildings/outline/roof", "building", "0.18",
         style="Eave Line",
         label="Verandah line",
         meaning="Edge of a verandah or awning roof.",
         how=("Shoot the outer edge of the verandah roof. The posts that hold it are coded SBB or FGF as "
              "they stand."),
         example="BBV1 ST, BBV1, BBV1 END"),
    code("BBH", "Hidden Outline", "line", "buildings/outline/hidden", "building", "0.13",
         style="Hidden Outline",
         label="Hidden outline",
         meaning="Building edge that shows on a plan but not above ground.",
         how=("Shoot an edge you know is there but cannot see, such as a basement wall from a plan. Say "
              "in the drafting note where you took it from."),
         example="BBH1 ST, BBH1, BBH1 END"),
    code("BBU", "Building Under Construction", "line", "buildings/outline/hidden", "building", "0.13",
         style="Hidden Outline",
         label="Building under construction",
         meaning="Outline of a building that is not yet finished.",
         how=("Shoot the outline as it stands at the time of survey. Recode as BBW at the next survey "
              "once the walls are complete."),
         example="BBU1 ST, BBU1, BBU1, BBU1 CL"),
    code("BBC", "Building Corner", "point", "buildings/outline/detail", "building", "0.35",
         symbol="Building Corner",
         label="Building corner",
         meaning="Building corner shot where a line cannot be strung.",
         how=("Shoot a corner you can reach but cannot string to its neighbours, for example behind a "
              "fence. Join it into the outline later with a JPN control.")),
    code("BBD", "Doorway", "point", "buildings/outline/detail", "building", "0.35",
         symbol="Doorway",
         label="Doorway",
         meaning="Door in a wall, at ground level.",
         how=("Shoot the middle of the threshold. The swing arc in the symbol is for reading only and is "
              "not to scale.")),

# ---- BS Buildings and Structures / Structures ----------------------------------------------------------------
    code("BSB", "Bridge Deck Edge", "line", "buildings/structures/bridge", "building", "0.50",
         style="continuous",
         label="Bridge deck edge",
         meaning="Edge of a bridge deck, taken at the kerb or the parapet.",
         how=("Shoot the deck edge at the kerb or parapet, at each pier and each change of direction. "
              "One string per side."),
         example="BSB1 ST, BSB1, BSB1 END"),
    code("BSP", "Bridge Column", "point", "buildings/structures/bridge", "building", "0.35",
         symbol="Bridge Column",
         label="Bridge column",
         meaning="Pier or column of a bridge.",
         how=("Shoot the middle of the column at ground level. A wide pier is outlined with a generic "
              "line (XGL).")),
    code("BST", "Mast", "point", "buildings/structures/structure", "building", "0.35",
         symbol="Mast",
         attrs=["Owner", "Height (m)"],
         label="Mast",
         meaning="Mast, tower or antenna that stands on its own.",
         how=("Shoot the middle of the base. Height (m) is to the top of the structure; Owner is the "
              "carrier or authority.")),
    code("BSK", "Storage Tank", "point", "buildings/structures/structure", "building", "0.35",
         symbol="Storage Tank",
         attrs=["Owner", "Contents"],
         label="Storage tank",
         meaning="Tank or silo above ground.",
         how=("Shoot the middle of the base. Contents says what it holds. A large tank is outlined with "
              "a generic line (XGL).")),
    code("BSF", "Flagpole", "point", "buildings/structures/structure", "building", "0.35",
         symbol="Flagpole",
         label="Flagpole",
         meaning="Flagpole standing alone, as at a school, an office or a park.",
         how="Shoot the middle of the pole at ground level. The pennant in the symbol is for reading only."),
    code("BSM", "Monument", "point", "buildings/structures/structure", "building", "0.35",
         symbol="Monument",
         attrs=["Name"],
         label="Monument",
         meaning="Monument, statue or memorial.",
         how="Shoot the middle of the base. Name is the inscription or the common name of the monument."),

# ---- BA Buildings and Structures / Steps and Access ----------------------------------------------------------
    code("BAS", "Steps", "line", "buildings/access/access", "building", "0.25",
         style="Steps Edge",
         label="Steps",
         meaning="Flight of steps, shot along the top nosing.",
         how=("Shoot the top nosing of the flight from one side to the other. The treads are drawn "
              "across the line for you. Shoot the bottom nosing with a second string."),
         example="BAS1 ST, BAS1 END"),
    code("BAH", "Handrail", "line", "buildings/access/access", "building", "0.13",
         style="continuous",
         label="Handrail",
         meaning="Handrail or balustrade beside steps, a ramp or a drop.",
         how="Shoot along the top of the rail at each post and at each change of direction.",
         example="BAH1 ST, BAH1, BAH1 END"),

# ---- FF Fences and Walls / Fences ----------------------------------------------------------------------------
    code("FFC", "Chain Mesh Fence", "both", "fences/fence/chain-mesh", "fence", "0.25",
         style="Chain Mesh Fence", symbol="Fence Post",
         attrs=["Height (m)", "Material", "Condition"],
         label="Chain mesh fence",
         meaning="Wire mesh fence on steel posts.",
         how=("Shoot the centre of each post in order. A post is drawn at every point, so shoot posts "
              "and corners, never mid-span. Height (m), Material and Condition apply to the string."),
         example="FFC1 ST, FFC1, FFC1 END"),
    code("FFP", "Paling Fence", "both", "fences/fence/timber", "fence", "0.25",
         style="Paling Fence", symbol="Fence Post",
         attrs=["Height (m)", "Material", "Condition"],
         label="Paling fence",
         meaning="Timber paling fence: upright boards nailed to rails between posts.",
         how=("Shoot each post in order. Height (m) is to the top of the palings. Note a lapped or "
              "capped fence in Condition."),
         example="FFP1 ST, FFP1, FFP1 END"),
    code("FFR", "Post and Rail Fence", "both", "fences/fence/timber", "fence", "0.25",
         style="Post and Rail Fence", symbol="Fence Post",
         attrs=["Height (m)", "Material", "Condition"],
         label="Post and rail fence",
         meaning="Timber or steel post and rail fence.",
         how="Shoot each post in order. Height (m) is to the top rail.",
         example="FFR1 ST, FFR1, FFR1 END"),
    code("FFW", "Post and Wire Fence", "both", "fences/fence/wire", "fence", "0.25",
         style="Post and Wire Fence", symbol="Fence Post",
         attrs=["Height (m)", "Material", "Condition"],
         label="Post and wire fence",
         meaning="Rural fence of plain wire on posts.",
         how="Shoot each strainer and corner post, and posts along long runs. Height (m) is to the top wire.",
         example="FFW1 ST, FFW1, FFW1 END"),
    code("FFB", "Barbed Wire Fence", "both", "fences/fence/wire", "fence", "0.25",
         style="Barbed Wire Fence", symbol="Fence Post",
         attrs=["Height (m)", "Material", "Condition"],
         label="Barbed wire fence",
         meaning="Rural fence with barbed wire.",
         how=("Shoot each strainer and corner post, and posts along long runs. Height (m) is to the top "
              "wire; the barbs are drawn on both sides."),
         example="FFB1 ST, FFB1, FFB1 END"),
    code("FFE", "Electric Fence", "both", "fences/fence/wire", "fence", "0.25",
         style="Electric Fence", symbol="Fence Post",
         attrs=["Height (m)", "Material", "Condition"],
         label="Electric fence",
         meaning="Fence with an electrified wire.",
         how=("Shoot each post in order. Treat it as live until the owner says otherwise, and say so in "
              "Condition."),
         example="FFE1 ST, FFE1, FFE1 END"),
    code("FFM", "Metal Palisade Fence", "both", "fences/fence/palisade", "fence", "0.25",
         style="Metal Palisade Fence", symbol="Fence Post",
         attrs=["Height (m)", "Material", "Condition"],
         label="Metal palisade fence",
         meaning="Steel palisade or pool-style fence.",
         how="Shoot each post in order. Height (m) is to the top of the palings. A pool fence's gate is FGP.",
         example="FFM1 ST, FFM1, FFM1 END"),

# ---- FW Fences and Walls / Walls -----------------------------------------------------------------------------
    code("FWB", "Brick Wall", "line", "fences/wall/masonry", "wall", "0.35",
         style="Brick Wall",
         attrs=["Height (m)", "Material", "Condition"],
         label="Brick wall",
         meaning="Brick wall, shot along the face on the lower side.",
         how=("Shoot the face on the lower-ground side at its base, then the top with a second string if "
              "its height matters. Height (m) is the height from the lower ground."),
         example="FWB1 ST, FWB1, FWB1 END"),
    code("FWK", "Block Wall", "line", "fences/wall/masonry", "wall", "0.35",
         style="Brick Wall",
         attrs=["Height (m)", "Material", "Condition"],
         label="Block wall",
         meaning="Concrete or masonry block wall.",
         how=("Shoot the face on the lower-ground side. Height (m) is from the lower ground. A rendered "
              "block wall is still a block wall: note the render in Material."),
         example="FWK1 ST, FWK1, FWK1 END"),
    code("FWS", "Stone Wall", "line", "fences/wall/masonry", "wall", "0.35",
         style="Stone Wall",
         attrs=["Height (m)", "Material", "Condition"],
         label="Stone wall",
         meaning="Dry or mortared stone wall.",
         how=("Shoot the face on the lower-ground side. Say dry or mortared in Material; a dry wall is "
              "often a heritage item (XHT)."),
         example="FWS1 ST, FWS1, FWS1 END"),
    code("FWC", "Concrete Wall", "line", "fences/wall/concrete", "wall", "0.35",
         style="continuous",
         attrs=["Height (m)", "Material", "Condition"],
         label="Concrete wall",
         meaning="Poured or precast concrete wall.",
         how="Shoot the face on the lower-ground side. Height (m) is from the lower ground.",
         example="FWC1 ST, FWC1, FWC1 END"),
    code("FWR", "Earth Retaining Wall", "line", "fences/wall/retaining", "wall", "0.35",
         style="Earth Retaining Wall",
         attrs=["Height (m)", "Material", "Condition"],
         label="Earth retaining wall",
         meaning="Wall that holds back earth; the triangles point downhill.",
         how=("Shoot the top of the wall face. Walk with the retained, higher ground on your RIGHT, so "
              "the triangles fall to the lower side on your left. Height (m) is the greatest retained "
              "height."),
         example="FWR1 ST, FWR1, FWR1 END"),
    code("FWT", "Sleeper Retaining Wall", "line", "fences/wall/retaining", "wall", "0.25",
         style="Earth Retaining Wall",
         attrs=["Height (m)", "Material", "Condition"],
         label="Sleeper retaining wall",
         meaning="Retaining wall of sleepers or logs.",
         how=("Shoot the top of the wall face, walking with the retained ground on your right, as for "
              "any retaining wall. Material says timber, steel or concrete sleeper."),
         example="FWT1 ST, FWT1, FWT1 END"),

# ---- FG Fences and Walls / Gates and Posts -------------------------------------------------------------------
    code("FGP", "Pedestrian Gate", "point", "fences/gate/gate", "fence", "0.25",
         symbol="Pedestrian Gate",
         label="Pedestrian gate",
         meaning="Gate for people: a pedestrian opening in a fence or a wall.",
         how="Shoot the middle of the opening at the gate's line. The two posts are shot as FGF or FGS."),
    code("FGV", "Vehicle Gate", "point", "fences/gate/gate", "fence", "0.25",
         symbol="Vehicle Gate",
         label="Vehicle gate",
         meaning="Gate wide enough for a vehicle.",
         how="Shoot the middle of the opening at the gate's line. The posts either side are shot as FGS."),
    code("FGS", "Strainer Post", "point", "fences/gate/post", "fence", "0.25",
         symbol="Strainer Post",
         label="Strainer post",
         meaning="Braced corner or end post.",
         how=("Shoot the middle of the post. Strainers carry the tension of a wire fence and are shot at "
              "every corner, end and gate.")),
    code("FGF", "Fence Post", "point", "fences/gate/post", "fence", "0.25",
         symbol="Fence Post",
         label="Fence post",
         meaning="Single fence post standing alone, not part of a fence string.",
         how=("Shoot the middle of the post. A post that belongs to a fence string is shot as part of "
              "it; use this code for a lone post.")),

# ---- GP Terrain and Breaklines / Ground Points ---------------------------------------------------------------
    code("GPG", "Ground Shot", "point", "terrain/points/ground", "ground", "0.13",
         symbol="Ground Shot", surface=True, hide=True,
         label="Ground shot",
         meaning="Level on natural ground: the height is what matters, the mark is a tiny plus.",
         how=("Use for every ordinary shot on natural ground. The rule marks it hidden, so a surface of "
              "ten thousand shots does not bury the plan; Katana does not yet apply hide, so for now a 1 "
              "mm plus shows.")),
    code("GPS", "Spot Level", "point", "terrain/points/spot", "ground", "0.13",
         symbol="Spot Level", surface=True,
         label="Spot level",
         meaning="Level on a feature, shown with its mark so it can be checked.",
         how=("Use for a level on something that is not natural ground, such as a step, a slab corner or "
              "a floor level. It stays visible so the checker can find it.")),
    code("GPH", "High Point", "point", "terrain/points/spot", "ground", "0.13",
         symbol="High Point", surface=True,
         label="High point",
         meaning="Local high point of the ground.",
         how=("Shoot the highest point of a mound, a crest or a hilltop, and only when it is a real "
              "feature of the ground.")),
    code("GPL", "Low Point", "point", "terrain/points/spot", "ground", "0.13",
         symbol="Low Point", surface=True,
         label="Low point",
         meaning="Local low point of the ground.",
         how=("Shoot the lowest point of a sag, a hollow or a sump. A low point that drains to a pit is "
              "coded GPI at the pit.")),
    code("GPR", "Road Surface Level", "point", "terrain/points/ground", "ground", "0.13",
         symbol="Ground Shot", surface=True, hide=True,
         label="Road surface level",
         meaning="Level on a paved surface: the height is what matters, the mark is a tiny plus.",
         how=("Use for ordinary shots on a sealed surface, a slab or a car park. The rule marks it "
              "hidden like GPG; Katana does not yet apply hide, so for now a 1 mm plus shows.")),
    code("GPI", "Invert Level", "point", "terrain/points/spot", "ground", "0.13",
         symbol="Invert Level", surface=True,
         label="Invert level",
         meaning="Level at the bottom of a pipe, a pit or a drain.",
         how=("Shoot the invert, the lowest inside point of the pipe or pit, with a staff or a depth "
              "gauge held on it. Record the pit or pipe with its own code beside it.")),

# ---- GB Terrain and Breaklines / Breaklines ------------------------------------------------------------------
    code("GBT", "Bank Top Edge", "line", "terrain/breaklines/bank", "breakline", "0.25",
         style="Bank Top Edge", surface=True,
         label="Bank top edge",
         meaning="Top edge of a slope; the ticks point downhill.",
         how=("Shoot along the top of the slope with the slope falling away on your LEFT, so the ticks "
              "fall downhill. Take a point at every change of grade."),
         example="GBT1 ST, GBT1, GBT1 END"),
    code("GBB", "Bank Toe Edge", "line", "terrain/breaklines/bank", "breakline", "0.25",
         style="Bank Toe Edge", surface=True,
         label="Bank toe edge",
         meaning="Bottom edge of a slope; the short ticks point downhill.",
         how=("Shoot along the toe of the slope with the slope rising on your RIGHT, so the short ticks "
              "fall away from the bank, downhill. Take a point at every change of grade."),
         example="GBB1 ST, GBB1, GBB1 END"),
    code("GBK", "Cutting Top Edge", "line", "terrain/breaklines/bank", "breakline", "0.25",
         style="Bank Top Edge", surface=True,
         label="Cutting top edge",
         meaning="Top edge of an excavated slope.",
         how=("Shoot along the top of the cut with the cutting on your LEFT, so the ticks fall down into "
              "it. A natural bank is GBT."),
         example="GBK1 ST, GBK1, GBK1 END"),
    code("GBF", "Fill Toe Edge", "line", "terrain/breaklines/bank", "breakline", "0.25",
         style="Bank Toe Edge", surface=True,
         label="Fill toe edge",
         meaning="Bottom edge of an embankment.",
         how=("Shoot along the toe of the fill with the embankment on your RIGHT, so the short ticks "
              "fall away from it, downhill. A natural bank toe is GBB."),
         example="GBF1 ST, GBF1, GBF1 END"),
    code("GBR", "Ridge Line", "line", "terrain/breaklines/ridge-gully", "breakline", "0.18",
         style="Ridge Line", surface=True,
         label="Ridge line",
         meaning="Line along a crest, where ground falls away on both sides.",
         how=("Shoot along the crest of the ridge, with a level at each point. The carets point up to "
              "say it is a high line."),
         example="GBR1 ST, GBR1, GBR1 END"),
    code("GBG", "Gully Line", "line", "terrain/breaklines/ridge-gully", "breakline", "0.18",
         style="Gully Line", surface=True,
         label="Gully line",
         meaning="Line along the bottom of a gully.",
         how=("Shoot along the lowest line of the gully, with a level at each point. The inverted carets "
              "point down to say it is a low line."),
         example="GBG1 ST, GBG1, GBG1 END"),
    code("GBH", "Hard Breakline", "line", "terrain/breaklines/general", "breakline", "0.25",
         style="continuous", surface=True,
         label="Hard breakline",
         meaning="Sharp change of grade that a surface must not smooth over.",
         how=("Use where grade changes abruptly and a surface must keep the edge, such as the back of a "
              "kerb or the lip of a slab. Take a point at every change of direction and level each one."),
         example="GBH1 ST, GBH1, GBH1 END"),
    code("GBS", "Soft Breakline", "line", "terrain/breaklines/general", "breakline", "0.13",
         style="Soft Breakline", surface=True,
         label="Soft breakline",
         meaning="Gentle change of grade that a surface may soften.",
         how=("Use where grade changes gradually and the surface may round it off. Level each point. "
              "When in doubt about hard or soft, choose hard."),
         example="GBS1 ST, GBS1, GBS1 END"),
    code("GBX", "Surface Exclusion Boundary", "line", "terrain/breaklines/exclusion", "breakline", "0.18",
         style="Exclusion Boundary",
         label="Surface exclusion boundary",
         meaning="Area that a surface must leave out, such as a building or a lake.",
         how=("Shoot the outline of the area where no ground model should exist and close with CL. The "
              "surface builder leaves out everything inside it. It does not take part in the surface "
              "itself."),
         example="GBX1 ST, GBX1, GBX1, GBX1 CL"),

# ---- GC Terrain and Breaklines / Contours --------------------------------------------------------------------
    code("GCI", "Index Contour", "line", "terrain/contours/index", "contour", "0.35",
         style="Index Contour",
         attrs=["Level (m)"],
         label="Index contour",
         meaning="Every fifth contour, drawn heavier and marked with a bead.",
         how=("Code a contour you were given, such as supplied mapping, and set Level (m) to its height. "
              "Contours made from your own surface are drawn from the model and need no code."),
         example="GCI1 ST, GCI1, GCI1 END"),
    code("GCM", "Intermediate Contour", "line", "terrain/contours/intermediate", "contour", "0.18",
         style="continuous",
         attrs=["Level (m)"],
         label="Intermediate contour",
         meaning="Ordinary contour between the index contours.",
         how="Code a contour you were given and set Level (m) to its height. One string per contour.",
         example="GCM1 ST, GCM1, GCM1 END"),
    code("GCD", "Depression Contour", "line", "terrain/contours/depression", "contour", "0.18",
         style="Depression Contour",
         attrs=["Level (m)"],
         label="Depression contour",
         meaning="Contour round a hollow; the ticks point downhill.",
         how=("Follow the contour with the hollow on your LEFT, so the ticks fall downhill into it. Set "
              "Level (m) to its height."),
         example="GCD1 ST, GCD1, GCD1, GCD1 CL"),

# ---- GW Terrain and Breaklines / Water Edges -----------------------------------------------------------------
    code("GWE", "Water Edge", "line", "terrain/water/edge", "waterway", "0.25",
         style="Water Edge", surface=True,
         attrs=["Water Level (m)"],
         label="Water edge",
         meaning="Edge of water at the time of survey.",
         how=("Shoot where the water meets the bank and set Water Level (m) when you have a level for "
              "it. Water moves: date the shot in the job notes."),
         example="GWE1 ST, GWE1, GWE1 END"),
    code("GWH", "High Water Mark", "line", "terrain/water/edge", "waterway", "0.25",
         style="Water Edge", surface=True,
         label="High water mark",
         meaning="Line of the highest recent water.",
         how=("Shoot the wrack line or stain left by the highest recent water, which is usually above "
              "the present edge."),
         example="GWH1 ST, GWH1, GWH1 END"),

# ---- GR Terrain and Breaklines / Rock and Cliff --------------------------------------------------------------
    code("GRO", "Rock Outcrop Edge", "line", "terrain/rock/edge", "contour", "0.18",
         style="Rock Outcrop Edge",
         label="Rock outcrop edge",
         meaning="Edge of exposed rock where it meets soil, grass or water.",
         how=("Shoot the edge of the outcrop where the rock meets soil or grass, at each change of "
              "direction. Close with CL for an isolated outcrop."),
         example="GRO1 ST, GRO1, GRO1, GRO1 CL"),
    code("GRC", "Cliff Edge", "line", "terrain/rock/edge", "contour", "0.25",
         style="Cliff Edge", surface=True,
         label="Cliff edge",
         meaning="Top edge of a cliff; the ticks point over the edge.",
         how=("Shoot along the top edge of the cliff with the drop on your LEFT, so the ticks fall over "
              "the edge. Stand well back and shoot it reflectorless."),
         example="GRC1 ST, GRC1, GRC1 END"),
    code("GRB", "Boulder", "point", "terrain/rock/boulder", "contour", "0.18",
         symbol="Boulder",
         label="Boulder",
         meaning="Boulder too large to move.",
         how=("Shoot the middle of the top of the boulder. A boulder that is a landmark is recorded as a "
              "heritage item as well (XHT).")),

# ---- MC Survey Control and Annotation / Control Marks --------------------------------------------------------
    code("MCT", "Trigonometric Station", "point", "control/marks/control", "control", "0.25",
         symbol="Trigonometric Station",
         attrs=["Mark Number", "Condition", "Order"],
         label="Trigonometric station",
         meaning="Trigonometric station of a national or state network.",
         how=("Shoot the centre of the mark in its cap. Mark Number is the register number; Order is its "
              "class in the network. Do not disturb it.")),
    code("MCC", "Control Mark", "point", "control/marks/control", "control", "0.25",
         symbol="Control Mark",
         attrs=["Mark Number", "Condition", "Order"],
         label="Control mark",
         meaning="Ground mark whose coordinates the project holds.",
         how=("Shoot the centre of the mark. Mark Number is the project's number for it and Order is its "
              "class. Say in Condition if it was reoccupied.")),
    code("MCB", "Level Benchmark", "point", "control/marks/level", "control", "0.25",
         symbol="Level Benchmark",
         attrs=["Mark Number", "Condition", "Level (m)"],
         label="Level benchmark",
         meaning="Mark with a published level.",
         how=("Shoot the point the level is quoted to, which is usually the top of the mark. Level (m) "
              "is the published level. Check your own against it.")),
    code("MCP", "Fixed Survey Mark", "point", "control/marks/control", "control", "0.25",
         symbol="Fixed Survey Mark",
         attrs=["Mark Number", "Condition"],
         label="Fixed survey mark",
         meaning="Mark set to last, such as a pin in concrete.",
         how=("Shoot the centre of the mark. A mark set for the life of the project is coded here; one "
              "set for the job only is MCW.")),
    code("MCG", "GNSS Base", "point", "control/marks/control", "control", "0.25",
         symbol="GNSS Base",
         attrs=["Mark Number", "Condition"],
         label="Gnss base",
         meaning="Mark where a GNSS base receiver was set up.",
         how=("Shoot the point the base antenna's height is measured to, and record Mark Number. The "
              "rays in the symbol suggest satellites.")),
    code("MCW", "Temporary Control", "point", "control/marks/temporary", "control", "0.25",
         symbol="Temporary Control",
         attrs=["Mark Number", "Condition"],
         label="Temporary control",
         meaning="Temporary mark set for this job.",
         how=("Shoot the centre of the mark. Remove it or hand it over at the end of the job, and say "
              "which in Condition.")),
    code("MCI", "Instrument Station", "point", "control/marks/temporary", "control", "0.25",
         symbol="Instrument Station",
         attrs=["Station Name", "Instrument Height (m)"],
         label="Instrument station",
         meaning="Position of the total station for one set-up.",
         how=("Shoot the centre of the instrument's mark. Station Name is the set-up's name and "
              "Instrument Height (m) is to the trunnion axis.")),

# ---- MI Survey Control and Annotation / Investigation Points -------------------------------------------------
    code("MIB", "Borehole", "point", "control/investigation/ground", "control", "0.25",
         symbol="Borehole",
         attrs=["Hole Number", "Depth (m)"],
         label="Borehole",
         meaning="Drilled hole for ground investigation.",
         how=("Shoot the centre of the borehole collar. Hole Number is the geotechnical report's number "
              "and Depth (m) is the final depth drilled.")),
    code("MIT", "Test Pit", "point", "control/investigation/ground", "control", "0.25",
         symbol="Test Pit",
         attrs=["Hole Number", "Depth (m)"],
         label="Test pit",
         meaning="Excavated pit for ground investigation.",
         how=("Shoot the centre of the test pit. Hole Number is the report's number and Depth (m) is the "
              "final depth dug.")),
    code("MIP", "Pothole", "point", "control/investigation/pothole", "control", "0.25",
         symbol="Pothole",
         attrs=["Pothole Number", "Depth to Top (m)", "Service Found"],
         label="Pothole",
         meaning="Hole dug to expose a buried service.",
         how=("Shoot the middle of the pothole, and the top of the service it exposed with that "
              "service's own code. Service Found names it, Depth to Top (m) is surface to the top of the "
              "pipe or cable.")),

# ---- MT Survey Control and Annotation / Text and Notes -------------------------------------------------------
    code("MTL", "Label Text", "text", "control/annotation/text", "note", "0.13",
         look="label",
         label="Label text",
         meaning="Short label for a feature, 2.0 mm high.",
         how=("Shoot the insertion point of the label. Katana stores this look (2.0 mm, left, on the "
              "baseline) but does not yet apply it, so the point shows as a plus and you place the words "
              "with TEXT.")),
    code("MTN", "Note Text", "text", "control/annotation/text", "note", "0.13",
         look="note",
         label="Note text",
         meaning="Note on the plan, 2.5 mm high.",
         how=("Shoot the insertion point of the note. Katana stores this look (2.5 mm, left, on the "
              "baseline) but does not yet apply it, so the point shows as a plus and you place the words "
              "with TEXT.")),
    code("MTH", "Heading Text", "text", "control/annotation/text", "note", "0.13",
         look="heading",
         label="Heading text",
         meaning="Heading, 5.0 mm high and bold.",
         how=("Shoot the insertion point of the heading. Katana stores this look (5.0 mm, bold) but does "
              "not yet apply it, so the point shows as a plus and you place the words with TEXT.")),
    code("MTS", "Spot Level Text", "text", "control/annotation/text", "note", "0.13",
         look="level",
         label="Spot level text",
         meaning="Level written beside a point, 2.0 mm italic.",
         how=("Shoot the point the level belongs beside. Katana stores this look (2.0 mm, italic, left) "
              "but does not yet apply it, so the point shows as a plus and you place the level with "
              "TEXT.")),
    code("MTR", "Road Name Text", "text", "control/annotation/text", "note", "0.13",
         look="road",
         label="Road name text",
         meaning="Road name, 3.0 mm italic and centred.",
         how=("Shoot the middle of the road name's position along the road. Katana stores this look (3.0 "
              "mm, italic, centred) but does not yet apply it, so the point shows as a plus and you "
              "place the name with TEXT.")),
    code("MTC", "Contour Value Text", "text", "control/annotation/text", "note", "0.13",
         look="contour",
         label="Contour value text",
         meaning="Contour level, 1.8 mm italic and centred.",
         how=("Shoot the middle of the contour value's position. Katana stores this look (1.8 mm, "
              "italic, centred) but does not yet apply it, so the point shows as a plus and you place "
              "the value with TEXT.")),

# ---- MM Survey Control and Annotation / Plan Marks -----------------------------------------------------------
    code("MMN", "North Arrow", "point", "control/plan/furniture", "note", "0.13",
         symbol="North Arrow",
         label="North arrow",
         meaning="North arrow placed on the plan.",
         how=("Shoot the point where the arrow should stand, in a clear part of the sheet. It is a "
              "symbol of the plan, not a feature of the site, so it carries no level.")),
    code("MML", "Leader Line", "both", "control/plan/furniture", "note", "0.13",
         style="continuous", symbol="Leader Dot",
         label="Leader line",
         meaning="Leader that runs from a note to the feature it names.",
         how=("Shoot the note first, then the feature: two points, a dot at each end. Do not run a "
              "leader through other features."),
         example="MML1 ST, MML1 END"),
    code("MMS", "Limit of Survey", "line", "control/plan/limit", "note", "0.50",
         style="Limit of Survey Line",
         label="Limit of survey",
         meaning="Edge of the area that was surveyed.",
         how=("Shoot the limit of the survey at each corner and close with CL. It tells the reader where "
              "the survey stops and nothing beyond it can be trusted."),
         example="MMS1 ST, MMS1, MMS1, MMS1 CL"),
    code("MMV", "Revision Cloud", "line", "control/plan/furniture", "note", "0.25",
         style="Revision Cloud Edge",
         label="Revision cloud",
         meaning="Cloud drawn round an area that has changed.",
         how="Shoot round the changed area walking CLOCKWISE, so the scallops bulge outward. Close with CL.",
         example="MMV1 ST, MMV1, MMV1, MMV1 CL"),

# ---- XH Miscellaneous / Hazards and Heritage -----------------------------------------------------------------
    code("XHZ", "Hazard Point", "point", "miscellaneous/hazards/hazard", "hazard", "0.25",
         symbol="Hazard Point",
         attrs=["Hazard Type", "Note"],
         label="Hazard point",
         meaning="Point hazard, such as an open hole or exposed wire.",
         how=("Shoot the middle of the hazard. Hazard Type names it and Note says what to do about it. "
              "Tell the client at once if it is dangerous.")),
    code("XHB", "Hazard Boundary", "line", "miscellaneous/hazards/hazard", "hazard", "0.25",
         style="Hazard Boundary",
         attrs=["Hazard Type", "Note"],
         label="Hazard boundary",
         meaning="Outline of a hazardous area.",
         how=("Shoot the outline of the area, such as contaminated ground or a drop, and close with CL. "
              "Hazard Type and Note as for a point hazard."),
         example="XHB1 ST, XHB1, XHB1, XHB1 CL"),
    code("XHT", "Heritage Item", "point", "miscellaneous/hazards/heritage", "note", "0.25",
         symbol="Heritage Item",
         attrs=["Name", "Listing"],
         label="Heritage item",
         meaning="Item of heritage value: a feature protected by a listing or an overlay.",
         how=("Shoot the middle of the item, or its base. Name is the common name and Listing is the "
              "register entry. Do not disturb it.")),
    code("XHH", "Habitat Marker", "point", "miscellaneous/hazards/habitat", "tree", "0.25",
         symbol="Habitat Marker",
         attrs=["Habitat Type"],
         label="Habitat marker",
         meaning="Nest, burrow or other habitat feature.",
         how=("Shoot the middle of the nest, the hollow or the burrow mouth. Habitat Type says which. "
              "Keep clear of it and leave it as you found it.")),

# ---- XG Miscellaneous / General ------------------------------------------------------------------------------
    code("XGP", "Generic Point", "point", "miscellaneous/general/generic", "note", "0.25",
         symbol="Generic Point",
         attrs=["Note"],
         label="Generic point",
         meaning="Point that has no better code.",
         how=("Shoot a point that fits no other code and say what it is in Note. If the same thing "
              "appears more than once, ask for a code for it.")),
    code("XGL", "Generic Line", "line", "miscellaneous/general/generic", "note", "0.25",
         style="continuous",
         attrs=["Note"],
         label="Generic line",
         meaning="Line that has no better code.",
         how=("Shoot a line that fits no other code and say what it is in Note. If the same thing "
              "appears more than once, ask for a code for it."),
         example="XGL1 ST, XGL1, XGL1 END"),
    code("XGQ", "Query Point", "point", "miscellaneous/general/query", "note", "0.25",
         symbol="Query Point",
         attrs=["Note"],
         label="Query point",
         meaning="Point the surveyor is not sure of; check before issue.",
         how=("Shoot a point you cannot name or place with confidence, and say why in Note. Every query "
              "point must be resolved or explained before the plan goes out.")),

# ---- XT Miscellaneous / Temporary Works ----------------------------------------------------------------------
    code("XTF", "Temporary Fence", "line", "miscellaneous/temporary/temporary", "hazard", "0.25",
         style="Temporary Works Line",
         label="Temporary fence",
         meaning="Temporary fence or hoarding.",
         how=("Shoot the line of the temporary fence at each change of direction. It will be gone when "
              "the work is done, so date it in the job notes."),
         example="XTF1 ST, XTF1, XTF1 END"),
    code("XTW", "Work Zone Edge", "line", "miscellaneous/temporary/temporary", "hazard", "0.25",
         style="Temporary Works Line",
         label="Work zone edge",
         meaning="Edge of a work zone: a temporary limit of cones, barriers or tape.",
         how=("Shoot the edge of the work zone at each change of direction and close with CL when it "
              "encloses an area."),
         example="XTW1 ST, XTW1, XTW1, XTW1 CL"),
]


def expand_rules(c):
    """The rules one code expands to, in the fixed order: feature, symbol, text, pipe, attributes, surface.
    Every rule's key is `KEY*`, so a string number (UWM1, UWM01) always matches. This is the contract's
    reference expansion: the assembler copies it, and the self-check below lints what it makes."""
    key = c["key"] + "*"
    rules = []
    feature = {"key": key, "sets": "feature", "layer": c["layer"], "colour": c["colour"]}
    if c["kind"] in ("line", "both"):
        feature["draw"] = "line"
        feature["linestyle"] = c["linestyle"]
    else:
        feature["draw"] = "point"
    feature["weight"] = c["weight"]
    feature["group"] = c["group"]
    feature["comment"] = c["comment"]
    rules.append(feature)
    if c["kind"] in ("point", "both"):
        rule = {"key": key, "sets": "symbol", "symbol": {"name": c["symbol"], "colour": c["colour"]}}
        if c.get("hide"):
            rule["hide"] = True
        rules.append(rule)
    if c["kind"] == "text":
        t = c["text"]
        rules.append({"key": key, "sets": "text", "text": {
            "style": t["style"], "colour": c["colour"], "units": t["units"], "size": t["size"],
            "justifyX": t["justifyX"], "justifyY": t["justifyY"], "italic": t["italic"], "weight": t["weight"]}})
    if c.get("pipe"):
        justify, shape, size1, size2 = c["pipe"]
        pipe = {"justify": justify, "shape": shape, "size1": size1, "active": True}
        if size2:
            pipe["size2"] = size2
        rules.append({"key": key, "sets": "pipe", "pipe": pipe})
    if c.get("attributes"):
        rules.append({"key": key, "sets": "attributes", "attributes": list(c["attributes"])})
    rules.append({"key": key, "sets": "surface", "surface": bool(c["surface"])})
    return rules


# ---- the self-check ------------------------------------------------------------------------------------------

CONTROL_WORDS = ("ST", "END", "CL", "BC", "EC", "JPN", "RECT")
# The words CLAUDE.md section 9 keeps out of new text, assembled from pieces so this file does not hold them.
FORBIDDEN = re.compile("|".join(["1" + "2d", "ex" + "ds", "n" + "sw"]), re.I)
MEMBERS = {"key", "group", "name", "kind", "layer", "colour", "weight", "linestyle", "symbol", "attributes",
           "surface", "hide", "pipe", "text", "comment", "meaning", "how", "example"}
# The members each kind of rule may carry (docs/customisation.md): anything else is "field outside section".
RULE_MEMBERS = {
    "feature": {"key", "sets", "layer", "colour", "draw", "linestyle", "weight", "group", "comment"},
    "symbol": {"key", "sets", "symbol", "hide", "comment"},
    "text": {"key", "sets", "text", "comment"},
    "pipe": {"key", "sets", "pipe", "attributes", "comment"},
    "attributes": {"key", "sets", "attributes", "comment"},
    "surface": {"key", "sets", "surface", "comment"},
}
SYMBOL_MEMBERS = {"name", "colour", "size", "rotation", "offset", "raise"}
TEXT_MEMBERS = {"style", "colour", "units", "size", "justifyX", "justifyY", "offset", "raise", "angle", "slant",
                "widthFactor", "underline", "strikeout", "italic", "weight"}
PIPE_MEMBERS = {"justify", "shape", "size1", "size2", "active"}
# Words that name a measured quantity: an attribute that has one carries its unit, as `Depth (m)`.
QUANTITY = ("depth", "diameter", "height", "width", "capacity", "level", "voltage", "rating", "size")


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


def check(contract=None):
    """Validate CODES; return (errors, warnings), two lists of sentences. `contract` is the parsed
    contract.json, or None: then the names of colours, linestyles and symbols are checked against the
    sibling modules colours.py, linestyles.py and symbols.py when they exist, and are reported unchecked
    when they do not."""
    errors, warnings = [], []
    err, warn = errors.append, warnings.append

    # Where the names come from.
    colours = _sibling("colours")
    palette = set(colours.COLOURS) if colours else set()
    text_colour = getattr(colours, "TEXT_COLOUR", None)
    if contract:
        palette = {p["name"] for p in contract["palette"]}
        linestyles = {d["name"] for d in contract["linestyles"]}
        symbols = {d["name"] for d in contract["symbols"]}
        source = "contract.json"
    else:
        ls_module, sy_module = _sibling("linestyles"), _sibling("symbols")
        linestyles = {d["name"] for d in ls_module.LINESTYLES} if ls_module else None
        symbols = {d["name"] for d in sy_module.SYMBOLS} if sy_module else None
        source = "the sibling modules"
        if linestyles is None or symbols is None:
            warn("linestyle and symbol names are not checked: no contract given and linestyles.py or symbols.py "
                 "is not beside this file")
    if linestyles is not None:
        linestyles = linestyles | {"continuous"}
    if not palette:
        warn("colour names are not checked: no contract given and colours.py is not beside this file")

    # Every code, on its own.
    seen_keys, seen_comments = {}, {}
    for c in CODES:
        k = c.get("key", "?")
        where = f"{k}"
        if set(c) != MEMBERS:
            err(f"{where}: members differ ({sorted(set(c) ^ MEMBERS)})")
            continue
        if not re.fullmatch(r"[A-Z]{3}", k):
            err(f"{where}: a key is three capital letters")
        if k in CONTROL_WORDS:
            err(f"{where}: a key may not be a control word")
        if k in seen_keys:
            err(f"{where}: key repeated")
        seen_keys[k] = c
        if k[:2] not in _GROUP:
            err(f"{where}: class and subgroup letters {k[:2]} are not in TAXONOMY")
            continue
        if c["group"] != _GROUP[k[:2]]:
            err(f"{where}: group {c['group']!r} should be {_GROUP[k[:2]]!r}")
        layer = c["layer"]
        if not re.fullmatch(r"[a-z]+(-[a-z]+)*(/[a-z]+(-[a-z]+)*){2}", layer):
            err(f"{where}: layer {layer!r} is three lower-case levels, words joined by hyphens")
        elif not layer.startswith(_LAYER[k[:2]] + "/"):
            err(f"{where}: layer {layer!r} should begin {_LAYER[k[:2]]}/")
        kind = c["kind"]
        if kind not in ("line", "point", "both", "text"):
            err(f"{where}: kind {kind!r}")
        if kind in ("line", "both") and not c["linestyle"]:
            err(f"{where}: a {kind} code needs a linestyle")
        if kind in ("point", "text") and c["linestyle"]:
            err(f"{where}: a {kind} code has no linestyle")
        if kind in ("point", "both") and not c["symbol"]:
            err(f"{where}: a {kind} code needs a symbol")
        if kind in ("line", "text") and c["symbol"]:
            err(f"{where}: a {kind} code has no symbol")
        if (kind == "text") != bool(c["text"]):
            err(f"{where}: only a text code carries a text look, and every text code does")
        if c["hide"] not in (None, True) or (c["hide"] and kind not in ("point", "both")):
            err(f"{where}: hide is True on a point code or absent")
        if c["pipe"] is not None:
            if kind != "line" or len(c["pipe"]) != 4:
                err(f"{where}: a pipe belongs to a line code and is (justify, shape, size1, size2)")
            else:
                justify, shape, size1, size2 = c["pipe"]
                if justify not in ("Centre", "Invert") or shape not in ("diameter", "culvert"):
                    err(f"{where}: pipe justify {justify!r} or shape {shape!r} is not one the library uses")
                names = {a["name"] for a in c["attributes"]}
                for size in (size1, size2):
                    if size and (not size.startswith("$") or size[1:] not in names):
                        err(f"{where}: pipe size {size!r} must name one of the code's own attributes with a $")
        if c["weight"] not in {w[0] for w in WEIGHTS}:
            err(f"{where}: weight {c['weight']!r} is not on the scale")
        elif c["weight"] == "0.70":
            warn(f"{where}: 0.70 is reserved in edition 1")
        if not isinstance(c["surface"], bool):
            err(f"{where}: surface is true or false")
        if palette and c["colour"] not in palette:
            err(f"{where}: colour {c['colour']!r} is not in the palette ({source})")
        if kind == "text" and text_colour and c["colour"] != text_colour:
            warn(f"{where}: text codes use {text_colour!r}")
        if linestyles is not None and c["linestyle"] and c["linestyle"] not in linestyles:
            err(f"{where}: linestyle {c['linestyle']!r} is not defined ({source})")
        if symbols is not None and c["symbol"] and c["symbol"] not in symbols:
            err(f"{where}: symbol {c['symbol']!r} is not defined ({source})")
        if c["text"] is not None:
            look = {kk: vv for kk, vv in c["text"].items() if kk not in ("style", "units")}
            if look not in TEXT_LOOKS.values() or c["text"].get("style") != "Standard" or c["text"].get("units") != "paper":
                err(f"{where}: text is a TEXT_LOOKS entry with style Standard and units paper")

        # attributes
        names = []
        for a in c["attributes"]:
            if set(a) != {"type", "name", "value"} or a["type"] not in ("text", "integer"):
                err(f"{where}: attribute {a!r} is {{type text|integer, name, value}}")
                continue
            names.append(a["name"])
            if a["value"] and not a["name"].startswith("utility."):
                err(f"{where}: attribute {a['name']!r} applies a value; only utility.* does, the rest are prompts")
            lower = a["name"].lower()
            if not a["name"].startswith("utility.") and any(w in lower for w in QUANTITY) \
                    and not re.search(r"\([^)]+\)$", a["name"]):
                err(f"{where}: attribute {a['name']!r} names a quantity and must carry its unit, as 'Depth (m)'")
        if len(names) != len(set(names)):
            err(f"{where}: an attribute is listed twice")
        if k[0] == "U":
            service = layer.split("/")[1] if layer.count("/") == 2 else None
            typed = [a["value"] for a in c["attributes"] if a["name"] == "utility.type"]
            if typed != [service]:
                err(f"{where}: a utility code sets utility.type to {service!r}, it sets {typed}")
            disused = [a["value"] for a in c["attributes"] if a["name"] == "utility.status"]
            if disused != (["disused"] if layer.endswith("/disused") else []):
                err(f"{where}: utility.status is 'disused' on a disused code and absent elsewhere")
        elif any(a["name"].startswith("utility.") for a in c["attributes"]):
            err(f"{where}: only utility codes set utility.* attributes")

        # prose
        comment = c["comment"]
        if not comment or len(comment) > 36:
            err(f"{where}: comment must be 1 to 36 characters (it is the legend label): {comment!r}")
        else:
            rest = " ".join(w for w in comment[1:].split() if not (len(w) > 1 and w.isupper()))
            if comment[0] != comment[0].upper() or rest != rest.lower():
                warn(f"{where}: comment {comment!r} is not sentence case")
            if k in comment.upper().split():
                err(f"{where}: the comment must not be the key")
            if comment.lower() in seen_comments:
                warn(f"{where}: comment {comment!r} repeats {seen_comments[comment.lower()]}'s")
            seen_comments[comment.lower()] = k
        meaning = c["meaning"]
        if not meaning or len(meaning) > 100 or "\n" in meaning:
            err(f"{where}: meaning is one line of at most 100 characters ({len(meaning)})")
        elif meaning[0] != meaning[0].upper() or not meaning.endswith("."):
            warn(f"{where}: meaning should be a sentence with a capital and a full stop")
        how = c["how"]
        if not how or "\n" in how:
            err(f"{where}: how is a non-empty single line")
        else:
            if len(how) > 300:
                warn(f"{where}: how is {len(how)} characters; a field-book line should be shorter")
            if not how.endswith("."):
                warn(f"{where}: how should end with a full stop")
        example = c["example"]
        if kind in ("line", "both") and not example:
            warn(f"{where}: a line code should show an example")
        if example:
            fields = [f.strip() for f in example.split(",")]
            for f in fields:
                if not re.fullmatch(re.escape(k) + r"\d*( (ST|END|CL|BC|EC|RECT|JPN \d+))?", f):
                    err(f"{where}: example field {f!r} is not {k}, a string number and one control word")
            if kind in ("line", "both") and len(fields) < 2:
                err(f"{where}: a line example needs at least two points")
        for s in _strings(c):
            if FORBIDDEN.search(s):
                err(f"{where}: forbidden word in {s!r}")
            if not s.isascii():
                warn(f"{where}: non-ASCII text in {s!r}")

    for pair, group in _GROUP.items():
        if not any(c["key"][:2] == pair for c in CODES):
            warn(f"subgroup {pair} ({group}) has no code")

    # The rules the codes expand to: legal members, no duplicate, no shadowed rule.
    rules = [r for c in CODES for r in expand_rules(c)]
    once = set()
    for r in rules:
        sets = r["sets"]
        if set(r) - RULE_MEMBERS[sets]:
            err(f"{r['key']} ({sets}): members outside the section: {sorted(set(r) - RULE_MEMBERS[sets])}")
        if (r["key"], sets) in once:
            err(f"{r['key']} ({sets}): rule repeated")
        once.add((r["key"], sets))
        if sets == "symbol" and set(r["symbol"]) - SYMBOL_MEMBERS:
            err(f"{r['key']}: symbol members {sorted(set(r['symbol']) - SYMBOL_MEMBERS)}")
        if sets == "text" and set(r["text"]) - TEXT_MEMBERS:
            err(f"{r['key']}: text members {sorted(set(r['text']) - TEXT_MEMBERS)}")
        if sets == "pipe" and set(r["pipe"]) - PIPE_MEMBERS:
            err(f"{r['key']}: pipe members {sorted(set(r['pipe']) - PIPE_MEMBERS)}")
        if sets in ("feature",) and not isinstance(r["weight"], str):
            err(f"{r['key']}: weight must be text")
    keys = sorted({r["key"] for r in rules})
    for a in keys:
        for b in keys:
            if a != b and b.startswith(a[:-1]):
                err(f"{a} shadows {b}")
    if any(r["key"] == "*" for r in rules):
        err("a bare * rule is not allowed")

    if contract:
        _compare(contract, errors, warnings)
    return errors, warnings


def _compare(contract, errors, warnings):
    """The frozen fields of every code against contract.json, and the census."""
    want = {c["key"]: c for c in contract["codes"]}
    have = {c["key"]: c for c in CODES}
    for k in want:
        if k not in have:
            errors.append(f"{k}: in the contract and not in CODES")
    for k in have:
        if k not in want:
            errors.append(f"{k}: in CODES and not in the contract")
    for k, c in have.items():
        w = want.get(k)
        if not w:
            continue
        for f in ("key", "group", "name", "kind", "layer", "colour", "weight", "linestyle", "symbol", "attributes",
                  "surface", "hide", "pipe", "text"):
            if c[f] != w[f]:
                errors.append(f"{k}: {f} is frozen by the contract ({c[f]!r} != {w[f]!r})")
    if TEXT_LOOKS != contract["textLooks"]:
        errors.append("TEXT_LOOKS differs from the contract")
    census = contract["census"]
    rules = [r for c in CODES for r in expand_rules(c)]
    if len(CODES) != census["codes"] or len(rules) != census["rules"]:
        errors.append(f"{len(CODES)} codes and {len(rules)} rules; the contract says {census['codes']} and {census['rules']}")
    by_class = {}
    for c in CODES:
        by_class[c["group"].split("/")[0]] = by_class.get(c["group"].split("/")[0], 0) + 1
    if by_class != census["byClass"]:
        errors.append(f"codes by class {by_class} differ from the contract {census['byClass']}")
    layers = {c["layer"] for c in CODES}
    if layers != set(contract["layers"]):
        errors.append(f"layers differ from the contract: {sorted(layers ^ set(contract['layers']))}")
    tax = [(t["letter"], t["top"], t["layer"], [(s["letter"], s["name"], s["layer"]) for s in t["subgroups"]])
           for t in contract["taxonomy"]]
    mine = [(t[0], t[1], t[2], [(s[0], s[1], s[2]) for s in t[4]]) for t in TAXONOMY]
    if tax != mine:
        errors.append("TAXONOMY differs from the contract")
    if [(w[0], w[1]) for w in WEIGHTS] != [(w["mm"], w["role"]) for w in contract["weights"]]:
        errors.append("WEIGHTS differ from the contract")


def summary():
    lines = []
    for top in TAXONOMY:
        in_class = [c for c in CODES if c["key"][0] == top[0]]
        kinds = {kind: sum(1 for c in in_class if c["kind"] == kind) for kind in ("line", "point", "both", "text")}
        shown = ", ".join(f"{n} {kind}" for kind, n in kinds.items() if n)
        lines.append(f"{top[0]}  {top[1]:<32}{len(in_class):>4} codes  ({shown})")
    lines.append(f"   {'all':<32}{len(CODES):>4} codes, {sum(len(expand_rules(c)) for c in CODES)} rules")
    return "\n".join(lines)


if __name__ == "__main__":
    import json
    if "--grammar" in sys.argv:
        print(GRAMMAR)
    if "--summary" in sys.argv:
        print(summary())
    if "--check" in sys.argv:
        loaded = None
        if "--contract" in sys.argv:
            with open(sys.argv[sys.argv.index("--contract") + 1], encoding="utf-8") as handle:
                loaded = json.load(handle)
        problems, advice = check(loaded)
        for sentence in advice[:80]:
            print("warning:", sentence)
        for sentence in problems[:200]:
            print("ERROR:", sentence)
        print(f"codes: {len(CODES)} codes, {sum(len(expand_rules(c)) for c in CODES)} rules, "
              f"{len(problems)} errors, {len(advice)} warnings")
        sys.exit(1 if problems else 0)
