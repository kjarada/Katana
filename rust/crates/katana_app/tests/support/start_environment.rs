//! The two environment variables a program's session reads when it starts
//! (cad's customisation_host), set for one test: the twin of
//! tests/app/start_environment.hpp, shared by the session's own tests and the
//! MCP server's, which both start a session as katana_cli and katana_mcp do.
//! Also the update tests' KATANA_UPDATE_TOKEN and KATANA_UPDATE_STATE_DIR.
//!
//! gtest ran each case in a process of its own; Rust runs a binary's cases on
//! parallel threads of one, and the environment is the process's. So a case
//! that sets a variable is in a target of its own (a `*_process` target),
//! and holds [`process_lock`] for its whole length, taken BEFORE any variable
//! is set: declared first, it is let go last, after every ScopedVariable has
//! put its variable back.
//!
//! One owner (step 6's S1); a process target includes it with
//! `#[path = "../support/start_environment.rs"] pub mod start_environment;`.

#![allow(
    dead_code,
    reason = "Each test target that includes the helpers uses a part of them."
)]
#![allow(
    unsafe_code,
    reason = "Edition 2024 makes std::env::set_var and remove_var unsafe: they race a thread that \
              reads the environment, which a case rules out by holding process_lock."
)]

use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard, PoisonError};

use katana_cad::customisation_host::{
    BUILT_IN_CUSTOMISATION_VARIABLE, KEPT_CUSTOMISATION_VARIABLE,
};

static PROCESS: Mutex<()> = Mutex::new(());

/// What the process shares - its environment, its working folder, its real
/// streams - is changed only while this is held. A case that panics while
/// holding it poisons it; the next takes it all the same, since what the
/// panicking case set was put back as it unwound.
pub fn process_lock() -> MutexGuard<'static, ()> {
    PROCESS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An environment variable set for one test and put back as it was - set or
/// not - when the test ends, so that no test depends on, or leaves a mark on,
/// the environment it runs in. The value is the OS string given, exactly: on
/// Windows its own wide text, so a folder named in any script is a value
/// (the C++'s second constructor, for a path).
pub struct ScopedVariable {
    name: &'static str,
    before: Option<OsString>,
}

impl ScopedVariable {
    /// The variable set to `value`.
    pub fn set(name: &'static str, value: impl AsRef<OsStr>) -> Self {
        let scoped = Self::remember(name);
        // SAFETY: the caller holds process_lock, so no other thread of this
        // process reads or writes the environment meanwhile.
        unsafe { std::env::set_var(name, value) };
        scoped
    }

    /// The variable not set at all (the C++'s nullptr).
    pub fn unset(name: &'static str) -> Self {
        let scoped = Self::remember(name);
        // SAFETY: as in `set`.
        unsafe { std::env::remove_var(name) };
        scoped
    }

    /// `value` when there is one, else unset.
    pub fn new(name: &'static str, value: Option<&str>) -> Self {
        match value {
            Some(value) => Self::set(name, value),
            None => Self::unset(name),
        }
    }

    fn remember(name: &'static str) -> Self {
        Self {
            name,
            before: std::env::var_os(name),
        }
    }
}

impl Drop for ScopedVariable {
    fn drop(&mut self) {
        match &self.before {
            // SAFETY: as in `set`: the case still holds process_lock.
            Some(before) => unsafe { std::env::set_var(self.name, before) },
            // SAFETY: as above.
            None => unsafe { std::env::remove_var(self.name) },
        }
    }
}

/// Both variables, set for one test: the built-in of the run, and the kept
/// file. None leaves that one unset.
pub struct StartEnvironment {
    pub built_in: ScopedVariable,
    pub kept: ScopedVariable,
}

impl StartEnvironment {
    pub fn new(built_in: Option<&str>, kept: Option<&str>) -> Self {
        Self {
            built_in: ScopedVariable::new(BUILT_IN_CUSTOMISATION_VARIABLE, built_in),
            kept: ScopedVariable::new(KEPT_CUSTOMISATION_VARIABLE, kept),
        }
    }
}

/// Two kanji, a hyphen and two Hebrew letters - U+65E5 U+672C, U+002D, U+05E2
/// U+05D1 - in UTF-8, each worked out from its code point: E6 97 A5, E6 9C AC,
/// 2D, D7 A2, D7 91. Two scripts because no ANSI code page holds both: on any
/// Windows machine whose code page is not UTF-8 itself, the C runtime's narrow
/// getenv has a '?' for some of them, and a file under a folder of this name
/// cannot be named through it.
pub const TWO_SCRIPTS: &str = "\u{65E5}\u{672C}-\u{05E2}\u{05D1}";

// Five characters in the eleven bytes the C++'s escapes spell.
const _: () = assert!(TWO_SCRIPTS.len() == 11);
