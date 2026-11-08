//! What the tests of this crate's programs share: a program run to its end within a time limit,
//! with what it printed, and a tool found on PATH as CMake's find_program finds one. These are the
//! twins of tests that CTest runs (tests/update/CMakeLists.txt and e2e.cmake), so a run is judged
//! as CTest judges one: by its exit code, or by a PASS_REGULAR_EXPRESSION on its whole output,
//! standard output and standard error together.

#![allow(
    dead_code,
    reason = "a shared support module: each test file uses part of it"
)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One program that ran: how it ended and what it printed.
#[derive(Debug, Default)]
pub struct Ran {
    /// None when it was ended by a signal, or by the time limit.
    pub code: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Ran {
    /// Standard output and standard error together, what CTest matches its expressions against.
    pub fn output(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }

    /// For a failure's message.
    pub fn describe(&self) -> String {
        format!(
            "exit {:?}{}\n---- standard output ----\n{}\n---- standard error ----\n{}",
            self.code,
            if self.timed_out { " (timed out)" } else { "" },
            self.stdout,
            self.stderr
        )
    }

    pub fn succeeded(&self) -> bool {
        !self.timed_out && self.code == Some(0)
    }
}

// What a stream held, read on a thread of its own so that a program that fills one pipe while
// the other is not read cannot stall.
fn drain(stream: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// Runs `command` to its end, or kills it at `limit` (CTest's TIMEOUT), with its input closed.
pub fn run(mut command: Command, limit: Duration) -> Ran {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("{command:?} could not be started: {error}"));
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok();
            }
            Err(_) => break None,
        }
    };
    let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();
    Ran {
        code: if timed_out {
            None
        } else {
            status.and_then(|status| status.code())
        },
        timed_out,
        stdout: text(out.join().unwrap_or_default()),
        stderr: text(err.join().unwrap_or_default()),
    }
}

/// The first `name` (with `.exe` on Windows) in a folder of PATH, as find_program looks for a
/// program; None when there is none.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        String::from(name)
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|folder| folder.join(&file))
        .find(|candidate| candidate.is_file())
}

/// The checkout, from this crate's folder.
pub fn checkout() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.."))
}

/// `text` with every run of blanks and line breaks made one blank: CMake wraps a message at 80
/// columns, at any space, so a phrase a test looks for can be broken across two lines
/// (tests/update/CMakeLists.txt, katana_phrase).
pub fn one_spaced(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A folder of a test's own under the integration tests' temporary folder (cargo's
/// CARGO_TARGET_TMPDIR, the twin of CMake's binary folder of the tests).
pub fn work_folder(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)
}
