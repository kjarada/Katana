//! What the window's test targets share (F0 owns it; every other target includes it with
//! `#[path = "../support/window_support.rs"] pub mod window_support;` and does not edit it): the
//! checkout's data where the C++ tests read it, a scratch folder of one test's own, the recording
//! hosts the C++ tests' runners stand for, and the harness on Qt's offscreen screen.

#![allow(
    dead_code,
    reason = "Each test target that includes this module uses only some of its helpers."
)]

use std::fs;
use std::path::{Path, PathBuf};

use katana_cad::Document;
use katana_window::drive::Harness;
use katana_window::host::{Answer, RecordingHost, VerbOutcome};

/// The checkout's root, from this crate's place in it (rust/crates/katana_window).
pub fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

/// samples/<name>: site_plan, gis, utilities.
pub fn samples(name: &str) -> PathBuf {
    checkout().join("samples").join(name)
}

/// tests/qt_widgets/data, the widget tests' own files.
pub fn qt_widgets_data() -> PathBuf {
    checkout().join("tests").join("qt_widgets").join("data")
}

/// tests/data/customisation, the committed customisation fixtures (fixture_customisation.rs).
pub fn customisation_data() -> PathBuf {
    checkout().join("tests").join("data").join("customisation")
}

/// tests/geo/data.
pub fn geo_data() -> PathBuf {
    checkout().join("tests").join("geo").join("data")
}

/// tests/archive12d/data.
pub fn archive_data() -> PathBuf {
    checkout().join("tests").join("archive12d").join("data")
}

/// A fresh folder for one test's files, removed when the test ends: named by the test and the
/// process, since a test binary runs its cases on parallel threads of one process (the C++'s
/// QTemporaryDir and fixed temporary paths).
pub struct Scratch {
    pub root: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("katana-window-tests-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the scratch folder is made");
        Self { root }
    }

    /// A path in the folder; nothing is made there.
    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// A file in the folder holding `text`.
    pub fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, text).expect("the scratch file is written");
        path
    }

    /// A folder in the folder.
    pub fn folder(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir_all(&path).expect("the scratch folder's folder is made");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A host answering every line with `outcome`: the C++ tests' FakeRunner with its answer set.
pub fn fixed_host(outcome: VerbOutcome) -> RecordingHost {
    RecordingHost::new().answering(Answer::Fixed(outcome))
}

/// A host over `document` that runs each line by its own interpreter on the drawing and hands
/// back what it replied or was refused with: fixture_customisation.hpp's InterpreterRunner, and
/// the runners of the tests that run a dialog's line for real.
pub fn interpreter_host(document: Document) -> RecordingHost {
    RecordingHost::with_document(document).answering(Answer::Interpreter)
}

/// An ok outcome replying `reply`.
pub fn ok(reply: &str) -> VerbOutcome {
    VerbOutcome::new(true, reply, "")
}

/// A refused outcome saying `error`.
pub fn refused(error: &str) -> VerbOutcome {
    VerbOutcome::new(false, "", error)
}

/// The width and height of Qt's offscreen platform's screen, which the C++ widget tests ran on:
/// 800 x 600 (test_screen_fit.cpp's header).
pub const OFFSCREEN_WIDTH: f32 = 800.0;
pub const OFFSCREEN_HEIGHT: f32 = 600.0;

/// A harness on Qt's offscreen screen, for the twins whose sizes the C++ compared with that
/// screen's.
pub fn offscreen_harness() -> Harness {
    Harness::with_screen(OFFSCREEN_WIDTH, OFFSCREEN_HEIGHT)
}
