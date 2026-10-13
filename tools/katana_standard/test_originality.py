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

# A definition that more than five strokes draw, so that it takes part in the comparison of shapes.
ELDER_STROKES = [["move", 0, 0], ["draw", 4, 0], ["draw", 4, 2], ["draw", 0, 2], ["draw", 0, 0], ["move", 1, 1],
                 ["circle", 0.5]]
PROBE_STROKES = [["move", 0, 0], ["draw", 3, 1], ["draw", 6, 0], ["move", 2, -1], ["draw", 2, 1], ["move", 4, -1],
                 ["draw", 4, 1]]
# A shape with no symmetry of its own, so that each of its eight turned and mirrored images is another list: an L
# (along the bottom from the origin, then up at the right end), a ring over its left part, a quarter arc over
# its right part (about (3, 2), from (4, 2) round to (3, 3)) and a dot at the top left. Its points fill a box 4
# wide and 3 high.
ELDER_KEY = [["move", 0, 0], ["draw", 4, 0], ["draw", 4, 1], ["move", 1, 2], ["circle", 0.5],
             ["move", 3, 2], ["arc", 1, 0, 90], ["move", 0, 3], ["dot", 0]]
# The same at twice the size, every length doubled and no angle.
ELDER_KEY_DOUBLED = [["move", 0, 0], ["draw", 8, 0], ["draw", 8, 2], ["move", 2, 4], ["circle", 1],
                     ["move", 6, 4], ["arc", 2, 0, 90], ["move", 0, 6], ["dot", 0]]


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
        self.assertEqual(found["shapes, as drawn"], ["Elder Line"])   # the mark has one stroke and is left out


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

    def test_a_shape_is_found_as_it_is_drawn_moved_and_scaled(self):
        found, _ = self.one(lambda c: c["linestyles"][0].update(strokes=copy.deepcopy(ELDER_STROKES)))
        self.assertEqual(sorted(found), ["shapes, as drawn", "shapes, moved", "shapes, scaled"])
        moved = [[s[0], s[1] + 5, s[2] - 3] if s[0] in ("move", "draw") else list(s) for s in ELDER_STROKES]
        found, _ = self.one(lambda c: c["linestyles"][0].update(strokes=moved))
        self.assertEqual(sorted(found), ["shapes, moved", "shapes, scaled"])
        doubled = [[s[0], s[1] * 2, s[2] * 2] if s[0] in ("move", "draw") else [s[0], s[1] * 2] for s in ELDER_STROKES]
        found, _ = self.one(lambda c: c["linestyles"][0].update(strokes=doubled))
        self.assertEqual(sorted(found), ["shapes, scaled"])
        self.assertEqual(found["shapes, scaled"], ["Probe Line"])

    def test_a_list_of_five_strokes_or_fewer_is_no_copy(self):
        # Both libraries draw a ring for their mark; one stroke is anybody's.
        found, result = self.one(lambda c: c["symbols"][0].update(strokes=[["circle", 1]]))
        self.assertEqual(found, {})
        self.assertEqual(result["definitions"], (2, 2, 1))   # one of the candidate's two is left out


class Shapes(unittest.TestCase):
    """What the tests of the comparison of shapes share; it has no test of its own."""

    ALL = ["shapes, as drawn", "shapes, moved", "shapes, scaled"]

    def found(self, ours, theirs=None, their_members=None):
        """Which of the three comparisons find the candidate's line (and nothing else of it) in the reference.
        `their_members` are further members of the reference's definition (an `origin`, a `factor`)."""
        reference, candidate = elder(), probe()
        reference["linestyles"][0]["strokes"] = copy.deepcopy(ELDER_KEY if theirs is None else theirs)
        reference["linestyles"][0].update(their_members or {})
        candidate["linestyles"][0]["strokes"] = copy.deepcopy(ours)
        found = failing(originality.scan(reference, candidate))
        for names in found.values():
            self.assertEqual(names, ["Probe Line"])
        return sorted(found)


class ShapesHoweverTheyAreWritten(Shapes):
    """What two definitions DRAW is compared, not how their lists are written: without regard to the order of the
    strokes, to the direction a line or an arc is drawn in, and under the eight symmetries of the square. The
    reference's line is ELDER_KEY and the candidate's is the list of each test, worked out by hand from it: a
    quarter turn counter-clockwise takes (x, y) to (-y, x) and adds 90 degrees to an angle, a mirror in the y
    axis takes x to -x and an angle a to 180 - a, so an arc then runs from where it ended. A list that only
    orders, reverses, turns or mirrors the reference's was a different list to a comparison of lists, and none
    of those was found."""

    @staticmethod
    def with_ring(radius):
        return [["circle", radius] if s[0] == "circle" else list(s) for s in ELDER_KEY]

    def test_the_same_strokes_in_another_order_are_the_same_shape(self):
        # The dot first, then the arc, the upright of the L, the ring, and the bottom of the L last: the L is two
        # lines now, and no longer one path.
        ours = [["move", 0, 3], ["dot", 0], ["move", 3, 2], ["arc", 1, 0, 90], ["move", 4, 0], ["draw", 4, 1],
                ["move", 1, 2], ["circle", 0.5], ["move", 0, 0], ["draw", 4, 0]]
        self.assertEqual(self.found(ours), self.ALL)

    def test_a_line_and_an_arc_drawn_from_the_other_end_are_the_same_shape(self):
        # The L from its top end back to the origin, and the arc from 90 degrees down to 0.
        ours = [["move", 4, 1], ["draw", 4, 0], ["draw", 0, 0], ["move", 1, 2], ["circle", 0.5],
                ["move", 3, 2], ["arc", 1, 90, 0], ["move", 0, 3], ["dot", 0]]
        self.assertEqual(self.found(ours), self.ALL)

    def test_a_shape_turned_by_quarter_turns_is_the_same_shape(self):
        # One quarter turn: (4, 0) to (0, 4), (4, 1) to (-1, 4), the ring to (-2, 1), the arc to (-2, 3) and
        # through 90 to 180, the dot to (-3, 0).
        quarter = [["move", 0, 0], ["draw", 0, 4], ["draw", -1, 4], ["move", -2, 1], ["circle", 0.5],
                   ["move", -2, 3], ["arc", 1, 90, 180], ["move", -3, 0], ["dot", 0]]
        # Two: every point through the origin, the arc through 180 to 270.
        half = [["move", 0, 0], ["draw", -4, 0], ["draw", -4, -1], ["move", -1, -2], ["circle", 0.5],
                ["move", -3, -2], ["arc", 1, 180, 270], ["move", 0, -3], ["dot", 0]]
        # Three: (x, y) to (y, -x), the arc through 270 to 360.
        three = [["move", 0, 0], ["draw", 0, -4], ["draw", 1, -4], ["move", 2, -1], ["circle", 0.5],
                 ["move", 2, -3], ["arc", 1, 270, 360], ["move", 3, 0], ["dot", 0]]
        for ours in (quarter, half, three):
            self.assertEqual(self.found(ours), self.ALL, ours)

    def test_a_shape_turned_and_then_moved_or_sized_is_found_moved_or_scaled(self):
        # The quarter turn above, then every point moved by (5, -3): no symmetry about the origin brings it back.
        moved = [["move", 5, -3], ["draw", 5, 1], ["draw", 4, 1], ["move", 3, -2], ["circle", 0.5],
                 ["move", 3, 0], ["arc", 1, 90, 180], ["move", 2, -3], ["dot", 0]]
        self.assertEqual(self.found(moved), ["shapes, moved", "shapes, scaled"])
        # The quarter turn at twice the size: every length doubled, and no angle.
        doubled = [["move", 0, 0], ["draw", 0, 8], ["draw", -2, 8], ["move", -4, 2], ["circle", 1],
                   ["move", -4, 6], ["arc", 2, 90, 180], ["move", -6, 0], ["dot", 0]]
        self.assertEqual(self.found(doubled), ["shapes, scaled"])

    def test_a_shape_mirrored_is_the_same_shape(self):
        # In the y axis: x to -x, and the arc, which ran from 0 to 90, runs from 180 back to 90.
        in_y = [["move", 0, 0], ["draw", -4, 0], ["draw", -4, 1], ["move", -1, 2], ["circle", 0.5],
                ["move", -3, 2], ["arc", 1, 90, 180], ["move", 0, 3], ["dot", 0]]
        # In the x axis: y to -y, the arc from 0 back to -90, which is 270 to 360.
        in_x = [["move", 0, 0], ["draw", 4, 0], ["draw", 4, -1], ["move", 1, -2], ["circle", 0.5],
                ["move", 3, -2], ["arc", 1, 270, 360], ["move", 0, -3], ["dot", 0]]
        # In the diagonal y = x: x and y change places. The arc's ends (4, 2) and (3, 3) go to (2, 4) and
        # (3, 3), which are at 90 and at 0 degrees from its new centre (2, 3).
        in_diagonal = [["move", 0, 0], ["draw", 0, 4], ["draw", 1, 4], ["move", 2, 1], ["circle", 0.5],
                       ["move", 2, 3], ["arc", 1, 0, 90], ["move", 3, 0], ["dot", 0]]
        # In the other diagonal: (x, y) to (-y, -x). The arc's ends go to (-2, -4) and (-3, -3), at 270 and at
        # 180 degrees from its new centre (-2, -3).
        in_other_diagonal = [["move", 0, 0], ["draw", 0, -4], ["draw", -1, -4], ["move", -2, -1], ["circle", 0.5],
                             ["move", -2, -3], ["arc", 1, 180, 270], ["move", -3, 0], ["dot", 0]]
        for ours in (in_y, in_x, in_diagonal, in_other_diagonal):
            self.assertEqual(self.found(ours), self.ALL, ours)

    def test_a_mark_that_differs_makes_another_shape(self):
        # The arc over the OTHER quarter, and nothing else mirrored with it: an arc is where it is.
        other_quarter = [["arc", 1, 90, 180] if s[0] == "arc" else list(s) for s in ELDER_KEY]
        # Stretched one way only, every x doubled: no turn, mirror, move or size undoes that.
        stretched = [["move", 0, 0], ["draw", 8, 0], ["draw", 8, 1], ["move", 2, 2], ["circle", 0.5],
                     ["move", 6, 2], ["arc", 1, 0, 90], ["move", 0, 3], ["dot", 0]]
        # A mark fewer (no dot), and a mark more (a bar through the ring).
        fewer = [list(s) for s in ELDER_KEY[:-2]]
        more = [list(s) for s in ELDER_KEY] + [["move", 0.5, 2], ["draw", 1.5, 2]]
        for ours in (other_quarter, stretched, fewer, more):
            self.assertEqual(self.found(ours), [], ours)

    def test_a_mark_written_twice_is_the_same_drawing(self):
        # The bottom of the L once more, backwards, after everything else.
        ours = [list(s) for s in ELDER_KEY] + [["move", 4, 0], ["draw", 0, 0]]
        self.assertEqual(self.found(ours), self.ALL)

    def test_a_pen_a_dot_s_radius_and_the_sign_of_a_radius_are_no_part_of_a_shape(self):
        # A pen is a colour; Katana draws a radius as a length whatever its sign, and a dot as one point
        # whatever its radius.
        ours = [["pen", "probe ink"], ["move", 0, 0], ["draw", 4, 0], ["draw", 4, 1], ["move", 1, 2],
                ["circle", -0.5], ["move", 3, 2], ["arc", -1, 0, 90], ["move", 0, 3], ["dot", 0.3]]
        self.assertEqual(self.found(ours), self.ALL)
        # A draw that goes nowhere marks a point, as a dot does.
        ours = [list(s) for s in ELDER_KEY[:-1]] + [["draw", 0, 3]]
        self.assertEqual(self.found(ours), self.ALL)

    def test_two_numbers_are_the_same_within_the_third_decimal_and_within_the_second_in_the_unit_box(self):
        # The ring's radius is 0.5. Its points' box is 4 wide, so in the unit box every length is a quarter.
        self.assertEqual(self.found(self.with_ring(0.5004)), self.ALL)   # 0.0004 away
        # 0.001 away is another number as drawn and as moved; in the unit box it is 0.12525 beside 0.125. (As
        # lists rounded to two decimals those two were 0.13 and 0.12, and the shape was not found.)
        self.assertEqual(self.found(self.with_ring(0.501)), ["shapes, scaled"])
        # 0.53 / 4 is 0.1325, which is 0.0075 from 0.125; 0.55 / 4 is 0.1375, which is 0.0125 from it.
        self.assertEqual(self.found(self.with_ring(0.53)), ["shapes, scaled"])
        self.assertEqual(self.found(self.with_ring(0.55)), [])

    def test_a_text_is_its_characters_its_place_and_its_height_and_is_sized_with_the_shape(self):
        def lettered(strokes, at, height, characters="E"):
            return [list(s) for s in strokes] + [["move", at[0], at[1]], ["text", {"text": characters,
                                                                                  "height": height}]]
        theirs = lettered(ELDER_KEY, (2, 1.5), 1)
        self.assertEqual(self.found(lettered(ELDER_KEY, (2, 1.5), 1), theirs), self.ALL)
        self.assertEqual(self.found(lettered(ELDER_KEY, (2, 1.5), 1, "P"), theirs), [])      # another letter
        self.assertEqual(self.found(lettered(ELDER_KEY, (2, 1.5), 1.5), theirs), [])         # another height
        self.assertEqual(self.found(lettered(ELDER_KEY, (2, 2.5), 1), theirs), [])           # another place
        # At twice the size the letter is twice as high, at twice the place; left at its height it is another
        # drawing (a quarter of the box's width in one, an eighth in the other).
        self.assertEqual(self.found(lettered(ELDER_KEY_DOUBLED, (4, 3), 2), theirs), ["shapes, scaled"])
        self.assertEqual(self.found(lettered(ELDER_KEY_DOUBLED, (4, 3), 1), theirs), [])

    def test_a_shape_that_is_all_about_one_point_is_sized_by_its_largest_radius(self):
        # Four rings and a dot about one point: the box round its points has no size to divide by.
        theirs = [["move", 0, 0], ["circle", 1], ["circle", 2], ["circle", 3], ["circle", 4], ["dot", 0]]
        elsewhere = [["move", 7, 7], ["circle", 4], ["circle", 2], ["circle", 3], ["circle", 1], ["dot", 0]]
        self.assertEqual(self.found(elsewhere, theirs), ["shapes, moved", "shapes, scaled"])
        # Twice the size: 2, 4, 6 and 8 of 8 are the quarters that 1, 2, 3 and 4 of 4 are.
        doubled = [["move", 7, 7], ["circle", 8], ["circle", 2], ["circle", 6], ["circle", 4], ["dot", 0]]
        self.assertEqual(self.found(doubled, theirs), ["shapes, scaled"])
        # 2, 4, 6 and 7 of 7 are not.
        other = [["move", 7, 7], ["circle", 7], ["circle", 2], ["circle", 6], ["circle", 4], ["dot", 0]]
        self.assertEqual(self.found(other, theirs), [])

    def test_a_list_that_draws_nothing_has_no_shape_and_dots_on_one_point_are_one_dot(self):
        # Six moves make no mark: two such lists draw nothing, and nothing is no shape to have in common.
        nothing = [["move", n, n] for n in range(6)]
        self.assertEqual(self.found(nothing, nothing), [])
        # Five dots on one point are one dot, whatever their radii: a mark written five times. A move and a
        # dot draw it, so the six strokes that wrote it do not make it a shape to compare.
        theirs = [["move", 0, 0]] + [["dot", 0] for _ in range(5)]
        ours = [["move", 3, 3]] + [["dot", 0.2] for _ in range(5)]
        self.assertEqual(self.found(ours, theirs), [])
        self.assertEqual(originality.strokes_to_draw(originality.drawing_of({"strokes": ours})), 2)

    def test_a_ring_with_a_cross_in_it_is_found_at_another_size_however_it_was_written(self):
        # Theirs: a spare move, a ring of radius 1 and the two strokes of a cross to 0.7 each way. Ours: the
        # ring first, 1.6 times the size, the cross's strokes in the other order and each from its other end.
        # In the unit box both rings are 1 / 1.4 = 1.6 / 2.24 = 0.714 about the middle, and both crosses are
        # the box's diagonals.
        theirs = [["move", 0, 0], ["move", 0, 0], ["circle", 1], ["move", -0.7, -0.7], ["draw", 0.7, 0.7],
                  ["move", -0.7, 0.7], ["draw", 0.7, -0.7]]
        ours = [["move", 0, 0], ["circle", 1.6], ["move", 1.12, -1.12], ["draw", -1.12, 1.12],
                ["move", 1.12, 1.12], ["draw", -1.12, -1.12]]
        self.assertEqual(self.found(ours, theirs), ["shapes, scaled"])


class TheDrawingAndNotTheList(Shapes):
    """A definition is compared as the DRAWING it makes where the renderer puts it: its marks less what a pen
    would not show. A line cut into pieces is the line, a dot on a line, a ring or an arc of its own definition
    is no mark, an `origin` and a `factor` put the marks where they are drawn, and an arc's angle is held as
    closely as its lengths. Which definitions take part is counted on the drawing too. Every expectation is
    worked out by hand from the rules in originality.py's header. A comparison of the marks as they are written
    found none of the pairs below that differ by a hidden dot, a cut line, a factor or a fraction of a degree,
    and left a reference definition of five strokes or fewer out whatever it drew."""

    # A square from (0, 0) to (2, 2) with both its diagonals, and one of side 3 about the origin. In the unit
    # box each is the four sides and the two diagonals of the square from (0, 0) to (1, 1).
    SQUARE = [["move", 0, 0], ["draw", 2, 0], ["draw", 2, 2], ["draw", 0, 2], ["draw", 0, 0],
              ["move", 0, 0], ["draw", 2, 2], ["move", 0, 2], ["draw", 2, 0]]
    OUR_SQUARE = [["move", -1.5, -1.5], ["draw", 1.5, -1.5], ["draw", 1.5, 1.5], ["draw", -1.5, 1.5],
                  ["draw", -1.5, -1.5], ["move", -1.5, -1.5], ["draw", 1.5, 1.5], ["move", -1.5, 1.5],
                  ["draw", 1.5, -1.5]]

    @staticmethod
    def strokes_to_draw(strokes):
        return originality.strokes_to_draw(originality.drawing_of({"strokes": strokes}))

    @staticmethod
    def joined(*lines):
        """The lines `originality.joined` makes of these, each as its two ends in order, in order."""
        return sorted(tuple(sorted((line[:2], line[2:]))) for line in originality.joined(lines))

    def test_a_dot_where_the_lines_of_its_definition_cross_is_no_mark(self):
        # (1, 1) is on both diagonals of the reference's square, so its dot there draws nothing that a line has
        # not drawn, and the two squares are one shape at another place and size.
        self.assertEqual(self.found(self.OUR_SQUARE, self.SQUARE + [["move", 1, 1], ["dot", 0]]),
                         ["shapes, scaled"])
        # The other way round: ours with a dot where its diagonals cross, theirs with none.
        self.assertEqual(self.found(self.OUR_SQUARE + [["move", 0, 0], ["dot", 0.3]], self.SQUARE),
                         ["shapes, scaled"])
        # (1, 0.5) is on no side (x or y is 0 or 2 on those) and on no diagonal (y is x on one and 2 - x on the
        # other). A dot there is a mark, and the other square has none.
        self.assertEqual(self.found(self.OUR_SQUARE, self.SQUARE + [["move", 1, 0.5], ["dot", 0]]), [])

    def test_a_dot_on_a_ring_or_on_an_arc_is_no_mark_and_a_dot_beside_one_is(self):
        # The key's ring has radius 0.5 about (1, 2), and its arc is the quarter from 0 to 90 degrees about
        # (3, 2) with radius 1. (1.5, 2) is on the ring. (3.6, 2.8) is 0.6 and 0.8 from the arc's centre, so 1
        # from it, at 53 degrees: on the arc.
        on_both = ELDER_KEY + [["move", 1.5, 2], ["dot", 0], ["move", 3.6, 2.8], ["dot", 0]]
        self.assertEqual(self.found(ELDER_KEY, on_both), self.ALL)
        # (2.4, 2.8) is as far from that centre at 127 degrees: on the arc's circle, and past its end.
        self.assertEqual(self.found(ELDER_KEY, ELDER_KEY + [["move", 2.4, 2.8], ["dot", 0]]), [])
        # The middle of a ring is not on the ring: a ring with a dot in it is two marks.
        self.assertEqual(self.found(ELDER_KEY, ELDER_KEY + [["move", 1, 2], ["dot", 0]]), [])

    def test_a_line_written_in_pieces_is_the_line(self):
        rest = ELDER_KEY[2:]   # the upright of the L, from (4, 0), and everything after it
        # The bottom of the L, from (0, 0) to (4, 0), as four pieces drawn end to end.
        chained = [["move", 0, 0], ["draw", 1, 0], ["draw", 2, 0], ["draw", 3, 0], ["draw", 4, 0]] + rest
        self.assertEqual(self.found(chained), self.ALL)
        # As two lines that overlap from 2 to 3, the second drawn backwards.
        overlapping = [["move", 0, 0], ["draw", 3, 0], ["move", 4, 0], ["draw", 2, 0], ["move", 4, 0]] + rest
        self.assertEqual(self.found(overlapping), self.ALL)
        # With nothing from 1.5 to 2 it is two lines, and the key has no such gap: a gap is no ink.
        broken = [["move", 0, 0], ["draw", 1.5, 0], ["move", 2, 0], ["draw", 4, 0]] + rest
        self.assertEqual(self.found(broken), [])

    def test_only_lines_on_one_straight_line_that_touch_or_overlap_are_joined(self):
        self.assertEqual(self.joined((0, 0, 1, 0), (1, 0, 2, 0)), [((0, 0), (2, 0))])
        self.assertEqual(self.joined((0, 0, 4, 0), (1, 0, 2, 0)), [((0, 0), (4, 0))])        # one within the other
        self.assertEqual(self.joined((0, 0, 1, 1), (2, 2, 1, 1)), [((0, 0), (2, 2))])        # one of them backwards
        # The piece that joins two others, written last.
        self.assertEqual(self.joined((0, 0, 1, 0), (2, 0, 3, 0), (1, 0, 2, 0)), [((0, 0), (3, 0))])
        # A plus written as four spokes from its middle is two lines.
        self.assertEqual(self.joined((0, 0, 1, 0), (0, 0, -1, 0), (0, 0, 0, 1), (0, 0, 0, -1)),
                         [((-1, 0), (1, 0)), ((0, -1), (0, 1))])
        # Not joined: two lines that meet at a corner, two that run side by side, two with a gap between them.
        self.assertEqual(self.joined((0, 0, 1, 0), (1, 0, 1, 1)), [((0, 0), (1, 0)), ((1, 0), (1, 1))])
        self.assertEqual(self.joined((0, 0, 2, 0), (0, 0.5, 2, 0.5)), [((0, 0), (2, 0)), ((0, 0.5), (2, 0.5))])
        self.assertEqual(self.joined((0, 0, 1, 0), (1.5, 0, 2, 0)), [((0, 0), (1, 0)), ((1.5, 0), (2, 0))])

    def test_the_strokes_that_draw_a_drawing_are_counted_on_the_drawing_and_not_on_the_list(self):
        count = self.strokes_to_draw
        dash = [["move", 0, 0], ["draw", 3, 0]]
        self.assertEqual(count(dash), 2)
        self.assertEqual(count([["move", 0, 0]] + [["draw", n, 0] for n in range(1, 11)]), 2)   # in ten pieces
        self.assertEqual(count(dash + [["move", 1, 0], ["dot", 0]]), 2)                         # a dot on it
        self.assertEqual(count(dash + [["move", 1, 1], ["text", {"text": "W", "height": 1}]]), 4)
        plus = [["move", -1, 0], ["draw", 1, 0], ["move", 0, -1], ["draw", 0, 1]]
        self.assertEqual(count(plus), 4)   # its two lines cross and do not meet at an end: two paths
        self.assertEqual(count([["move", 0, 0], ["draw", 1, 0], ["move", 0, 0], ["draw", -1, 0],
                                ["move", 0, 0], ["draw", 0, 1], ["move", 0, 0], ["draw", 0, -1]]), 4)
        # A square is one path round, as one path or as four lines of two strokes each.
        self.assertEqual(count(self.SQUARE[:5]), 5)
        self.assertEqual(count([["move", 0, 0], ["draw", 2, 0], ["move", 2, 2], ["draw", 2, 0],
                                ["move", 2, 2], ["draw", 0, 2], ["move", 0, 0], ["draw", 0, 2]]), 5)
        # With both diagonals three lines meet at each of its four corners: two paths, and six lines.
        self.assertEqual(count(self.SQUARE), 8)
        self.assertEqual(count([["move", 0, 0], ["draw", 1, 1], ["draw", 2, 0], ["draw", 3, 1]]), 4)   # a zigzag
        # A bar and a stem to its middle: the stem ends ON the bar and at no end of it, so they are two paths.
        self.assertEqual(count([["move", 0, 0], ["draw", 2, 0], ["move", 1, 0], ["draw", 1, -1]]), 4)
        # Three lines from one point: one line ends at each of three points and three at the fourth.
        self.assertEqual(count([["move", 0, 0], ["draw", 1, 0], ["move", 0, 0], ["draw", -1, 1],
                                ["move", 0, 0], ["draw", -1, -1]]), 5)
        ring = [["move", 0, 0], ["circle", 1]]
        self.assertEqual(count(ring), 2)
        self.assertEqual(count(ring + ring), 2)                                       # written twice
        self.assertEqual(count(ring + [["dot", 0]]), 4)                               # a dot in its middle
        self.assertEqual(count(ring + [["circle", 2], ["circle", 3]]), 6)             # a move for each ring
        # A ring with two chords across it, from (-0.6, -0.8) and from (0.6, -0.8) straight up: two more paths.
        self.assertEqual(count(ring + [["move", -0.6, -0.8], ["draw", -0.6, 0.8],
                                       ["move", 0.6, -0.8], ["draw", 0.6, 0.8]]), 6)

    def test_a_drawing_of_five_strokes_or_fewer_is_left_out_however_many_strokes_wrote_it(self):
        # A dash from 0 to 6 written as six pieces is seven strokes, and a move and a draw make it. The
        # reference has the dash at half the length; a dash is anybody's.
        reference, candidate = elder(), probe()
        reference["linestyles"][0]["strokes"] = [["move", 0, 0], ["draw", 3, 0]]
        candidate["linestyles"][0]["strokes"] = [["move", 0, 0]] + [["draw", n, 0] for n in range(1, 7)]
        result = originality.scan(reference, candidate)
        self.assertEqual(failing(result), {})
        self.assertEqual(result["definitions"], (2, 2, 2))   # the dash and the mark of one ring are both left out

    def test_every_definition_of_the_reference_takes_part_however_few_strokes_wrote_it(self):
        # Three rings about one point. The reference writes them after ONE move, in four strokes; ours has a
        # move for each, at twice the size and elsewhere. Sized by the largest radius both are rings of a
        # third, two thirds and the whole.
        theirs = [["move", 0, 0], ["circle", 1], ["circle", 2], ["circle", 3]]
        ours = [["move", 5, 5], ["circle", 2], ["move", 5, 5], ["circle", 4], ["move", 5, 5], ["circle", 6]]
        self.assertEqual(self.found(ours, theirs), ["shapes, scaled"])

    def test_a_definition_is_read_where_its_origin_and_its_factor_put_it(self):
        # The key written at a quarter of its size about (1, 2): a point p is written p / 4 + (1, 2) and a
        # radius r is written r / 4. The origin (1, 2) and the factor 4 put every one of them back where the
        # key has it, so the two are one drawing as they are drawn and not only at one size.
        quarter = [["move", 1, 2], ["draw", 2, 2], ["draw", 2, 2.25], ["move", 1.25, 2.5], ["circle", 0.125],
                   ["move", 1.75, 2.5], ["arc", 0.25, 0, 90], ["move", 1, 2.75], ["dot", 0]]
        placing = {"origin": [1, 2], "factor": 4}
        self.assertEqual(self.found(ELDER_KEY, quarter, placing), self.ALL)
        # Without them it is the key elsewhere, at a quarter of its size.
        self.assertEqual(self.found(ELDER_KEY, quarter), ["shapes, scaled"])
        # A text's height is not sized by the factor, since the renderer sizes a text by where the definition
        # is put: the letter written 1 high at (1.5, 2.375) is drawn 1 high at (2, 1.5).
        lettered = quarter + [["move", 1.5, 2.375], ["text", {"text": "E", "height": 1}]]
        same = ELDER_KEY + [["move", 2, 1.5], ["text", {"text": "E", "height": 1}]]
        taller = ELDER_KEY + [["move", 2, 1.5], ["text", {"text": "E", "height": 4}]]
        self.assertEqual(self.found(same, lettered, placing), self.ALL)
        self.assertEqual(self.found(taller, lettered, placing), [])

    def test_an_angle_is_the_same_when_no_point_of_the_arc_is_a_step_apart_for_it(self):
        def swept_to(end):
            return [["arc", 1, 0, end] if s[0] == "arc" else list(s) for s in ELDER_KEY]

        # The key's arc has radius 1, so a degree more moves its end 0.01745 round the circle; its box is 4
        # wide. 0.03 of a degree moves the end 0.0005: the same arc as it is drawn.
        self.assertEqual(self.found(swept_to(90.03)), self.ALL)
        # 0.1 of a degree moves it 0.0017, which is apart as drawn and 0.0004 in the unit box.
        self.assertEqual(self.found(swept_to(90.1)), ["shapes, scaled"])
        # Two degrees move it 0.035, which is 0.0087 in the unit box; three move it 0.052, which is 0.013.
        self.assertEqual(self.found(swept_to(92)), ["shapes, scaled"])
        self.assertEqual(self.found(swept_to(93)), [])


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
