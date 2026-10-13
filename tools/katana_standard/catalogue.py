"""The catalogue of Katana Standard, docs/katana_standard.md, made from the data.

`make_katana_standard.py --docs` calls `catalogue(data, root)` and writes the text. Nothing here is typed twice:
a definition's description, a code's meaning and what it draws, a colour's role and contrast, a count, all come
from the four data modules (and, for the comparison with the other built-in, from `parity.py` and the exceptions
file), so the catalogue cannot disagree with the file it describes, and `--check` finds the day it does. The
prose around the tables is here, in one place, because it is the part that is not data: how to read the
library, how to load it, what Katana applies today and what it only stores, why the choices were made, and what
is not done.

Written for a reader with a survey to draw, then for a maintainer. British spelling.
"""
import json
import os
import re
import sys

IMAGES = "images/katana-standard-"
# The test names the document cites; check_docs.py fails the day one is renamed away.
CLI_IMPORT_TEST = "cli.katana_standard_imports_a_field_file_and_reports_the_mistyped_code"
CLI_LINEWORK_TEST = "cli.katana_standard_strings_control_words_into_a_curve_a_rectangle_and_a_closed_line"
CLI_CHECK_TEST = "cli.katana_standard_code_check_finds_no_problem"
REFERENCE = os.path.join("resources", "customisation", "nsw.customisation.json")


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


def comma_list(items):
    items = list(items)
    return items[0] if len(items) == 1 else ", ".join(items[:-1]) + " and " + items[-1]


def _also(c):
    """What a code does besides drawing, as a cell: its surface flag, the attributes it applies and asks for."""
    parts = []
    for r in c["rules"]:
        if r["sets"] == "surface":
            parts.append("kept out of surfaces" if not r["surface"] else "joins surfaces")
        elif r["sets"] in ("attributes", "vertexAttributes"):
            applied = ["%s=%s" % (a["name"], a["value"]) for a in r["attributes"] if a["value"]]
            asked = [a["name"] for a in r["attributes"] if not a["value"]]
            word = "each point" if r["sets"] == "vertexAttributes" else "sets"
            if applied:
                parts.append("%s %s" % (word, ", ".join("`%s`" % a for a in applied)))
            if asked:
                parts.append("asks %s" % ", ".join(asked))
    return "; ".join(parts) if parts else ""


def _draws(c, colours, codes_module):
    """What a code draws, as a cell."""
    st = codes_module.styling(c)
    kind = codes_module.kind_of(c)
    parts = []
    if kind in ("line", "both"):
        parts.append("line `%s`" % st["linestyle"])
    if kind == "both":
        parts.append("symbol `%s` at every vertex" % st["symbol"])
    elif kind == "point":
        parts.append("point, symbol `%s`" % st["symbol"] if st["symbol"] else "point")
        if st["hide"]:
            parts[-1] += " (hidden)"
    elif kind == "text":
        size = next(r["size"] for r in c["rules"] if r["sets"] == "text")
        parts.append("text %g mm" % size)
    elif kind == "layer":
        parts.append("the layer of the text")
    elif kind == "pipe":
        # Katana stores a pipe rule and applies none, so this code draws nothing today.
        parts.append("nothing yet: its pipe rules (a round pipe sized from `Diameter (mm)`) are stored and not applied")
    elif kind == "surface":
        parts.append("nothing")
    if st["colour"]:
        parts.append("`%s` (%s)" % (st["colour"], colours.COLOURS[st["colour"]]))
    if st["weight"]:
        parts.append("%s mm" % st["weight"])
    return "; ".join(parts)


def _reference_figures(data, root):
    """The comparison with the other built-in, by count, from parity.py: (report, exceptions, parity) or None when
    the reference file is not there. The candidate is the data in memory, so a stale file on disk cannot change it."""
    path = os.path.join(root, REFERENCE)
    if not os.path.exists(path):
        return None
    parity = _sibling("parity")
    reference = parity.load_document(path)
    exceptions = parity.load_exceptions(os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                                     "parity_exceptions.json"))
    return parity.compare(reference, _candidate(data), exceptions), exceptions, parity


def _candidate(data):
    """The library as a customisation document, from the data in memory."""
    return {"format": "katana-customisation", "version": 1, "name": "Katana Standard",
            "colours": data.colours.COLOURS, "linestyles": data.linestyles, "symbols": data.symbols,
            "codes": data.rules}


def catalogue(data, root):
    colours = data.colours

    def on_white(name):
        """The "On white" cell: the contrast as the pen prints, and what the print does to a colour that is not
        printed as drawn."""
        value = colours.COLOURS[name]
        ratio = colours.paper_contrast(value)
        if colours.printed(value) != value:
            return "%.2f : 1 (prints black)" % ratio
        if name in colours.PRINTS_LIGHT:
            return "%.2f : 1 (prints light)" % ratio
        return "%.2f : 1" % ratio

    codes_module = data.codes_module
    sym_module = _sibling("symbols")
    lin_module = _sibling("linestyles")
    taxonomy = codes_module.TAXONOMY
    codes = data.codes
    styled = {c["key"]: codes_module.styling(c) for c in codes}
    by_style = {}
    for c in codes:
        for field in ("linestyle", "symbol"):
            name = styled[c["key"]][field]
            if name and name != "continuous":
                by_style.setdefault(name, []).append(c["key"])
    group_order = []
    for top in taxonomy:
        for sub in top[4]:
            group_order.append(top[1] + "/" + sub[0])
            if top[0] == "U" and sub[0] == "Unknown Services":
                group_order.append("Utilities/All Services")
    n_ls, n_sy, n_codes, n_rules = len(data.linestyles), len(data.symbols), len(codes), len(data.rules)
    n_groups_codes = sum(len(top[4]) for top in taxonomy)
    n_prefix = sum(1 for c in codes if c["key"].endswith("*") and c["key"] != "*")
    n_exact = sum(1 for c in codes if not c["key"].endswith("*"))
    n_lines = sum(1 for c in codes if styled[c["key"]]["linestyle"])
    n_plain = sum(1 for c in codes if styled[c["key"]]["linestyle"] == "continuous")
    style_names = {d["name"] for d in data.linestyles}
    n_shared = sum(1 for name, keys in by_style.items() if len(keys) > 1 and name in style_names)
    max_shared = max(len(keys) for name, keys in by_style.items() if name in style_names)
    layers = sorted({st["layer"] for st in styled.values() if st["layer"]})
    kinds = {}
    for c in codes:
        kinds[codes_module.kind_of(c)] = kinds.get(codes_module.kind_of(c), 0) + 1
    extra_rules = 0
    extra_keys = 0
    figures = _reference_figures(data, root)
    if figures:
        report, exceptions, parity = figures
        extra_rules, extra_keys = report["extras"]["rules"], report["extras"]["keys"]
        by_reason = {}
        for _, reason, _ in exceptions:
            by_reason[reason] = by_reason.get(reason, 0) + 1
        merged_rules = sum(count or 1 for _, reason, count in exceptions if reason == parity.MERGED)
        ref = report["reference"]
    else:
        report = exceptions = parity = None

    def applied(name):
        """How many keys apply a value to the attribute `name`."""
        return sum(1 for c in codes for r in c["rules"] if r["sets"] in ("attributes", "vertexAttributes")
                   for a in r["attributes"] if a["name"] == name and a["value"])

    # What comes from the other library beside the keys, by count, and how many labels are near its own.
    shared = {"diameters": applied("Diameter (mm)"), "heights": applied("Height (mm)"),
              "zones": applied("Check Zone"), "texts": kinds.get("text", 0),
              "near_labels": "(at 0.85 alike or more)", "near_any": ""}
    if figures:
        scan = _sibling("originality").scan(parity.load_document(os.path.join(root, REFERENCE)),
                                            _candidate(data))
        shared["near_labels"] = "(%d at 0.85 alike or more)" % len(scan["same_key_labels"])
        n_names, n_labels = len(scan["near_names"]), len(scan["near_labels"])
        shared["near_any"] = ("%s as near as 0.9 to anything the other library calls a thing, whatever the key. "
                              % ("No definition name and no legend label is" if n_names + n_labels == 0 else
                                 "%s and %s are" % (plural(n_names, "definition name"),
                                                    plural(n_labels, "legend label"))))
    no_symbol = [c["key"] for c in codes if codes_module.kind_of(c) == "point" and not styled[c["key"]]["symbol"]]
    labelled = {}
    for c in codes:
        if styled[c["key"]]["comment"]:
            labelled.setdefault(styled[c["key"]]["comment"], []).append(c)
    off_centre = []
    for warning in sym_module.check()[1]:
        match = re.match(r"(.+?): centre \(([-0-9.]+), ([-0-9.]+)\) is a little off the origin", warning)
        if match:
            off_centre.append((match.group(1), max(abs(float(match.group(2))), abs(float(match.group(3))))))
    out = []
    w = out.append

    w("# Katana Standard")
    w("")
    w("Katana Standard is a survey feature library written for Katana: one Katana customisation file, "
      "`resources/customisation/katana-standard.customisation.json`, with %d linestyles, %d symbols and %d survey "
      "codes (%d rules), a palette of %d colours and %d layers. Its survey code **keys** are the NSW "
      "customisation's, byte for byte, and each key stands for the same thing and has the same kinds of rule "
      "there as here, so a field file coded for one is read by the other and makes the same kinds of entity in "
      "both. That is the owner's decision. What the library names, draws and words is Katana's own: every "
      "linestyle and symbol, every layer path, colour, pen weight, text style and legend label was designed and "
      "written for it (\"What is shared and what is not\", below, says exactly what comes from the other "
      "library). This document is its catalogue: how to load it, how to read a code, every definition and "
      "every code, and what Katana does and does not yet do with what the file says."
      % (n_ls, n_sy, n_codes, n_rules, len(colours.COLOURS), len(layers)))
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
      "on a bridge, contours and a creek, control marks and the labels. Every line and symbol is the size it is "
      "on paper and is drawn by its code (`tools/katana_standard/showcase.py`); the words take their layer and "
      "colour from a text code and their height from the script, since Katana does not yet apply a text rule. "
      "Nor does it yet apply a code's pen weight, so every line is drawn at one width here and the weight ladder "
      "of \"Pen weights\" is not shown.*" % (len(_showcase_keys()), n_codes))
    w("")

    w("**Contents.** [At a glance](#at-a-glance) · [Using it](#using-it) · [Reading a code](#reading-a-code) · "
      "[What is shared and what is not](#what-is-shared-and-what-is-not) · "
      "[The palette](#the-palette) · [Linestyles](#linestyles) · [Symbols](#symbols) · [The codes](#the-codes) · "
      "[Index by key](#the-codes-in-alphabetical-order) · [Index by label](#the-codes-by-label) · "
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
        ["Survey codes", "%d keys: %d prefix keys that end in `*`, %d exact keys and the bare `*`; in %d classes "
                         "and %d groups; %d rules" % (n_codes, n_prefix, n_exact, data.classes - 0, n_groups_codes,
                                                      n_rules)],
        ["The keys", "the other built-in's, byte for byte; what each key stands for (with the size or the model "
                     "zone the other gives it) and its rule kinds, draw mode, text and pipe rules and surface "
                     "flag follow the other built-in's too (\"What is shared and what is not\")"],
        ["Layers", "%d, always three levels (`class/group/family`), lower case" % len(layers)],
        ["Palette", "%d named colours, each at least 3:1 against the plan view's ground and against white paper "
                    "as it prints (yellow gas and cream sewer, which are AS 5488's, print light); the service "
                    "colours are AS 5488's and `UTILITY DRAW`'s" % len(colours.COLOURS)],
        ["Says nothing of", "linework control words and import automation: loading it never resets a colleague's "
                            "spellings or switches"],
        ["Notice", " ".join(data.NOTICE)],
    ]))
    w("")

    # ---- using it ---------------------------------------------------------------------------------------------
    w("## Using it")
    w("")
    w("What is true now. Katana Standard **is** the built-in (the owner's ruling, 2026-10-08): a program starts "
      "with it, and the NSW customisation is compiled in beside it as an alternate that Settings (Built-in "
      "library, then Use) and `CUSTOMISE BUILTIN NSW` choose. It also loads like any customisation file "
      "(`docs/customisation.md`, \"Using the customisation\").")
    w("")
    w("1. **Load it alone.** `CUSTOMISE REPLACE resources/customisation/katana-standard.customisation.json` in "
      "the window's command line, in `katana_cli` or through `katana_mcp`; or File > Settings > Import with "
      "\"Replace instead of merging\" ticked. Its definitions become the whole library and its rules the whole "
      "of the codes. It brings no linework spellings and no import switches, so the session keeps its own.")
    w("2. **Merging works, with one caution.** `CUSTOMISE <file>`, or Import without the tick, merges by name. "
      "The most specific rule wins field by field, so a broader prefix rule already in the session can still "
      "supply a field these rules leave unsaid (a symbol, a text). Merged onto the NSW customisation, which "
      "has the same keys and the same kinds of rule for each, every rule of that one is replaced by this "
      "library's, so the codes become Katana Standard's while the other's definitions and colours stay loaded "
      "beside these, named by no code: load it alone when you want its library and nothing else.")
    w("3. **Start with it, or go back to it.** It is what a program starts with when nothing else is said. After "
      "another built-in was chosen, `CUSTOMISE BUILTIN \"Katana Standard\"` or Settings > Built-in library > Use "
      "puts it back, and `CUSTOMISE RESET` does too unless the run was started with another built-in as its "
      "own: `KATANA_BUILTIN_CUSTOMISATION=NSW` starts with the other, and RESET then gives that one "
      "(`docs/headless.md`, \"The customisation a run starts with\"). `KATANA_BUILTIN_CUSTOMISATION=none` "
      "starts with no built-in.")
    w("4. **Compiled in**: `-DKATANA_BUILTIN_CUSTOMISATION=<file>` at configure time embeds a different default, "
      "and `-DKATANA_ALTERNATE_CUSTOMISATIONS=<files>` further built-ins (`docs/building.md`, \"The built-in "
      "customisation and the reference folder\"). A build directory configured before this became the default "
      "that still has the earlier default is moved to this one by its next configure, once, with a line saying so.")
    w("5. **Then work as usual.** `SURVEY IMPORT <file>` or Survey > Import Survey Points codes and strings the "
      "points by these rules in one undo step; `CODE`, `LINEWORK`, `CODE EXPLAIN <code>`, `CODE LIST` and "
      "`CODE CHECK` read them. `CODE CHECK` reports no problems on the file: %d rules checked, no error and no "
      "warning (`%s`)." % (n_rules, CLI_CHECK_TEST))
    w("")
    w("A worked example, run for this document and held by `" + CLI_IMPORT_TEST + "`. The invented field file "
      "`tests/data/katana_standard/street_corner.fld` has twenty entered coordinates: a lot boundary coded "
      "`BT` in string 1 and closed by opcode 20, a kerb `KB`, a fence `FE`, a water main `WM` and a buried power "
      "line `EU`, four single points (an isolating valve `PWSV`, a tree trunk `PT04`, a gully pit `PGUL` and a "
      "bench mark `PBMK`) and one mistyped code, `PWSW`. With the library loaded by `CUSTOMISE REPLACE`, "
      "`SURVEY IMPORT` draws 20 points and 5 lines: 19 points are coded onto 9 layers in 9 styles, and the 20th, "
      "`PWSW`, is reported as a code no rule answers, since only the bare `*` fits it and that rule names no "
      "layer, no linestyle and no symbol. The lot boundary is a closed line of 100 m round, 600 square metres; "
      "the kerb and the water main are 24 m, the fence 10 m and the power line 20 m. Worked out by hand from the "
      "codes below before it was run.")
    w("")
    w("![The street corner field file after SURVEY IMPORT, in the plan view](" + IMAGES + "plan.png)")
    w("")
    w("*After the import, from the top: the closed lot boundary with its bench mark and its tree trunk; the "
      "fence beside it, drawn as posts on a wire; the kerb with a gully pit; the water main with its valve; the "
      "buried power line. The "
      "plain cross inside the lot is `PWSW`, the point no rule answers. Pen weights are not yet applied, so the "
      "boundary is no heavier than the fence. The words beside each feature are laid on the screenshot "
      "afterwards, from the library's own legend labels.*")
    w("")

    # ---- reading a code ---------------------------------------------------------------------------------------
    w("## Reading a code")
    w("")
    w("A code is a **key**, and the keys are the NSW customisation's: `AC*` is a bollard, `PT04` a tree trunk "
      "400 mm across, `WM*` a water main. In the field a point's code is the key and a string number, then "
      "control words: `WM1 ST`, `WM1`, `WM1 END` is one water main. The grammar, in full:")
    w("")
    for n, item in enumerate(numbered(codes_module.GRAMMAR), 1):
        w("%d. %s" % (n, item))
    w("")
    w("### The thirteen classes")
    w("")
    w("The classes and groups arrange the keys for a reader and fix the first two levels of a layer. A class "
      "letter is a label for this table and for the picture scripts, and no key is built from one.")
    w("")
    rows = []
    for top in taxonomy:
        n = sum(1 for c in codes if c["class"] == top[0])
        rows.append(["`%s`" % top[0], top[1], n, ", ".join(sub[0] for sub in top[4]), top[3]])
    w(table(["Letter", "Class", "Codes", "Groups", "What it holds"], rows))
    w("")
    w("The bare `*` belongs to no class. What each kind of code draws, by count:")
    w("")
    point_words = "a point, with a symbol"
    if no_symbol:
        point_words += " (all but %s, which %s none and %s as a plain point)" % (
            comma_list("`%s`" % k for k in no_symbol), "name" if len(no_symbol) > 1 else "names",
            "draw" if len(no_symbol) > 1 else "draws")
    kind_words = [("line", "a line, joined from the points"), ("point", point_words),
                  ("both", "a line with a symbol at every vertex"), ("text", "text of a size"),
                  ("layer", "the layer of the text codes"), ("surface", "a surface flag and nothing else"),
                  ("pipe", "nothing yet: the pipe rules, which say that any string carrying a diameter is a round "
                           "pipe, and which Katana stores and does not apply")]
    w(table(["Kind", "Codes", "Meaning"], [[k, kinds.get(k, 0), words] for k, words in kind_words if kinds.get(k)]))
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
      "offset and raise. So the four hidden symbols still draw, the seven text codes draw a plus until text "
      "rules are applied, and the plain continuous lines of one colour on one layer, such as the kerb top "
      "`KB*` and the kerb edge of a median `TM*`, look the same on the plan until the weight is applied. The "
      "rules are written for the day it does.")
    w("")

    # ---- shared ---------------------------------------------------------------------------------------------
    w("## What is shared and what is not")
    w("")
    w("**Shared, by the owner's decision: the survey code keys, what each key stands for, and what it does.** "
      "Three things come from the NSW customisation, and each is a fact about a key and not a name, a drawing "
      "or a sentence:")
    w("")
    w("1. **The keys.** The same %d keys, byte for byte, including every prefix rule ending in `*`, the "
      "numbered families and the bare `*`." % n_codes)
    w("2. **What each key stands for.** A key says nothing by itself: that `AC*` is a bollard and `U3*` a "
      "375 mm pipe is read from the other library's label for the key, and so is every size and number a code "
      "here states. %d keys apply a pipe diameter and %d a culvert height that the other library's label "
      "gives and the key does not; %d keys apply the model zone of the other library's attributes rule; the "
      "%d text keys have its text sizes. The reading is this library's, worded in its own sentences (\"Not "
      "done\" says where a label left the purpose open)."
      % (shared["diameters"], shared["heights"], shared["zones"], shared["texts"]))
    w("3. **What each key does.** The same kinds of rule in the same order, the same draw mode (line or "
      "point), the same hide and surface flags, a text rule of the same units, size and justification where "
      "the other has one, and pipe rules of the same justification and shape where the other has them. A field "
      "file therefore makes the same kinds of entity in either.")
    w("")
    w("These are the other library's identifiers and its facts about them, reused for field-data compatibility "
      "(`LICENSING.md` says what that leaves open). `tools/katana_standard/parity.py` compares the two files "
      "and the tests hold the result.")
    w("")
    w("**Katana's own: everything the library names, draws and words.** Linestyles and symbols (every one "
      "drawn here, from a written brief and looked at in Katana's own renderer) and their names, group paths, "
      "layer paths, colours and the colour table, pen weights, attribute names, text styles, legend labels, and "
      "every sentence of every meaning and of this document. `tools/katana_standard/originality.py` scans the "
      "two files for what a person could copy; `katana_standard_originality` runs it on them with the other "
      "tests, and `KatanaStandard.NothingButTheKeysIsSharedWithTheNswFileTrackedBesideIt` holds the names and "
      "the colours once more in C++. No definition name, group path, layer path, legend label, colour name, "
      "attribute name or text style is the same in both. Nor does a definition here draw the shape of one "
      "there, beyond the dash, the plus and the square that any library draws: each is compared with every "
      "one of the other library's by the marks it makes (its lines, circles, arcs, dots and texts), whatever "
      "order and direction the strokes were written in, turned by quarter turns and mirrored (the eight "
      "symmetries of the square), and three ways: as drawn, moved, and scaled to one size. Two numbers are "
      "the same there to the third decimal, and to the second once scaled. A name is the same when its letters "
      "and digits are, whatever its case, blanks and punctuation; and a definition name is compared with "
      "everything the other library calls a thing: its "
      "definition names, those names without the capitals it puts in front of them, and its legend labels, "
      "since a symbol named after the other library's label would be its name for the thing all the same. "
      "None is. Of colour values one is in both: pure white, `#FFFFFF`, which AS 5488 names for "
      "communications and which every drawing system has. The script and the tests excuse that one white, the "
      "communications colour of `utilityTypeColour`, and no other value: a second white would fail them.")
    w("")
    w("**What two libraries cannot help sharing** is the trade's own names for real things, and the sizes "
      "above. A give way line, a sewage pumping station and a culvert by its height are called that by anyone, "
      "so some legend labels here are near the other library's label for the same key without being it: "
      "`originality.py` counts them %s and prints this library's side, as a figure to read. %sThe meanings "
      "are sentences written for this document, and where one uses a term of the trade or of the law (a "
      "permanent survey mark, an endangered ecological community) it is because that is the thing's name."
      % (shared["near_labels"], shared["near_any"]))
    w("")
    if figures:
        w("**One field file, the same kinds of entity.** A code is answered by several keys at once, the exact "
          "key and every prefix key that fits it, and each field comes from the most specific rule that says "
          "it; so beside comparing key with key, `parity.py` looks up %d codes in both files as the program "
          "does (every key's own code, alone and followed by a string number, a padded one and a letter) and "
          "compares what comes out: the draw mode, the surface flag, whether a symbol is drawn and whether it "
          "is hidden, the text and the pipe rules. They are the same for every one."
          % report["codes_resolved"])
        w("")
        n_hidden = sum(1 for st in styled.values() if st["hide"])
        pipe_attributes = sum(count or 1 for _, reason, count in exceptions if reason == parity.PIPE_ATTRIBUTES)
        w("**Where the other library is not clean, this one is.** The NSW customisation holds %d rules over %d "
          "keys. %d of the rules repeat an earlier rule of their key exactly and %d are shadowed, in the sense "
          "`CODE CHECK` gives the word (every field such a rule sets, an earlier rule of its key has set, so a "
          "lookup takes nothing from it), in %d of the keys; %d more say something the rules before them leave "
          "unsaid, which a lookup then takes (in %d keys a `hide`, so the symbol is hidden); %d keys name a pen "
          "colour that resolves to nothing; %d draws its symbol through the linestyle slot, which `CODE CHECK` "
          "reports as a linestyle that is a symbol; and the pipe rules of the bare `*`, written for a system in "
          "which a rule can be for one case of an attribute, carry those cases as attributes, which a lookup "
          "here applies to every code (%d of them). Katana Standard keeps one clean rule set per key: one rule "
          "of each kind, "
          "every name resolved, the symbol said in a symbol rule, `hide` said where the lookup makes it true, "
          "and no attribute on a pipe rule. Those %d differences are listed in "
          "`tools/katana_standard/parity_exceptions.json`, one entry for each key and reason, and `parity.py` "
          "fails on an entry that explains nothing or a difference that has none."
          % (ref["rules"], ref["keys"], ref["collapsible_duplicates"], ref["collapsible_shadowed"],
             ref["keys_with_a_collapsible_rule"], merged_rules, n_hidden, by_reason.get(parity.COLOUR, 0),
             by_reason.get(parity.LINESTYLE_SYMBOL, 0), pipe_attributes, len(exceptions)))
        w("")
        w(table(["Reason", "Keys", "What the other library does and this one does not"], [
            [parity.DUPLICATE, by_reason.get(parity.DUPLICATE, 0), "repeats a rule of the key field for field"],
            [parity.SHADOWED, by_reason.get(parity.SHADOWED, 0),
             "has a later rule of a kind that `CODE CHECK` calls shadowed: it sets no field and names no "
             "attribute that an earlier rule of the key has not"],
            [parity.MERGED, by_reason.get(parity.MERGED, 0),
             "has a later rule of a kind that says a field, or names an attribute, that the rules before it "
             "leave unsaid; here the one rule says the field"],
            [parity.COLOUR, by_reason.get(parity.COLOUR, 0), "names a pen colour that resolves to nothing"],
            [parity.LINESTYLE_SYMBOL, by_reason.get(parity.LINESTYLE_SYMBOL, 0),
             "names a symbol as a linestyle; here a symbol rule follows the feature rule"],
            [parity.PIPE_ATTRIBUTES, by_reason.get(parity.PIPE_ATTRIBUTES, 0),
             "carries attributes on its pipe rules, which a lookup applies to every code the key answers; here "
             "a pipe rule carries none"],
        ]))
        w("")
        w("**What this library adds.** %d keys carry one more attributes rule than the other library has for them, "
          "last: the service word `utility.type` and the prompts Owner, Depth (m) and Condition (grammar rule 9). "
          "The rule makes no entity, so the same field file still makes the same kinds of entity; it is why a coded "
          "string is typed for `UTILITY DRAW`. `parity.py` counts these apart (%d rules) and `--no-extra-attributes` "
          "makes them differences. So the kinds of entity are the same in the two libraries and the properties "
          "on them are not: a service coded here carries its service word and its prompts, and nothing coded "
          "here carries the attributes the other's `*` rules apply." % (extra_keys, extra_rules))
        w("")

    # ---- palette ----------------------------------------------------------------------------------------------
    w("## The palette")
    w("")
    w("Twenty-eight colours, each named `katana <role>` so a rule reads as the thing it colours, and none can "
      "fold onto one of the 27 standard names Katana already knows. Each is at least 3:1 against the plan "
      "view's ground (`%s`, a blue-grey that is almost black) and against white paper, since one library is "
      "plotted and read on the screen (two AS 5488 colours are excused on paper, below); and no two are closer "
      "than 0.05 in OKLab, about where two thin lines of different colour stop being told apart. That leaves "
      "only a narrow band of lightness, so the six warm classes (kerb, wall, contour, fuel, fence, breakline) "
      "were placed by a search, each at least %.2f from every other colour, and the six classes drawn with the "
      "thinnest pens (tree, planting, ground, building, wall, communications) keep at least %.2f : 1 on both "
      "grounds, as they print. Colour does not carry a class alone: for a reader who confuses red with green "
      "some warm pairs stay close whatever their values, so every pipe has its letter and every line class its "
      "own pattern." % (colours.GROUND, colours.WARM_FLOOR, colours.THIN_FLOOR))
    w("")
    w("**The service colours are AS 5488's.** AS 5488.1 has a primary code and line colour table (its "
      "Appendix B) that names a colour for each service: water blue, electricity orange, communications white "
      "(black when drawn on a white ground), gas yellow, sewer cream, stormwater and drainage green, recycled "
      "water purple, fire service red and unidentified services pink; published guides add petroleum brown. "
      "The standard names the colours and gives no sRGB, and it leaves the colours of plans to whoever draws "
      "them (clause 1.2.2 of the 2013 edition), so each value here is Katana's rendering of the name. The "
      "sources, the names and the values are in `docs/subsurface_utilities.md`, \"Colours\". Traffic systems "
      "have no AS 5488 colour and are Katana's choice. The US convention (electricity red, communications "
      "orange, sewer green), which this library first used, is not Australian, so it is rejected: the "
      "library follows AS 5488.")
    w("")
    w("**The two utility conventions agree.** `UTILITY DRAW` draws its layers in these same values "
      "(`utilityTypeColour`), and `KatanaStandard.EveryServiceColourIsTheColourUtilityDrawUses` holds the "
      "library and that table equal, service by service, and holds every utility code to its service's colour. "
      "A drawing that mixes coded survey strings and `UTILITY DRAW` output has one convention in it. Hazard is "
      "the one other colour that moved: AS 5488's electricity orange left the old hazard orange 0.043 away, "
      "under the 0.05 floor, so hazard is a red-orange (`#FF5A37`) 0.077 from electricity.")
    w("")
    w("**Yellow and cream print light.** On white paper yellow gas (%.2f : 1) and cream sewer (%.2f : 1) cannot "
      "reach 3:1 and still be yellow and cream. The colours are the standard's, so they are not darkened: they "
      "keep their 3:1 against the plan ground and print light, and the tests name them as the only two "
      "excused. Communications is white, which the plot prints black (`cad::paperColour`: every channel at "
      "230 or more), so it is held to the floor as printed (%.2f : 1). A plot that must show gas and sewer "
      "firmly plots monochrome, which prints every pen black, or recolours those layers." %
      (colours.paper_contrast(colours.COLOURS["katana gas"]), colours.paper_contrast(colours.COLOURS["katana sewer"]),
       colours.paper_contrast(colours.COLOURS["katana telecom"])))
    w("")
    w("![The 28 colours of Katana Standard on the plan ground and on white paper](" + IMAGES + "palette.png)")
    w("")
    for heading, names in colours.FAMILIES:
        w("**%s**" % heading)
        w("")
        w(table(["Colour", "Value", "Used for", "AS 5488", "On the ground", "On white"],
                [["`%s`" % n, "`%s`" % colours.COLOURS[n], colours.ROLES[n],
                  colours.AS5488_NAMES.get(n) or ("Katana's choice" if n in colours.AS5488_NAMES else ""),
                  "%.2f : 1" % colours.contrast(colours.COLOURS[n], colours.GROUND),
                  on_white(n)] for n in names]))
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
      "same width here. This is a plot on white paper: the communications cable prints black, and the sewer and "
      "gas pipes print light, since AS 5488's cream and yellow are under 3:1 on white (see the palette).*")
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
    w("*As the Symbol Library draws them, each in the colour of a code that uses it. The dialog scales every "
      "icon to fill its tile, so the figures under a name are the symbol's width and height on paper: the "
      "canopies and the trunks differ by size alone.*")
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
    w("%d codes by class and group. *Label* is the code's legend label, the name a plot legend prints in "
      "capitals and `CODE LIST` shows as the rule's comment; a key that draws nothing of its own (a surface "
      "flag, a text size, the bare `*`) has none. *Draws* is the linestyle or symbol, the colour and the pen "
      "weight; *Also* is what the code does besides drawing: whether its points join a surface, and the "
      "attributes it applies (in backticks) or asks the surveyor for. Where a code draws the plain continuous "
      "line, Katana's own word `continuous` is its linestyle. A field-book code is a key and a string number, "
      "`WM1 ST, WM1, WM1 END`. To find a code from what it is called, use [the index by label]"
      "(#the-codes-by-label); from its key, [the index by key](#the-codes-in-alphabetical-order)." % n_codes)
    w("")
    w("### Every code: the bare `*`")
    w("")
    w("The bare `*` matches every code, so it holds only what applies to any string.")
    w("")
    code_header = ["Code", "Label", "Draws", "Layer", "Also", "What it is"]
    star = [c for c in codes if c["class"] == "-"]
    w(table(code_header,
            [["`%s`" % c["key"], styled[c["key"]]["comment"] or "", _draws(c, colours, codes_module), "", _also(c),
              c["meaning"]] for c in star]))
    w("")
    for top in taxonomy:
        w("### Class %s: %s" % (top[0], top[1]))
        w("")
        w(top[3])
        w("")
        for sub in top[4]:
            group = top[1] + "/" + sub[0]
            rows = [c for c in codes if c["group"] == group]
            if not rows:
                continue
            w("#### %s" % sub[0])
            w("")
            w("%s (%s)" % (sub[2], plural(len(rows), "code")))
            w("")
            body = []
            for c in rows:
                layer = styled[c["key"]]["layer"]
                body.append(["`%s`" % c["key"], styled[c["key"]]["comment"] or "", _draws(c, colours, codes_module),
                             "`%s`" % layer if layer else "", _also(c), c["meaning"]])
            w(table(code_header, body))
            w("")

    w("### The codes in alphabetical order")
    w("")
    w("Every code once, for looking one up by its key: the key, its legend label, where it is catalogued and "
      "what it is.")
    w("")
    w(table(["Code", "Label", "Class and group", "Meaning"],
            [["`%s`" % c["key"], styled[c["key"]]["comment"] or "", c["group"] or "(no class)", c["meaning"]]
             for c in sorted(codes, key=lambda c: c["key"])]))
    w("")
    w("### The codes by label")
    w("")
    w("Every legend label once, in alphabetical order, for looking a code up by what it is called: %d labels "
      "for the %d keys that have one. A label that several keys share is listed once with all of them."
      % (len(labelled), sum(len(v) for v in labelled.values())))
    w("")
    w(table(["Label", "Code", "Class and group"],
            [[label, " ".join("`%s`" % c["key"] for c in sorted(users, key=lambda c: c["key"])),
              "; ".join(sorted({c["group"] for c in users}))]
             for label, users in sorted(labelled.items(), key=lambda item: (item[0].lower(), item[0]))]))
    w("")

    # ---- decisions --------------------------------------------------------------------------------------------
    w("## Decisions, and what was rejected")
    w("")
    w("- **The keys are the NSW customisation's.** The owner ruled that Katana Standard has the same survey codes "
      "as the NSW one, with different symbols and linestyles, so that a field file works with either. The "
      "library began with a vocabulary of its own, 245 codes of three capital letters each; it is gone, with its "
      "documents, tests and pictures, since the library is new and nothing but those depended on it. Rejected: "
      "keeping both vocabularies, which would double the codes a surveyor learns and leave two answers to one "
      "field file.")
    w("- **The structure follows the keys, the look does not.** A key makes the same kinds of entity in both "
      "libraries because the rule kinds, the draw mode, the text and pipe rules and the surface and hide flags "
      "are the other's; every visible choice is Katana's. Rejected: copying a look that a person could copy "
      "back, and a looser match (the same key but another draw mode), under which the same field file would "
      "make a point in one library and a line in the other.")
    w("- **One clean rule set per key.** The other library repeats and shadows rules, names pen colours that "
      "resolve to nothing, draws a symbol through a linestyle slot and applies the attributes of its `*` pipe "
      "rules to every code. Reproducing that would give `CODE CHECK` hundreds of warnings on a new library and "
      "put those attributes on whatever is coded; each difference is a counted exception that `parity.py` "
      "holds.")
    w("- **A service code asks for its service's attributes.** Where the other library has no attributes rule "
      "for a service key, one is added last (grammar rule 9), so that `UTILITY DRAW` reads a coded string as a "
      "typed service. Rejected: strict equality of the rule kinds, which would drop the typing the utility tools "
      "read and leave every coded service \"unknown\".")
    w("- **Paper units for every definition, no `twoPoint`.** Measured in the engine, a stretched definition "
      "scales its marks with the line's length, so an arrowhead grows with the leader. Rejected: `world` "
      "units, whose marks would change size with the zoom and be unreadable at plan scale.")
    w("- **No left or right suffix.** The rule engine matches a key exactly or by prefix and has no other "
      "form, so a handed pair is two keys (`CV*` and `CY*`, `DA*` and `DC*`, `LH*` and `LR*`, `HE*` and `HR*`) "
      "and otherwise handedness is by direction of travel: ticks, barbs and chevrons fall to the LEFT.")
    w("- **Every key is matched as written.** A typo is then reported as a code no rule answers, unless some "
      "prefix key fits it by chance; the invented field file's `PWSW` is none of the %d keys' prefixes." % n_codes)
    w("- **The library says nothing of linework spellings or automation.** Loading it onto a session must not "
      "reset a colleague's control words, so `linework` and `automation` are absent from the file, not "
      "written at their defaults.")
    w("- **A dot is a short dash.** Katana draws a `dot` stroke as one pen-width point whatever its radius: a "
      "speck a third of the line's weight on a plot and one pixel on screen. Dash-dot lines and the centres "
      "of symbols are therefore drawn with 0.6 mm dashes and a very small ring, measured in the renderer, "
      "and the definitions say so.")
    w("- **Layers are what one switches on and off together.** One per family, three levels, never one per "
      "code: %d layers for %d codes." % (len(layers), n_codes))
    w("- **Edges that are one hard line stay plain.** %d of the %d line codes use Katana's plain continuous "
      "line, since a solid line is the right drawing for a hard edge or a design string; where two lines of "
      "one class are NOT the same thing they have a pattern each." % (n_plain, n_lines))
    w("- **Stormwater pits and pipes are typed for the utility tools.** The pits and structures and the pipes "
      "and culverts of class K set `utility.type` to `stormwater`, though they are catalogued as drainage; the "
      "kerbs, channels and creeks do not. Rejected: leaving them untyped, which makes `UTILITY DRAW` read a "
      "coded stormwater pipe as an unknown service.")
    w("- **A size a key stands for is applied.** The key itself states none: a pipe `U3*` is 375 mm, an invert "
      "`PI04` is in a 450 mm pipe and a box culvert `D1*` is 1200 mm high because the other library's labels "
      "for those keys say so (\"What is shared and what is not\"). Each applies `Diameter (mm)` or "
      "`Height (mm)`, so the surveyor is not asked what the code already means.")
    w("")

    w("## How it is made and held")
    w("")
    w("- **The data** is plain Python in `tools/katana_standard/`: `colours.py`, `linestyles.py`, `symbols.py` "
      "and `codes.py`, each importing nothing of the outside and each with its own `--check`; the check of "
      "`colours.py` also reads `kNearWhiteChannel` from `include/katana/cad/plot.hpp`, so the white rule it "
      "prints by is the plot's. `codes.py --check` also holds that every definition is named by a code and "
      "every colour of the palette is used. "
      "`make_katana_standard.py` is the only thing that knows the file format. `--json` writes the file, "
      "`--docs` this document, `--check` exits 1 when either committed output differs from what the data "
      "makes or a picture was drawn from something else than is committed (\"Images\", below), `--images` "
      "draws the sheets with `render_images.py`. Run twice, `--json` and `--docs` give the same bytes.")
    w("- **The bytes are the writer's.** The generator writes the canonical form `customisationToJson` writes "
      "(`docs/customisation.md`, \"What the writer writes\"): a file loaded by `CUSTOMISE <file>` and written "
      "again by `CUSTOMISE EXPORT <out> CODES LINESTYLES SYMBOLS` comes back byte for byte, except for the one "
      "line the session adds, its record of what it was loaded from (`make_katana_standard.py --check "
      "--round-trip <katana_cli>`). The tests below check the same on the value itself.")
    w("- **A second opinion.** `tools/katana_standard/census.py` counts the committed file with the standard "
      "`json` module and no Katana code, and `tools/customisation_census.py` counts the generic figures. The "
      "tests pin the figures those scripts gave.")
    w("- **The comparison with the other library** is `tools/katana_standard/parity.py` (keys and structure, with "
      "`parity_exceptions.json` and its own tests in `test_parity.py`) and `originality.py` (what else is "
      "shared, with its own tests in `test_originality.py`). `katana_standard_parity` and "
      "`katana_standard_originality` run them against the two tracked files, and `katana_standard_parity_script` "
      "and `katana_standard_originality_script` run the scripts' own tests on small invented libraries; "
      "`KatanaStandard.EveryKeyOfTheNswBuiltInIsHereAndNoOtherKey` and "
      "`KatanaStandard.FortyCodesAcrossAllClassesResolveToTheKindOfEntityTheNswBuiltInMakes` do the same in C++, "
      "from the two files alone.")
    w("- **Tests.** `KatanaStandard.*` (`tests/cad/customisation/test_katana_standard.cpp`) reads the committed "
      "file with the strict reader and holds the counts, the grammar, every name resolved, every definition "
      "used and catalogued, the weights, the utility words, the geometry bounds of the contract and the "
      "byte-for-byte writer round trip. `" + CLI_IMPORT_TEST + "` imports the invented field file; `" +
      CLI_LINEWORK_TEST + "` strings control words, which no field-file reader carries, into a curve, a "
      "rectangle and a closed line; `" + CLI_CHECK_TEST + "` holds that `CODE CHECK` finds nothing. A "
      "counts-only test cannot see a swapped colour, weight or linestyle, so "
      "`katana_standard_current` runs `make_katana_standard.py --check` (the committed file and this document "
      "are what the data makes, and each picture was drawn from what is committed) and "
      "`katana_standard_colours`, `_linestyles`, `_symbols` and `_codes` run each "
      "data module's own check; they exist where Python does.")
    w("- **Images.** `docs/images/katana-standard-symbols.png`, `-linestyles.png`, `-plan.png` and "
      "`-showcase.png` are drawn by Katana itself from the committed file, headlessly "
      "(`tools/katana_standard/render_images.py`; `docs/headless.md`): the symbols are the Symbol Library's own "
      "icons, tinted with the colour of a code that uses each (a code of the definition's own group; what "
      "several services share takes the first that uses it) and laid out on one sheet with each symbol's size "
      "on paper under its name, since the dialog scales every icon to fill its tile; the linestyles are a "
      "300 dpi plot, each sample drawn by such a code, the rows as far apart as their linestyles need; "
      "the plan is the plan view at twice the pixels, each feature named beside it by the "
      "script; the showcase is the plan view at the plot scale 1:500, in a few screenshots laid side by side. "
      "`-palette.png` is laid "
      "out from the palette's values by `palette_preview.py`, since a palette is a table and not a drawing. The "
      "images are not byte-reproducible, since fonts differ between machines, so `--check` cannot compare "
      "their bytes. Each carries instead, in the text of its PNG, the SHA-256 of what it was drawn from: the "
      "committed file for the four Katana draws (the plan also its field file, the showcase also the script of "
      "its scene) and `colours.py` for the palette. `--check` reports a picture whose record is not that of "
      "the committed files, or that has none, so a picture of an earlier edition of the library fails "
      "`katana_standard_current` until it is drawn again; `tools/katana_standard/test_pictures.py` tests that "
      "check on pictures it writes itself. The script that lays a sheet out is not in the record: whoever "
      "changes it draws the sheet again. Each is under 500 KB.")
    w("")

    w("## Not done")
    w("")
    w("- **Yellow and cream print light.** AS 5488's gas and sewer colours are under 3:1 on white paper, and the "
      "plot has no rule that darkens a light colour: it turns only white and near-white pens black "
      "(`cad::paperColour`). A rule that did, for any pen under 3:1, would change every plot, so it is the "
      "owner's decision; until then they print light, as the palette section says.")
    w("- **The earlier file names of an old project** belonged to the NSW customisation and are answered by it "
      "alone (`docs/customisation.md`, \"Several built-ins\"): opened under Katana Standard such a project lists "
      "them as not loaded, and `CUSTOMISE BUILTIN NSW` clears them. The warning does not name that verb for "
      "them, since what is missing is the old file names. Narrow: a project saved in the first days of the "
      "built-in.")
    w("- **Whether the licence reaches this library is the owner's to settle.** `LICENSING.md` lists what the "
      "project licence does not cover. The library was written by AI agents under the owner's direction, and "
      "its survey code keys are another library's identifiers, reused on the owner's decision for field-data "
      "compatibility; what either means for copyright is a question for the owner and a lawyer.")
    w("- **The meaning of some keys is a reading.** Several dozen keys have a label in the other library and no "
      "stated purpose (the unclassified road design strings, the hinges and bases of a cross-section, a few "
      "marks and lane lines). Their meanings are worded as generally as the label allows, and the unclassified "
      "design strings share one layer family each; a surveyor who knows what one is for should say so.")
    w("- **What the comparison of shapes does not see.** `originality.py` holds two definitions to be one shape "
      "when they make the same marks at any place and size, turned by quarter turns or mirrored. A drawing "
      "turned by another angle, stretched one way, a circle drawn as a ring of short lines, or a line cut in two "
      "where another meets it makes other marks and is another shape to it, and a definition of five strokes or "
      "fewer is not compared at all. No C++ test repeats the comparison, so it is held where Python is "
      "(`katana_standard_originality`).")
    w("- **Katana does not yet apply** `weight`, `group`, `hide`, `surface`, text rules, pipe rules, or a symbol's "
      "rotation, offset and raise. The library carries them for the day it does.")
    w("- **A curve cannot come from a field file.** The readers Katana has strung points by the file's own "
      "strings and close one with opcode 20; control words (`BC`, `EC`, `ST`, `END`, `CL`, `RECT`) reach the "
      "linework only from a code kept in a point's property, so the curve and the rectangle are tested through "
      "`LINEWORK PROPERTY` and not through an import.")
    w("- **A control word typed alone is a code, and six of the seven are answered.** Four keys are spelled like "
      "a default control word (`BC*`, `CL*`, `EC*`, `ST*`) and two more answer one by prefix (`EN*` answers "
      "`END` and `RE*` answers `RECT`), as in the other library; only `JPN` is answered by no key. A control "
      "word is read only after the key, so a point coded `ST` is a string called ST, a septic tank, and one "
      "coded `END` an electrical sump. Nothing is lost, but a surveyor who types a control word in the code "
      "column gets a feature.")
    w("- **A pipe is sized from this library's own attribute.** The pipe rules of the bare `*` take a size from "
      "`Diameter (mm)`. The other library's take theirs from attributes of other names (and have culvert "
      "rules sized from a width and a height, where this library keeps the one round-pipe rule its lookup "
      "resolves to), so a field file that carries those attributes will size no pipe here on the day Katana "
      "applies pipe rules, and the unit is millimetres by the attribute's name alone: nothing converts a "
      "value. Nothing differs today, since pipe rules are stored and not applied.")
    w("- **Undo leaves the parent layers.** Katana's layer table makes every ancestor of a layer path a layer of "
      "its own, and Undo removes the layer a command made and not its ancestors: `LAYER NEW x/y/z` then `UNDO` "
      "leaves `x` and `x/y`. The library's layers are three levels deep, so one undo of the street corner's import "
      "takes the points, lines and styles and the ten layers they sit on away and leaves 15 empty parent layers. "
      "It is the layer table's behaviour, not the library's. Not changed here.")
    w("- **%d linestyles are shared by two to %d codes**, so the plot legend uses the linestyle's name for them "
      "and not the code's label." % (n_shared, max_shared))
    if off_centre:
        names = [n for n, _ in off_centre]
        w("- **Some symbols sit off the centre of their box**: %s, by %.2f to %.2f mm. The surveyed point is not "
          "the middle of their drawing, and each description says where the origin is." %
          (comma_list(names), min(o for _, o in off_centre), max(o for _, o in off_centre)))
    return "\n".join(out).rstrip("\n") + "\n"


def _sibling(name):
    import importlib.util
    here = os.path.dirname(os.path.abspath(__file__))
    key = "ks_" + name
    if key in sys.modules:
        return sys.modules[key]
    spec = importlib.util.spec_from_file_location(key, os.path.join(here, name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules[key] = module
    spec.loader.exec_module(module)
    return module
