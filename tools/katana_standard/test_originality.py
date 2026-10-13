#!/usr/bin/env python3
"""Tests of originality.py on two small invented customisations, "Elder" (the reference) and "Probe" (the
candidate), every expectation worked out by hand from the rules in that script's header. The pair starts with
nothing in common but its keys; each test then copies ONE thing from the reference and expects the scan to name it.
They name no library but their own two.

    python tools/katana_standard/test_originality.py
"""
import contextlib
import copy
import io
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import originality  # noqa: E402

# A definition of more than five strokes, so that it takes part in the stroke comparison.
ELDER_STROKES = [["move", 0, 0], ["draw", 4, 0], ["draw", 4, 2], ["draw", 0, 2], ["draw", 0, 0], ["move", 1, 1],
                 ["circle", 0.5]]
PROBE_STROKES = [["move", 0, 0], ["draw", 3, 1], ["draw", 6, 0], ["move", 2, -1], ["draw", 2, 1], ["move", 4, -1],
                 ["draw", 4, 1]]


def elder():
    """The reference: one line code that draws in the standard name `white` (no table entry for it), one point."""
    return {
        "format": "katana-customisation", "version": 1, "name": "Elder",
        "colours": {"elder ink": "#102030"},
        "linestyles": [{"name": "Elder Line", "group": "Elder/Lines", "units": "paper", "length": 6,
                        "strokes": ELDER_STROKES}],
        "symbols": [{"name": "Elder Mark", "group": "Elder/Marks", "units": "paper", "atVertices": True,
                     "strokes": [["circle", 1]]}],
        "codes": [
            {"key": "AB*", "sets": "feature", "layer": "ELDER WALL", "colour": "white", "draw": "line",
             "linestyle": "Elder Line", "group": "Elder/Lines", "comment": "[AB] Stone wall face"},
            {"key": "AB*", "sets": "attributes", "attributes": [{"type": "text", "name": "Elder Note"}]},
            {"key": "PQ", "sets": "feature", "layer": "ELDER MARK", "colour": "elder ink", "draw": "point",
             "comment": "Pit lid"},
            {"key": "7", "sets": "text", "text": {"style": "Elder Text", "units": "paper", "size": 2}},
        ],
    }


def probe():
    """The candidate: the same keys, and nothing else of the reference. Its communications colour is white."""
    return {
        "format": "katana-customisation", "version": 1, "name": "Probe",
        "colours": {"probe comms": "#FFFFFF", "probe ink": "#445566"},
        "linestyles": [{"name": "Probe Line", "group": "Probe/Lines", "units": "paper", "length": 6,
                        "strokes": PROBE_STROKES}],
        "symbols": [{"name": "Probe Mark", "group": "Probe/Marks", "units": "paper", "atVertices": True,
                     "strokes": [["circle", 2]]}],
        "codes": [
            {"key": "AB*", "sets": "feature", "layer": "probe/wall/face", "colour": "probe comms", "draw": "line",
             "linestyle": "Probe Line", "group": "Probe/Lines", "comment": "Cable route"},
            {"key": "AB*", "sets": "attributes",
             "attributes": [{"type": "text", "name": "utility.type", "value": "telecommunications"},
                            {"type": "text", "name": "Probe Note"}]},
            {"key": "PQ", "sets": "feature", "layer": "probe/mark/lid", "colour": "probe ink", "draw": "point",
             "comment": "Access cover"},
            {"key": "7", "sets": "text", "text": {"style": "Probe Text", "units": "paper", "size": 2}},
        ],
    }


def failing(result):
    """What the scan found in common, as {what: [the candidate's names]} without the empty lists."""
    return {what: items for what, items in result["exact"].items() if items}


class NothingInCommon(unittest.TestCase):
    def test_a_pair_that_shares_only_its_keys_passes_and_its_one_white_is_excused(self):
        result = originality.scan(elder(), probe())
        self.assertEqual(failing(result), {})
        self.assertEqual(result["failing"], 0)
        self.assertEqual(result["keys"], (3, 3, 3))          # AB*, PQ and 7 in both
        self.assertEqual(result["excused"], ["probe comms"])  # white, and the colour of the communications code
        self.assertEqual(result["same_key_labels"], [])

    def test_a_file_compared_with_itself_shares_everything_it_has(self):
        result = originality.scan(elder(), elder())
        found = failing(result)
        self.assertEqual(found["definition names"], ["Elder Line", "Elder Mark"])
        self.assertEqual(found["layer paths"], ["ELDER MARK", "ELDER WALL"])
        self.assertEqual(found["colour names"], ["elder ink"])
        self.assertEqual(found["attribute names"], ["Elder Note"])
        self.assertEqual(found["text styles"], ["Elder Text"])
        self.assertEqual(found["stroke lists, exact"], ["Elder Line"])   # the mark has one stroke and is left out


class OneThingCopied(unittest.TestCase):
    def one(self, change):
        candidate = probe()
        change(candidate)
        result = originality.scan(elder(), candidate)
        return failing(result), result

    def test_a_definition_name_is_found_whatever_its_case_and_blanks(self):
        def change(c):
            c["linestyles"][0]["name"] = "elder  LINE"
            c["codes"][0]["linestyle"] = "elder  LINE"
        found, _ = self.one(change)
        self.assertEqual(found, {"definition names": ["elder  LINE"]})

    def test_a_group_path_is_found_on_a_definition_and_on_a_rule(self):
        found, _ = self.one(lambda c: c["symbols"][0].update(group="Elder/Marks"))
        self.assertEqual(found, {"group paths": ["Elder/Marks"]})
        found, _ = self.one(lambda c: c["codes"][0].update(group="Elder/Lines"))
        self.assertEqual(found, {"group paths": ["Elder/Lines"]})

    def test_a_layer_path_is_found(self):
        found, _ = self.one(lambda c: c["codes"][2].update(layer="elder mark"))
        self.assertEqual(found, {"layer paths": ["elder mark"]})

    def test_a_legend_label_is_found_without_the_reference_s_key_lead_in(self):
        found, result = self.one(lambda c: c["codes"][0].update(comment="Stone wall face"))
        self.assertEqual(found, {"legend labels": ["Stone wall face"]})
        self.assertEqual(result["same_key_labels"], ["Stone wall face"])

    def test_a_label_near_the_reference_s_for_the_same_key_is_counted_and_does_not_fail(self):
        # "stone wall faces" against "stone wall face": 30 of 31 characters match, a ratio of 0.97.
        found, result = self.one(lambda c: c["codes"][0].update(comment="Stone wall faces"))
        self.assertEqual(found, {})
        self.assertEqual(result["same_key_labels"], ["Stone wall faces"])
        self.assertEqual(result["near_labels"], ["Stone wall faces"])
        # The same words under ANOTHER key are near in the whole list and not for the key.
        found, result = self.one(lambda c: c["codes"][2].update(comment="Stone wall faces"))
        self.assertEqual(result["same_key_labels"], [])
        self.assertEqual(result["near_labels"], ["Stone wall faces"])

    def test_a_colour_name_is_found(self):
        def change(c):
            c["colours"]["Elder_Ink"] = c["colours"].pop("probe ink")
            c["codes"][2]["colour"] = "Elder_Ink"
        found, _ = self.one(change)
        self.assertEqual(found, {"colour names": ["Elder_Ink"]})

    def test_a_colour_value_of_the_reference_s_table_is_found(self):
        found, _ = self.one(lambda c: c["colours"].update({"probe ink": "#102030"}))
        self.assertEqual(found, {"colour values (but the one white of communications)": ["probe ink"]})

    def test_the_value_of_a_standard_name_the_reference_draws_in_is_found(self):
        # The reference has no table entry for white: it draws in the standard name. A second white is in common.
        found, result = self.one(lambda c: c["colours"].update({"probe ink": "#ffffff"}))
        self.assertEqual(found, {"colour values (but the one white of communications)": ["probe ink"]})
        self.assertEqual(result["excused"], ["probe comms"])

    def test_white_is_excused_only_for_the_communications_colour(self):
        def change(c):
            c["colours"]["probe comms"] = "#FEFEFE"   # communications is no longer white
            c["colours"]["probe ink"] = "#FFFFFF"     # and some other colour is
        found, result = self.one(change)
        self.assertEqual(result["excused"], [])
        self.assertEqual(found, {"colour values (but the one white of communications)": ["probe ink"]})

    def test_white_is_not_excused_when_no_code_says_it_is_communications(self):
        found, result = self.one(lambda c: c["codes"][1]["attributes"].pop(0))
        self.assertEqual(result["excused"], [])
        self.assertEqual(found, {"colour values (but the one white of communications)": ["probe comms"]})

    def test_a_standard_colour_the_reference_never_draws_in_is_no_value_of_it(self):
        # Pure red is a standard colour, and the reference names it nowhere.
        found, _ = self.one(lambda c: c["colours"].update({"probe ink": "#FF0000"}))
        self.assertEqual(found, {})

    def test_an_attribute_name_and_a_text_style_are_found(self):
        found, _ = self.one(lambda c: c["codes"][1]["attributes"].append({"type": "text", "name": "Elder Note"}))
        self.assertEqual(found, {"attribute names": ["Elder Note"]})
        found, _ = self.one(lambda c: c["codes"][3]["text"].update(style="Elder Text"))
        self.assertEqual(found, {"text styles": ["Elder Text"]})

    def test_a_stroke_list_is_found_as_it_is_moved_and_scaled(self):
        found, _ = self.one(lambda c: c["linestyles"][0].update(strokes=copy.deepcopy(ELDER_STROKES)))
        self.assertEqual(sorted(found), ["stroke lists, exact", "stroke lists, scaled", "stroke lists, translated"])
        moved = [[s[0], s[1] + 5, s[2] - 3] if s[0] in ("move", "draw") else list(s) for s in ELDER_STROKES]
        found, _ = self.one(lambda c: c["linestyles"][0].update(strokes=moved))
        self.assertEqual(sorted(found), ["stroke lists, scaled", "stroke lists, translated"])
        doubled = [[s[0], s[1] * 2, s[2] * 2] if s[0] in ("move", "draw") else [s[0], s[1] * 2] for s in ELDER_STROKES]
        found, _ = self.one(lambda c: c["linestyles"][0].update(strokes=doubled))
        self.assertEqual(sorted(found), ["stroke lists, scaled"])
        self.assertEqual(found["stroke lists, scaled"], ["Probe Line"])

    def test_a_list_of_five_strokes_or_fewer_is_no_copy(self):
        # Both libraries draw a ring for their mark; one stroke is anybody's.
        found, result = self.one(lambda c: c["symbols"][0].update(strokes=[["circle", 1]]))
        self.assertEqual(found, {})
        self.assertEqual(result["definitions"], (2, 2, 1))   # one of the candidate's two is left out


class WhatTheReferenceCallsAThing(unittest.TestCase):
    """A definition name of the candidate against everything the reference calls a thing (its definition names
    less a lead-in, and its legend labels), a legend label of the candidate against the reference's definition
    names, and what "equal" means for all of them."""

    def scan(self, change_reference, change_candidate):
        reference, candidate = elder(), probe()
        change_reference(reference)
        change_candidate(candidate)
        result = originality.scan(reference, candidate)
        return failing(result), result

    def test_a_name_is_found_under_the_reference_s_lead_in_with_or_without_its_single_capital(self):
        for theirs in ("E WALL Stone Line", "WALL Stone Line", "E 12 Stone Line"):
            found, _ = self.scan(lambda r: r["linestyles"][0].update(name=theirs),
                                 lambda c: c["linestyles"][0].update(name="Stone Line"))
            self.assertEqual(found, {"definition names and reference names less their lead-in": ["Stone Line"]},
                             theirs)

    def test_two_lead_ins_are_taken_off_one_after_the_other(self):
        # "AB CD Peg" is also "CD Peg" and "Peg".
        for ours in ("Peg", "Cd Peg"):
            found, _ = self.scan(lambda r: r["symbols"][0].update(name="AB CD Peg"),
                                 lambda c: c["symbols"][0].update(name=ours))
            self.assertEqual(found, {"definition names and reference names less their lead-in": [ours]}, ours)

    def test_a_word_in_mixed_case_is_no_lead_in_and_a_name_of_one_word_has_none(self):
        # "Elder Line" loses nothing, so "Line" alone is no name of the reference; nor is "MARK" less than itself.
        found, _ = self.scan(lambda r: r["symbols"][0].update(name="MARK"),
                             lambda c: c["linestyles"][0].update(name="Line"))
        self.assertEqual(found, {})
        self.assertEqual(originality.without_lead_ins("Elder Line"), [])
        self.assertEqual(originality.without_lead_ins("MARK"), [])
        self.assertEqual(originality.without_lead_ins("A B Line"), [])   # one capital is not a word of capitals
        self.assertEqual(originality.without_lead_ins("E WALL Stone Line"), ["Stone Line"])
        self.assertEqual(originality.without_lead_ins("AB CD Peg"), ["CD Peg", "Peg"])

    def test_a_definition_named_after_the_reference_s_legend_label_is_found(self):
        # "[AB] Stone wall face" and "Pit lid" are the reference's labels: a symbol called either is called what
        # the reference calls the thing.
        for ours in ("Stone Wall Face", "Pit Lid"):
            found, _ = self.scan(lambda r: None, lambda c: c["symbols"][0].update(name=ours))
            self.assertEqual(found, {"definition names and reference legend labels": [ours]}, ours)

    def test_a_legend_label_that_is_a_definition_name_of_the_reference_is_found(self):
        found, _ = self.scan(lambda r: None, lambda c: c["codes"][2].update(comment="Elder mark"))
        self.assertEqual(found, {"legend labels and reference definition names": ["Elder mark"]})
        # And one that is a name of the reference less its lead-in.
        found, _ = self.scan(lambda r: r["linestyles"][0].update(name="E WALL Stone Line"),
                             lambda c: c["codes"][2].update(comment="Stone line"))
        self.assertEqual(found, {"legend labels and reference definition names": ["Stone line"]})

    def test_names_are_equal_whatever_their_blanks_and_punctuation(self):
        # A hyphen, a comma, a stop or a missing blank does not make a name another name.
        for ours in ("Elder-Line", "Elder, Line", "Elderline", "ELDER_LINE."):
            found, _ = self.scan(lambda r: None, lambda c: c["linestyles"][0].update(name=ours))
            self.assertEqual(found, {"definition names": [ours]}, ours)
        found, _ = self.scan(lambda r: None, lambda c: c["codes"][0].update(comment="Stone-wall face."))
        self.assertEqual(found, {"legend labels": ["Stone-wall face."]})
        found, _ = self.scan(lambda r: None, lambda c: c["codes"][2].update(layer="Elder-Mark"))
        self.assertEqual(found, {"layer paths": ["Elder-Mark"]})

    def test_a_label_s_lead_in_of_up_to_eight_capitals_and_digits_is_taken_off(self):
        for theirs in ("E WALLFACE Stone coping", "[PQ] E W2 Stone coping", "[PQ]Stone coping"):
            found, _ = self.scan(lambda r: r["codes"][2].update(comment=theirs),
                                 lambda c: c["codes"][2].update(comment="Stone coping"))
            self.assertEqual(found, {"legend labels": ["Stone coping"]}, theirs)
        # Nine capitals are a word of the label and not a lead-in, so the two labels differ.
        found, _ = self.scan(lambda r: r["codes"][2].update(comment="E WALLFACES Stone coping"),
                             lambda c: c["codes"][2].update(comment="Stone coping"))
        self.assertEqual(found, {})

    def test_a_name_near_one_the_reference_gives_is_listed_and_does_not_fail(self):
        # "stone lines" against "stone line": 10 characters in common of 11 and 10, a ratio of 20 / 21 = 0.95.
        found, result = self.scan(lambda r: r["linestyles"][0].update(name="E WALL Stone Line"),
                                  lambda c: c["linestyles"][0].update(name="Stone Lines"))
        self.assertEqual(found, {})
        self.assertEqual(result["near_names"], ["Stone Lines"])
        # Near a legend label of the reference: "pit lids" against "pit lid", 14 / 15 = 0.93.
        found, result = self.scan(lambda r: None, lambda c: c["symbols"][0].update(name="Pit Lids"))
        self.assertEqual(found, {})
        self.assertEqual(result["near_names"], ["Pit Lids"])
        # An equal name is counted as equal and is not listed again as near; nor is the untouched pair's.
        _, result = self.scan(lambda r: None, lambda c: c["symbols"][0].update(name="Pit Lid"))
        self.assertEqual(result["near_names"], [])
        _, result = self.scan(lambda r: None, lambda c: None)
        self.assertEqual(result["near_names"], [])
        self.assertEqual(result["near_labels"], [])


class CommandLine(unittest.TestCase):
    def run_main(self, reference, candidate, *options):
        with tempfile.TemporaryDirectory() as folder:
            paths = []
            for name, document in (("reference.json", reference), ("candidate.json", candidate)):
                paths.append(os.path.join(folder, name))
                with open(paths[-1], "w", encoding="utf-8") as handle:
                    json.dump(document, handle)
            out = io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
                code = originality.main(paths + list(options))
            return code, out.getvalue()

    def test_the_exit_status_is_zero_only_when_nothing_is_in_common(self):
        code, text = self.run_main(elder(), probe())
        self.assertEqual(code, 0)
        self.assertIn("originality=PASS", text)
        self.assertIn("survey code keys in common=3 (reference 3, candidate 3)", text)
        self.assertIn("white excused for the communications colour=1", text)
        code, text = self.run_main(elder(), elder())
        self.assertEqual(code, 1)
        self.assertIn("originality=FAIL", text)

    def test_the_reference_s_words_are_never_printed(self):
        candidate = probe()
        candidate["codes"][0]["comment"] = "Stone wall faces"
        _, text = self.run_main(elder(), candidate, "--limit", "0")
        self.assertNotIn("Elder", text)
        self.assertNotIn("Stone wall face\n", text)
        self.assertIn("for the same key (>=0.85, not a failure)=1", text)

    def test_a_file_that_cannot_be_read_and_a_missing_argument_are_status_two(self):
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(originality.main(["nowhere.json", "nowhere-either.json"]), 2)
            self.assertEqual(originality.main(["only-one.json"]), 2)


class StandardColours(unittest.TestCase):
    def test_the_table_is_the_one_the_reader_compiles_in(self):
        """The 27 values against src/katana_entity/colour_names.cpp, read as text; skipped outside a checkout."""
        import re
        source = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "src", "katana_entity",
                              "colour_names.cpp")
        if not os.path.exists(source):
            self.skipTest("no colour_names.cpp beside this script's checkout")
        with open(source, encoding="utf-8") as handle:
            text = handle.read()
        # An entry of the table, however it is laid out: {"dark red", {139, 0, 0, 255}}.
        number = r"\s*(\d+)\s*"
        entry = r'\{\s*"([a-z ]+)"\s*,\s*\{' + number + "," + number + "," + number + r",\s*255\s*\}\s*\}"
        compiled = {m.group(1): "#%02x%02x%02x" % tuple(int(v) for v in m.group(2, 3, 4))
                    for m in re.finditer(entry, text)}
        self.assertEqual(len(compiled), 27)
        self.assertEqual(originality.STANDARD_COLOURS, compiled)


if __name__ == "__main__":
    unittest.main()
