#!/usr/bin/env python3
"""Tests of parity.py on small invented customisations, every expectation worked out by hand from the rules in
that script's header and from CODE CHECK's notions of a duplicate and a shadowed rule (code_table.cpp,
lintSurveyMap). They name no library but their own invented "Probe" one.

    python tools/katana_standard/test_parity.py
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
import parity  # noqa: E402


def document(*rules, colours=None):
    return {
        "format": "katana-customisation", "version": 1, "name": "Probe",
        "colours": {"probe ink": "#112233"} if colours is None else colours,
        "linestyles": [{"name": "Probe Line", "group": "Probe", "units": "paper", "length": 4,
                        "strokes": [["move", 0, 0], ["draw", 3, 0]]}],
        "symbols": [{"name": "Probe Mark", "group": "Probe", "units": "paper", "atVertices": True,
                     "strokes": [["circle", 1]]}],
        "codes": list(rules),
    }


def feature(key, draw="line", colour="probe ink", layer="probe/layer/one", **more):
    rule = {"key": key, "sets": "feature", "layer": layer, "colour": colour, "draw": draw}
    if draw == "line":
        rule["linestyle"] = "Probe Line"
    rule.update(more)
    return rule


def symbol(key, **more):
    rule = {"key": key, "sets": "symbol", "symbol": {"name": "Probe Mark"}}
    rule.update(more)
    return rule


def surface(key, flag=False):
    return {"key": key, "sets": "surface", "surface": flag}


def run(reference, candidate, exceptions=(), **options):
    return parity.compare(document(*reference) if isinstance(reference, list) else reference,
                          document(*candidate) if isinstance(candidate, list) else candidate,
                          exceptions, **options)


class Classification(unittest.TestCase):
    def test_an_identical_later_rule_of_the_same_key_is_a_duplicate(self):
        rules = [feature("AB*"), feature("AB*")]
        self.assertEqual(parity.classify(rules), ["ok", "duplicate"])

    def test_a_later_rule_that_says_nothing_an_earlier_one_did_not_is_shadowed(self):
        # The second differs (a comment) from the first, so it is no duplicate; the comment is the only thing it
        # sets and the first already has one, so it contributes nothing.
        rules = [feature("AB*", comment="first"), feature("AB*", comment="second")]
        self.assertEqual(parity.classify(rules), ["ok", "shadowed"])

    def test_a_later_rule_that_sets_a_new_field_contributes(self):
        rules = [feature("AB*"), feature("AB*", weight="0.25")]
        self.assertEqual(parity.classify(rules), ["ok", "ok"])

    def test_a_later_rule_with_a_new_attribute_name_contributes_but_the_same_name_does_not(self):
        def pipe(name):
            return {"key": "*", "sets": "pipe", "pipe": {"shape": "diameter"},
                    "attributes": [{"type": "text", "name": name, "value": "x"}]}
        self.assertEqual(parity.classify([pipe("A"), pipe("B")]), ["ok", "ok"])
        self.assertEqual(parity.classify([pipe("A"), dict(pipe("A"), pipe={"shape": "culvert"})]),
                         ["ok", "shadowed"])

    def test_only_rules_of_the_same_key_shadow_each_other(self):
        rules = [feature("AB*"), feature("ABC*")]
        self.assertEqual(parity.classify(rules), ["ok", "ok"])


class Colours(unittest.TestCase):
    def test_a_name_is_folded_before_it_is_looked_up(self):
        self.assertEqual(parity.fold_colour("Dark_Gray"), "dark grey")
        self.assertEqual(parity.fold_colour("GRAY"), "grey")
        self.assertEqual(parity.fold_colour("red_"), "red")
        self.assertEqual(parity.fold_colour("dark  red"), "dark  red")  # inner blanks are kept

    def test_the_table_then_the_standard_names_resolve_and_nothing_else_does(self):
        colours = parity.Colours(document(colours={"Probe-Ink": "#112233"}))
        self.assertEqual(colours.state("probe ink"), "known")
        self.assertEqual(colours.state("Light Pink"), "known")
        self.assertEqual(colours.state("pen 018"), "unknown")
        self.assertEqual(colours.state(""), "absent")


class Structure(unittest.TestCase):
    def test_hide_that_is_absent_reads_as_false(self):
        colours = parity.Colours(document())
        self.assertEqual(parity.structure(symbol("AB*"), colours), parity.structure(symbol("AB*", hide=False), colours))
        self.assertNotEqual(parity.structure(symbol("AB*"), colours), parity.structure(symbol("AB*", hide=True), colours))

    def test_how_a_rule_looks_is_not_part_of_its_structure(self):
        colours = parity.Colours(document())
        plain = feature("AB*")
        restyled = feature("AB*", layer="other/place/here", colour="red", linestyle="Other", weight="1", group="G",
                           comment="c")
        self.assertEqual(parity.structure(plain, colours), parity.structure(restyled, colours))


class Keys(unittest.TestCase):
    def test_identical_files_have_parity(self):
        rules = [feature("AB*"), symbol("AB*"), surface("AB*")]
        report = run(rules, rules)
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["identical_keys"], 1)

    def test_missing_and_extra_keys_are_reported_byte_for_byte(self):
        report = run([feature("AB*"), feature("CD*")], [feature("ab*"), feature("CD*"), feature("EF")])
        self.assertEqual(report["missing_keys"], ["AB*"])
        self.assertEqual(report["extra_keys"], ["ab*", "EF"])
        self.assertFalse(report["ok"])

    def test_a_prefix_key_and_the_exact_key_are_two_keys(self):
        report = run([feature("AB*"), feature("AB", draw="point")], [feature("AB*")])
        self.assertEqual(report["missing_keys"], ["AB"])


class Behaviour(unittest.TestCase):
    def test_a_different_draw_mode_hide_flag_or_surface_flag_is_a_difference(self):
        reference = [feature("AB*"), symbol("AB*"), surface("AB*", False)]
        for index, change in ((0, {"draw": "point"}), (1, {"hide": True}), (2, {"surface": True})):
            candidate = copy.deepcopy(reference)
            candidate[index].update(change)
            report = run(reference, candidate)
            self.assertEqual([s["key"] for s in report["structure"]], ["AB*"], change)
            self.assertFalse(report["ok"], change)

    def test_a_missing_or_extra_kind_of_rule_is_a_difference(self):
        reference = [feature("AB*"), symbol("AB*")]
        self.assertIn("kinds of rule", run(reference, [feature("AB*")])["structure"][0]["problems"][0])
        self.assertIn("kinds of rule", run(reference, reference + [surface("AB*")])["structure"][0]["problems"][0])

    def test_the_order_of_the_kinds_is_part_of_the_structure(self):
        report = run([feature("AB*"), symbol("AB*")], [symbol("AB*"), feature("AB*")])
        self.assertEqual(len(report["structure"]), 1)

    def test_text_size_and_justification_are_behaviour_but_the_face_is_not(self):
        def text(size, face):
            return {"key": "1", "sets": "text", "text": {"style": face, "units": "paper", "size": size,
                                                       "justifyX": "left", "justifyY": "bottom"}}
        self.assertTrue(run([text(1.5, "A")], [text(1.5, "B")])["ok"])
        self.assertFalse(run([text(1.5, "A")], [text(2.5, "A")])["ok"])


class Collapse(unittest.TestCase):
    """The reference repeats rules; a candidate with one clean set differs only by what the exceptions explain."""
    # The second symbol rule says hide=true; the first says hide=false and has the symbol, so it never contributes.
    reference = [feature("AB*"), symbol("AB*", hide=False), feature("AB*"), symbol("AB*", hide=True)]
    candidate = [feature("AB*"), symbol("AB*", hide=False)]

    def test_a_duplicate_and_a_shadowed_rule_left_out_need_one_exception_each(self):
        report = run(self.reference, self.candidate)
        self.assertEqual(report["required"], {("AB*", parity.DUPLICATE): 1, ("AB*", parity.SHADOWED): 1})
        self.assertEqual(len(report["undocumented"]), 2)
        self.assertFalse(report["ok"])

    def test_with_the_exceptions_parity_holds(self):
        exceptions = [("AB*", parity.DUPLICATE, 1), ("AB*", parity.SHADOWED, None)]
        report = run(self.reference, self.candidate, exceptions)
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["equal_after_collapse"], 1)

    def test_a_wrong_count_an_unneeded_exception_and_a_missing_one_each_fail(self):
        both = [("AB*", parity.DUPLICATE, 1), ("AB*", parity.SHADOWED, 1)]
        self.assertFalse(run(self.reference, self.candidate, [("AB*", parity.DUPLICATE, 5), both[1]])["ok"])
        self.assertEqual(len(run(self.reference, self.candidate, both + [("ZZ*", parity.DUPLICATE, 1)])
                             ["unused_exceptions"]), 1)
        self.assertEqual(len(run(self.reference, self.candidate, both[:1])["undocumented"]), 1)

    def test_a_candidate_that_keeps_the_repeat_has_a_defect_and_no_exception_is_needed(self):
        report = run(self.reference, self.reference)
        self.assertEqual(report["required"], {})
        self.assertEqual(report["defect_counts"], {"duplicate": 1, "shadowed": 1})
        self.assertFalse(report["ok"])
        self.assertTrue(run(self.reference, self.reference, allow_candidate_defects=True)["ok"])

    def test_the_first_rule_of_a_kind_is_the_one_that_counts(self):
        # The later hide=true is the shadowed one: a candidate that follows IT instead differs.
        candidate = [feature("AB*"), symbol("AB*", hide=True)]
        report = run(self.reference, candidate, [("AB*", parity.DUPLICATE, 1), ("AB*", parity.SHADOWED, 1)])
        self.assertFalse(report["ok"])
        self.assertIn("hide", report["structure"][0]["problems"][0])


class UnknownColour(unittest.TestCase):
    reference = [feature("AB*", colour="pen 018")]

    def test_replacing_a_colour_that_resolves_to_nothing_needs_its_exception(self):
        report = run(self.reference, [feature("AB*")])
        self.assertEqual(report["required"], {("AB*", parity.COLOUR): 1})
        self.assertTrue(run(self.reference, [feature("AB*")], [("AB*", parity.COLOUR, None)])["ok"])

    def test_keeping_the_unresolvable_colour_is_a_defect_of_the_candidate(self):
        report = run(self.reference, self.reference)
        self.assertEqual(report["defect_counts"], {"unknown-colour": 1})
        self.assertFalse(report["ok"])


class SymbolSaidThroughTheLinestyle(unittest.TestCase):
    reference = [feature("AB*", draw="point", linestyle="Probe Mark")]

    def test_the_clean_set_has_the_symbol_rule_the_reference_never_wrote(self):
        candidate = [feature("AB*", draw="point"), symbol("AB*")]
        report = run(self.reference, candidate)
        self.assertEqual(report["required"], {("AB*", parity.LINESTYLE_SYMBOL): 1})
        self.assertTrue(run(self.reference, candidate, [("AB*", parity.LINESTYLE_SYMBOL, 1)])["ok"])

    def test_a_candidate_with_no_symbol_rule_has_lost_the_symbol(self):
        report = run(self.reference, [feature("AB*", draw="point")], [("AB*", parity.LINESTYLE_SYMBOL, 1)])
        self.assertIn("kinds of rule", report["structure"][0]["problems"][0])
        self.assertFalse(report["ok"])

    def test_a_plain_linestyle_or_a_real_linestyle_is_not_such_a_case(self):
        for word in ("0", "Probe Line"):
            # A repeat sends the key through the collapse path, where only the duplicate is required.
            reference = [feature("AB*", linestyle=word), feature("AB*", linestyle=word)]
            self.assertEqual(run(reference, [feature("AB*")])["required"], {("AB*", parity.DUPLICATE): 1}, word)


class Extras(unittest.TestCase):
    reference = [feature("AB*")]
    candidate = [feature("AB*"), {"key": "AB*", "sets": "attributes",
                                  "attributes": [{"type": "text", "name": "Owner", "value": ""}]}]

    def test_a_prompt_the_reference_does_not_ask_for_is_an_allowed_extra(self):
        report = run(self.reference, self.candidate)
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["extras"], {"rules": 1, "keys": 1})

    def test_it_is_a_difference_when_extras_are_refused(self):
        self.assertFalse(run(self.reference, self.candidate, allow_extras=False)["ok"])

    def test_where_the_reference_has_an_attributes_rule_the_candidate_needs_one_that_matches(self):
        def attributes(value):
            return {"key": "AB*", "sets": "attributes", "attributes": [{"type": "integer", "name": "Zone", "value": value}]}
        self.assertTrue(run([feature("AB*"), attributes("2")], [feature("AB*"), attributes("2")])["ok"])
        self.assertFalse(run([feature("AB*"), attributes("2")], [feature("AB*"), attributes("3")])["ok"])
        self.assertFalse(run([feature("AB*"), attributes("2")], [feature("AB*")])["ok"])


class Defects(unittest.TestCase):
    def kinds(self, *rules, **document_members):
        doc = document(*rules)
        doc.update(document_members)
        return sorted({kind for _, _, kind, _ in parity.document_defects(doc)})

    def test_a_clean_file_has_none(self):
        self.assertEqual(self.kinds(feature("AB*"), symbol("AB*"), surface("AB*")), [])

    def test_names_nothing_defines_are_defects(self):
        self.assertEqual(self.kinds(feature("AB*", linestyle="Nowhere")), ["unresolved-linestyle"])
        self.assertEqual(self.kinds({"key": "AB*", "sets": "symbol", "symbol": {"name": "Nowhere"}}),
                         ["unresolved-symbol"])

    def test_a_symbol_drawn_along_a_line_and_a_line_drawn_at_a_point_are_defects(self):
        self.assertEqual(self.kinds(feature("AB*", linestyle="Probe Mark")), ["linestyle-is-symbol"])
        self.assertEqual(self.kinds({"key": "AB*", "sets": "symbol", "symbol": {"name": "Probe Line"}}),
                         ["symbol-is-linestyle"])

    def test_plain_linestyle_words_need_no_definition(self):
        for word in ("0", "1", "Continuous", ""):
            self.assertEqual(self.kinds(feature("AB*", linestyle=word)), [], word)

    def test_a_member_that_does_not_belong_to_its_kind_is_a_defect(self):
        self.assertEqual(self.kinds(dict(symbol("AB*"), layer="a/b/c")), ["field-outside-section"])

    def test_a_feature_rule_needs_a_proper_layer_path(self):
        self.assertEqual(self.kinds(feature("AB*", layer="a//b")), ["layer"])
        self.assertEqual(self.kinds(feature("AB*", layer="")), ["layer"])

    def test_a_second_rule_of_one_kind_that_still_contributes_is_a_defect(self):
        self.assertEqual(self.kinds(feature("AB*"), feature("AB*", weight="0.25")), ["repeated-kind"])


class Exceptions(unittest.TestCase):
    def write(self, text):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = os.path.join(directory.name, "exceptions.json")
        with open(path, "w", encoding="utf-8", newline="\n") as file:
            file.write(text)
        return path

    def test_a_good_file_is_read(self):
        path = self.write('[{"key": "AB*", "reason": "%s", "count": 2, "note": "why"}]' % parity.DUPLICATE)
        self.assertEqual(parity.load_exceptions(path), [("AB*", parity.DUPLICATE, 2)])

    def test_the_committed_file_is_a_list(self):
        self.assertIsInstance(parity.load_exceptions(parity.DEFAULT_EXCEPTIONS), list)

    def test_every_malformed_file_is_refused(self):
        for text in ("{}", "[1]", '[{"key": "A*"}]', '[{"key": "A*", "reason": "because"}]',
                     '[{"key": "A*", "reason": "%s", "colour": 1}]' % parity.DUPLICATE,
                     '[{"key": "A*", "reason": "%s", "count": 0}]' % parity.DUPLICATE,
                     '[{"key": "A*", "reason": "%s"}, {"key": "A*", "reason": "%s"}]' % (parity.DUPLICATE, parity.DUPLICATE),
                     "not json"):
            with self.assertRaises(parity.ParityError, msg=text):
                parity.load_exceptions(self.write(text))


class CommandLine(unittest.TestCase):
    def files(self, reference, candidate):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        paths = []
        for name, rules in (("reference.json", reference), ("candidate.json", candidate)):
            paths.append(os.path.join(directory.name, name))
            with open(paths[-1], "w", encoding="utf-8", newline="\n") as file:
                json.dump(document(*rules), file)
        return paths

    def main(self, *argv):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            code = parity.main(list(argv))
        return code, out.getvalue()

    def test_exit_status_is_0_for_parity_1_for_a_difference_and_2_for_a_file_it_cannot_read(self):
        same = self.files([feature("AB*")], [feature("AB*")])
        self.assertEqual(self.main(*same, "--exceptions", "none")[0], 0)
        different = self.files([feature("AB*")], [feature("CD*")])
        code, text = self.main(*different, "--exceptions", "none")
        self.assertEqual(code, 1)
        self.assertIn("missing keys: 1", text)
        self.assertEqual(self.main(same[0], same[0] + ".absent", "--exceptions", "none")[0], 2)

    def test_suggested_exceptions_are_the_json_the_comparison_needs(self):
        paths = self.files([feature("AB*"), feature("AB*")], [feature("AB*")])
        code, text = self.main(*paths, "--exceptions", "none", "--suggest-exceptions")
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(text), [{"key": "AB*", "reason": parity.DUPLICATE, "count": 1}])


if __name__ == "__main__":
    unittest.main()
