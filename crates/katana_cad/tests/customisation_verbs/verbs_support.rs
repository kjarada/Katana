//! What the cases of tests/cad/customisation/test_customisation_verbs.cpp share (agent J3's; the
//! targets tests/customisation_verbs and tests/customisation_verbs_env include it): the
//! customisations they load, a session with no host and one started as a front end starts one,
//! and the reading of a reply's records.
//!
//! "The built-in" is the small customisation written out below, handed over as a host hands one
//! over, and "the kept file" a path in a folder of the test's own. Nothing here reads what a build
//! may have compiled in or what the environment names, so the cases are the same on every machine.
//!
//! Every count is by hand from the two files below:
//!
//! ```text
//!   BUILT_IN "Small Built In"   TEST Fence (a linestyle), TEST Peg (a symbol,
//!                               at vertices); rules FE* feature (layer FENCES,
//!                               colour sui gas, linestyle TEST Fence) and PG*
//!                               symbol (TEST Peg); one colour, sui gas.
//!                               2 definitions, 2 rules over 2 keys.
//!   SITE "Site"                 symbols TEST Peg (again) and SITE Tree; rules
//!                               FE* feature (layer SITE FENCES), TR* symbol
//!                               (SITE Tree), GT* feature (layer GATES,
//!                               linestyle SITE Gate - which nothing defines).
//! ```
//!
//! The built-ins section (CUSTOMISE BUILTIN) offers BUILT_IN and SITE as the two built-ins of a
//! host, as a host's own list stands in for the build: the default is "Small Built In" (2
//! definitions, 2 codes, 2 rules) and the alternate "Site" (2 definitions, 3 codes, 3 rules, by
//! the lists above).
//!
//! Site MERGED into the built-in: TEST Peg replaced, SITE Tree added (3 definitions); FE* feature
//! replaced, TR* symbol and GT* feature added (4 rules over 4 keys: FE*, PG*, TR*, GT*); SITE Gate
//! asked for and undefined. Site in the built-in's PLACE: TEST Fence and PG* are gone (2
//! definitions, 3 rules over 3 keys), and the session takes Site's name, having nothing of the
//! built-in's definitions or rules left to be named by.

#![allow(
    dead_code,
    reason = "Each test target that includes this module uses only some of its helpers."
)]

use std::fs;
use std::path::{Path, PathBuf};

use katana_cad::customisation_host::{BuiltInEntry, CustomisationHost, start_customisation};
use katana_cad::customisation_state::CustomisationState;
use katana_cad::{CommandInterpreter, Document};
use katana_core::Error;
use katana_core::text::{ReplyRecord, read_reply_record, split_lines};
use katana_entity::customisation::customisation_from_json;
use katana_entity::style_library::add_or_replace;
use katana_entity::{Customisation, CustomisationSourceNote, LineStyle};

use crate::customisation_support::{Scratch, built_in_from, path_text};

pub const BUILT_IN: &str = r##"{
  "format": "katana-customisation", "version": 1,
  "name": "Small Built In",
  "description": "Two definitions and two rules, for tests",
  "notice": ["Written for these tests."],
  "colours": {"sui gas": "#FF7F00"},
  "linestyles": [
    {"name": "TEST Fence", "strokes": [["move", 0, 0], ["draw", 2, 0]]}
  ],
  "symbols": [
    {"name": "TEST Peg", "atVertices": true, "strokes": [["move", 0, 0], ["circle", 0.5]]}
  ],
  "codes": [
    {"key": "FE*", "sets": "feature", "layer": "FENCES", "colour": "sui gas",
     "linestyle": "TEST Fence"},
    {"key": "PG*", "sets": "symbol", "symbol": {"name": "TEST Peg", "size": 1.5}}
  ]
}"##;

pub const SITE: &str = r#"{
  "format": "katana-customisation", "version": 1, "name": "Site",
  "symbols": [
    {"name": "SITE Tree", "strokes": [["circle", 2]]},
    {"name": "TEST Peg", "strokes": [["circle", 1]]}
  ],
  "codes": [
    {"key": "FE*", "sets": "feature", "layer": "SITE FENCES"},
    {"key": "TR*", "sets": "symbol", "symbol": {"name": "SITE Tree"}},
    {"key": "GT*", "sets": "feature", "layer": "GATES", "linestyle": "SITE Gate"}
  ]
}"#;

// Three more, for what a PART of a session says of where it came from:
//
//   OWN "Own"      one linestyle, OWN Line: a colleague's own session.
//   MARKS "Marks"  a symbol, MARK Cross, the rule MK* that draws it, and a
//                  notice of its own.
//   TINTS "Tints"  neither definitions nor rules: a table of one colour, and
//                  a notice of its own.
//
// Marks and Tints MERGED into the built-in: the session is still "Small Built
// In" with its own notice, and its sources are Small Built In (definitions,
// rules, no notice of its own - that is the session's), Marks (definitions,
// rules, its notice) and Tints (neither, its notice); 3 definitions, 3 rules,
// 2 colours.
pub const OWN: &str = r#"{
  "format": "katana-customisation", "version": 1, "name": "Own",
  "linestyles": [
    {"name": "OWN Line", "strokes": [["move", 0, 0], ["draw", 1, 0]]}
  ]
}"#;

pub const MARKS: &str = r#"{
  "format": "katana-customisation", "version": 1, "name": "Marks",
  "notice": ["Marks: drawn by hand."],
  "symbols": [
    {"name": "MARK Cross", "atVertices": true, "strokes": [["move", -1, 0], ["draw", 1, 0]]}
  ],
  "codes": [
    {"key": "MK*", "sets": "symbol", "symbol": {"name": "MARK Cross"}}
  ]
}"#;

pub const TINTS: &str = r##"{
  "format": "katana-customisation", "version": 1, "name": "Tints",
  "notice": ["Tints: free to use."],
  "colours": {"tint teal": "#008080"}
}"##;

/// A style library as another program writes one: not a Katana customisation.
pub const LEGACY_LIBRARY: &str = "worldstyle \"X\" { move 0 0 draw 1 0 }\n";

pub const DEFAULT_LINEWORK: &str = concat!(
    "linework linework.start=ST linework.end=END linework.close=CL linework.arcstart=BC ",
    "linework.arcend=EC linework.join=JPN linework.rectangle=RECT"
);

/// What a session with definitions loaded and a drawing with no styles is told.
pub const NO_STYLES: &str = concat!(
    "This drawing has no styles yet; the customisation's styles are made when survey data is ",
    "imported or when a rule draws."
);

/// The bytes of `path` as text; empty when it cannot be read.
pub fn contents_of(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// A path as a reply writes one: absolute, and with '/' on every platform.
pub fn shown(path: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = path_text(&absolute);
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

/// A path as a line takes one: quoted, so that a folder with a blank in its name is still one
/// word.
pub fn typed(path: &Path) -> String {
    format!("\"{}\"", shown(path))
}

/// `file` with `suffix` added to its name: the `.bak`, `.bad` or `.tmp` beside it.
pub fn beside(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[track_caller]
pub fn parsed(text: &str) -> Customisation {
    customisation_from_json(text.as_bytes()).unwrap_or_else(|error| panic!("{}", error.describe()))
}

/// `text` as an entry of the list a host offers: named as the customisation names itself, with
/// the file it is said to have been compiled in from, and the digest of its bytes worked out as
/// the built-in's is (built_in_from), so that the run's own entry is known as the run's own.
pub fn entry_from(text: &str, source: &str) -> BuiltInEntry {
    let built_in = built_in_from(text);
    let customisation = built_in
        .customisation
        .clone()
        .expect("the test's own built-in reads");
    BuiltInEntry {
        name: customisation.name.clone(),
        description: customisation.description.clone(),
        source: String::from(source),
        built_in,
    }
}

/// The `nth` record of `reply` whose first word is `word`, read by the one reader of reply
/// records (core's read_reply_record), so that a value is compared as it is and not as it happens
/// to be quoted.
#[track_caller]
pub fn record(reply: &str, word: &str, nth: usize) -> ReplyRecord {
    let mut seen = 0;
    for line in split_lines(reply.as_bytes()) {
        let text = String::from_utf8_lossy(line);
        if let Some(one) = read_reply_record(&text)
            && one.words.first().is_some_and(|first| first == word)
        {
            if seen == nth {
                return one;
            }
            seen += 1;
        }
    }
    panic!("no record \"{word}\" number {nth} in:\n{reply}");
}

pub fn records_of(reply: &str, word: &str) -> usize {
    split_lines(reply.as_bytes())
        .into_iter()
        .filter(|line| {
            read_reply_record(&String::from_utf8_lossy(line))
                .is_some_and(|one| one.words.first().is_some_and(|first| first == word))
        })
        .count()
}

pub fn field(from: &ReplyRecord, key: &str) -> String {
    from.value(key)
        .map_or_else(|| String::from("(absent)"), String::from)
}

/// {name, brought definitions, brought rules, its notice}.
pub fn source(
    name: &str,
    definitions: bool,
    rules: bool,
    notice: &[&str],
) -> CustomisationSourceNote {
    CustomisationSourceNote {
        name: String::from(name),
        definitions,
        rules,
        notice: notice.iter().map(|line| String::from(*line)).collect(),
    }
}

// The reply to `line`, failing the test when it is refused.
#[track_caller]
fn reply_to(document: &mut Document, interpreter: &mut CommandInterpreter, line: &str) -> String {
    let reply = match interpreter.run(document, line) {
        Ok(reply) => reply,
        Err(error) => panic!("{line}\n  -> {}", error.describe()),
    };
    // A script fails a line on this word, so no reply to a line that
    // succeeded may hold it.
    assert!(!reply.contains("error"), "{line}:\n{reply}");
    assert!(!reply.ends_with('\n'), "{line}");
    reply
}

// What `line` is refused with, failing the test when it is accepted.
#[track_caller]
fn refusal_of(document: &mut Document, interpreter: &mut CommandInterpreter, line: &str) -> Error {
    match interpreter.run(document, line) {
        Ok(reply) => panic!("{line} was accepted:\n{reply}"),
        Err(error) => error,
    }
}

/// A session with no host: a bare Document, as a test or a tool has one.
#[derive(Default)]
pub struct Bare {
    pub document: Document,
    pub interpreter: CommandInterpreter,
}

impl Bare {
    #[track_caller]
    pub fn ok(&mut self, line: &str) -> String {
        reply_to(&mut self.document, &mut self.interpreter, line)
    }

    #[track_caller]
    pub fn refused(&mut self, line: &str) -> Error {
        refusal_of(&mut self.document, &mut self.interpreter, line)
    }

    pub fn state(&self) -> &CustomisationState {
        self.document.customisation_state()
    }

    /// The built-in loaded as any file is, for the cases that need a session and no host.
    #[track_caller]
    pub fn load_built_in(&mut self, scratch: &Scratch) {
        let line = format!(
            "CUSTOMISE {}",
            typed(&scratch.write("builtin.customisation.json", BUILT_IN))
        );
        self.ok(&line);
    }
}

/// A session started as a front end starts one: the host handed to the interpreter, then
/// start_customisation.
pub struct Hosted {
    pub document: Document,
    pub interpreter: CommandInterpreter,
    pub scratch: Scratch,
    pub kept_file: PathBuf,
    pub host: CustomisationHost,
}

impl Hosted {
    /// With the built-in and a kept file.
    pub fn new(name: &str) -> Self {
        Self::with(name, true, true)
    }

    /// The kept file in a folder that does not exist yet, as a per-user folder nothing has
    /// written to does not; absolute and normal, as a front end gives one, so that a reply names
    /// it by the very text handed over.
    pub fn with(name: &str, built_in: bool, kept: bool) -> Self {
        let scratch = Scratch::new(name);
        let kept_file = std::path::absolute(scratch.root.join("kept").join("customisation.json"))
            .expect("the kept file has an absolute path");
        let mut host = CustomisationHost::default();
        if built_in {
            host.built_in = built_in_from(BUILT_IN);
        }
        // The built-ins the host offers by name are the test's own, whatever
        // this build compiled in: the one it starts with, and no other.
        let mut offered = Vec::new();
        if built_in {
            offered.push(entry_from(BUILT_IN, ""));
        }
        host.built_ins = Some(offered);
        if kept {
            host.kept_file = kept_file.clone();
        }
        Self {
            document: Document::default(),
            interpreter: CommandInterpreter::new(),
            scratch,
            kept_file,
            host,
        }
    }

    pub fn start(&mut self) {
        self.interpreter.set_customisation_host(self.host.clone());
        start_customisation(&mut self.document, &self.host);
    }

    #[track_caller]
    pub fn ok(&mut self, line: &str) -> String {
        reply_to(&mut self.document, &mut self.interpreter, line)
    }

    #[track_caller]
    pub fn refused(&mut self, line: &str) -> Error {
        refusal_of(&mut self.document, &mut self.interpreter, line)
    }

    pub fn state(&self) -> &CustomisationState {
        self.document.customisation_state()
    }

    pub fn backup(&self) -> PathBuf {
        beside(&self.kept_file, ".bak")
    }

    pub fn bad(&self) -> PathBuf {
        beside(&self.kept_file, ".bad")
    }

    /// What the folder of the kept file holds, by name, in order.
    pub fn kept_folder(&self) -> Vec<String> {
        let folder = self
            .kept_file
            .parent()
            .expect("the kept file is in a folder");
        let mut names: Vec<String> = fs::read_dir(folder)
            .expect("the kept file's folder is read")
            .map(|entry| {
                entry
                    .expect("an entry of the kept file's folder")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

/// CUSTOMISE BUILTIN [<name>]: a host that offers two built-ins, BUILT_IN (the one it starts
/// with, so the default) and SITE. The counts are those at the top of this module.
pub fn two_built_ins(name: &str) -> Hosted {
    let mut hosted = Hosted::new(name);
    hosted
        .host
        .built_ins
        .as_mut()
        .expect("the host offers its own list")
        .push(entry_from(SITE, "site.customisation.json"));
    hosted
}

/// A definition put into the session's library as an editor puts one: from no customisation, and
/// named by nothing until a test names it. `symbol`: listed as a symbol and drawn at vertices,
/// else a linestyle.
#[track_caller]
pub fn add_definition(document: &mut Document, name: &str, symbol: bool) {
    let mut library = document.style_library().clone();
    let definition = LineStyle {
        name: String::from(name),
        symbol,
        at_vertices: symbol,
        ..LineStyle::default()
    };
    assert!(add_or_replace(&mut library, definition).is_ok(), "{name}");
    document.set_style_library(library);
}
