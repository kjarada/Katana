"""The Katana Standard palette: 28 named colours, each with the job it does.

    python tools/katana_standard/colours.py --check     validate the palette, exit 1 on a problem
    python tools/katana_standard/colours.py --table     print name, hex, role and both contrast ratios

`COLOURS` maps a colour name to `#RRGGBB`; it becomes the `colours` table of the customisation file, and
every rule of `codes.py` names its colour from it. `ROLES` says what each colour is for, and `FAMILIES`
groups the colours as the palette preview and the catalogue show them.

Why these twenty-eight, and why these values:

- Names are lower case with the prefix `katana ` and then the role, so a colour in a rule reads as the thing
  it colours (`katana water`) and none can fold onto one of the 27 standard names Katana already knows.
- The plan view's ground is `#1E2329` (a blue-grey that is almost black) and paper is white, and the same
  library is plotted on paper and read on the screen. So each colour keeps a contrast ratio of at least 3:1
  (the WCAG floor for graphical objects) against BOTH (gas and sewer excepted on paper, below), and no two
  colours sit closer than 0.05 in OKLab, which is about where two thin lines of different colour stop being
  told apart.
- The buried-service colours are AS 5488's. The standard (Appendix B, the primary code and line colour table)
  names a colour for each service: water blue, electricity orange, communications white, gas yellow, sewer
  cream, stormwater green, recycled water purple, fire service red, unidentified services pink, and petroleum
  brown (`AS5488_NAMES`; `docs/subsurface_utilities.md` has the sources). The standard names the colours and
  gives no sRGB for them, so each value here is Katana's rendering of the name, and it is the same value
  `UTILITY DRAW` draws in (`utilityTypeColour`, `include/katana/cad/utilities/utility_drawing.hpp`);
  `tests/cad/customisation/test_katana_standard.cpp` holds the two equal. Traffic systems have no AS 5488
  colour and stay Katana's choice. The US convention (electricity red, communications orange, sewer green)
  is not Australian and is not used.
- Yellow and cream are the exception to the 3:1 floor on paper. They cannot reach 3:1 on white and still be
  yellow and cream, and the colour is the standard's, so they print light (`PRINTS_LIGHT`) and keep the floor
  against the plan ground. Communications is white, which the plot prints black (`printed`, the rule of
  `cad::paperColour`), so it is held to the floor as printed.
- Everything above ground is keyed by what it is made of or what it is for, so a plan reads by colour before
  it reads by symbol: boundaries violet, kerbs taupe, vegetation green, walls brick, fences tan.
- Lightness is not free. A colour needs 3:1 against the dark ground (relative luminance of at least about
  0.15) and against white (at most 0.30), which leaves OKLab lightness between about 0.54 and 0.67, so
  only hue and chroma tell the classes apart. The warm classes (kerb, wall, contour, fuel, fence, breakline)
  are therefore placed by a search, not by eye: each sits at least 0.06 from every other colour and from
  the other five (`WARM`, checked below), and the classes drawn with the thinnest pens (`THIN`: tree, planting,
  ground, building, wall, telecom) keep at least 3.25:1 on both grounds so a 0.13 mm line stays visible.
  Hazard is the one class that moved for the services: AS 5488's electricity orange left the old hazard
  orange 0.043 away, under the 0.05 floor, so hazard is a red-orange 0.077 from electricity.
- What colour cannot do is carry a class alone: for a reader who confuses red with green or blue with purple
  some pairs stay close whatever the values (OKLab distance under simulated colour blindness is as low as
  0.02 for a few warm pairs). So no class is identified by colour alone: utility pipes carry a letter in
  their linestyle, and every line class has a pattern of its own.

The palette is frozen by the Katana Standard contract; a change goes through its owner, not into this file.
"""
import math
import re
import sys

GROUND = "#1E2329"   # the plan view's ground
PAPER = "#FFFFFF"    # white paper

COLOURS = {
    "katana water": "#2F80ED",
    "katana sewer": "#F0E2B0",
    "katana gas": "#FFD500",
    "katana electricity": "#E07000",
    "katana telecom": "#FFFFFF",
    "katana recycled": "#9B59D0",
    "katana fire": "#E53935",
    "katana fuel": "#A36215",
    "katana its": "#0B9BA8",
    "katana unknown": "#F2559B",
    "katana stormwater": "#00A800",
    "katana waterway": "#6399BE",
    "katana pavement": "#789295",
    "katana kerb": "#907368",
    "katana boundary": "#796EDA",
    "katana contour": "#B9704F",
    "katana breakline": "#BE7D83",
    "katana ground": "#688F68",
    "katana tree": "#5C8627",
    "katana planting": "#779624",
    "katana building": "#627B8C",
    "katana fence": "#A18C65",
    "katana wall": "#BA5556",
    "katana rail": "#816FA3",
    "katana street": "#A2759D",
    "katana control": "#0999C9",
    "katana note": "#9A8EA4",
    "katana hazard": "#FF5A37",
}

# What AS 5488 calls the colour of each buried-service class: the name, never an sRGB, because the standard gives
# a name and no value, so each value of COLOURS above is Katana's rendering of the name. None where AS 5488 names
# no colour and the class is Katana's choice. Petroleum's brown is in three published guides and not in a fourth's
# chart (docs/subsurface_utilities.md, "Colours"), which is why `katana fuel` is brown and says so there.
AS5488_NAMES = {
    "katana water": "blue",
    "katana electricity": "orange",
    "katana telecom": "white",
    "katana gas": "yellow",
    "katana recycled": "purple",
    "katana fire": "red",
    "katana sewer": "cream",
    "katana stormwater": "green",
    "katana fuel": "brown",
    "katana unknown": "pink",
    "katana its": None,
}

# The AS 5488 colours that cannot reach the 3:1 floor against white paper and stay the colour the standard names.
# They print light, and hold the floor against the plan ground only. Nothing else is excused: a colour that
# comes to need it is added here on purpose, with the reason, and `problems()` says so when one on this list
# no longer needs it.
PRINTS_LIGHT = ("katana gas", "katana sewer")

# The channel at or above which a pen prints black, all three of them: cad::kNearWhiteChannel
# (include/katana/cad/plot.hpp), the plot's rule for white and near-white pens (PlotSettings::whiteToBlack).
NEAR_WHITE_CHANNEL = 230

ROLES = {
    "katana water": "potable water",
    "katana sewer": "sewer",
    "katana gas": "gas",
    "katana electricity": "electricity",
    "katana telecom": "communications",
    "katana recycled": "recycled water",
    "katana fire": "fire service",
    "katana fuel": "fuel and petroleum",
    "katana its": "traffic systems",
    "katana unknown": "unidentified service",
    "katana stormwater": "stormwater pipes and pits",
    "katana waterway": "creeks, open drains and water edges",
    "katana pavement": "road edges, markings and paths",
    "katana kerb": "kerbs",
    "katana boundary": "lot boundaries, easements and boundary marks",
    "katana contour": "contours, rock and cliff",
    "katana breakline": "breaklines",
    "katana ground": "ground shots and levels",
    "katana tree": "trees",
    "katana planting": "shrubs, hedges and ground cover",
    "katana building": "buildings and structures",
    "katana fence": "fences and gates",
    "katana wall": "walls",
    "katana rail": "track, overhead wire and platforms",
    "katana street": "street furniture and signs",
    "katana control": "survey control and investigation points",
    "katana note": "text and plan marks",
    "katana hazard": "hazards",
}

# The warm classes that were re-spaced by search (see the notes above), and the classes drawn with the
# thinnest pens. `problems()` holds each to the floor the notes give.
WARM = ("katana kerb", "katana wall", "katana contour", "katana fuel", "katana fence", "katana breakline")
WARM_FLOOR = 0.06
THIN = ("katana tree", "katana planting", "katana ground", "katana building", "katana wall", "katana telecom")
THIN_FLOOR = 3.25

# The colour every text code is drawn in (the MT codes), so notes never compete with the features they label.
TEXT_COLOUR = "katana note"

# The palette as the preview and the catalogue lay it out: (heading, [colour names]).
FAMILIES = [
    ("Buried services", ["katana water", "katana sewer", "katana gas", "katana electricity", "katana telecom",
                         "katana recycled", "katana fire", "katana fuel", "katana its", "katana unknown"]),
    ("Water and drainage", ["katana stormwater", "katana waterway"]),
    ("Roads, street and rail", ["katana pavement", "katana kerb", "katana street", "katana rail"]),
    ("Land, ground and boundaries", ["katana boundary", "katana contour", "katana breakline", "katana ground"]),
    ("Vegetation", ["katana tree", "katana planting"]),
    ("Built form", ["katana building", "katana fence", "katana wall"]),
    ("Control, notes and hazards", ["katana control", "katana note", "katana hazard"]),
]

# The 27 standard colour names Katana resolves before it looks at a file's table (docs/customisation.md).
STANDARD_NAMES = (
    "red green blue yellow cyan magenta white black grey orange brown purple pink violet "
    "dark_red dark_green dark_blue dark_cyan dark_magenta dark_orange dark_grey "
    "light_grey light_blue light_green light_cyan light_yellow light_pink"
).split()


def rgb(text):
    """(r, g, b) of '#RRGGBB'."""
    return tuple(int(text[i:i + 2], 16) for i in (1, 3, 5))


def _linear(channel):
    c = channel / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def luminance(text):
    """WCAG relative luminance of '#RRGGBB'."""
    r, g, b = (_linear(v) for v in rgb(text))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    """WCAG contrast ratio of two '#RRGGBB' colours, 1 to 21."""
    la, lb = luminance(a), luminance(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def printed(text):
    """The colour a pen of '#RRGGBB' prints in on white paper: black when all three channels are at least
    NEAR_WHITE_CHANNEL (the plot's white rule), else as it is."""
    return "#000000" if min(rgb(text)) >= NEAR_WHITE_CHANNEL else text


def paper_contrast(text):
    """Contrast of a pen as it prints against white paper: `contrast(printed(text), PAPER)`."""
    return contrast(printed(text), PAPER)


def oklab(text):
    """(L, a, b) of '#RRGGBB' in OKLab (Bjorn Ottosson's matrices)."""
    r, g, b = (_linear(v) for v in rgb(text))
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l, m, s = (math.copysign(abs(v) ** (1.0 / 3.0), v) for v in (l, m, s))
    return (0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s)


def distance(a, b):
    """Euclidean distance of two '#RRGGBB' colours in OKLab."""
    return math.dist(oklab(a), oklab(b))


def fold(name):
    """How Katana compares colour names: lower case, '_' and '-' as blank, 'gray' as 'grey'."""
    return name.lower().replace("_", " ").replace("-", " ").replace("gray", "grey")


def problems():
    """Every way this palette breaks its own rules, as a list of sentences (empty when sound)."""
    found = []
    if set(COLOURS) != set(ROLES):
        found.append("COLOURS and ROLES name different colours")
    listed = [name for _, names in FAMILIES for name in names]
    if sorted(listed) != sorted(COLOURS):
        found.append("FAMILIES must list every colour exactly once")
    if TEXT_COLOUR not in COLOURS:
        found.append(f"TEXT_COLOUR {TEXT_COLOUR!r} is not in the palette")
    standard = {fold(n) for n in STANDARD_NAMES}
    folded = {}
    for name, value in COLOURS.items():
        if not re.fullmatch(r"katana [a-z]+", name):
            found.append(f"{name!r}: a name is 'katana ' and one lower-case word")
        if fold(name) in standard:
            found.append(f"{name!r} folds to a standard colour name")
        if fold(name) in folded:
            found.append(f"{name!r} and {folded[fold(name)]!r} fold to one name")
        folded[fold(name)] = name
        if not re.fullmatch(r"#[0-9A-F]{6}", value):
            found.append(f"{name!r}: {value!r} is not '#RRGGBB' in capitals")
            continue
        for ground, label, ratio in ((GROUND, "the plan ground", contrast(value, GROUND)),
                                     (PAPER, "white paper", paper_contrast(value))):
            if ratio < 3.0 and not (ground == PAPER and name in PRINTS_LIGHT):
                found.append(f"{name!r} has contrast {ratio:.2f} against {label} as it prints; the floor is 3.0")
        if not ROLES.get(name):
            found.append(f"{name!r} has no role")
    if not set(AS5488_NAMES) <= set(COLOURS):
        found.append("AS5488_NAMES names a colour the palette does not have")
    for name in PRINTS_LIGHT:
        if AS5488_NAMES.get(name) is None:
            found.append(f"{name!r} is excused from the paper floor and is not an AS 5488 colour; only those are")
        elif name in COLOURS and paper_contrast(COLOURS[name]) >= 3.0:
            found.append(f"{name!r} reads at {paper_contrast(COLOURS[name]):.2f}:1 on paper and no longer needs "
                         "to be in PRINTS_LIGHT")
    for name in THIN:
        for ground, label, ratio in ((GROUND, "the plan ground", contrast(COLOURS[name], GROUND)),
                                     (PAPER, "white paper", paper_contrast(COLOURS[name]))):
            if ratio < THIN_FLOOR:
                found.append(f"{name!r} is a thin-pen class and has contrast {ratio:.2f} against {label} as it prints; its floor is {THIN_FLOOR}")
    names = sorted(COLOURS)
    for i, a in enumerate(names):
        for b in names[i + 1:]:
            if (a in WARM or b in WARM) and COLOURS[a] != COLOURS[b] and distance(COLOURS[a], COLOURS[b]) < WARM_FLOOR:
                found.append(f"{a!r} and {b!r}: a warm class is only {distance(COLOURS[a], COLOURS[b]):.3f} from the other; its floor is {WARM_FLOOR}")
            if COLOURS[a] == COLOURS[b]:
                found.append(f"{a!r} and {b!r} are the same colour")
            elif distance(COLOURS[a], COLOURS[b]) < 0.05:
                found.append(f"{a!r} and {b!r} are only {distance(COLOURS[a], COLOURS[b]):.3f} apart in OKLab; the floor is 0.05")
    return found


def closest_pairs(count=6):
    """The `count` closest pairs of colours as (distance, name, name): what to look at first in a preview."""
    names = sorted(COLOURS)
    pairs = [(distance(COLOURS[a], COLOURS[b]), a, b) for i, a in enumerate(names) for b in names[i + 1:]]
    return sorted(pairs)[:count]


def table():
    lines = [f"{'name':<20}{'hex':<9}{'ground':>7}{'paper':>7}  role"]
    for name, value in COLOURS.items():
        lines.append(f"{name:<20}{value:<9}{contrast(value, GROUND):>7.2f}{paper_contrast(value):>7.2f}  {ROLES[name]}")
    return "\n".join(lines)


if __name__ == "__main__":
    if "--table" in sys.argv:
        print(table())
    if "--check" in sys.argv or "--table" not in sys.argv:
        found = problems()
        for sentence in found:
            print("ERROR:", sentence)
        print(f"colours: {len(COLOURS)} colours, {len(found)} errors; closest pair "
              + "%.3f (%s, %s)" % closest_pairs(1)[0])
        sys.exit(1 if found else 0)
