#!/usr/bin/env python3
"""What do two Katana customisation files share besides survey code keys? Counts, never names.

    python tools/katana_standard/originality.py REFERENCE.json CANDIDATE.json [--limit N]

Katana Standard reuses the survey code KEYS of another library, on the owner's decision, so that a field file
coded for either draws in both (docs/katana_standard.md, "What is shared and what is not"). Everything else in it
is meant to be Katana's own, and this script is the scan that holds that to account. It reads both files with the
standard `json` module, shares no code with Katana, and compares what a person could copy:

  definition names    a linestyle or symbol name EQUAL to one of the reference's: the same letters and digits in
                      the same order, whatever the case, the blanks and the punctuation (`Flag Pole`, `flag-pole`
                      and `Flagpole` are one name). A name is compared three ways, each counted apart: with the
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
  stroke lists        a definition whose strokes equal one of the reference's: exactly; after moving the first
                      point to the origin (translated); or after also scaling to a unit box (scaled). A list of
                      five strokes or fewer (a dash, a plus, a square) is left out: any library draws those
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
import re
import sys


TRIVIAL = 5   # strokes: a dash, a plus, a square or a ring, which any two libraries draw
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
    return flat([[s[0], s[1] - first[0], s[2] - first[1]] if s[0] in ("move", "draw") else s for s in strokes])


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

    def real(document):
        return [d for d in definitions(document) if len(strokes_of(d)) > TRIVIAL]

    ref_flat = {flat(strokes_of(d)) for d in real(reference)}
    ref_trans = {translated(strokes_of(d)) for d in real(reference)}
    ref_scaled = {scaled(strokes_of(d)) for d in real(reference)}
    exact["stroke lists, exact"] = [d["name"] for d in real(candidate) if flat(strokes_of(d)) in ref_flat]
    exact["stroke lists, translated"] = [d["name"] for d in real(candidate) if translated(strokes_of(d)) in ref_trans]
    exact["stroke lists, scaled"] = [d["name"] for d in real(candidate) if scaled(strokes_of(d)) in ref_scaled]

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
                            len(definitions(candidate)) - len(real(candidate)))}


def render(result, limit):
    out = ["reference definitions=%d candidate definitions=%d (%d of the candidate's have %d strokes or fewer and "
           "are left out of the stroke comparison)" % (result["definitions"] + (TRIVIAL,)),
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
