//! The committed, hand-written customisation fixture in the Katana format (tests/data/
//! customisation), and the ONE place the window's tests read it from: a test names the fixtures it
//! wants and gets a Document that holds them, loaded as a person's would be - read by the format's
//! reader, merged in the order named, installed. The port of tests/qt_widgets/customisation/
//! fixture_customisation.hpp (F0 owns it; a target includes it with
//! `#[path = "../support/fixture_customisation.rs"] pub mod fixture_customisation;`).
//!
//! The three files, counted by hand from their text:
//!
//! ```text
//! test_linestyles  3 linestyles - TEST Dashed Kerb (paper, Test/Lines), TEST Gate (two-point,
//!                  Test/Lines), TEST Water Main (world, Test/Services, one pen "blue"); none at
//!                  vertices
//! test_symbols     4 symbols - TEST Survey Mark, TEST U Turn, TEST Valve (at vertices, Test/Marks;
//!                  the valve carries one text) and TEST Tree (NOT at vertices, Test/Vegetation,
//!                  one pen "green"), which only the list it sits in makes a symbol
//! test_survey      11 rules over 8 keys - 7 feature (WM*, KB*, AC*, TR*, 1*, 2*, PX*), 3 symbol
//!                  (AC*, TR*, PX*), 1 attributes (*)
//! ```
//!
//! A definition's source is its customisation's name ("test_symbols"), and a session that had no
//! name takes the first loaded customisation's.
//!
//! Those three say nothing of who wrote them. A second session, written here as text, is four
//! customisations that each carry an author's NOTICE, for the tests of what travels with a
//! customisation's data (`install_noticed_session`).
//!
//! The C++'s InterpreterRunner - each line recorded as it is handed over, run by an interpreter on
//! the same Document, and what it replied or was refused with handed back - is window_support's
//! `interpreter_host`: the RecordingHost answering by its interpreter, its `lines` the C++'s `ran`
//! and its `last_reply` the C++'s `lastReply`.

#![allow(
    dead_code,
    reason = "Each test target that includes this module uses only some of its helpers."
)]

use std::path::{Path, PathBuf};

use katana_cad::customisation_merge::{LoadMode, merge_customisation};
use katana_cad::{CustomisationOrigin, Document};
use katana_core::path_text::{generic_path_utf8, read_file_bytes};
use katana_entity::Customisation;
use katana_entity::customisation::customisation_from_json;

/// tests/data/customisation, found from this crate's place in the checkout.
pub fn customisation_fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("data")
        .join("customisation")
}

/// The file of the fixture `name`: "test_symbols" is
/// tests/data/customisation/test_symbols.customisation.json.
pub fn customisation_fixture_file(name: &str) -> PathBuf {
    customisation_fixture_directory().join(format!("{name}.customisation.json"))
}

/// `text` read by the format's own reader. A failure is the test's, said here.
pub fn customisation_from_text(text: &str) -> Customisation {
    customisation_from_json(text.as_bytes())
        .unwrap_or_else(|error| panic!("the fixture does not read: {}", error.describe()))
}

/// The fixture `name`, read by the format's own reader.
pub fn read_customisation_fixture(name: &str) -> Customisation {
    let path = customisation_fixture_file(name);
    let bytes = read_file_bytes(&path)
        .unwrap_or_else(|error| panic!("the fixture {name} is not there: {}", error.describe()));
    customisation_from_json(&bytes)
        .unwrap_or_else(|error| panic!("the fixture {name} does not read: {}", error.describe()))
}

/// Loads `loaded` into `document`, merged in the order given on top of whatever it holds:
/// merge_customisation, then install_customisation, which is what a load is everywhere
/// (cad/customisation_merge).
pub fn install_customisations(document: &mut Document, loaded: &[Customisation]) {
    let merged = merge_customisation(&document.customisation(), loaded, LoadMode::Merge);
    assert!(
        merged.ok(),
        "{}",
        merged.problems.first().cloned().unwrap_or_default()
    );
    document
        .install_customisation(merged.merged, CustomisationOrigin::Loaded, false)
        .unwrap_or_else(|error| panic!("{}", error.describe()));
}

/// The fixtures every customisation test loads unless it names others, in this order.
pub const DEFAULT_FIXTURES: [&str; 3] = ["test_linestyles", "test_survey", "test_symbols"];

/// Loads the named fixtures into `document`, in the order given.
pub fn install_customisation_fixtures(document: &mut Document, names: &[&str]) {
    let loaded: Vec<Customisation> = names
        .iter()
        .map(|name| read_customisation_fixture(name))
        .collect();
    install_customisations(document, &loaded);
}

/// `file` as the word a dialog writes it by in a line: in quotes, with '/'.
pub fn quoted_file(file: &Path) -> String {
    format!("\"{}\"", String::from_utf8_lossy(&generic_path_utf8(file)))
}

// ---- a session whose parts each carry a notice ------------------------------------------------
//
// Four customisations, loaded in this order into an empty Document:
//
//   base    description "The base set.", notice "Base: all rights reserved.", based on "base"
//           0123456789abcdef (it says what it was made from, as a copy kept from a built-in does);
//           one symbol, BASE Peg, whose pen is "tint teal"; one rule, PG* feature, layer BASE
//           PEGS, colour "tint teal", drawn as a point
//   client  notice "Client codes, for this job only."; one rule, FN* feature, layer CLIENT
//           FENCES - rules alone, and no colour named
//   marks   notice "Marks drawn by hand."; one symbol, MARK Cross, with no pen - definitions alone
//   tints   notice "Tints: free to use."; the colours "tint teal" #008080 and "tint rose" #FF007F
//           - neither definitions nor rules
//
// What the session then is, by cad/customisation_merge: it had no name, so it is named "base" and
// takes base's description and notice, and what base is based on; its sources, in load order, are
// base (definitions and rules; no notice of its own, that having become the session's), client
// (rules, its notice), marks (definitions, its notice) and tints (neither, its notice). Its rules
// are PG* then FN*; its colours both of tints'.

pub const NOTICED_BASE: &str = r##"{
  "format": "katana-customisation",
  "version": 1,
  "name": "base",
  "description": "The base set.",
  "notice": [
    "Base: all rights reserved."
  ],
  "basedOn": {"name": "base", "digest": "0123456789abcdef"},
  "symbols": [
    {"name": "BASE Peg", "atVertices": true, "strokes": [
      ["pen", "tint teal"],
      ["circle", 0.5]
    ]}
  ],
  "codes": [
    {"key": "PG*", "sets": "feature", "layer": "BASE PEGS", "colour": "tint teal", "draw": "point"}
  ]
}
"##;

pub const NOTICED_CLIENT: &str = r##"{
  "format": "katana-customisation",
  "version": 1,
  "name": "client",
  "notice": [
    "Client codes, for this job only."
  ],
  "codes": [
    {"key": "FN*", "sets": "feature", "layer": "CLIENT FENCES"}
  ]
}
"##;

pub const NOTICED_MARKS: &str = r##"{
  "format": "katana-customisation",
  "version": 1,
  "name": "marks",
  "notice": [
    "Marks drawn by hand."
  ],
  "symbols": [
    {"name": "MARK Cross", "atVertices": true, "strokes": [
      ["move", -0.5, 0],
      ["draw", 0.5, 0]
    ]}
  ]
}
"##;

pub const NOTICED_TINTS: &str = r##"{
  "format": "katana-customisation",
  "version": 1,
  "name": "tints",
  "notice": [
    "Tints: free to use."
  ],
  "colours": {
    "tint rose": "#FF007F",
    "tint teal": "#008080"
  }
}
"##;

/// Loads the four noticed customisations into `document`, in their order.
pub fn install_noticed_session(document: &mut Document) {
    install_customisations(
        document,
        &[
            customisation_from_text(NOTICED_BASE),
            customisation_from_text(NOTICED_CLIENT),
            customisation_from_text(NOTICED_MARKS),
            customisation_from_text(NOTICED_TINTS),
        ],
    );
}
