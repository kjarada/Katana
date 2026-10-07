"""The catalogue of Katana Standard, docs/katana_standard.md, made from the data.

`make_katana_standard.py --docs` calls `catalogue(data, root)` and writes the text. Nothing here is typed twice:
a definition's description, a code's meaning and field-book line, a colour's role and contrast, a count, all
come from the four data modules, so the catalogue cannot disagree with the file it describes, and `--check`
finds the day it does. The prose around the tables is here, in one place, because it is the part that is not
data: how to read the library, how to load it, what Katana applies today and what it only stores, why the
choices were made, and what is not done.

Written for a reader with a survey to draw, then for a maintainer. British spelling.
"""
import re

IMAGES = "images/katana-standard-"
# The test names the document cites; check_docs.py fails the day one is renamed away.
CLI_IMPORT_TEST = "cli.katana_standard_imports_a_field_file_and_reports_the_mistyped_code"
CLI_LINEWORK_TEST = "cli.katana_standard_strings_control_words_into_a_curve_a_rectangle_and_a_closed_line"


def _showcase_keys():
    """The codes the showcase scene uses (showcase.py), by key."""
    return _sibling("showcase").commands()[1]


def esc(text):
    """Text for a table cell: a pipe would end the cell."""
    return text.replace("|", "\\|").replace("\n", " ")


def table(header, rows):
    out = ["| " + " | ".join(header) + " |", "|" + "|".join("---" for _ in header) + "|"]
    out += ["| " + " | ".join(esc(str(c)) for c in row) + " |" for row in rows]
    return "\n".join(out)


def plural(n, one, many=None):
    return "%d %s" % (n, one if n == 1 else (many or one + "s"))


def numbered(text):
    """The items of a numbered paragraph block ("1. text, wrapped over indented lines") as a list of strings."""
    items = []
    for line in text.splitlines():
        match = re.match(r"^(\d+)\. (.*)$", line)
        if match:
            items.append(match.group(2).strip())
        elif line.startswith("   ") and items:
            items[-1] += " " + line.strip()
    return items


def house_style(doc, marker):
    """The `term. text` paragraphs after `marker` in a module docstring as (term, text) pairs."""
    body = doc.split(marker, 1)[1]
    pairs = []
    for line in body.splitlines():
        match = re.match(r"^([A-Z][a-z]+)\.\s+(.*)$", line)
        if match:
            pairs.append([match.group(1), match.group(2).strip()])
        elif line.startswith("  ") and pairs:
            pairs[-1][1] += " " + line.strip()
    return [tuple(p) for p in pairs]


def bullets(doc, marker, end):
    """The `* text` items between two markers of a module docstring, each joined into one line."""
    body = doc.split(marker, 1)[1].split(end, 1)[0]
    items = []
    for line in body.splitlines():
        if line.startswith("* "):
            items.append(line[2:].strip())
        elif line.startswith("  ") and items:
            items[-1] += " " + line.strip()
    return items


def catalogue(data, root):
    import os
    colours = data.colours
    codes_module = data.codes_module
    sym_module = _sibling("symbols")
    lin_module = _sibling("linestyles")
    taxonomy = codes_module.TAXONOMY
    codes = data.codes
    by_style = {}
    for c in codes:
        for field in ("linestyle", "symbol"):
            name = c.get(field)
            if name and name != "continuous":
                by_style.setdefault(name, []).append(c["key"])
    group_order = []
    for top in taxonomy:
        for sub in top[4]:
            group_order.append(top[1] + "/" + sub[1])
            if top[0] == "U" and sub[0] == "X":
                group_order.append("Utilities/All Services")
    n_ls, n_sy, n_codes, n_rules = len(data.linestyles), len(data.symbols), len(codes), len(data.rules)
    n_lines = sum(1 for c in codes if c["linestyle"])
    n_plain = sum(1 for c in codes if c["linestyle"] == "continuous")
    style_names = {d["name"] for d in data.linestyles}
    n_shared = sum(1 for name, keys in by_style.items() if len(keys) > 1 and name in style_names)
    max_shared = max(len(keys) for name, keys in by_style.items() if name in style_names)
    layers = sorted({c["layer"] for c in codes})
    out = []
    w = out.append

    w("# Katana Standard")
    w("")
    w("Katana Standard is a survey feature library written for Katana: one Katana customisation file, "
      "`resources/customisation/katana-standard.customisation.json`, with %d linestyles, %d symbols and %d survey "
      "codes (%d rules) in %d classes, and a palette of %d colours. It is original work. Every name, code, colour "
      "and stroke was designed for it from a written specification and none was taken from another library "
      "(\"Originality\", below). This document is its catalogue: how to load it, how to read a code, every "
      "definition and every code, and what Katana does and does not yet do with what the file says."
      % (n_ls, n_sy, n_codes, n_rules, data.classes, len(colours.COLOURS)))
    w("")
    w("Everything below that is a table, a count or a description is generated from the data by "
      "`tools/katana_standard/make_katana_standard.py --docs`, so it cannot disagree with the file; the prose "
      "around it is written by hand in `tools/katana_standard/catalogue.py`. \"How it is made and held\" says what "
      "keeps the two true.")
    w("")
    w("![An invented street corner drawn entirely by Katana Standard, in Katana's plan view](" + IMAGES + "showcase.png)")
    w("")
    w("*An invented street corner, drawn in Katana's plan view with nothing but the library: %d of its %d codes "
      "are in it, kerbs and lots, houses, fences, trees, the services under and over the road, a stretch of rail "
      "on a bridge, contours and a creek, control marks and the labels. Every mark is the size it is on paper "
      "and sits where its code put it (`tools/katana_standard/showcase.py`). Katana does not yet apply a code's "
      "pen weight, so every line is drawn at one width here and the weight ladder of \"Pen weights\" is not "
      "shown.*" % (len(_showcase_keys()), n_codes))
    w("")

    w("**Contents.** [At a glance](#at-a-glance) · [Using it](#using-it) · [Reading a code](#reading-a-code) · "
      "[The palette](#the-palette) · [Linestyles](#linestyles) · [Symbols](#symbols) · [The codes](#the-codes) · "
      "[Alphabetical index](#the-codes-in-alphabetical-order) · [Originality](#originality) · "
      "[Decisions](#decisions-and-what-was-rejected) · [How it is made and held](#how-it-is-made-and-held) · "
      "[Not done](#not-done)")
    w("")
    w("## At a glance")
    w("")
    w(table(["", ""], [
        ["Name", "`Katana Standard`, edition 1, Katana customisation format version 1"],
        ["Definitions", "%d: %s and %s, in %d groups, all in paper units, so a mark keeps its size at any zoom and "
                        "plot scale" % (n_ls + n_sy, plural(n_ls, "linestyle"), plural(n_sy, "symbol"),
                                        len({d["group"] for d in data.linestyles + data.symbols}))],
        ["Survey codes", "%d, each three capital letters, in %d classes and %d subgroups; %d rules" %
         (n_codes, data.classes, len(group_order) - 1, n_rules)],
        ["Layers", "%d, always three levels (`class/subgroup/family`), lower case" % len(layers)],
        ["Palette", "%d named colours, each at least 3:1 against the plan view's ground and against white paper"
         % len(colours.COLOURS)],
        ["Says nothing of", "linework control words and import automation: loading it never resets a colleague's "
                            "spellings or switches"],
        ["Notice", " ".join(data.NOTICE)],
    ]))
    w("")

    # ---- using it ---------------------------------------------------------------------------------------------
    w("## Using it")
    w("")
    w("What is true now. Katana Standard is **not** the built-in: the program still starts with the customisation "
      "compiled into it, and there is no chooser between two compiled-in libraries (\"Not done\", below). It is "
      "loaded like any customisation file (`docs/customisation.md`, \"Using the customisation\").")
    w("")
    w("1. **Load it alone.** `CUSTOMISE REPLACE resources/customisation/katana-standard.customisation.json` in "
      "the window's command line, in `katana_cli` or through `katana_mcp`; or File > Settings > Import with "
      "\"Replace instead of merging\" ticked. Its definitions become the whole library and its rules the whole "
      "of the codes. It brings no linework spellings and no import switches, so the session keeps its own.")
    w("2. **Merging works, with one caution.** `CUSTOMISE <file>`, or Import without the tick, merges by name. "
      "The rules here are exact three-letter prefix rules, and the most specific rule wins field by field, so a "
      "broader prefix rule already in the session can still supply a field these rules leave unsaid (a symbol, "
      "a text). Load it alone when you want its library and nothing else.")
    w("3. **For one run**, name it as the built-in for that run: `KATANA_BUILTIN_CUSTOMISATION=<file>` in the "
      "environment of `katana`, `katana_cli` or `katana_mcp` (`docs/headless.md`, \"The customisation a run "
      "starts with\").")
    w("4. **Compiled in**: `-DKATANA_BUILTIN_CUSTOMISATION=<file>` at configure time embeds a different file "
      "(`docs/building.md`, \"The built-in customisation and the reference folder\"). Only the one-run form of "
      "this has been run with Katana Standard; no program was built with it embedded.")
    w("5. **Then work as usual.** `SURVEY IMPORT <file>` or Survey > Import Survey Points codes and strings the "
      "points by these rules in one undo step; `CODE`, `LINEWORK`, `CODE EXPLAIN <code>`, `CODE LIST` and "
      "`CODE CHECK` read them. `CODE CHECK` reports no problems on the file: %d rules checked." % n_rules)
    w("")
    w("A worked example, run for this document and held by `" + CLI_IMPORT_TEST + "`. The invented field file "
      "`tests/data/katana_standard/street_corner.fld` has twenty entered coordinates: a lot boundary coded "
      "`CBT` in string 1 and closed by opcode 20, a kerb `KKT`, a paling fence `FFP`, a water main `UWM` and a "
      "disused cable `UED`, five single points (a valve `UWV`, a tree `VTB`, a gully pit `KPG`, a control mark "
      "`MCC`) and one mistyped code, `UWQ`. With the library loaded by `CUSTOMISE REPLACE`, `SURVEY IMPORT` "
      "draws 20 points and 5 lines: 19 points are coded onto 9 layers in 9 styles, and the 20th, `UWQ`, is "
      "reported as a code no rule answers, since every code has its own rule and no family rule swallows a "
      "typo. The lot boundary is a closed line of 100 m round, 600 square metres; the kerb and the water main "
      "are 24 m, the fence 10 m and the cable 20 m. Worked out by hand from the codes below before it was run.")
    w("")
    w("![The street corner field file after SURVEY IMPORT, in the plan view](" + IMAGES + "plan.png)")
    w("")
    w("*After the import, from the top: the closed lot boundary with its control mark and its tree; the paling "
      "fence beside it; the kerb with a gully pit; the water main with its valve; the disused cable. The plain "
      "cross inside the lot is `UWQ`, the point no rule answers. Pen weights are not yet applied, so the "
      "boundary is no heavier than the fence. The words beside each feature are laid on the screenshot "
      "afterwards, from the library's own legend labels.*")
    w("")

    # ---- reading a code ---------------------------------------------------------------------------------------
    w("## Reading a code")
    w("")
    w("A code is **three capital letters**: the class, the subgroup in that class, and the item. `UWM` is "
      "Utilities, Water, Main; `KKT` is Kerbs and Drainage, Kerbs, Top edge. In the field a point's code is the "
      "key and a string number, then control words: `UWM1 ST`, `UWM1`, `UWM1 END` is one water main. The "
      "grammar, in full:")
    w("")
    for n, item in enumerate(numbered(codes_module.GRAMMAR), 1):
        item = item.replace("are in TAXONOMY and fix", "are in the table of classes below and fix")
        item = item.replace("each code's `how` says", "each code's field-book line says")
        item = item.replace("Longer text is `meaning` and `how`.", "Longer text is the meaning and the field-book line.")
        w("%d. %s" % (n, item))
    w("")
    w("### The twelve classes")
    w("")
    rows = []
    for top in taxonomy:
        n = sum(1 for c in codes if c["key"][0] == top[0])
        rows.append(["`%s`" % top[0], top[1], n, ", ".join("`%s%s` %s" % (top[0], s[0], s[1]) for s in top[4]),
                     top[3]])
    w(table(["Letter", "Class", "Codes", "Subgroups", "What it holds"], rows))
    w("")
    w("### Pen weights")
    w("")
    w("Every code names one of five weights, in millimetres of pen; a sixth is reserved. Katana stores a rule's "
      "`weight` and does not yet apply it to what it draws, so the scale is the documented intent, for the day "
      "it does.")
    w("")
    w(table(["Weight", "Role", "For"], [[a, b, c] for a, b, c in codes_module.WEIGHTS]))
    w("")
    w("**What Katana applies today.** `CODE` and a survey import apply layer, colour, linestyle, symbol name and "
      "the string attributes, and the plot legend prints the comment. Katana stores, lists and lints, and does "
      "not yet apply: `weight`, `group`, `hide`, `surface`, text rules, pipe rules, and a symbol's rotation, "
      "offset and raise. So the codes `GPG` and `GPR` (ground points shown by their level) are marked `hide` and "
      "still draw their one-millimetre plus, the six text codes draw a plus until text rules are applied, and "
      "the plain continuous lines of one colour on one layer, such as the two carriageway edges `RCE` and `RCI`, "
      "or the kerb top `KKT` and the kerb return `KKR` (one feature, straight and curved), look the same on "
      "the plan until the weight is applied. The rules are written for the day it does.")
    w("")

    # ---- palette ----------------------------------------------------------------------------------------------
    w("## The palette")
    w("")
    w("Twenty-eight colours, each named `katana <role>` so a rule reads as the thing it colours, and none can "
      "fold onto one of the 27 standard names Katana already knows. Each is at least 3:1 against the plan "
      "view's ground (`%s`, a blue-grey that is almost black) and against white paper, since one library is "
      "plotted and read on the screen; and no two are closer than 0.05 in OKLab, about where two thin lines "
      "of different colour stop being told apart. That leaves only a narrow band of lightness, so the six warm "
      "classes (kerb, wall, contour, fuel, fence, breakline) were placed by a search, each at least %.2f from "
      "every other colour, and the six classes drawn with the thinnest pens (tree, planting, ground, building, "
      "wall, communications) keep at least %.2f : 1 on both grounds. The six buried-service colours follow the "
      "widely published convention for marking buried utilities (water blue, sewer green, gas yellow, "
      "electricity red, communications orange, recycled water purple), tuned so the yellow and the orange still "
      "read on white. Colour does not carry a class alone: for a reader who confuses red with green some warm "
      "pairs stay close whatever their values, so every pipe has its letter and every line class its own pattern."
      % (colours.GROUND, colours.WARM_FLOOR, colours.THIN_FLOOR))
    w("")
    w("**The two utility conventions differ.** Katana's own `UTILITY DRAW` has colours of its own "
      "(electricity orange, communications white, sewer cream, stormwater green; `docs/subsurface_utilities.md`: "
      "they are Katana's defaults, and AS 5488 sets none). A drawing that mixes coded survey strings and "
      "`UTILITY DRAW` output has two conventions in it and wants a legend. Aligning one to the other is the "
      "owner's decision (\"Not done\").")
    w("")
    w("![The 28 colours of Katana Standard on the plan ground and on white paper](" + IMAGES + "palette.png)")
    w("")
    for heading, names in colours.FAMILIES:
        w("**%s**" % heading)
        w("")
        w(table(["Colour", "Value", "Used for", "On the ground", "On white"],
                [["`%s`" % n, "`%s`" % colours.COLOURS[n], colours.ROLES[n],
                  "%.2f : 1" % colours.contrast(colours.COLOURS[n], colours.GROUND),
                  "%.2f : 1" % colours.contrast(colours.COLOURS[n], colours.PAPER)] for n in names]))
        w("")

    # ---- linestyles -------------------------------------------------------------------------------------------
    w("## Linestyles")
    w("")
    w("%s, each a repeating cell. A linestyle named by a rule replaces the line, so the gaps are the library's "
      "job; every one is drawn so that it reads at plan scale on screen and on a plotted sheet." % plural(n_ls, "linestyle"))
    w("")
    w("![All %d linestyles of Katana Standard, drawn by Katana](" % n_ls + IMAGES + "linestyles.png)")
    w("")
    w("*One sample of each, drawn by a code that uses it. Pen weights are not yet applied, so every line is the "
      "same width here.*")
    w("")
    w("**How they are drawn.** The rules the data's own check enforces:")
    w("")
    for item in bullets(lin_module.__doc__, "The house rules, which the check enforces:", '"""'):
        w("- " + item)
    w("")
    w("Reading a table: *Period* is the length of one cell in millimetres of paper; *Used by* lists the codes "
      "that name it. A linestyle shared by several codes is the same line in each: the layer and the "
      "colour tell them apart.")
    w("")
    for group in group_order:
        defs = [d for d in data.linestyles if d["group"] == group]
        if not defs:
            continue
        w("### " + group)
        w("")
        w(table(["Linestyle", "Period (mm)", "What it looks like and when to use it", "Used by"],
                [["`%s`" % d["name"], "%g" % d["length"], d["description"],
                  " ".join("`%s`" % k for k in by_style.get(d["name"], []))] for d in defs]))
        w("")

    # ---- symbols ----------------------------------------------------------------------------------------------
    w("## Symbols")
    w("")
    w("%s, every one drawn at its vertex, unrotated, in paper millimetres, with its origin at the centre of "
      "its box unless its description says where else." % plural(n_sy, "symbol"))
    w("")
    w("![All %d symbols of Katana Standard, drawn by Katana](" % n_sy + IMAGES + "symbols.png)")
    w("")
    w("**The house style**, which makes each symbol learnt once:")
    w("")
    for term, text in house_style(sym_module.__doc__, "THE HOUSE STYLE"):
        w("- **%s** %s" % (term + ".", text))
    w("")
    w("Reading a table: *Size* is the symbol's width and height in millimetres of paper, measured from its "
      "strokes.")
    w("")
    for group in group_order:
        defs = [d for d in data.symbols if d["group"] == group]
        if not defs:
            continue
        w("### " + group)
        w("")
        rows = []
        for d in defs:
            x0, y0, x1, y1 = sym_module._bounds(d["strokes"])
            rows.append(["`%s`" % d["name"], "%.1f x %.1f" % (x1 - x0, y1 - y0), d["description"],
                         " ".join("`%s`" % k for k in by_style.get(d["name"], []))])
        w(table(["Symbol", "Size (mm)", "What it looks like and when to use it", "Used by"], rows))
        w("")

    # ---- codes ------------------------------------------------------------------------------------------------
    w("## The codes")
    w("")
    w("%d codes by class and subgroup. *Draws* is the linestyle or symbol, the colour and the pen weight; the "
      "text says what the code is and how to shoot it, and an example is the code field of consecutive "
      "points, separated by commas. Where a code draws the plain continuous line, Katana's own word "
      "`continuous` is its linestyle." % n_codes)
    w("")
    for top in taxonomy:
        w("### Class %s: %s" % (top[0], top[1]))
        w("")
        w(top[3])
        w("")
        for sub in top[4]:
            key2 = top[0] + sub[0]
            rows = [c for c in codes if c["key"][:2] == key2]
            if not rows:
                continue
            w("#### `%s` %s" % (key2, sub[1]))
            w("")
            w(sub[3])
            w("")
            body = []
            for c in rows:
                parts = []
                if c["kind"] == "text":
                    look = codes_module.TEXT_LOOKS
                    parts.append("text, %s" % c["text"]["weight"].lower())
                if c.get("linestyle"):
                    parts.append("`%s`" % c["linestyle"])
                if c.get("symbol"):
                    parts.append("symbol `%s`" % c["symbol"])
                parts.append("`%s` (%s)" % (c["colour"], colours.COLOURS[c["colour"]]))
                parts.append("%s mm" % c["weight"])
                text = "%s %s" % (c["meaning"], c["how"])
                if c.get("example"):
                    text += " Example: `%s`." % c["example"]
                body.append(["`%s`" % c["key"], c["name"], "; ".join(parts), "`%s`" % c["layer"], text])
            w(table(["Code", "Name", "Draws", "Layer", "What it is and how to shoot it"], body))
            w("")

    w("### The codes in alphabetical order")
    w("")
    w("Every code once, for looking one up: the key, its name, where it is catalogued and what it is.")
    w("")
    w(table(["Code", "Name", "Class and subgroup", "Meaning"],
            [["`%s`" % c["key"], c["name"], c["group"], c["meaning"]] for c in sorted(codes, key=lambda c: c["key"])]))
    w("")

    # ---- decisions --------------------------------------------------------------------------------------------
    w("## Originality")
    w("")
    w("The built-in customisation of this repository was converted from a third party's files, and Katana "
      "Standard was made so that nothing of them is in it. Its authors were given the built-in's structure and "
      "its statistics (how many groups, codes and strokes, which kinds of rule it uses and where it is weak) to "
      "know what a full survey library covers, and were forbidden to copy or paraphrase any name, group, layer, "
      "code key, comment, stroke, text or colour of it. They were not to read its contents, but in one "
      "exploratory run some of its names were printed in a terminal; none of them was written to a file of this "
      "library. The library was written from a specification that "
      "fixes its own taxonomy, its own vocabulary and its own palette; every stroke was drawn from a short "
      "written brief and looked at, in Katana's own renderer, before it was kept. "
      "Reviewers then compared every definition name, group path, layer, survey code key, comment and colour "
      "with the built-in's, and every stroke list by exact, translated and scaled match, and found no exact "
      "match of a name, key or colour and no stroke list beyond a single dash; what overlaps is single generic "
      "words and names of real objects that any survey library has to use (a bollard, a fire hydrant, a "
      "retaining wall, layer words such as `water`), which cannot be avoided without being wrong. The test "
      "`KatanaStandard.*` repeats the name, key and colour comparison at run time, by count. The library is "
      "licensed with the rest of the repository (`LICENSING.md`).")
    w("")
    w("## Decisions, and what was rejected")
    w("")
    w("- **Paper units for every definition, no `twoPoint`.** Measured in the engine, a stretched definition "
      "scales its marks with the line's length, so an arrowhead grows with the leader. Rejected: `world` "
      "units, whose marks would change size with the zoom and be unreadable at plan scale.")
    w("- **Three-letter keys, digits never part of one.** `UWM`, `UWM1` and `UWM01` are the same code; the "
      "digits are the string number, which the linework already reads. A key written with a digit before its "
      "`*` would match only numbered names. Rejected: two-letter keys, which leave no room for a family of "
      "services under one class.")
    w("- **No left or right suffix.** The rule engine matches a key exactly or by prefix and has no other "
      "form, so handedness is by direction of travel: ticks, barbs and chevrons fall to the LEFT, and each "
      "code says which way to walk.")
    w("- **Every code owns its `KEY*` rule.** A typo (`UWQ`) is then reported as a code no rule answers. "
      "Rejected: a family rule per class, which would swallow a mistyped code and style it as something "
      "else.")
    w("- **The library says nothing of linework spellings or automation.** Loading it onto a session must not "
      "reset a colleague's control words, so `linework` and `automation` are absent from the file, not "
      "written at their defaults.")
    w("- **A dot is a short dash.** Katana draws a `dot` stroke as one pen-width point whatever its radius: a "
      "speck a third of the line's weight on a plot and one pixel on screen. Dash-dot lines and the centres "
      "of symbols are therefore drawn with 0.6 mm dashes and a very small ring, measured in the renderer, "
      "and the definitions say so.")
    w("- **Layers are what one switches on and off together.** One per family, three levels, never one per "
      "code and never one per lifecycle: a disused service is an attribute, `utility.status`, and its own "
      "layer only in the utility group where the state is itself something one switches off.")
    w("- **Edges that are one hard line stay plain.** %d of the %d line codes use Katana's plain continuous "
      "line, since a solid line is the right drawing for a hard edge: the carriageway and path edges, the "
      "building wall, the kerb top and return (one feature, straight and curved), concrete walls, the bridge "
      "deck and the hairline detail. Where pens are applied the weight tells them apart; until then they differ "
      "by layer alone. Where two lines of one class are NOT the same thing they have a pattern each: the gutter "
      "lip is `Gutter Lip Line` and the back of the kerb `Kerb Back Line`, so a kerb reads as three lines on a "
      "plan whether or not the weights are applied." % (n_plain, n_lines))
    w("- **Stormwater pits and pipes are typed for the utility tools.** The pits and structures (`KP`) and the "
      "pipes and culverts (`KC`) of class K set `utility.type` to `stormwater`, though they are catalogued as "
      "drainage; the kerbs, channels and creeks do not. Rejected: leaving them untyped, which makes `UTILITY "
      "DRAW` read a coded stormwater pipe as an unknown service.")
    w("")

    w("## How it is made and held")
    w("")
    w("- **The data** is plain Python in `tools/katana_standard/`: `colours.py`, `linestyles.py`, `symbols.py` "
      "and `codes.py`, each importing nothing of the outside and each with its own `--check`. "
      "`make_katana_standard.py` is the only thing that knows the file format. `--json` writes the file, "
      "`--docs` this document, `--check` exits 1 when either committed output differs from what the data "
      "makes, `--images` draws the sheets with `render_images.py`. Run twice, it gives the same bytes.")
    w("- **The bytes are the writer's.** The generator writes the canonical form `customisationToJson` writes "
      "(`docs/customisation.md`, \"What the writer writes\"): a file loaded by `CUSTOMISE <file>` and written "
      "again by `CUSTOMISE EXPORT <out> CODES LINESTYLES SYMBOLS` comes back byte for byte, except for the one "
      "line the session adds, its record of what it was loaded from (`make_katana_standard.py --check "
      "--round-trip <katana_cli>`). The tests below check the same on the value itself.")
    w("- **A second opinion.** `tools/katana_standard/census.py` counts the committed file with the standard "
      "`json` module and no Katana code, and `tools/customisation_census.py` counts the generic figures. The "
      "tests pin the figures those scripts gave.")
    w("- **Tests.** `KatanaStandard.*` (`tests/cad/customisation/test_katana_standard.cpp`) reads the committed "
      "file with the strict reader and holds the counts, the grammar, every name resolved, every definition "
      "used and catalogued, the weights, the utility words, the geometry bounds of the contract and the "
      "byte-for-byte writer round trip. `" + CLI_IMPORT_TEST + "` imports the invented field file; `" +
      CLI_LINEWORK_TEST + "` strings control words, which no field-file reader carries, into a curve, a "
      "rectangle and a closed line. A counts-only test cannot see a swapped colour, weight or linestyle, so "
      "`katana_standard_current` runs `make_katana_standard.py --check` (the committed file and this document "
      "are what the data makes) and `katana_standard_colours`, `_linestyles`, `_symbols` and `_codes` run each "
      "data module's own check; they exist where Python does.")
    w("- **Images.** `docs/images/katana-standard-symbols.png`, `-linestyles.png`, `-plan.png` and "
      "`-showcase.png` are drawn by Katana itself from the committed file, headlessly "
      "(`tools/katana_standard/render_images.py`; `docs/headless.md`): the symbols are the Symbol Library's own "
      "icons, tinted with the colour of the code that uses each and laid out on one sheet; the linestyles are a "
      "300 dpi plot; the plan is the plan view at twice the pixels, each feature named beside it by the "
      "script; the showcase is the plan view at the plot scale 1:500, in a few screenshots laid side by side. "
      "`-palette.png` is laid "
      "out from the palette's values by `palette_preview.py`, since a palette is a table and not a drawing. The "
      "images are not byte-reproducible, since fonts differ between machines, so `--check` does not compare "
      "them. Each is under 500 KB.")
    w("")

    w("## Not done")
    w("")
    w("- **It is not the built-in.** The default stays the existing built-in, and a second compiled-in "
      "customisation with a chooser would touch the window, `katana_cli` and `katana_mcp` alike. Whether Katana "
      "Standard becomes the default, or a second built-in with a chooser, is the owner's decision.")
    w("- **The colour convention** of the buried services differs from `UTILITY DRAW`'s. Aligning one to the "
      "other is the owner's decision.")
    w("- **Whether the licence reaches this library is the owner's to settle.** `LICENSING.md` lists what the "
      "project licence does not cover and says the library is covered; the library was written by AI agents "
      "under the owner's direction, and what that means for copyright is a question for the owner and a lawyer.")
    w("- **Katana does not yet apply** `weight`, `group`, `hide`, `surface`, text rules, pipe rules, or a symbol's "
      "rotation, offset and raise. The library carries them for the day it does.")
    w("- **A curve cannot come from a field file.** The readers Katana has strung points by the file's own "
      "strings and close one with opcode 20; control words (`BC`, `EC`, `ST`, `END`, `CL`, `RECT`) reach the "
      "linework only from a code kept in a point's property, so the curve and the rectangle are tested through "
      "`LINEWORK PROPERTY` and not through an import.")
    w("- **Undo leaves the parent layers.** Katana's layer table makes every ancestor of a layer path a layer of "
      "its own, and Undo removes the layer a command made and not its ancestors: `LAYER NEW x/y/z` then `UNDO` "
      "leaves `x` and `x/y`. The library's layers are three levels deep, so one undo of the street corner's import "
      "takes the points, lines and styles and the ten layers they sit on away and leaves 15 empty parent layers. It is the "
      "layer table's behaviour, not the library's: the flat layer names of other libraries never showed it. "
      "Not changed here.")
    w("- **Two inconsistencies** are known and left as they are, because the names are frozen: "
      "`Pit Size (mm)` is text where every other millimetre attribute is an integer, and %d linestyles "
      "are shared by two to %d codes, so the plot legend uses the linestyle's name for them." % (n_shared, max_shared))
    w("- **Some symbols sit off the centre of their box**: Booster Connection, Building Corner, Doorway and "
      "North Arrow by 0.4 to 0.6 mm, and a few others (the valves, the camera, the trigonometric station) by "
      "0.25 to 0.35. The surveyed point is not the middle of their drawing, and each description says where "
      "the origin is. The Communications Maintenance Hole, a double ring with a C, reads as a copyright sign "
      "at small size.")
    return "\n".join(out).rstrip("\n") + "\n"


def _sibling(name):
    import importlib.util
    import os
    import sys
    here = os.path.dirname(os.path.abspath(__file__))
    key = "ks_" + name
    if key in sys.modules:
        return sys.modules[key]
    spec = importlib.util.spec_from_file_location(key, os.path.join(here, name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules[key] = module
    spec.loader.exec_module(module)
    return module
