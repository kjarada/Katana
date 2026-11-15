//! What the app tests share to run a session's line and read what it said:
//! the twin of the helpers at the head of tests/app/test_session.cpp (and of
//! their copies in the other tests/app files).
//!
//! The C++ captured std::cout and std::cerr process-wide around a line
//! (gtest's CaptureStdout); a Rust session writes through the console it is
//! lent, so `run` lends it a capture and nothing process-wide is touched -
//! the cases can run on parallel threads.
//!
//! One owner (step 6's S1); every app test target includes it with
//! `#[path = "../support/app_support.rs"] pub mod app_support;`.

#![allow(
    dead_code,
    reason = "Each test target that includes the helpers uses a part of them."
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use katana_app::{Captured, Session};
use katana_core::path_text::generic_path_utf8;

/// What running a line printed, stream by stream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Printed {
    pub ok: bool,
    pub out: String,
    pub err: String,
}

/// Runs `line` on `session` with a capture lent for it.
pub fn run(session: &mut Session, line: impl AsRef<[u8]>) -> Printed {
    let mut captured = Captured::default();
    let ok = session.run(line, &mut captured);
    Printed {
        ok,
        out: captured.out,
        err: captured.err,
    }
}

pub fn contains(text: &str, part: &str) -> bool {
    text.contains(part)
}

/// A path as a line or a record writes it: UTF-8, '/' between the names (the
/// C++'s generic_string()).
pub fn generic(path: &Path) -> String {
    String::from_utf8_lossy(&generic_path_utf8(path)).into_owned()
}

/// tests/data/customisation, where the committed customisation files are.
pub const CUSTOMISATION_DATA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tests/data/customisation"
);

/// One of the committed Katana customisation files (tests/data/customisation).
/// By their text: test_symbols holds four symbols and no rule, test_survey
/// eleven rules and no definition.
pub fn customisation_file(name: &str) -> String {
    generic(&Path::new(CUSTOMISATION_DATA).join(name))
}

/// A directory of the test's own, removed afterwards. The C++ named it after
/// the test alone; libtest runs a binary's cases on threads of one process,
/// and the targets one after another, so the process and a count are in the
/// name too.
pub struct ScratchDirectory {
    pub path: PathBuf,
}

impl ScratchDirectory {
    pub fn new(name: &str) -> Self {
        static MADE: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "katana-session-{name}-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, Ordering::Relaxed) + 1
        ));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }

    /// Writes `text` to `name` here and returns its path, quoted for a line.
    pub fn file(&self, name: &str, text: impl AsRef<[u8]>) -> String {
        let at = self.path.join(name);
        std::fs::write(&at, text).expect("a scratch file is written");
        format!("\"{}\"", generic(&at))
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
