#!/usr/bin/env python3
"""An independent census of the Katana Standard customisation file.

    python tools/katana_standard/census.py [FILE]          key=value lines
    python tools/katana_standard/census.py [FILE] --json   the same as one JSON object

It reads the committed file with the standard `json` module and imports nothing of Katana's code and nothing of
the data modules (`codes.py`, `linestyles.py`, `symbols.py`) that made it, so its figures are a second opinion:
the figures `tests/cad/customisation/test_katana_standard.cpp` pins were counted here, by a program that shares
no code with the one under test. `tools/customisation_census.py` counts the same file for the generic figures
(definitions, strokes, rules, keys) and gives the same numbers; this one adds what only Katana Standard promises:
the twelve classes, the three-level layers, the weights, the utility words.

With FILE absent it reads resources/customisation/katana-standard.customisation.json beside the tools folder.
"""
import collections
import json
import os
import re
import sys

DEFAULT = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
                       "resources", "customisation", "katana-standard.customisation.json")

# The words CLAUDE.md section 9 keeps out of new text, assembled from pieces so that this file does not hold them.
FORBIDDEN = re.compile("|".join(["1" + "2d", "ex" + "ds", "tf" + "nsw", "transport for n" + "sw", "n" + "sw"]), re.I)


def decimals(value):
    """Digits after the decimal point of a number as JSON wrote it (repr: the shortest text of the double)."""
    text = repr(float(value))
    return len(text.split(".")[1]) if "." in text and "e" not in text and not text.endswith(".0") else 0


def census(document, raw_text):
    linestyles = document.get("linestyles", [])
    symbols = document.get("symbols", [])
    definitions = linestyles + symbols
    rules = document.get("codes", [])
    by_sets = collections.Counter(r["sets"] for r in rules)
    keys = [r["key"] for r in rules]
    distinct = sorted(set(keys))
    features = [r for r in rules if r["sets"] == "feature"]
    stroke_kinds = collections.Counter(s[0] for d in definitions for s in d.get("strokes", []))
    classes = collections.Counter(k[0] for k in distinct)
    kinds = collections.Counter()
    symbol_keys = {r["key"] for r in rules if r["sets"] == "symbol"}
    text_keys = {r["key"] for r in rules if r["sets"] == "text"}
    for f in features:
        drawn_line = f.get("draw") == "line"
        has_symbol = f["key"] in symbol_keys
        kinds["text" if f["key"] in text_keys else "both" if drawn_line and has_symbol else
              "line" if drawn_line else "point"] += 1
    used = set()
    for r in rules:
        if r.get("linestyle"):
            used.add(r["linestyle"])
        if r.get("symbol", {}).get("name"):
            used.add(r["symbol"]["name"])
    names = {d["name"] for d in definitions}
    colour_table = document.get("colours", {})
    colours_used = set()
    for r in rules:
        for c in (r.get("colour"), r.get("symbol", {}).get("colour"), r.get("text", {}).get("colour")):
            if c:
                colours_used.add(c)
    layers = sorted({f["layer"] for f in features})
    attribute_lists = [r["attributes"] for r in rules if r["sets"] == "attributes"]
    attributes = [a for lst in attribute_lists for a in lst]
    surface_true = sorted(r["key"][:-1] for r in rules if r["sets"] == "surface" and r["surface"] is True)
    numbers = []

    def walk(value):
        if isinstance(value, bool):
            return
        if isinstance(value, (int, float)):
            numbers.append(value)
        elif isinstance(value, list):
            for v in value:
                walk(v)
        elif isinstance(value, dict):
            for v in value.values():
                walk(v)
    for d in definitions:
        walk(d.get("strokes", []))
        walk(d.get("length", 0))
    return {
        "name": document["name"],
        "definitions": len(definitions),
        "linestyles": len(linestyles),
        "symbols": len(symbols),
        "symbolsAtVertices": sum(1 for d in symbols if d.get("atVertices")),
        "linestylesAtVertices": sum(1 for d in linestyles if d.get("atVertices")),
        "definitionGroups": len({d["group"] for d in definitions}),
        "ruleGroups": len({f["group"] for f in features}),
        "strokes": sum(stroke_kinds.values()),
        "strokeKinds": dict(sorted(stroke_kinds.items())),
        "maxStrokesLinestyle": max(len(d["strokes"]) for d in linestyles),
        "maxStrokesSymbol": max(len(d["strokes"]) for d in symbols),
        "unitsPaper": sum(1 for d in definitions if d.get("units") == "paper"),
        "maxDecimals": max(decimals(n) for n in numbers),
        "colours": len(colour_table),
        "coloursUnused": len(set(colour_table) - colours_used),
        "coloursUndefined": len(colours_used - set(colour_table)),
        "rules": len(rules),
        "rulesBySets": dict(sorted(by_sets.items())),
        "keys": len(distinct),
        "keysOfThreeCapitals": sum(1 for k in rules if re.fullmatch(r"[A-Z]{3}\*", k["key"])),
        "keysRepeatedInASection": sum(v - 1 for v in collections.Counter((r["key"], r["sets"]) for r in rules).values()),
        "classes": dict(sorted(classes.items())),
        "kinds": dict(sorted(kinds.items())),
        "layers": len(layers),
        "layerDepths": sorted({len(layer.split("/")) for layer in layers}),
        "weights": dict(sorted(collections.Counter(f["weight"] for f in features).items())),
        "hideRules": sum(1 for r in rules if r.get("hide") is True),
        "surfaceTrue": len(surface_true),
        "surfaceTrueKeys": surface_true,
        "pipeRules": by_sets["pipe"],
        "attributeRules": by_sets["attributes"],
        "attributesPrompted": sum(1 for a in attributes if not a.get("value")),
        "attributesApplied": sum(1 for a in attributes if a.get("value")),
        "referencedNames": len(used),
        "definitionsUnused": len(names - used),
        "namesUndefined": sorted(used - names),
        "linework": "linework" in document,
        "automation": "automation" in document,
        "sources": "sources" in document,
        "forbiddenWordHits": len(FORBIDDEN.findall(raw_text)),
    }


def main(argv):
    paths = [a for a in argv if not a.startswith("--")]
    path = paths[0] if paths else DEFAULT
    with open(path, "rb") as handle:
        raw = handle.read().decode("utf-8")
    figures = census(json.loads(raw), raw)
    if "--json" in argv:
        print(json.dumps(figures, indent=1, sort_keys=True))
        return 0
    for name, value in figures.items():
        if name == "surfaceTrueKeys":
            value = " ".join(value)
        elif isinstance(value, dict):
            value = " ".join("%s:%s" % item for item in value.items())
        elif isinstance(value, list):
            value = " ".join(str(v) for v in value)
        print("%s=%s" % (name, value))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
