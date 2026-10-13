#!/usr/bin/env python3
"""Tests of how `make_katana_standard.py --check` holds the catalogue's pictures: by the record each carries, in
the text of its PNG, of what it was drawn from. The pictures here are one pixel, written by this file with `zlib`
and `struct` alone; what they are drawn from is three small files in a scratch folder and a scene of two commands,
never the library. Every digest expected was worked out outside Python, with coreutils:

    printf 'abc' | sha256sum                    ba7816bf...  (the first vector of FIPS 180-2, appendix B.1, too)
    printf 'abd' | sha256sum                    a52d159f...
    printf 'a\\nb\\n' | sha256sum                 911169dd...
    printf 'a\\r\\nb\\r\\n' | sha256sum             58055bdc...
    printf 'x = 1\\n' | sha256sum                 9e26bf36...
    printf 'POINT 1,2\\nCODE\\n' | sha256sum      2dc10c8d...

    python tools/katana_standard/test_pictures.py
"""
import os
import struct
import sys
import tempfile
import unittest
import zlib
from unittest import mock

sys.dont_write_bytecode = True  # as the generator: nothing is left behind in the tools folder
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import make_katana_standard as standard  # noqa: E402

ABC = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
ABD = "sha256:a52d159f262b2c6ddb724a61840befc36eb30c88877a4030b65cbe86298449c9"
TWO_LINES = "sha256:911169ddaaf146aff539f58c26c489af3b892dff0fe283c1c264c65ae5aa59a2"
TWO_LINES_CRLF = "sha256:58055bdcc73787eb88c78d36f0b4939e9c5dc1c3ad17e25cc85a6833cf1a0cab"
ONE_COLOUR = "sha256:9e26bf369911c45c243c684147b23fc9e1dcfcf257d299a1c632016a6fcd33f4"
TWO_COMMANDS = "sha256:2dc10c8d1ba9a266e5d4ee4100fa9c34ebfc76e4b1e9959419114ddb6b564c9c"

CUSTOMISATION = "katana-standard:customisation"
FIELD_FILE = "katana-standard:field-file"
SCENE = "katana-standard:scene"
PALETTE = "katana-standard:palette"


def chunk(kind, body):
    """One chunk of a PNG: the body's length, the type, the body, and the CRC of type and body."""
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))


def text_chunk(keyword, text):
    return chunk(b"tEXt", keyword.encode("latin-1") + b"\0" + text.encode("latin-1"))


def png(texts=()):
    """A PNG of one grey pixel with a text chunk for each (keyword, text) of `texts`, before the pixel."""
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 0, 0, 0, 0))
            + b"".join(text_chunk(keyword, text) for keyword, text in texts)
            + chunk(b"IDAT", zlib.compress(b"\0\0")) + chunk(b"IEND", b""))


class Scene:
    """What stands for showcase.py: a scene of the commands given."""

    def __init__(self, lines):
        self.lines = lines

    def commands(self):
        return self.lines, []


class Pictures(unittest.TestCase):
    """A scratch folder that holds the pictures and everything they are drawn from: a customisation file `abc`, a
    field file of two lines, a palette of one line and a scene of two commands."""

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.folder = scratch.name
        self.customisation = self.write("library.json", b"abc")
        self.field_file = self.write("corner.fld", b"a\nb\n")
        self.palette = self.write("colours.py", b"x = 1\n")
        self.scene = Scene(["POINT 1,2", "CODE"])
        for name, value in (("JSON_PATH", self.customisation), ("FIELD_FILE", self.field_file),
                            ("PALETTE_FILE", self.palette), ("_module", self.module)):
            patch = mock.patch.object(standard, name, value)
            patch.start()
            self.addCleanup(patch.stop)

    def module(self, name):
        self.assertEqual(name, "showcase")   # the one module a record is made from
        return self.scene

    def write(self, name, payload):
        path = os.path.join(self.folder, name)
        with open(path, "wb") as handle:
            handle.write(payload)
        return path

    def picture(self, sheet, texts=()):
        self.write(os.path.basename(standard.picture_path(sheet, self.folder)), png(texts))

    def problem(self, sheet):
        return standard.picture_problem(sheet, self.folder)


class TheTextOfAPng(Pictures):
    def test_every_text_chunk_is_read_and_a_picture_with_none_has_none(self):
        path = self.write("two.png", png([("Title", "a street corner"), (CUSTOMISATION, ABC)]))
        self.assertEqual(standard.png_text(path), {"Title": "a street corner", CUSTOMISATION: ABC})
        self.assertEqual(standard.png_text(self.write("none.png", png())), {})

    def test_a_file_that_is_not_there_or_is_no_png_has_no_text_to_read(self):
        self.assertIsNone(standard.png_text(os.path.join(self.folder, "nowhere.png")))
        self.assertIsNone(standard.png_text(self.write("empty.png", b"")))
        self.assertIsNone(standard.png_text(self.write("other.png", b"GIF89a" + png([(CUSTOMISATION, ABC)]))))

    def test_a_file_cut_short_gives_the_chunks_that_are_whole(self):
        whole = png([(CUSTOMISATION, ABC), (FIELD_FILE, TWO_LINES)])
        second = whole.index(b"tEXt", whole.index(b"tEXt") + 4)
        # cut ten characters into the second chunk's text (half a digest is no record), inside its keyword, inside
        # its header, and just after the first chunk
        for end in (second + 4 + len(FIELD_FILE) + 1 + 10, second + 20, second - 2, second - 4):
            with self.subTest(end=end):
                self.assertEqual(standard.png_text(self.write("cut.png", whole[:end])), {CUSTOMISATION: ABC})
        # and inside the first: nothing is whole
        self.assertEqual(standard.png_text(self.write("cut.png", whole[:whole.index(b"tEXt") + 20])), {})

    def test_a_text_chunk_with_no_keyword_separator_is_not_a_record(self):
        broken = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 0, 0, 0, 0))
                  + chunk(b"tEXt", b"no separator here") + text_chunk(CUSTOMISATION, ABC) + chunk(b"IEND", b""))
        self.assertEqual(standard.png_text(self.write("broken.png", broken)), {CUSTOMISATION: ABC})

    def test_the_library_that_draws_the_pictures_writes_what_is_read_here_and_reads_what_is_written_here(self):
        try:
            from PIL import Image, PngImagePlugin
        except ImportError:
            self.skipTest("no Pillow: nothing here draws a picture, and the check needs none")
        record = PngImagePlugin.PngInfo()
        record.add_text(CUSTOMISATION, ABC)
        record.add_text(SCENE, TWO_COMMANDS)
        path = os.path.join(self.folder, "pillow.png")
        # saved as render_images.py saves a sheet: a palette image, optimised
        Image.new("RGB", (3, 2), (30, 35, 41)).quantize(colors=32).save(path, optimize=True, pnginfo=record)
        self.assertEqual(standard.png_text(path), {CUSTOMISATION: ABC, SCENE: TWO_COMMANDS})
        with Image.open(self.write("ours.png", png([(PALETTE, ONE_COLOUR)]))) as ours:
            self.assertEqual(ours.text, {PALETTE: ONE_COLOUR})


class WhatAPictureRecords(Pictures):
    def test_each_picture_records_the_digest_of_each_thing_it_is_drawn_from(self):
        self.assertEqual(standard.picture_record("symbols"), {CUSTOMISATION: ABC})
        self.assertEqual(standard.picture_record("linestyles"), {CUSTOMISATION: ABC})
        self.assertEqual(standard.picture_record("plan"), {CUSTOMISATION: ABC, FIELD_FILE: TWO_LINES})
        self.assertEqual(standard.picture_record("showcase"), {CUSTOMISATION: ABC, SCENE: TWO_COMMANDS})
        self.assertEqual(standard.picture_record("palette"), {PALETTE: ONE_COLOUR})

    def test_every_picture_of_the_catalogue_has_a_record_and_a_way_to_draw_it_again(self):
        self.assertEqual(sorted(standard.PICTURES), ["linestyles", "palette", "plan", "showcase", "symbols"])
        for sheet, (sources, redraw) in standard.PICTURES.items():
            with self.subTest(sheet=sheet):
                self.assertTrue(sources)
                self.assertTrue(all(source in standard.SOURCES for source in sources))
                self.assertIn("palette_preview.py" if sheet == "palette" else "--images " + sheet, redraw)

    def test_a_text_file_is_recorded_as_the_commit_has_it_whatever_line_ends_the_checkout_gave_it(self):
        """core.autocrlf: the same commit checked out on Windows has CRLF, and must not read as another file."""
        self.write("corner.fld", b"a\r\nb\r\n")
        self.write("colours.py", b"x = 1\r\n")
        self.assertEqual(standard.picture_record("plan")[FIELD_FILE], TWO_LINES)
        self.assertEqual(standard.picture_record("palette"), {PALETTE: ONE_COLOUR})

    def test_the_customisation_file_is_recorded_byte_for_byte(self):
        """It is exact bytes on every checkout (.gitattributes), so a CR in it is a change and not a line end."""
        self.write("library.json", b"a\r\nb\r\n")
        self.assertEqual(standard.picture_record("symbols"), {CUSTOMISATION: TWO_LINES_CRLF})

    def test_a_picture_cannot_be_recorded_as_drawn_from_a_file_that_is_not_there(self):
        os.remove(self.field_file)
        with self.assertRaises(FileNotFoundError):
            standard.picture_record("plan")
        self.assertEqual(standard.picture_record("symbols"), {CUSTOMISATION: ABC})   # it reads no field file


class WhetherAPictureIsOfWhatIsCommitted(Pictures):
    def test_a_picture_with_the_record_of_what_is_committed_is_current(self):
        self.picture("symbols", [(CUSTOMISATION, ABC)])
        self.picture("linestyles", [(CUSTOMISATION, ABC), ("Title", "more text than the record is no matter")])
        self.picture("plan", [(FIELD_FILE, TWO_LINES), (CUSTOMISATION, ABC)])
        self.picture("showcase", [(CUSTOMISATION, ABC), (SCENE, TWO_COMMANDS)])
        self.picture("palette", [(PALETTE, ONE_COLOUR)])
        for sheet in standard.PICTURES:
            with self.subTest(sheet=sheet):
                self.assertIsNone(self.problem(sheet))
        self.assertEqual(standard.stale_pictures(self.folder), {})

    def test_a_picture_drawn_before_the_library_changed_is_stale(self):
        """What happened: the symbols were renamed after the sheets were drawn, and the catalogue kept the sheets."""
        self.picture("symbols", [(CUSTOMISATION, ABC)])
        self.picture("plan", [(CUSTOMISATION, ABC), (FIELD_FILE, TWO_LINES)])
        self.write("library.json", b"abd")
        said = "was drawn from another customisation file than the committed one"
        self.assertEqual(self.problem("symbols"), said)
        self.assertEqual(self.problem("plan"), said)
        # and drawn again from the file as it now is, it is current
        self.picture("symbols", [(CUSTOMISATION, ABD)])
        self.assertIsNone(self.problem("symbols"))

    def test_a_picture_with_no_record_is_stale_and_is_told_which_record_it_lacks(self):
        self.picture("symbols")
        self.assertEqual(self.problem("symbols"), "does not say which customisation file it was drawn from")
        self.picture("plan", [(CUSTOMISATION, ABC)])
        self.assertEqual(self.problem("plan"), "does not say which field file (street_corner.fld) it was drawn from")
        self.picture("showcase", [(SCENE, TWO_COMMANDS)])
        self.assertEqual(self.problem("showcase"), "does not say which customisation file it was drawn from")
        self.picture("palette", [(CUSTOMISATION, ABC)])   # the palette is drawn from colours.py, not from the file
        self.assertEqual(self.problem("palette"), "does not say which palette (colours.py) it was drawn from")

    def test_each_other_thing_a_picture_is_drawn_from_makes_it_stale_when_it_changes(self):
        self.picture("plan", [(CUSTOMISATION, ABC), (FIELD_FILE, TWO_LINES)])
        self.picture("showcase", [(CUSTOMISATION, ABC), (SCENE, TWO_COMMANDS)])
        self.picture("palette", [(PALETTE, ONE_COLOUR)])
        self.write("corner.fld", b"a\nb\nc\n")
        self.scene.lines = ["POINT 1,3", "CODE"]
        self.write("colours.py", b"x = 2\n")
        self.assertEqual(self.problem("plan"),
                         "was drawn from another field file (street_corner.fld) than the committed one")
        self.assertEqual(self.problem("showcase"), "was drawn from another scene (showcase.py) than the committed one")
        self.assertEqual(self.problem("palette"), "was drawn from another palette (colours.py) than the committed one")

    def test_a_record_must_be_the_whole_digest(self):
        for text in (ABC[:-1], ABC.upper(), ABC[len("sha256:"):], ""):
            with self.subTest(text=text):
                self.picture("symbols", [(CUSTOMISATION, text)])
                self.assertEqual(self.problem("symbols"),
                                 "was drawn from another customisation file than the committed one")

    def test_a_picture_that_is_missing_or_is_no_picture_is_stale(self):
        self.assertEqual(self.problem("symbols"), "is missing, or is no PNG")
        self.write(os.path.basename(standard.picture_path("plan", self.folder)), b"not a picture")
        self.assertEqual(self.problem("plan"), "is missing, or is no PNG")

    def test_a_picture_whose_source_is_gone_is_stale_and_the_check_says_what_is_gone(self):
        self.picture("plan", [(CUSTOMISATION, ABC), (FIELD_FILE, TWO_LINES)])
        os.remove(self.field_file)
        self.assertEqual(self.problem("plan"), "cannot be compared with what it was drawn from: the field file "
                         "(street_corner.fld) of the plan picture is not there")

    def test_the_stale_pictures_of_a_folder_are_listed_with_why_and_the_current_ones_are_not(self):
        self.picture("symbols", [(CUSTOMISATION, ABC)])
        self.picture("linestyles", [(CUSTOMISATION, ABD)])
        self.picture("palette")
        self.picture("showcase", [(CUSTOMISATION, ABC), (SCENE, TWO_COMMANDS)])
        self.assertEqual(standard.stale_pictures(self.folder), {
            "linestyles": "was drawn from another customisation file than the committed one",
            "palette": "does not say which palette (colours.py) it was drawn from",
            "plan": "is missing, or is no PNG",
        })


if __name__ == "__main__":
    unittest.main()
