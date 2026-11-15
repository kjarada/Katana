//! The road and sheet verbs driven through the command interpreter (cad wave
//! J, plan agent J2), one module per C++ test file:
//!
//! ```text
//! road_object_scope   tests/cad/road/test_road_object_scope.cpp
//! road_object_verbs   tests/cad/road/test_road_object_verbs.cpp
//! road_objects        tests/cad/road/test_road_objects.cpp
//! road_verbs          tests/cad/road/test_road_verbs.cpp
//! generate_rotate     tests/cad/plotting/test_generate_rotate.cpp
//! sheet_agent_verbs   tests/cad/plotting/test_sheet_agent_verbs.cpp
//! sheet_verbs         tests/cad/plotting/test_sheet_verbs.cpp
//! sheets_append       tests/cad/plotting/test_sheets_append.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. The lines go through
//! CommandInterpreter::run on a Document the test holds, as the window's
//! command line, katana_cli and katana_mcp drive them. The C++ wrote to fixed
//! paths under the temporary directory; Rust runs a binary's cases on parallel
//! threads of one process, so each case here writes in a scratch directory of
//! its own (ScratchDirectory).

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/road_fixtures.rs"]
pub mod road_fixtures;

mod generate_rotate;
mod road_object_scope;
mod road_object_verbs;
mod road_objects;
mod road_verbs;
mod sheet_agent_verbs;
mod sheet_verbs;
mod sheets_append;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use katana_cad::plotting::SheetSet;
use katana_cad::{CommandInterpreter, Document};
use katana_commands::create_alignment;
use katana_core::text::split_lines;
use katana_core::{Error, Result};
use katana_entity::Alignment;
use katana_geometry::{AlignmentPi, HorizontalAlignment, Point2, ProfilePvi, VerticalAlignment};

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

/// A result the case cannot go on without: its value, or the error described.
#[track_caller]
pub fn must<T>(result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// The C++ tests' `contains`.
pub fn contains(text: &str, part: &str) -> bool {
    text.contains(part)
}

/// The first `count` bytes of `text`, or all of it when it is shorter: the
/// C++ `substr(0, count)`.
pub fn head(text: &str, count: usize) -> &str {
    text.get(..count).unwrap_or(text)
}

/// The lines of a reply as core's splitLines splits them: at "\n", "\r" or
/// "\r\n", with no empty line after a last line break.
pub fn core_lines(text: &str) -> Vec<String> {
    split_lines(text.as_bytes())
        .into_iter()
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect()
}

/// The site the road object tests stand on (their C++ Site): alignment Main,
/// straight from (0,0) to (100,0) - so station s, offset o is the point
/// (s, o), left of the centreline being +y - with, when `with_profile`, a
/// design profile from level 10 at station 0 to 10 + `rise` at station 100.
#[track_caller]
pub fn road_site(with_profile: bool, rise: f64) -> Session {
    let mut session = Session::default();
    let vertical = if with_profile {
        Some(VerticalAlignment {
            pvis: vec![
                ProfilePvi {
                    station: 0.0,
                    elevation: 10.0,
                    ..ProfilePvi::default()
                },
                ProfilePvi {
                    station: 100.0,
                    elevation: 10.0 + rise,
                    ..ProfilePvi::default()
                },
            ],
        })
    } else {
        None
    };
    let main = Alignment {
        name: String::from("Main"),
        horizontal: HorizontalAlignment {
            pis: vec![
                AlignmentPi {
                    point: Point2::new(0.0, 0.0),
                    ..AlignmentPi::default()
                },
                AlignmentPi {
                    point: Point2::new(100.0, 0.0),
                    ..AlignmentPi::default()
                },
            ],
            ..HorizontalAlignment::default()
        },
        vertical,
        ..Alignment::default()
    };
    must(session.document.execute(create_alignment(main)));
    session
}

/// A document and the interpreter that types into it (the C++ tests' Session
/// and Site).
#[derive(Default)]
pub struct Session {
    pub document: Document,
    pub interpreter: CommandInterpreter,
}

impl Session {
    /// What `line` gives, refused or not: the C++ `interpreter.run(line)`.
    pub fn run(&mut self, line: &str) -> Result<String> {
        self.interpreter.run(&mut self.document, line)
    }

    /// The reply to `line`; the case fails, naming the line, when it is
    /// refused.
    #[track_caller]
    pub fn ok(&mut self, line: &str) -> String {
        match self.run(line) {
            Ok(reply) => reply,
            Err(error) => panic!("{line}\n  -> {}", error.describe()),
        }
    }

    /// The error `line` is refused with; the case fails when it is not.
    #[track_caller]
    pub fn refused(&mut self, line: &str) -> Error {
        match self.run(line) {
            Ok(reply) => panic!("{line} was accepted:\n{reply}"),
            Err(error) => error,
        }
    }

    /// The undo steps the document holds.
    pub fn steps(&self) -> usize {
        self.document.history().undo_count()
    }

    /// The document's sheet set.
    pub fn set(&self) -> Arc<SheetSet> {
        self.document.sheet_set()
    }
}

/// A directory of a case's own under the temporary directory, named after the
/// case and the process, removed with its contents when dropped.
pub struct ScratchDirectory {
    pub path: PathBuf,
}

impl ScratchDirectory {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "katana-cad-road-sheet-verbs-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The C++ `path.generic_string()`: forward slashes.
pub fn generic(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// A path as a verb takes it: forward slashes, in quotes.
pub fn quoted(path: &Path) -> String {
    format!("\"{}\"", generic(path))
}

/// `bytes` written to `path`, replacing what was there.
pub fn write_file(path: &Path, bytes: &[u8]) {
    if let Err(error) = std::fs::write(path, bytes) {
        panic!("{}: {error}", path.display());
    }
}

/// The bytes of `path`; nothing when it cannot be read, as the C++'s
/// ifstream gives.
pub fn read_file(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

/// The text of `path` (its bytes, any that are not UTF-8 replaced).
pub fn read_text(path: &Path) -> String {
    String::from_utf8_lossy(&read_file(path)).into_owned()
}
