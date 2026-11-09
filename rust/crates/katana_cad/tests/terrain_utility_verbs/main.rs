//! The terrain and utility verbs and the surface edit tools, driven as a
//! person or an agent drives them: through the interpreter's lines and the
//! tools' clicks (the port's cad wave J, plan agent J1). One module per C++
//! test file:
//!
//! ```text
//! surface_display_verbs   tests/cad/terrain/test_surface_display_verbs.cpp
//! surface_edit_tools      tests/cad/terrain/test_surface_edit_tools.cpp
//! surface_edit_verbs      tests/cad/terrain/test_surface_edit_verbs.cpp
//! surface_verbs           tests/cad/terrain/test_surface_verbs.cpp
//! volume_verbs            tests/cad/terrain/test_volume_verbs.cpp
//! utility_data            tests/cad/utilities/test_utility_data.cpp
//! utility_geometry        tests/cad/utilities/test_utility_geometry.cpp
//! utility_verbs           tests/cad/utilities/test_utility_verbs.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. The C++ fixtures that hold a Document
//! and the interpreter bound to it are a [`Session`] here: the interpreter
//! does not hold the Document (command_interpreter.rs, its module comment),
//! so each line lends it.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/circle_spline.rs"]
pub mod circle_spline;
#[path = "../support/surface_edit_support.rs"]
pub mod surface_edit_support;
#[path = "../support/surface_geometry.rs"]
pub mod surface_geometry;
#[allow(
    dead_code,
    reason = "This target uses a part of the Document core's surface helpers."
)]
#[path = "../support/surface_test_support.rs"]
pub mod surface_test_support;
#[path = "../support/tool_driver.rs"]
pub mod tool_driver;

mod surface_display_verbs;
mod surface_edit_tools;
mod surface_edit_verbs;
mod surface_verbs;
mod utility_data;
mod utility_geometry;
mod utility_verbs;
mod volume_verbs;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use katana_cad::{CommandInterpreter, Document};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::{Entity, EntityId};
use katana_survey::subsurface::{UtilityLine, parse_utility_csv};
use katana_terrain::TinSurface;

/// samples/utilities, the schedules the utility tests read (the C++
/// KATANA_UTILITY_SAMPLES).
pub const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../samples/utilities");

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_DOUBLE_EQ: within four units in the last place, as gtest compares.
#[track_caller]
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}

/// ASSERT_TRUE(result.ok()) << result.error().describe(), and the value.
#[track_caller]
pub fn must<T>(result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// ASSERT_TRUE(status.ok()) << status.error().describe().
#[track_caller]
pub fn ok(status: Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}

/// Whether `text` holds `part` (the C++ tests' `contains` and `has`).
pub fn contains(text: &str, part: &str) -> bool {
    text.contains(part)
}

/// Owned strings, for a line's words.
pub fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

/// `*surface.elevationAt(x, y)`: the surface's level there; the test fails
/// where the surface has none.
#[track_caller]
pub fn level(surface: &TinSurface, x: f64, y: f64) -> f64 {
    match surface.elevation_at_xy(x, y) {
        Some(level) => level,
        None => panic!("the surface has no level at {x},{y}"),
    }
}

/// std::regex_search for the only patterns the C++ tests search with:
/// literal pieces with one or more spaces between them ("W1 +W1-3 +ok" is
/// the pieces "W1", "W1-3" and "ok"). Each piece begins with a character
/// that is not a space, so taking every space before it is what the
/// pattern's greedy " +" takes.
pub fn spaced_search(text: &str, pieces: &[&str]) -> bool {
    fn spaced_tail(text: &str, pieces: &[&str]) -> bool {
        let Some((&first, rest)) = pieces.split_first() else {
            return true;
        };
        let after = text.trim_start_matches(' ');
        after.len() < text.len()
            && after.starts_with(first)
            && spaced_tail(&after[first.len()..], rest)
    }
    let Some((&first, rest)) = pieces.split_first() else {
        return true;
    };
    let mut from = 0;
    while let Some(found) = text[from..].find(first) {
        let start = from + found;
        if spaced_tail(&text[start + first.len()..], rest) {
            return true;
        }
        // On to the next character, whatever its length in UTF-8.
        from = start + first.chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// A document and the interpreter that types into it - the C++ fixtures'
/// `Document document; CommandInterpreter interpreter{document};`.
#[derive(Default)]
pub struct Session {
    pub document: Document,
    pub interpreter: CommandInterpreter,
}

impl Session {
    /// The interpreter's answer to `line`, run on the document.
    pub fn run(&mut self, line: &str) -> Result<String> {
        self.interpreter.run(&mut self.document, line)
    }

    /// The reply to `line`; the test fails, naming the line, when it is
    /// refused.
    #[track_caller]
    pub fn ok(&mut self, line: &str) -> String {
        match self.run(line) {
            Ok(reply) => reply,
            Err(error) => panic!("{line}\n  -> {}", error.describe()),
        }
    }

    /// The error `line` is refused with; the test fails when it is not.
    #[track_caller]
    pub fn refused(&mut self, line: &str) -> Error {
        match self.run(line) {
            Ok(reply) => panic!("{line} was accepted:\n{reply}"),
            Err(error) => error,
        }
    }

    /// The code `line` is refused with; the test fails when it is not.
    #[track_caller]
    pub fn fails(&mut self, line: &str) -> ErrorCode {
        self.refused(line).code
    }

    /// The undo steps the document holds.
    pub fn steps(&self) -> usize {
        self.document.history().undo_count()
    }
}

/// Every entity of the drawing by its id, as values: what a step must leave
/// as it was, or put back.
pub fn snapshot(document: &Document) -> BTreeMap<EntityId, Entity> {
    let mut entities = BTreeMap::new();
    document.model().entities.for_each(|entity| {
        entities.entry(entity.id).or_insert_with(|| entity.clone());
    });
    entities
}

/// The text up to its first line break, or all of it.
pub fn first_line(text: &str) -> String {
    String::from(text.split('\n').next().unwrap_or(""))
}

/// The lines a utility schedule's text reads as; the test fails when it does
/// not read.
#[track_caller]
pub fn parsed(text: &str) -> Vec<UtilityLine> {
    match parse_utility_csv(text) {
        Ok(lines) => lines,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// std::filesystem::path::generic_string: the path with '/' between its
/// names, as the C++ writes a path into a line.
pub fn generic(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

/// A directory of a test's own under the temporary directory, removed with
/// its contents when dropped. The C++ names it `name` alone, since ctest runs
/// each case in a process of its own; the cases here share one process, so
/// the name carries the process too, and each case gives a name of its own.
pub struct ScratchFolder {
    pub path: PathBuf,
}

impl ScratchFolder {
    #[track_caller]
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        if let Err(error) = std::fs::create_dir_all(&path) {
            panic!("{}: {error}", path.display());
        }
        Self { path }
    }

    /// Writes `bytes` to `name` here and returns the path, quoted for a line.
    #[track_caller]
    pub fn file(&self, name: &str, bytes: &str) -> String {
        let at = self.path.join(name);
        if let Err(error) = std::fs::write(&at, bytes) {
            panic!("{}: {error}", at.display());
        }
        format!("\"{}\"", generic(&at))
    }

    /// The path of `name` here, as a line writes it (unquoted).
    pub fn at(&self, name: &str) -> String {
        generic(&self.path.join(name))
    }

    /// What `name` here holds; empty when there is no such file.
    pub fn read(&self, name: &str) -> String {
        match std::fs::read(self.path.join(name)) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::new(),
        }
    }
}

impl Drop for ScratchFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
