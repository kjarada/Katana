#!/usr/bin/env python3
"""Assemble Katana Standard: the customisation file and its catalogue, from the four data modules.

    python tools/katana_standard/make_katana_standard.py --json      write resources/customisation/katana-standard.customisation.json
    python tools/katana_standard/make_katana_standard.py --docs      write docs/katana_standard.md
    python tools/katana_standard/make_katana_standard.py --check     exit 1 when a committed output differs from what this makes
    python tools/katana_standard/make_katana_standard.py --images    draw the catalogue's sheets with Katana itself (render_images.py;
                                                                     needs Pillow and a built katana, and says so)
    python tools/katana_standard/make_katana_standard.py --stats     print the counts

Beside it: `catalogue.py` writes the document, `census.py` counts the committed file with no Katana code (the
figures the tests pin), `showcase.py` is the street corner of docs/images/katana-standard-showcase.png.

The data is `colours.py`, `linestyles.py`, `symbols.py` and `codes.py` beside this file, each plain Python
that imports nothing of the outside, and each with its own `--check`. This script is the only thing that
knows the file format. Its output is a function of the data alone: no date, no machine, no random order, so
that running it twice gives the same bytes and `--check` is a fair test of "has the data changed since the
file was written".

THE BYTES ARE THE WRITER'S. `entity::customisationToJson` writes a customisation in one canonical form
(docs/customisation.md, "What the writer writes", eleven rules), and this script writes that same form: a file
loaded by `CUSTOMISE <file>` and written by `CUSTOMISE EXPORT <out>` comes back byte for byte (the `Round trip`
check of `--check --round-trip KATANA_CLI`, and a test in tests/cad/customisation/test_katana_standard.cpp).
So a committed edition diffs against the next as text, a stroke and a rule a line, and a person who opens the
file in Katana and keeps it changes nothing.

What the file carries, and what it leaves out, on purpose:

* `name` "Katana Standard", a description and a three-line notice, from the counts of the data.
* `colours`: the 28 of `colours.py`.
* `linestyles` and `symbols`: every definition, `paper` units, in name order. The one-line `description` of a
  definition is NOT in the file (the format has no member for it); it is in the catalogue, docs/katana_standard.md.
* `codes`: the rules of `codes.expand_rules`, a code after a code in the order of `CODES`.
* No `linework` and no `automation`: the library says nothing about control words or about what is applied
  on import, so loading it never resets a colleague's own settings.
* No `sources` and no `basedOn`: it was made here, from nothing.
"""
import json
import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True  # a generator leaves nothing behind in the tools folder

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
JSON_PATH = os.path.join(ROOT, "resources", "customisation", "katana-standard.customisation.json")
DOCS_PATH = os.path.join(ROOT, "docs", "katana_standard.md")
NAME = "Katana Standard"
EDITION = 1


def _module(name):
    """The data module `name` beside this file, loaded without leaving bytecode."""
    import importlib.util
    spec = importlib.util.spec_from_file_location("ks_" + name, os.path.join(HERE, name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules["ks_" + name] = module
    spec.loader.exec_module(module)
    return module


class Data:
    """The four modules, and the few figures derived from them that more than one output quotes."""

    def __init__(self):
        self.colours = _module("colours")
        self.linestyles = _module("linestyles").LINESTYLES
        self.symbols = _module("symbols").SYMBOLS
        self.codes_module = _module("codes")
        self.codes = self.codes_module.CODES
        self.rules = [rule for code in self.codes for rule in self.codes_module.expand_rules(code)]
        self.classes = len(self.codes_module.TAXONOMY)

    def description(self):
        return ("%s, edition %d: an original survey feature library for Katana, with %d linestyles, %d symbols "
                "and %d survey codes in %d classes." % (NAME, EDITION, len(self.linestyles), len(self.symbols),
                                                         len(self.codes), self.classes))

    NOTICE = ["Katana Standard is original work made for Katana.",
              "It contains no third-party customisation, style library or survey code file.",
              "Its licence is that of the Katana repository."]


# ---- the writer's form ---------------------------------------------------------------------------------------
# The member order of each kind of object is the order docs/customisation.md, "The members", lists them in.

RULE_ORDER = ("key", "sets", "layer", "colour", "draw", "linestyle", "weight", "group", "comment", "surface",
              "hide", "symbol", "text", "pipe", "vertexPipe", "segmentPipe", "attributes", "vertexAttributes",
              "segmentAttributes")
SYMBOL_ORDER = ("name", "colour", "size", "rotation", "offset", "raise")
TEXT_RULE_ORDER = ("style", "colour", "units", "size", "justifyX", "justifyY", "offset", "raise", "angle", "slant",
                   "widthFactor", "underline", "strikeout", "italic", "weight")
PIPE_ORDER = ("justify", "shape", "size1", "size2", "active")
ATTRIBUTE_ORDER = ("type", "name", "value")
STROKE_TEXT_ORDER = ("text", "angle", "height", "justify", "font", "widthFactor", "extra")

# A member written only when it is not its default (an optional that is present, `hide` and `surface`, is
# always written when the rule has it).
RULE_DEFAULTS = {"layer": "", "colour": "", "linestyle": "", "weight": "", "group": "", "comment": ""}
SYMBOL_DEFAULTS = {"colour": "", "size": 0, "rotation": 0, "offset": 0, "raise": 0}
TEXT_RULE_DEFAULTS = {"style": "", "colour": "", "units": "", "size": 0, "justifyX": "", "justifyY": "", "offset": 0,
                      "raise": 0, "angle": 0, "slant": 0, "widthFactor": 1, "underline": False, "strikeout": False,
                      "italic": False, "weight": ""}
PIPE_DEFAULTS = {"justify": "", "shape": "", "size1": "", "size2": "", "active": False}
ATTRIBUTE_DEFAULTS = {"value": ""}
STROKE_TEXT_DEFAULTS = {"text": "", "angle": 0, "height": 0, "justify": "", "font": "", "widthFactor": 1}


def number(value):
    """The shortest text that reads back as the same double: `8` not `8.0`, `-0.25`, `1e-05`; negative zero
    is `-0.0`, which a JSON reader would otherwise take for the integer 0 (core::formatExactReal)."""
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise TypeError("not a number: %r" % (value,))
    value = float(value)
    if value != value or value in (float("inf"), float("-inf")):
        raise ValueError("JSON has no form for a number that is not finite")
    if value == 0.0:
        return "-0.0" if str(value).startswith("-") else "0"
    if value == int(value) and abs(value) < 1e15:
        return str(int(value))
    return repr(value)


def text(value):
    """A JSON string as the library writes it: `"` and `\\` escaped, controls as \\n \\t \\r \\b \\f or \\u00xx
    in lower case, `/` and everything outside ASCII as it is."""
    return json.dumps(value, ensure_ascii=False)


def scalar(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, str):
        return text(value)
    return number(value)


def inline(obj, order, defaults, always=()):
    """One object on one line, members in `order`, a member at its default left out unless it is in `always`."""
    parts = []
    unknown = set(obj) - set(order)
    if unknown:
        raise ValueError("members the writer has no place for: %s" % sorted(unknown))
    for member in order:
        if member not in obj:
            continue
        value = obj[member]
        if member not in always and member in defaults and value == defaults[member] \
                and type(value) is type(defaults[member]):
            continue
        parts.append("%s: %s" % (text(member), member_value(member, value)))
    return "{" + ", ".join(parts) + "}"


def member_value(member, value):
    if member == "symbol":
        return inline(value, SYMBOL_ORDER, SYMBOL_DEFAULTS)
    if member == "text":
        return inline(value, TEXT_RULE_ORDER, TEXT_RULE_DEFAULTS)
    if member in ("pipe", "vertexPipe", "segmentPipe"):
        return inline(value, PIPE_ORDER, PIPE_DEFAULTS)
    if member in ("attributes", "vertexAttributes", "segmentAttributes"):
        return "[" + ", ".join(inline(a, ATTRIBUTE_ORDER, ATTRIBUTE_DEFAULTS, always=("type", "name"))
                               for a in value) + "]"
    return scalar(value)


def rule_line(rule):
    return inline(rule, RULE_ORDER, RULE_DEFAULTS, always=("key", "sets"))


def stroke_line(stroke):
    kind = stroke[0]
    if kind in ("move", "draw"):
        return '["%s", %s, %s]' % (kind, number(stroke[1]), number(stroke[2]))
    if kind == "arc":
        return '["arc", %s, %s, %s]' % (number(stroke[1]), number(stroke[2]), number(stroke[3]))
    if kind in ("circle", "dot"):
        return '["%s", %s]' % (kind, number(stroke[1]))
    if kind == "text":
        body = stroke[1]
        parts = []
        for member in STROKE_TEXT_ORDER:
            if member not in body:
                continue
            value = body[member]
            if member == "extra":
                if all(v == 0 for v in value):
                    continue
                parts.append('"extra": [%s]' % ", ".join(number(v) for v in value))
                continue
            if member in STROKE_TEXT_DEFAULTS and value == STROKE_TEXT_DEFAULTS[member]:
                continue
            parts.append("%s: %s" % (text(member), scalar(value)))
        return '["text", {%s}]' % ", ".join(parts)
    if kind == "pen":
        return '["pen", %s]' % text(stroke[1])
    raise ValueError("unknown stroke kind %r" % (kind,))


def definition_block(definition, symbol):
    """One definition: its members on one line up to `"strokes": [`, a stroke a line, then `]}`."""
    head = ['"name": %s' % text(definition["name"])]
    if definition.get("group"):
        head.append('"group": %s' % text(definition["group"]))
    if definition.get("units", "world") != "world":
        head.append('"units": %s' % text(definition["units"]))
    if symbol and definition.get("atVertices"):
        head.append('"atVertices": true')
    length = definition.get("length", 0)
    if length != 0:
        head.append('"length": %s' % number(length))
    strokes = definition.get("strokes", [])
    if not strokes:
        return "    {" + ", ".join(head) + "}"
    lines = ["    {" + ", ".join(head) + ', "strokes": [']
    lines.append(",\n".join("      " + stroke_line(s) for s in strokes))
    lines.append("    ]}")
    return "\n".join(lines)


def fold(name):
    """Colour names compare folded: lower case, `_` and `-` as blank, `gray` as `grey`."""
    return name.lower().replace("_", " ").replace("-", " ").replace("gray", "grey")


def customisation_json(data):
    """The text of the file, exactly as `customisationToJson` writes it."""
    out = ["{", '  "format": "katana-customisation",', '  "version": 1,', '  "name": %s,' % text(NAME),
           '  "description": %s,' % text(data.description())]
    out.append('  "notice": [\n' + ",\n".join("    " + text(line) for line in data.NOTICE) + "\n  ],")
    colours = sorted(data.colours.COLOURS.items(), key=lambda item: fold(item[0]))
    out.append('  "colours": {\n' + ",\n".join("    %s: %s" % (text(n), text(v)) for n, v in colours) + "\n  },")
    # Definitions are in byte order of the name, each kind in its own list.
    out.append('  "linestyles": [\n'
               + ",\n".join(definition_block(d, False) for d in sorted(data.linestyles, key=lambda d: d["name"]))
               + "\n  ],")
    out.append('  "symbols": [\n'
               + ",\n".join(definition_block(d, True) for d in sorted(data.symbols, key=lambda d: d["name"]))
               + "\n  ],")
    out.append('  "codes": [\n' + ",\n".join("    " + rule_line(r) for r in data.rules) + "\n  ]")
    out.append("}")
    return "\n".join(out) + "\n"


# ---- the checks and the command line -------------------------------------------------------------------------

def read_bytes(path):
    try:
        with open(path, "rb") as handle:
            return handle.read()
    except OSError:
        return None


def write_bytes(path, payload):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as handle:
        handle.write(payload)


def round_trip(path, cli, scratch):
    """Load `path` in katana_cli and write it back, as a part, with CUSTOMISE EXPORT: the bytes must be the
    file's own. The one thing a session adds that the file has no member for is its record of what it was
    loaded from, `"sources"` (three lines: the name and what it brought); it is taken out before comparing.
    The same property without the session is tested on the value itself in test_katana_standard.cpp."""
    os.makedirs(scratch, exist_ok=True)
    out = os.path.join(scratch, "round_trip.customisation.json")
    if os.path.exists(out):
        os.remove(out)
    env = dict(os.environ, KATANA_BUILTIN_CUSTOMISATION="none")
    env.pop("KATANA_CUSTOMISATION", None)
    result = subprocess.run([cli, "-c", 'CUSTOMISE "%s"' % path, "-c", 'CUSTOMISE EXPORT "%s" CODES LINESTYLES SYMBOLS' % out],
                            env=env, capture_output=True, text=True)
    if result.returncode != 0:
        return "katana_cli failed (exit %d): %s" % (result.returncode, (result.stderr or result.stdout).strip())
    written = read_bytes(out)
    original = read_bytes(path)
    if written is None or original is None:
        return "no file was written"
    written = re.sub(rb'  "sources": \[\n    \{[^\n]*\}\n  \],\n', b"", written, count=1)
    if written != original:
        at = next((i for i, (a, b) in enumerate(zip(written, original)) if a != b), min(len(written), len(original)))
        return "the writer's bytes differ from the file's from byte %d" % at
    return None


def stats(data):
    return {"linestyles": len(data.linestyles), "symbols": len(data.symbols),
            "definitions": len(data.linestyles) + len(data.symbols),
            "groups": len({d["group"] for d in data.linestyles + data.symbols}),
            "codes": len(data.codes), "rules": len(data.rules), "colours": len(data.colours.COLOURS),
            "layers": len({c["layer"] for c in data.codes}), "classes": data.classes}


def main(argv):
    data = Data()
    wanted = set(a for a in argv if a.startswith("--"))
    if not wanted:
        print(__doc__)
        return 0
    status = 0
    if "--stats" in wanted:
        for name, value in stats(data).items():
            print("%s=%d" % (name, value))
    if "--json" in wanted:
        payload = customisation_json(data).encode("utf-8")
        write_bytes(JSON_PATH, payload)
        print("wrote %s (%d bytes)" % (os.path.relpath(JSON_PATH, ROOT), len(payload)))
    if "--docs" in wanted:
        docs = _module("catalogue")
        payload = docs.catalogue(data, ROOT).encode("utf-8")
        write_bytes(DOCS_PATH, payload)
        print("wrote %s (%d bytes)" % (os.path.relpath(DOCS_PATH, ROOT), len(payload)))
    if "--images" in wanted:
        status |= subprocess.call([sys.executable, os.path.join(HERE, "render_images.py")] + argv[argv.index("--images") + 1:])
    if "--check" in wanted:
        expected = {JSON_PATH: customisation_json(data).encode("utf-8")}
        if os.path.exists(os.path.join(HERE, "catalogue.py")):
            expected[DOCS_PATH] = _module("catalogue").catalogue(data, ROOT).encode("utf-8")
        for path, payload in expected.items():
            found = read_bytes(path)
            if found is not None and path == DOCS_PATH:
                # A text document: a Windows checkout (core.autocrlf) has CRLF line ends the generator never wrote.
                # The JSON is exact bytes (.gitattributes: -text), so only the document is read this way.
                found = found.replace(b"\r\n", b"\n")
            if found != payload:
                print("STALE: %s differs from what the data makes; run this script with --json --docs" %
                      os.path.relpath(path, ROOT))
                status = 1
            else:
                print("current: %s" % os.path.relpath(path, ROOT))
        if "--round-trip" in argv:
            cli = argv[argv.index("--round-trip") + 1]
            scratch = os.path.join(os.environ.get("TEMP", "/tmp"), "katana_standard_round_trip")
            problem = round_trip(JSON_PATH, cli, scratch)
            print("round trip: " + (problem or "identical"))
            status |= 1 if problem else 0
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
