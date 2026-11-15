//! The session and UPDATE cases that change what the process shares (step 6,
//! S1): the customisation variables a program's session reads at its start,
//! KATANA_UPDATE_TOKEN and KATANA_UPDATE_STATE_DIR. One module per C++ test
//! file they come from:
//!
//! ```text
//! session        tests/app/test_session.cpp      Session: the 8 start-up cases
//! update_verbs   tests/app/test_update_verbs.cpp UpdateVerbs: the 5 contexts of a program,
//!                                                 and katana_cli's session (CliSession) -
//!                                                 2 portable, 3 Windows-only
//! ```
//!
//! gtest ran each case in a process of its own; Rust runs a binary's tests on
//! parallel threads of one, so each case here holds start_environment's
//! process lock for its whole length, and the variables it sets are put back
//! before it lets go. The other cases of those files are in the target
//! tests/session.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/app_support.rs"]
pub mod app_support;
#[path = "../support/start_environment.rs"]
pub mod start_environment;
#[path = "../../../katana_update/tests/test_key/mod.rs"]
pub mod test_key;
#[path = "../support/update_rig.rs"]
pub mod update_rig;

mod session;
mod update_verbs;
