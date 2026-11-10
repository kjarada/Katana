//! The session katana_cli and katana_mcp share, and the UPDATE verbs it takes
//! (step 6, S1): the cases of these C++ files that change nothing the process
//! shares. One module per C++ test file:
//!
//! ```text
//! session        tests/app/test_session.cpp      Session (5 of 18)
//! update_verbs   tests/app/test_update_verbs.cpp UpdateVerbs (72 of 85: 47 portable, the
//!                                                 Linux-only one, 24 Windows-only)
//! view_scale     tests/app/test_view_scale.cpp   ViewScaleSession
//! ```
//!
//! The cases that set an environment variable or need the session of a
//! program (which reads the customisation variables) are in the target
//! tests/session_process, behind one lock. Of test_session.cpp, the reference
//! layers' cases are wave 2's (A, B); of test_update_verbs.cpp, the three of
//! katana_mcp's checkUpdateReply are the MCP server's (C).

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/app_support.rs"]
pub mod app_support;
#[path = "../../../katana_update/tests/test_key/mod.rs"]
pub mod test_key;
#[path = "../support/update_rig.rs"]
pub mod update_rig;

mod session;
mod update_verbs;
mod view_scale;
