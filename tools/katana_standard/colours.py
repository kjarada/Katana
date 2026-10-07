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
  (the WCAG floor for graphical objects) against BOTH, and no two colours sit closer than 0.05 in OKLab, which
  is about where two thin lines of different colour stop being told apart.
- The six buried-service colours follow the widely published convention for marking buried utilities: water
  blue, sewer green, gas yellow, electricity red, communications orange, recycled water purple. They are tuned
  so the yellow and the orange still read on white. NOTE: Katana's own `UTILITY DRAW` uses the AS 5488 plan
  colours instead (electricity orange, communications white, sewer cream, stormwater green), so the two
  conventions differ and a drawing that mixes them needs a legend.
- Everything above ground is keyed by what it is made of or what it is for, so a plan reads by colour before
  it reads by symbol: boundaries violet, kerbs rose-brown, vegetation green, walls brick, fences timber.

The palette is frozen by the Katana Standard contract; a change goes through its owner, not into this file.
"""
import math
import re
import sys

GROUND = "#1E2329"   # the plan view's ground
PAPER = "#FFFFFF"    # white paper

COLOURS = {
    "katana water": "#3986E4",
    "katana sewer": "#1DA758",
    "katana gas": "#A68E12",
    "katana electricity": "#E24942",
    "katana telecom": "#CA8407",
    "katana recycled": "#9565C7",
    "katana fire": "#DC6995",
    "katana fuel": "#995F2E",
    "katana its": "#0B9BA8",
    "katana unknown": "#C344AE",
    "katana stormwater": "#44A080",
    "katana waterway": "#6399BE",
    "katana pavement": "#789295",
    "katana kerb": "#A27B75",
    "katana boundary": "#796EDA",
    "katana contour": "#AB704C",
    "katana breakline": "#C37B83",
    "katana ground": "#7C9976",
    "katana tree": "#60812B",
    "katana planting": "#859E3A",
    "katana building": "#637482",
    "katana fence": "#A18863",
    "katana wall": "#B35A47",
    "katana rail": "#816FA3",
    "katana street": "#A2759D",
    "katana control": "#0999C9",
    "katana note": "#9A8EA4",
    "katana hazard": "#DD5C00",
}

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
        for ground, label in ((GROUND, "the plan ground"), (PAPER, "white paper")):
            ratio = contrast(value, ground)
            if ratio < 3.0:
                found.append(f"{name!r} has contrast {ratio:.2f} against {label}; the floor is 3.0")
        if not ROLES.get(name):
            found.append(f"{name!r} has no role")
    names = sorted(COLOURS)
    for i, a in enumerate(names):
        for b in names[i + 1:]:
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
        lines.append(f"{name:<20}{value:<9}{contrast(value, GROUND):>7.2f}{contrast(value, PAPER):>7.2f}  {ROLES[name]}")
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
