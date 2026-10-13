#!/usr/bin/env python3
"""Do two Katana customisation files hold the same survey codes, rule for rule in structure?

    python tools/katana_standard/parity.py REFERENCE.json CANDIDATE.json [options]

Katana Standard is held to carry the survey codes of another library: the same key strings, and for every key
the same BEHAVIOUR - the same ordered kinds of rule (feature, symbol, text, pipe, vertexPipe, segmentPipe,
attributes, vertexAttributes, surface), the same draw mode, the same hide and surface flags - so that one field
file makes the same kinds of entity (points, lines, text, pipes, surface members) in either. What it LOOKS like
(definitions, layers, colours, weights, comments, text styles) is each library's own and is not compared here.
This script is that comparison. It uses only the Python standard library and none of Katana's code, and the
files are named on the command line, so no definition, group, layer or comment of either library is in this
source. The reason words of the exceptions below are the one place a library is named: the exceptions file is
keyed by them. Tests: tools/katana_standard/test_parity.py.

WHAT IT REPORTS, in the order it is printed
  1. KEYS      keys the reference has and the candidate lacks, and the reverse. Byte for byte: `AB*`, `AB`,
               `AB1*` and `*` are four keys.
  2. STRUCTURE for each key held by both, the candidate's rules against the reference's EFFECTIVE rules. The
               effective rule of one kind (`sets`) under a key is what a lookup makes of the key's rules of that
               kind: each field comes from the first rule that says it (`SurveyMap::lookup`), so a later rule is
               a duplicate or shadowed in CODE CHECK's words unless it says a field the earlier ones leave
               unsaid, and then the effective rule carries that field (the reference's second file hides the
               symbol of four keys that its first file left unsaid). Compared per rule, in order:
                 feature    draw (line, point or none), that a layer is given, the state of the colour name
                 symbol     that a symbol is named, hide (absent reads as false)
                 text       units, size, justifyX, justifyY, the state of the colour name
                 pipe, vertexPipe, segmentPipe   justify, shape, active, whether size1 and size2 are given
                 attributes, vertexAttributes    each entry's type and what its value is (a prompt, deferred,
                                                 the code itself, a number as written, or some text)
                 surface    the flag
               A pipe rule's own attribute list is not compared: it belongs to the reference's wording.
               Attribute rules the candidate has under a key where the reference has none (a library may ask
               for more than the reference does; they make no entity) are EXTRAS: counted, allowed, and
               refused by --no-extra-attributes.
  3. EXCEPTIONS the reference is not clean: it repeats and shadows rules, and names colours that nothing
               resolves. A candidate that keeps ONE clean rule set per key leaves those out, so its raw rule
               list differs from the reference's. Each such key needs an entry in the exceptions file
               (`parity_exceptions.json`, a list of {"key", "reason", optional "count"}), with one of
                 nsw-duplicate-collapsed      a rule the same as an earlier one, field for field, was left out
                 nsw-shadowed-collapsed       a rule that adds nothing to an earlier one of its key was left out
                 nsw-unknown-colour-replaced  an effective rule named a colour that resolves to nothing, and the
                                              candidate names one that does
                 nsw-linestyle-symbol-resolved  an effective feature rule named a vertex definition (a symbol)
                                              as its LINESTYLE, which draws the symbol at each point but is a
                                              fault CODE CHECK reports; the candidate says it properly, with a
                                              symbol rule after the feature rule
                 nsw-fields-merged            a later rule of a kind said a field the first left unsaid and so
                                              changed the effective structure (a symbol's hide); the candidate's
                                              one rule of that kind carries the field
               An entry that explains nothing is itself an error, so the list cannot go stale.
  4. DEFECTS   in the CANDIDATE, by the notions of CODE CHECK (src/katana_cad/customisation/code_table.cpp): a
               duplicate rule, a shadowed rule, an unknown colour, a linestyle or symbol nothing defines, a
               linestyle that is a symbol, a symbol that is a linestyle, a field outside its section, a bad or
               missing layer; and a second rule of one kind under one key (only the first applies to the field
               it sets). The candidate is to have none. --allow-candidate-defects skips this part, which is how a
               file is compared with itself.

EXIT STATUS   0 parity holds; 1 it does not (every difference is unexplained until the exceptions file says
              otherwise); 2 a file cannot be read.

OPTIONS       --exceptions FILE         default: parity_exceptions.json beside this script ('none' for no file)
              --allow-candidate-defects part 4 is reported and does not fail the run
              --no-extra-attributes     extras are differences
              --suggest-exceptions      print the exceptions file this comparison needs, as JSON, and exit 0
              --limit N                 lines per section (default 12, 0 for all)
              --json                    one JSON object instead of key=value lines
"""
import argparse
import collections
import json
import os
import sys

DEFAULT_EXCEPTIONS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "parity_exceptions.json")

DUPLICATE = "nsw-duplicate-collapsed"
SHADOWED = "nsw-shadowed-collapsed"
COLOUR = "nsw-unknown-colour-replaced"
LINESTYLE_SYMBOL = "nsw-linestyle-symbol-resolved"
MERGED = "nsw-fields-merged"
REASONS = (DUPLICATE, SHADOWED, COLOUR, LINESTYLE_SYMBOL, MERGED)

SETS = ("feature", "symbol", "text", "pipe", "vertexPipe", "segmentPipe", "attributes", "vertexAttributes",
        "surface")

# The 27 standard colour names Katana resolves without a table (src/katana_entity/colour_names.cpp).
STANDARD_COLOURS = frozenset([
    "red", "green", "blue", "yellow", "cyan", "magenta", "white", "black", "grey", "orange", "brown", "purple",
    "pink", "violet", "dark red", "dark green", "dark blue", "dark cyan", "dark magenta", "dark orange",
    "dark grey", "light grey", "light blue", "light green", "light cyan", "light yellow", "light pink"])

# Plain linestyle words, folded: a rule naming one needs no definition (isPlainLinestyle).
PLAIN_LINESTYLES = frozenset(["", "0", "1", "continuous"])

# The members a rule of each kind uses; any other member is a "field outside section" (fieldsOutsideSection).
# `key`, `sets` and `comment` may sit on any rule.
COMMON_MEMBERS = frozenset(["key", "sets", "comment"])
SECTION_MEMBERS = {
    "feature": frozenset(["layer", "colour", "linestyle", "weight", "group", "draw"]),
    "symbol": frozenset(["symbol", "hide"]),
    "text": frozenset(["text"]),
    "pipe": frozenset(["pipe", "attributes"]),
    "vertexPipe": frozenset(["vertexPipe", "vertexAttributes"]),
    "segmentPipe": frozenset(["segmentPipe", "segmentAttributes"]),
    "attributes": frozenset(["attributes"]),
    "vertexAttributes": frozenset(["vertexAttributes"]),
    "surface": frozenset(["surface"]),
}
ATTRIBUTE_SCOPES = ("attributes", "vertexAttributes", "segmentAttributes")


class ParityError(Exception):
    """A file that cannot be compared (unreadable, not a customisation, a malformed exceptions file)."""


# ---- reading --------------------------------------------------------------------------------------------------

def load_document(path):
    try:
        with open(path, encoding="utf-8-sig") as file:
            document = json.load(file)
    except (OSError, ValueError) as error:
        raise ParityError("cannot read %s: %s" % (path, error))
    if not isinstance(document, dict) or document.get("format") != "katana-customisation":
        raise ParityError("%s is not a Katana customisation file" % path)
    rules = document.get("codes", [])
    if not isinstance(rules, list) or not all(isinstance(r, dict) and isinstance(r.get("key"), str)
                                              and r.get("sets") in SETS for r in rules):
        raise ParityError("%s: every entry of `codes` needs a text `key` and a `sets` that is one of the nine "
                          "words" % path)
    return document


def ascii_lower(text):
    return "".join(c.lower() if c < "\x80" else c for c in text)


def fold_colour(name):
    """The form a colour name is compared in (foldColourName): ASCII lower case, `_` and `-` as blanks, trimmed,
    and `gray` as `grey` when it is the whole name or its last word."""
    folded = ascii_lower(name).replace("_", " ").replace("-", " ").strip(" \t\n\r\f\v")
    if folded == "gray":
        return "grey"
    if folded.endswith(" gray"):
        return folded[:-4] + "grey"
    return folded


class Colours:
    """Which colour names a document resolves: its own table, then the 27 standard names."""

    def __init__(self, document):
        table = document.get("colours", {})
        self.table = {fold_colour(name) for name in table} if isinstance(table, dict) else set()

    def state(self, name):
        if not name:
            return "absent"
        folded = fold_colour(name)
        return "known" if folded in self.table or folded in STANDARD_COLOURS else "unknown"


# ---- the notions of CODE CHECK ---------------------------------------------------------------------------------

def fields_set(rule):
    """What a lookup would take from this rule: the fields it says something about (FieldSpec::sets)."""
    said = {name for name in ("layer", "colour", "linestyle", "weight", "group", "comment") if rule.get(name)}
    said.update(name for name in ("draw", "surface", "hide", "symbol", "text", "pipe", "vertexPipe",
                                  "segmentPipe") if name in rule)
    return said


def attributes_named(rule):
    return {(scope, a.get("name")) for scope in ATTRIBUTE_SCOPES for a in rule.get(scope, [])
            if isinstance(a, dict)}


def by_key(rules):
    table = collections.OrderedDict()
    for index, rule in enumerate(rules):
        table.setdefault(rule["key"], []).append(index)
    return table


def classify(rules):
    """'ok', 'duplicate' or 'shadowed' for every rule, as lintSurveyMap decides it. Only the earlier rules of the
    SAME key count: a longer prefix catches only some of the codes a shorter one does, so it shadows nothing."""
    status = ["ok"] * len(rules)
    for indices in by_key(rules).values():
        before = []
        for index in indices:
            rule = rules[index]
            if any(rules[j] == rule for j in before):
                status[index] = "duplicate"
            elif before:
                said = fields_set(rule)
                contributes = any(not any(f in fields_set(rules[j]) for j in before) for f in said)
                contributes = contributes or any(
                    not any(a in attributes_named(rules[j]) for j in before) for a in attributes_named(rule))
                if not contributes:
                    status[index] = "shadowed"
            before.append(index)
    return status


def first_of_each_kind(rules, indices):
    """The rules of one key a lookup can use: the first of each `sets`, in the order they first appear."""
    seen = set()
    kept = []
    for index in indices:
        if rules[index]["sets"] not in seen:
            seen.add(rules[index]["sets"])
            kept.append(index)
    return kept


def merged_rule(rules, indices):
    """The one rule a lookup makes of `indices`, rules of one kind under one key, in order: each field from the
    first rule that says it, attribute lists accumulating by name (`SurveyMap::lookup`). A field a rule holds
    at its default (an empty text) says nothing; `hide: false` does say something."""
    out = {}
    for index in indices:
        for name, value in rules[index].items():
            if name in ATTRIBUTE_SCOPES:
                have = out.setdefault(name, [])
                known = {a.get("name") for a in have if isinstance(a, dict)}
                have.extend(a for a in value if not isinstance(a, dict) or a.get("name") not in known)
            elif name not in out and value not in ("", None):
                out[name] = value
    return out


def kinds_under(rules, indices):
    """[(kind, [indices of that kind])] in the order the kinds first appear under the key."""
    table = collections.OrderedDict()
    for index in indices:
        table.setdefault(rules[index]["sets"], []).append(index)
    return list(table.items())


def layer_problem(layer):
    if not layer:
        return "a feature rule with no layer"
    levels = layer.split("/")
    if any(level == "" or level != level.strip(" \t") for level in levels):
        return "layer %r is not a layer path (an empty level, or blanks around one)" % layer
    return None


def document_defects(document):
    """The faults CODE CHECK reports for a document, as (rule index, key, kind, message)."""
    rules = document.get("codes", [])
    colours = Colours(document)
    linestyles = {d["name"]: d for d in document.get("linestyles", []) if isinstance(d, dict)}
    symbols = {d["name"]: d for d in document.get("symbols", []) if isinstance(d, dict)}
    defects = []

    def add(index, kind, message):
        defects.append((index, rules[index]["key"], kind, message))

    status = classify(rules)
    for index, rule in enumerate(rules):
        if status[index] == "duplicate":
            add(index, "duplicate", "the same as an earlier rule of the key, field for field")
        elif status[index] == "shadowed":
            add(index, "shadowed", "an earlier rule of the key already sets everything it sets")
        word = rule.get("linestyle", "")
        if word and ascii_lower(word) not in PLAIN_LINESTYLES:
            if word in symbols and word not in linestyles:
                add(index, "linestyle-is-symbol", "linestyle %r is a symbol" % word)
            elif word in linestyles and linestyles[word].get("atVertices"):
                add(index, "linestyle-is-symbol", "linestyle %r is drawn at vertices" % word)
            elif word not in linestyles:
                add(index, "unresolved-linestyle", "linestyle %r is defined nowhere in the file" % word)
        name = (rule.get("symbol") or {}).get("name", "")
        if name:
            if name in symbols or (name in linestyles and linestyles[name].get("atVertices")):
                pass
            elif name in linestyles:
                add(index, "symbol-is-linestyle", "symbol %r is a linestyle, not a symbol" % name)
            else:
                add(index, "unresolved-symbol", "symbol %r is defined nowhere in the file" % name)
        for what, colour in (("colour", rule.get("colour", "")),
                             ("symbol colour", (rule.get("symbol") or {}).get("colour", "")),
                             ("text colour", (rule.get("text") or {}).get("colour", ""))):
            if colours.state(colour) == "unknown":
                add(index, "unknown-colour", "%s %r resolves to nothing" % (what, colour))
        if rule["sets"] == "feature":
            problem = layer_problem(rule.get("layer", ""))
            if problem:
                add(index, "layer", problem)
        elif "layer" in rule:
            problem = layer_problem(rule["layer"])
            if problem:
                add(index, "layer", problem)
        if rule["key"] != rule["key"].strip(" \t") or rule["key"] == "":
            add(index, "key-blanks", "the key has blanks around it, or is empty")
        outside = sorted(set(rule) - COMMON_MEMBERS - SECTION_MEMBERS[rule["sets"]])
        if outside:
            add(index, "field-outside-section", "a %s rule holds %s" % (rule["sets"], ", ".join(outside)))
    for indices in by_key(rules).values():
        seen = set()
        for index in indices:
            kind = rules[index]["sets"]
            if kind in seen and status[index] == "ok":
                add(index, "repeated-kind",
                    "a second %s rule under the key: only the first applies to the field it sets" % kind)
            seen.add(kind)
    return sorted(defects)


# ---- structure -------------------------------------------------------------------------------------------------

def value_kind(attribute, key):
    value = attribute.get("value", "")
    if value == "":
        return "prompt"
    if attribute.get("type") == "integer":
        return "number " + value
    if value.startswith("$"):
        return "deferred"
    if value == key.rstrip("*"):
        return "the code"
    return "text"


def structure(rule, colours):
    """What a rule makes happen, without how it looks. A dict of words, so a difference can be printed."""
    kind = rule["sets"]
    out = {"sets": kind}
    if kind == "feature":
        out["draw"] = rule.get("draw", "none")
        out["layer"] = "given" if rule.get("layer") else "absent"
        out["colour"] = colours.state(rule.get("colour", ""))
    elif kind == "symbol":
        symbol = rule.get("symbol")
        out["symbol"] = "absent" if symbol is None else ("named" if symbol.get("name") else "unnamed")
        out["hide"] = bool(rule.get("hide", False))
    elif kind == "text":
        text = rule.get("text")
        if text is None:
            out["text"] = "absent"
        else:
            for name in ("units", "size", "justifyX", "justifyY"):
                out[name] = text.get(name, "")
            out["colour"] = colours.state(text.get("colour", ""))
    elif kind in ("pipe", "vertexPipe", "segmentPipe"):
        pipe = rule.get(kind)
        if pipe is None:
            out[kind] = "absent"
        else:
            out["justify"] = pipe.get("justify", "")
            out["shape"] = pipe.get("shape", "")
            out["active"] = bool(pipe.get("active", False))
            out["size1"] = "given" if pipe.get("size1") else "absent"
            out["size2"] = "given" if pipe.get("size2") else "absent"
    elif kind in ("attributes", "vertexAttributes"):
        out["entries"] = [[a.get("type", ""), value_kind(a, rule["key"])] for a in rule.get(kind, [])]
    elif kind == "surface":
        out["surface"] = rule.get("surface", "absent")
    return out


def names_a_symbol(rule, document):
    """Whether a feature rule's linestyle is a definition drawn at vertices: the symbol of the point, said in
    the wrong place (CODE CHECK: "linestyle is a symbol")."""
    word = rule.get("linestyle", "")
    if not word or ascii_lower(word) in PLAIN_LINESTYLES:
        return False
    if any(d.get("name") == word for d in document.get("symbols", [])):
        return not any(d.get("name") == word for d in document.get("linestyles", []))
    return any(d.get("name") == word and d.get("atVertices") for d in document.get("linestyles", []))


def effective_structure(document, indices, colours):
    """The structure a clean rule set for one key of `document` has, in order, and whether a symbol was said
    through a linestyle (so the clean set holds a symbol rule the key's rules do not)."""
    rules = document["codes"]
    merged = [merged_rule(rules, of_kind) for _, of_kind in kinds_under(rules, indices)]
    expected = [structure(rule, colours) for rule in merged]
    if "symbol" not in {e["sets"] for e in expected}:
        for position, rule in enumerate(merged):
            if rule["sets"] == "feature" and names_a_symbol(rule, document):
                expected.insert(position + 1, {"sets": "symbol", "symbol": "named", "hide": False})
                return expected, True
    return expected, False


def differences(expected, got):
    """Why two lists of structures differ, as sentences; an empty list when they are the same. A difference in
    the colour state alone is returned apart, as ('colour', index), for the exceptions to explain."""
    if [e["sets"] for e in expected] != [g["sets"] for g in got]:
        return ["kinds of rule: expected [%s], found [%s]" % (", ".join(e["sets"] for e in expected),
                                                               ", ".join(g["sets"] for g in got))], []
    problems, colour_only = [], []
    for position, (e, g) in enumerate(zip(expected, got), 1):
        for member in e:
            if e[member] == g.get(member):
                continue
            if member == "colour" and e[member] == "unknown" and g.get(member) == "known":
                colour_only.append(position)
                continue
            problems.append("rule %d (%s) %s: expected %s, found %s" % (
                position, e["sets"], member, json.dumps(e[member]), json.dumps(g.get(member))))
    return problems, colour_only


# ---- exceptions ------------------------------------------------------------------------------------------------

def load_exceptions(path):
    """[(key, reason, count or None)]; refuses anything that is not a list of {"key", "reason", optional "count",
    optional "note"} naming a reason from REASONS, or that names one (key, reason) twice."""
    if path is None:
        return []
    try:
        with open(path, encoding="utf-8-sig") as file:
            data = json.load(file)
    except (OSError, ValueError) as error:
        raise ParityError("cannot read the exceptions file %s: %s" % (path, error))
    if not isinstance(data, list):
        raise ParityError("%s: the exceptions file is a JSON list" % path)
    out, seen = [], set()
    for number, entry in enumerate(data):
        where = "%s entry %d" % (path, number)
        if not isinstance(entry, dict) or set(entry) - {"key", "reason", "count", "note"}:
            raise ParityError("%s: an entry has the members key, reason and optionally count and note" % where)
        if not isinstance(entry.get("key"), str) or entry.get("reason") not in REASONS:
            raise ParityError("%s: `key` is text and `reason` is one of %s" % (where, ", ".join(REASONS)))
        count = entry.get("count")
        if count is not None and (not isinstance(count, int) or isinstance(count, bool) or count < 1):
            raise ParityError("%s: `count` is a whole number above 0" % where)
        if (entry["key"], entry["reason"]) in seen:
            raise ParityError("%s: %s is given twice for %r" % (where, entry["reason"], entry["key"]))
        seen.add((entry["key"], entry["reason"]))
        out.append((entry["key"], entry["reason"], count))
    return out


# ---- the comparison --------------------------------------------------------------------------------------------

def compare(reference, candidate, exceptions=(), allow_extras=True, allow_candidate_defects=False):
    """The whole comparison, as a dict of lists and counts. `ok` is True when parity holds."""
    ref_rules, cand_rules = reference.get("codes", []), candidate.get("codes", [])
    ref_colours, cand_colours = Colours(reference), Colours(candidate)
    ref_keys, cand_keys = by_key(ref_rules), by_key(cand_rules)
    ref_status = classify(ref_rules)
    # What a lookup can never use in the reference, key by key: every rule that is not the first of its kind.
    collapsible = {}
    for key, indices in ref_keys.items():
        kept = set(first_of_each_kind(ref_rules, indices))
        counts = collections.Counter()
        for _, of_kind in kinds_under(ref_rules, indices):
            for position, index in enumerate(of_kind):
                if index in kept:
                    continue
                if ref_status[index] == "duplicate":
                    counts[DUPLICATE] += 1
                    continue
                # Does this rule change what the key's rules of its kind make, once the ones before it are in?
                before = structure(merged_rule(ref_rules, of_kind[:position]), ref_colours)
                after = structure(merged_rule(ref_rules, of_kind[:position + 1]), ref_colours)
                counts[MERGED if before != after else SHADOWED] += 1
        collapsible[key] = counts

    report = {
        "missing_keys": [k for k in ref_keys if k not in cand_keys],
        "extra_keys": [k for k in cand_keys if k not in ref_keys],
        "structure": [],            # {key, problems}
        "required": {},             # (key, reason) -> count the comparison needs an exception for
        "undocumented": [],
        "count_mismatch": [],
        "unused_exceptions": [],
        "defects": [],
        "extras": {"rules": 0, "keys": 0},
        "identical_keys": 0,
        "equal_after_collapse": 0,
        "reference": {"keys": len(ref_keys), "rules": len(ref_rules),
                      "collapsible_duplicates": sum(c[DUPLICATE] for c in collapsible.values()),
                      "collapsible_shadowed": sum(c[SHADOWED] for c in collapsible.values()),
                      "keys_with_a_collapsible_rule": sum(1 for c in collapsible.values() if c)},
        "candidate": {"keys": len(cand_keys), "rules": len(cand_rules)},
    }

    for key, ref_indices in ref_keys.items():
        if key not in cand_keys:
            continue
        cand_indices = cand_keys[key]
        raw_ref = [structure(ref_rules[i], ref_colours) for i in ref_indices]
        raw_cand = [structure(cand_rules[i], cand_colours) for i in cand_indices]
        expected, said_through_linestyle = effective_structure(reference, ref_indices, ref_colours)
        # The very same rules, fault for fault, are no difference (a file compared with itself); the candidate's
        # faults are part 4. A symbol said through the linestyle is the one fault whose absence changes the
        # structure, so a candidate without the symbol rule is identical only if it says it the same way.
        if raw_ref == raw_cand and (not said_through_linestyle or any(
                cand_rules[i]["sets"] == "feature" and names_a_symbol(cand_rules[i], candidate)
                for i in cand_indices)):
            report["identical_keys"] += 1
            continue

        got_indices = list(cand_indices)
        if allow_extras:
            wants = {e["sets"] for e in expected}
            extras = [i for i in got_indices if cand_rules[i]["sets"] == "attributes" and "attributes" not in wants]
            got_indices = [i for i in got_indices if i not in extras]
            report["extras"]["rules"] += len(extras)
            report["extras"]["keys"] += 1 if extras else 0
        got = [structure(cand_rules[i], cand_colours) for i in got_indices]

        problems, colour_positions = differences(expected, got)
        if problems:
            report["structure"].append({"key": key, "problems": problems})
            continue
        report["equal_after_collapse"] += 1
        for reason, count in collapsible[key].items():
            report["required"][(key, reason)] = count
        if colour_positions:
            report["required"][(key, COLOUR)] = len(colour_positions)
        if said_through_linestyle:
            report["required"][(key, LINESTYLE_SYMBOL)] = 1

    documented = {(key, reason): count for key, reason, count in exceptions}
    used = set()
    for (key, reason), count in sorted(report["required"].items()):
        if (key, reason) not in documented:
            report["undocumented"].append({"key": key, "reason": reason, "count": count})
            continue
        used.add((key, reason))
        if documented[(key, reason)] is not None and documented[(key, reason)] != count:
            report["count_mismatch"].append({"key": key, "reason": reason, "documented": documented[(key, reason)],
                                             "actual": count})
    report["unused_exceptions"] = [{"key": key, "reason": reason} for (key, reason) in documented
                                   if (key, reason) not in used]

    report["defects"] = [{"rule": i, "key": key, "kind": kind, "message": message}
                         for i, key, kind, message in document_defects(candidate)]
    report["defect_counts"] = dict(collections.Counter(d["kind"] for d in report["defects"]))
    failing = (report["missing_keys"] or report["extra_keys"] or report["structure"] or report["undocumented"]
               or report["count_mismatch"] or report["unused_exceptions"]
               or (report["defects"] and not allow_candidate_defects))
    report["ok"] = not failing
    return report


# ---- output ----------------------------------------------------------------------------------------------------

def suggested_exceptions(report):
    return [{"key": key, "reason": reason, "count": count}
            for (key, reason), count in sorted(report["required"].items())]


def render(report, reference_path, candidate_path, limit, allow_candidate_defects):
    lines = []
    ref, cand = report["reference"], report["candidate"]
    required_by_reason = collections.Counter(reason for (_, reason) in report["required"])
    lines.append("reference=%s keys=%d rules=%d collapsible_duplicates=%d collapsible_shadowed=%d "
                 "keys_with_a_collapsible_rule=%d"
                 % (reference_path, ref["keys"], ref["rules"], ref["collapsible_duplicates"],
                    ref["collapsible_shadowed"], ref["keys_with_a_collapsible_rule"]))
    lines.append("candidate=%s keys=%d rules=%d" % (candidate_path, cand["keys"], cand["rules"]))
    lines.append("keys missing=%d extra=%d" % (len(report["missing_keys"]), len(report["extra_keys"])))
    lines.append("structure identical_keys=%d equal_after_collapse=%d differing_keys=%d"
                 % (report["identical_keys"], report["equal_after_collapse"], len(report["structure"])))
    lines.append("extras attribute_rules=%d keys=%d" % (report["extras"]["rules"], report["extras"]["keys"]))
    lines.append("exceptions required=%d (%s) undocumented=%d count_mismatch=%d unused=%d"
                 % (len(report["required"]),
                    ", ".join("%s=%d" % (r, required_by_reason[r]) for r in REASONS),
                    len(report["undocumented"]), len(report["count_mismatch"]),
                    len(report["unused_exceptions"])))
    lines.append("candidate_defects total=%d%s %s" % (
        len(report["defects"]), " (allowed)" if allow_candidate_defects else "",
        " ".join("%s=%d" % kv for kv in sorted(report["defect_counts"].items()))))

    def section(title, items, text):
        if not items:
            return
        lines.append("%s: %d" % (title, len(items)))
        for item in (items if limit == 0 else items[:limit]):
            lines.append("  " + text(item))
        if limit and len(items) > limit:
            lines.append("  ... %d more (--limit 0 for all)" % (len(items) - limit))

    section("missing keys", report["missing_keys"], lambda k: k)
    section("extra keys", report["extra_keys"], lambda k: k)
    section("structure differs", report["structure"], lambda s: "%s: %s" % (s["key"], "; ".join(s["problems"])))
    section("undocumented collapse", report["undocumented"],
            lambda e: "%s %s count=%d" % (e["key"], e["reason"], e["count"]))
    section("exception count mismatch", report["count_mismatch"],
            lambda e: "%s %s documented=%d actual=%d" % (e["key"], e["reason"], e["documented"], e["actual"]))
    section("unused exception", report["unused_exceptions"], lambda e: "%s %s" % (e["key"], e["reason"]))
    section("candidate defect", report["defects"],
            lambda d: "rule #%d %s: %s: %s" % (d["rule"], d["key"], d["kind"], d["message"]))
    lines.append("parity=%s" % ("PASS" if report["ok"] else "FAIL"))
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description="Compare the survey-code structure of two customisation files.")
    parser.add_argument("reference")
    parser.add_argument("candidate")
    parser.add_argument("--exceptions", default=DEFAULT_EXCEPTIONS)
    parser.add_argument("--allow-candidate-defects", action="store_true")
    parser.add_argument("--no-extra-attributes", action="store_true")
    parser.add_argument("--suggest-exceptions", action="store_true")
    parser.add_argument("--limit", type=int, default=12)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        reference = load_document(args.reference)
        candidate = load_document(args.candidate)
        exceptions = load_exceptions(None if args.exceptions == "none" else args.exceptions)
    except ParityError as error:
        sys.stderr.write("parity: %s\n" % error)
        return 2
    report = compare(reference, candidate, exceptions, allow_extras=not args.no_extra_attributes,
                     allow_candidate_defects=args.allow_candidate_defects)
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(newline="\n")  # LF on Windows too
    if args.suggest_exceptions:
        sys.stdout.write(json.dumps(suggested_exceptions(report), indent=1) + "\n")
        return 0
    if args.json:
        out = dict(report, required=[dict(key=k, reason=r, count=c) for (k, r), c in sorted(report["required"].items())])
        sys.stdout.write(json.dumps(out, indent=1, sort_keys=True) + "\n")
    else:
        sys.stdout.write(render(report, args.reference, args.candidate, args.limit,
                                args.allow_candidate_defects) + "\n")
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
