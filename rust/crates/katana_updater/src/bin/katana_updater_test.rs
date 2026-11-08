//! katana_updater_test: the helper's own source, built with the test key in the place of the pinned
//! ones and with the four options that exist only here - --die-at, --registry-base, --timings and
//! --stand-in (src/lib.rs, "TRUST"). Never installed: the end-to-end tests and tests/helper.rs
//! drive it as the real program. It is the C++'s main.cpp compiled again with
//! KATANA_UPDATER_TEST_BUILD (tests/update/CMakeLists.txt); here the same library, given
//! Build::test.
//!
//! The test keys are named in the tests' own module (rust/crates/katana_update/tests/test_key),
//! which this program shares as the C++ test executables link tests/update/test_key.cpp: the
//! helper that ships names none of them, and tests/shipped.rs looks for them in its bytes, with
//! this program as the control that holds them.

use std::process::ExitCode;

#[allow(
    dead_code,
    reason = "the module of the test keys is shared with the tests; this program uses the trust alone"
)]
#[path = "../../../katana_update/tests/test_key/mod.rs"]
mod test_key;

fn main() -> ExitCode {
    katana_updater::run_program(katana_updater::Build::test(test_key::test_trust))
}
