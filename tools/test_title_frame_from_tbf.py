#!/usr/bin/env python3
"""Tests of title_frame_from_tbf.py on a small invented title block file, every expectation worked
out by hand from the format's rules in that script's header (and, for the colours, from the CAD
colour index's published hue and shade rule, checked against the two colours the default frame's
measured palette gives for it: acad 030 #ff8000 and acad 162 #0029a6).

    python tools/test_title_frame_from_tbf.py
"""
import io
import json
import math
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import title_frame_from_tbf as converter  # noqa: E402

# An invented A4 landscape frame (297 x 210 mm), its entities in this order:
#   0 the border, a closed rectangle 19,29 - 288,201 (its first point repeated)
#   1 the title block, a closed rectangle 19,9 - 288,29
#   2 a dashed underline 100,15 - 150,15 in "DGN Style 1", colour acad 030
#   3 a guide on the paper's edge, 0,0 - 297,210
#   4 a filled shape: an arc of radius 5 from 200,12 to 210,12, then a line to 205,20
#   5 the label SCALE, at 30,20 with offset 1 and raise 0.5, bold
#   6 a text of three fields, turned 90 degrees
#   7 a symbol at 40,15
#   8 a two-line text with quotes, its colour given only as the string's colour
FIXTURE = r'''title_block {

  name ""

  info {
    text    "plot_frame_name"   "PROBE FRAME"
    text    "plot_frame_sheet_size"   "A4 Landscape"
    real    "plot_frame_left_margin"   20
    real    "plot_frame_right_margin"   10
    real    "plot_frame_top_margin"   10
    real    "plot_frame_bottom_margin"   30
    integer "plot_frame_viewport"   1
  }
  data {
    string super {
      name      "Probe Borders"
      colour    white
      style     "1"
      weight    0.5

      closed 1
      data_2d {
        19 201
        288 201
        288 29
        19 29
        19 201
      }
    }
    string super {
      name      "Probe Borders"
      colour    white
      style     "1"
      weight    0.5

      closed 1
      data_2d {
        19 29
        288 29
        288 9
        19 9
      }
    }
    string super {
      name      "Probe Borders"
      colour    "acad 030"
      style     "DGN Style 1"
      weight    0.13

      closed 0
      data_2d {
        100 15
        150 15
      }
    }
    string super {
      name      "Probe Guide"
      colour    "acad 030"
      style     "1"

      closed 1
      data_2d {
        0 0
        297 0
        297 210
        0 210
      }
    }
    string super {
      name      ""
      colour    "acad 162"
      style     "1"

      closed 0
      data_3d {
        200 12 0
        210 12 -999
        205 20 -999
      }
      radius_data {
        5 0 0
      }
      major_data {
        f f f
      }
      solid_fill {
        colour    "probe ink"
        blend     1
      }
    }
    string text {
      name      "Probe Labels"
      colour    "pen 050"
      style     "1"

      worldsize   2
      textstyle   "82Arial"
      angle       0
      x_factor    0.82
      slant       0
      offset      1
      raise       0.5
      text_colour "acad 007"
      justify     "bottom-left"
      ttf_weight    700

      x         30
      y         20
      z         -999
      text "SCALE"
    }

    string text {
      name      "Probe Labels"
      colour    white
      style     "1"

      worldsize   1.5
      textstyle   "Arial Narrow"
      angle       90
      x_factor    1
      slant       0
      offset      0
      raise       0
      text_colour white
      justify     "middle-centre"

      x         60
      y         20
      z         -999
      text "$user_text<17,SCALE> / $drawing_number $time<%d/%m/%y>"
    }

    string super {
      name      "PROBE"
      breakline point
      colour    green
      style     "0"

      closed 0
      data_3d {
        40 15 -999
      }
      symbol_data {
        properties {
          style     "PROBE Mark"
          colour    "probe ink"
          size      0.5
          rotation  30
          offset    0
          raise     0
        }
      }
    }
    string text {
      name      ""
      colour    "pen 007"
      style     "1"

      worldsize   1
      x         150
      y         25
      text "SAY \"HI\"\nTWO"
    }
  }
}
'''

PALETTE = {"probe ink": "#112233"}


def convert(text=FIXTURE, **options):
    options.setdefault("palette", PALETTE)
    return converter.convert(text, "probe.tbf", **options)


def entity(frame, index):
    return next(e for e in frame["entities"] if e["index"] == index)


class Sheet(unittest.TestCase):
    def test_the_sheet_is_the_named_iso_paper_with_the_files_margins(self):
        sheet = convert()["sheet"]
        # A4 is 210 x 297; landscape turns it.
        self.assertEqual((sheet["width_mm"], sheet["height_mm"]), (297, 210))
        self.assertEqual(sheet["margins_mm"], {"left": 20, "right": 10, "top": 10, "bottom": 30})
        # The viewport: the paper inside the margins, 20..287 x 30..200.
        self.assertEqual(sheet["viewport_tbf"], {"x0": 20, "y0": 30, "x1": 287, "y1": 200})
        self.assertEqual(sheet["sheet_edge_tbf"], {"x0": 0, "y0": 0, "x1": 297, "y1": 210})

    def test_the_border_and_the_title_block_are_found_among_the_closed_lines(self):
        sheet = convert()["sheet"]
        # The largest closed line around the viewport's centre, and the largest under it.
        self.assertEqual(sheet["drawing_border_tbf"],
                         {"x0": 19, "y0": 29, "x1": 288, "y1": 201, "entity": 0, "weight_mm": 0.5})
        self.assertEqual(sheet["title_block_tbf"],
                         {"x0": 19, "y0": 9, "x1": 288, "y1": 29, "entity": 1, "weight_mm": 0.5})

    def test_a_paper_without_a_table_needs_its_size_given(self):
        text = FIXTURE.replace('"A4 Landscape"', '"B2 Landscape"')
        with self.assertRaises(converter.ConversionError) as caught:
            convert(text)
        self.assertIn("B2 Landscape", str(caught.exception))
        sheet = convert(text, size="500x300")["sheet"]
        self.assertEqual((sheet["width_mm"], sheet["height_mm"]), (500.0, 300.0))

    def test_margins_that_leave_no_drawing_area_are_refused(self):
        text = FIXTURE.replace('"plot_frame_left_margin"   20', '"plot_frame_left_margin"   290')
        with self.assertRaises(converter.ConversionError):
            convert(text)

    def test_counts_and_the_info_block_are_recorded(self):
        converted = convert()["converted_from"]
        self.assertEqual(converted["counts"], {"poly": 5, "text": 3, "symbol": 1})
        self.assertEqual(converted["info"]["plot_frame_name"], "PROBE FRAME")
        self.assertEqual(converted["file"], "probe.tbf")


class Lines(unittest.TestCase):
    def test_every_line_keeps_its_points_colour_weight_and_style(self):
        frame = convert()
        border = entity(frame, 0)
        self.assertEqual(border["points_tbf"], [[19, 201], [288, 201], [288, 29], [19, 29], [19, 201]])
        self.assertTrue(border["closed"])
        self.assertEqual(border["weight_mm"], 0.5)
        # CAD white plots black on paper.
        self.assertEqual(border["colour_hex"], "#000000")
        self.assertIsNone(border["dash_mm"])
        self.assertEqual(border["role"], "border")

        underline = entity(frame, 2)
        self.assertEqual(underline["dash_mm"], [2.2, 1.1])
        self.assertEqual(underline["role"], "field-underline")
        self.assertEqual(underline["orientation"], "horizontal")
        self.assertEqual(underline["colour_hex"], "#ff8000")

    def test_a_line_without_a_weight_takes_the_programs_default(self):
        self.assertEqual(entity(convert(), 3)["weight_mm"], 0.18)

    def test_a_line_on_the_papers_edge_is_a_construction_guide(self):
        self.assertEqual(entity(convert(), 3)["role"], "construction")

    def test_an_arc_is_drawn_counter_clockwise_for_a_positive_radius_within_the_tolerance(self):
        shape = entity(convert(), 4)
        points = shape["points_tbf"]
        self.assertEqual(shape["arcs"], [{"segment": 0, "radius": 5, "major": False}])
        # The chord 200,12 - 210,12 is a diameter: the centre is 205,12 and the half turn
        # counter-clockwise from 200,12 passes BELOW the chord. Its pieces subtend
        # 2 acos(1 - 0.005 / 5) = 0.0894 rad at most: 36 pieces for pi, so 35 points
        # between the ends, the 18th of them at the bottom, 205,7.
        self.assertEqual(points[0], [200, 12])
        self.assertEqual(points[36], [210, 12])
        self.assertEqual(points[37], [205, 20])
        self.assertEqual(len(points), 38)
        self.assertEqual(points[18], [205.0, 7.0])
        for x, y in points[1:36]:
            self.assertAlmostEqual(math.hypot(x - 205, y - 12), 5, delta=1e-6)
            self.assertLessEqual(y, 12)
        # No piece strays further than the tolerance from the arc.
        for (x0, y0), (x1, y1) in zip(points[:36], points[1:37]):
            middle = math.hypot((x0 + x1) / 2 - 205, (y0 + y1) / 2 - 12)
            self.assertLessEqual(5 - middle, converter.ARC_TOLERANCE_MM + 1e-9)

    def test_a_negative_radius_turns_the_other_way(self):
        text = FIXTURE.replace("        5 0 0\n", "        -5 0 0\n")
        points = entity(convert(text), 4)["points_tbf"]
        self.assertTrue(all(y >= 12 for x, y in points[1:36]))
        self.assertEqual(points[18], [205.0, 17.0])

    def test_a_filled_line_keeps_its_fill_colour(self):
        shape = entity(convert(), 4)
        self.assertEqual(shape["fill_token"], "probe ink")
        self.assertEqual(shape["fill_hex"], "#112233")
        self.assertEqual(shape["colour_hex"], "#0029a6")
        self.assertNotIn("fill_hex", entity(convert(), 0))

    def test_an_unknown_line_style_is_refused_by_name(self):
        text = FIXTURE.replace('"DGN Style 1"', '"DGN Style 9"')
        with self.assertRaises(converter.ConversionError) as caught:
            convert(text)
        self.assertIn("DGN Style 9", str(caught.exception))


class Texts(unittest.TestCase):
    def test_a_text_is_anchored_at_its_position_plus_its_offset_and_raise(self):
        label = entity(convert(), 5)
        self.assertEqual((label["x_tbf"], label["y_tbf"]), (30, 20))
        self.assertEqual(label["anchor_tbf"], [31, 20.5])
        self.assertEqual(label["cap_height_mm"], 2)
        # 2 / 0.716 = 2.7932...
        self.assertEqual(label["em_size_mm"], 2.793)
        self.assertEqual(label["font_face"], "Arial")
        self.assertEqual(label["font_style_token"], "82Arial")
        self.assertTrue(label["bold"])
        self.assertEqual(label["x_factor"], 0.82)
        self.assertEqual(label["justify"], "bottom-left")
        self.assertEqual(label["colour_hex"], "#000000")
        self.assertEqual(label["role"], "label")
        self.assertEqual(label["fields"], [])

    def test_a_text_with_tokens_is_a_field_and_names_them_as_the_default_frame_does(self):
        frame = convert()
        field = entity(frame, 6)
        self.assertEqual(field["role"], "field")
        self.assertEqual(field["fields"], ["user_text_17", "drawing_number", "plot_date"])
        self.assertEqual(field["angle_deg"], 90)
        self.assertEqual(field["font_face"], "Arial Narrow")
        self.assertFalse(field["bold"])
        self.assertEqual(field["justify"], "middle-centre")
        keys = [(f["key"], f["label_in_tbf"], f["entities"]) for f in frame["fields"]]
        self.assertEqual(keys, [("user_text_17", "SCALE", [6]),
                                ("drawing_number", "Drawing Number", [6]),
                                ("plot_date", "Plot Date", [6])])

    def test_quotes_and_line_breaks_are_read_as_the_text_holds_them(self):
        text = entity(convert(), 8)
        self.assertEqual(text["raw_text"], 'SAY "HI"\nTWO')
        self.assertEqual(text["lines"], 2)
        # No text colour: the string's own, a black pen. Nothing else given: the defaults.
        self.assertEqual(text["colour_hex"], "#000000")
        self.assertEqual(text["justify"], "bottom-left")
        self.assertEqual(text["cap_height_mm"], 1.0)
        self.assertEqual(text["font_face"], "Arial")


class Symbols(unittest.TestCase):
    def test_a_symbol_keeps_its_style_place_size_turn_and_colour(self):
        symbol = entity(convert(), 7)
        self.assertEqual(symbol["kind"], "symbol")
        self.assertEqual(symbol["symbol_style"], "PROBE Mark")
        self.assertEqual(symbol["at_tbf"], [40, 15])
        self.assertEqual(symbol["size_mm"], 0.5)
        self.assertEqual(symbol["rotation_deg"], 30)
        self.assertEqual(symbol["colour_hex"], "#112233")


class Cells(unittest.TestCase):
    def test_without_cells_the_frame_has_its_drawing_area_alone_and_no_user_slots(self):
        frame = convert()
        self.assertEqual([cell["id"] for cell in frame["cells"]], ["drawing_area"])
        self.assertEqual(frame["cells"][0]["tbf"], [19, 29, 288, 201])
        self.assertEqual(frame["user_slots"], [])

    def test_given_cells_decide_the_legend_and_the_stamp(self):
        cells = [{"id": "legend", "tbf": [25, 10, 50, 25], "purpose": "another frame's words"},
                 {"id": "stamp", "tbf": [140, 22, 160, 28]}]
        frame = convert(cells=cells, user_slots=["logo"])
        # Inside the legend cell: the symbol at 40,15 and the label anchored at 31,20.5.
        self.assertEqual(entity(frame, 7)["role"], "legend")
        self.assertEqual(entity(frame, 5)["role"], "legend")
        # The text anchored at 150,25 is in the stamp cell.
        self.assertEqual(entity(frame, 8)["role"], "stamp")
        # A cell's id and rectangle are copied; its words were about another frame.
        self.assertEqual(frame["cells"][1], {"id": "legend", "tbf": [25, 10, 50, 25]})
        self.assertEqual(frame["cells"][0]["id"], "drawing_area")
        self.assertEqual(frame["user_slots"], ["logo"])


class Colours(unittest.TestCase):
    def test_the_cad_colour_index_follows_its_hue_and_shade_rule(self):
        self.assertEqual(converter.aci_colour(1), "#ff0000")
        self.assertEqual(converter.aci_colour(7), "#000000")
        self.assertEqual(converter.aci_colour(10), "#ff0000")
        # Hue 30 degrees, full value: green is 255 x 0.5 = 127.5, rounded up.
        self.assertEqual(converter.aci_colour(30), "#ff8000")
        # Hue 225 degrees at value 0.65: blue 165.75 -> 166, green 0.65 x 0.25 x 255 = 41.4 -> 41.
        self.assertEqual(converter.aci_colour(162), "#0029a6")
        # An odd last digit is half saturation: hue 0, value 1, red 255, the others 127.5 -> 128.
        self.assertEqual(converter.aci_colour(11), "#ff8080")
        self.assertEqual(converter.aci_colour(250), "#333333")
        with self.assertRaises(converter.ConversionError):
            converter.aci_colour(256)

    def test_a_colour_name_the_palette_does_not_give_is_refused_by_name(self):
        with self.assertRaises(converter.ConversionError) as caught:
            convert(palette={})
        self.assertIn("probe ink", str(caught.exception))

    def test_the_palette_records_every_colour_used(self):
        palette = convert()["palette"]
        self.assertEqual(palette, {"acad 007": "#000000", "acad 030": "#ff8000",
                                   "acad 162": "#0029a6", "pen 007": "#000000",
                                   "probe ink": "#112233", "white": "#000000"})


class Reading(unittest.TestCase):
    def test_utf16_with_a_byte_order_mark_reads_as_utf8_does(self):
        utf8 = convert()
        utf16 = converter.convert(converter.decode(b"\xff\xfe" + FIXTURE.encode("utf-16-le")),
                                  "probe.tbf", palette=PALETTE)
        self.assertEqual(utf16, utf8)
        bom = converter.decode(b"\xef\xbb\xbf" + FIXTURE.replace("\n", "\r\n").encode("utf-8"))
        self.assertEqual(converter.convert(bom, "probe.tbf", palette=PALETTE), utf8)

    def test_an_unclosed_block_is_refused(self):
        with self.assertRaises(converter.ConversionError):
            convert(FIXTURE.rstrip().rstrip("}"))

    def test_a_file_without_a_title_block_is_refused(self):
        with self.assertRaises(converter.ConversionError):
            convert("data {\n}\n")


class Command(unittest.TestCase):
    def test_the_command_writes_the_frame_with_line_feeds_and_reads_back(self):
        with tempfile.TemporaryDirectory() as folder:
            source = os.path.join(folder, "probe.tbf")
            palette = os.path.join(folder, "palette.json")
            out = os.path.join(folder, "probe.json")
            with open(source, "w", encoding="utf-8", newline="\r\n") as stream:
                stream.write(FIXTURE)
            with open(palette, "w", encoding="utf-8", newline="\n") as stream:
                json.dump(PALETTE, stream)
            printed = io.StringIO()
            stdout, sys.stdout = sys.stdout, printed
            try:
                code = converter.main([source, out, "--palette", palette, "--user-slots", "notes"])
            finally:
                sys.stdout = stdout
            self.assertEqual(code, 0)
            self.assertIn("5 lines, 3 texts, 1 symbols", printed.getvalue())
            with open(out, "rb") as stream:
                written = stream.read()
            self.assertNotIn(b"\r", written)
            self.assertTrue(written.endswith(b"}\n"))
            frame = json.loads(written.decode("utf-8"))
            self.assertEqual(frame["user_slots"], ["notes"])
            self.assertEqual(frame["entities"], convert()["entities"])

    def test_a_user_slot_that_is_not_one_is_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            source = os.path.join(folder, "probe.tbf")
            with open(source, "w", encoding="utf-8", newline="\n") as stream:
                stream.write(FIXTURE)
            with self.assertRaises(converter.ConversionError):
                converter.main([source, os.path.join(folder, "out.json"), "--user-slots", "signature"])


if __name__ == "__main__":
    unittest.main()
