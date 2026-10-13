#!/usr/bin/env python3
"""What do two Katana customisation files share besides survey code keys? Counts, never names.

    python tools/katana_standard/originality.py REFERENCE.json CANDIDATE.json [--limit N]

Katana Standard reuses the survey code KEYS of another library, on the owner's decision, so that a field file
coded for either draws in both (docs/katana_standard.md, "What is shared and what is not"). Everything else in it
is meant to be Katana's own, and this script is the scan that holds that to account. It reads both files with the
standard `json` module, shares no code with Katana, and compares what a person could copy:

  definition names    a linestyle or symbol name equal after folding case, blanks and punctuation
  group paths         a definition's group, and a rule's group, equal after folding
  layer paths         a rule's layer
  legend labels       a rule's comment, with the reference's own "[KEY]" and "E XXXX" lead-ins taken off
  colour names        a name in the colour table; and a colour VALUE equal to one of the reference's (pure white is
                      the one value AS 5488 gives communications, so it is counted apart)
  stroke lists        a definition whose strokes equal one of the reference's: exactly; after moving the first
                      point to the origin (translated); or after also scaling to a unit box (scaled). A list of
                      five strokes or fewer (a dash, a plus, a square) is left out: any library draws those
  near names          a definition name or a legend label at least 0.9 alike (difflib), listed to --limit

The exit status is 0 when every exact count is 0 and 1 otherwise. The reference's own words appear in no output:
only the candidate's, and only to say which of ITS names to look at.
"""
import difflib
import json
import re
import sys


TRIVIAL = 5   # strokes; see main()


def load(path):
    with open(path, encoding="utf-8-sig") as handle:
        return json.load(handle)


def fold(text):
    return re.sub(r"[\s_\-/]+", " ", text.strip().lower())


def clean_label(text):
    text = re.sub(r"^\[[^\]]*\]\s*", "", text.strip())
    text = re.sub(r"^[A-Z]\s+[A-Z]{2,5}\s+", "", text)
    return fold(text)


def strokes_of(definition):
    return [list(s) if not isinstance(s, dict) else s for s in definition.get("strokes", [])]


def flat(strokes):
    """(kind, numbers...) tuples with every number rounded to 3 decimals; text strokes by their text and height."""
    out = []
    for s in strokes:
        kind = s[0]
        if kind == "text":
            body = s[1]
            out.append((kind, body.get("text", ""), round(float(body.get("height", 0)), 3)))
        elif kind == "pen":
            out.append((kind, s[1]))
        else:
            out.append((kind,) + tuple(round(float(v), 3) for v in s[1:]))
    return tuple(out)


def translated(strokes):
    """The strokes with the first move or draw point at the origin."""
    first = next(((s[1], s[2]) for s in strokes if s[0] in ("move", "draw")), None)
    if first is None:
        return flat(strokes)
    shifted = []
    for s in strokes:
        if s[0] in ("move", "draw"):
            shifted.append([s[0], s[1] - first[0], s[2] - first[1]])
        elif s[0] == "text":
            shifted.append(s)
        else:
            shifted.append(s)
    return flat(shifted)


def scaled(strokes):
    """Translated, then scaled so the larger extent of the move/draw points is 1."""
    pts = [(s[1], s[2]) for s in strokes if s[0] in ("move", "draw")]
    if not pts:
        return flat(strokes)
    extent = max(max(p[0] for p in pts) - min(p[0] for p in pts), max(p[1] for p in pts) - min(p[1] for p in pts))
    if extent == 0:
        return flat(strokes)
    first = pts[0]
    out = []
    for s in strokes:
        if s[0] in ("move", "draw"):
            out.append([s[0], (s[1] - first[0]) / extent, (s[2] - first[1]) / extent])
        elif s[0] in ("circle", "dot"):
            out.append([s[0], s[1] / extent])
        elif s[0] == "arc":
            out.append([s[0], s[1] / extent, s[2], s[3]])
        else:
            out.append(s)
    return tuple(tuple(round(v, 2) if isinstance(v, float) else v for v in t) for t in flat(out))


def main(argv):
    paths = [a for a in argv if not a.startswith("--")]
    limit = int(argv[argv.index("--limit") + 1]) if "--limit" in argv else 10
    if "--limit" in argv:
        paths = [p for p in paths if p != argv[argv.index("--limit") + 1]]
    reference, candidate = load(paths[0]), load(paths[1])
    exact = {}

    def defs(doc):
        return doc.get("linestyles", []) + doc.get("symbols", [])

    ref_names = {fold(d["name"]) for d in defs(reference)}
    exact["definition names"] = [d["name"] for d in defs(candidate) if fold(d["name"]) in ref_names]

    ref_groups = {fold(d.get("group", "")) for d in defs(reference)} | {fold(r.get("group", "")) for r in reference["codes"]}
    cand_groups = {d.get("group", "") for d in defs(candidate)} | {r.get("group", "") for r in candidate["codes"]}
    exact["group paths"] = sorted(g for g in cand_groups if g and fold(g) in ref_groups)

    ref_layers = {fold(r["layer"]) for r in reference["codes"] if r.get("layer")}
    cand_layers = {r["layer"] for r in candidate["codes"] if r.get("layer")}
    exact["layer paths"] = sorted(l for l in cand_layers if fold(l) in ref_layers)

    ref_labels = {clean_label(r["comment"]) for r in reference["codes"] if r.get("comment")}
    cand_labels = {r["comment"] for r in candidate["codes"] if r.get("comment")}
    exact["legend labels"] = sorted(c for c in cand_labels if fold(c) in ref_labels)

    ref_colour_names = {fold(n) for n in reference.get("colours", {})}
    exact["colour names"] = sorted(n for n in candidate.get("colours", {}) if fold(n) in ref_colour_names)

    ref_values = {v.lower() for v in reference.get("colours", {}).values()}
    white = "#ffffff"
    shared_values = [n for n, v in candidate.get("colours", {}).items() if v.lower() in ref_values and v.lower() != white]
    exact["colour values (not white)"] = shared_values

    # A list of TRIVIAL length is a dash, a plus, a square or a ring: any two libraries draw those, and a match is
    # no copy. Only lists of more than TRIVIAL strokes are compared.
    def real(doc):
        return [d for d in defs(doc) if len(strokes_of(d)) > TRIVIAL]

    ref_flat = {flat(strokes_of(d)) for d in real(reference)}
    ref_trans = {translated(strokes_of(d)) for d in real(reference)}
    ref_scaled = {scaled(strokes_of(d)) for d in real(reference)}
    exact["stroke lists, exact"] = [d["name"] for d in real(candidate) if flat(strokes_of(d)) in ref_flat]
    exact["stroke lists, translated"] = [d["name"] for d in real(candidate) if translated(strokes_of(d)) in ref_trans]
    exact["stroke lists, scaled"] = [d["name"] for d in real(candidate) if scaled(strokes_of(d)) in ref_scaled]
    trivial = len(defs(candidate)) - len(real(candidate))

    near = []
    ref_name_list = sorted(ref_names)
    for d in defs(candidate):
        match = difflib.get_close_matches(fold(d["name"]), ref_name_list, n=1, cutoff=0.9)
        if match:
            near.append(d["name"])
    ref_label_list = sorted(ref_labels)
    near_labels = []
    for c in sorted(cand_labels):
        match = difflib.get_close_matches(fold(c), ref_label_list, n=1, cutoff=0.9)
        if match and fold(c) not in ref_labels:
            near_labels.append(c)

    shared_keys = {r["key"] for r in reference["codes"]} & {r["key"] for r in candidate["codes"]}
    out = ["reference definitions=%d candidate definitions=%d (%d of the candidate's have %d strokes or fewer and "
           "are left out of the stroke comparison)" % (len(defs(reference)), len(defs(candidate)), trivial, TRIVIAL),
           "survey code keys in common=%d (reference %d, candidate %d)" % (
               len(shared_keys), len({r["key"] for r in reference["codes"]}), len({r["key"] for r in candidate["codes"]}))]
    failing = 0
    for what, items in exact.items():
        out.append("%s in common=%d" % (what, len(items)))
        failing += len(items)
        for item in items[:limit]:
            out.append("  " + item)
    out.append("near definition names (>=0.9)=%d" % len(near))
    for item in near[:limit]:
        out.append("  " + item)
    out.append("near legend labels (>=0.9)=%d" % len(near_labels))
    for item in near_labels[:limit]:
        out.append("  " + item)
    out.append("originality=%s" % ("PASS" if failing == 0 else "FAIL"))
    print("\n".join(out))
    return 0 if failing == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
