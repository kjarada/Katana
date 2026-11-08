//! What the build promises about the helper that ships, held on the Rust programs: the twins of the
//! CTest cases of tests/update/CMakeLists.txt that look at a program's bytes. Each runs the same
//! CMake script as its C++ twin, on the Rust programs, and is judged by the same expressions.
//!
//! ```text
//!   update_test_keys_are_in_no_shipped_program          check_no_test_keys.cmake, EXPECT=absent,
//!                                                       on the helper that ships
//!   update_test_keys_check_refuses_a_list_holding_a_program_that_has_them
//!                                                       the same on the test helper: it must refuse
//!   update_test_keys_control_is_found_in_katana_update_e2e
//!   update_test_keys_control_is_found_in_katana_updater_test
//!                                                       EXPECT=present: the controls, which carry
//!                                                       the keys, so that "absent" is not a scan
//!                                                       that cannot see
//!   katana_updater_imports_only_os_dlls                 check_updater_imports.cmake on the
//!   katana_updater_test_imports_only_os_dlls            programs' import tables (Windows)
//! ```
//!
//! trust.rs promises that the trust root is a parameter and only the test programs pass a test
//! key; the helper's library holds none, and the two test programs name the tests' own module of
//! them (src/bin/). The key list is read from tests/update/test_key.hpp, as tests/update/
//! CMakeLists.txt reads it, so that there is one. Of the four programs the C++ check looks at, the
//! helper is the one the port has written so far; katana, katana_cli and katana_mcp join the list
//! when they are written in Rust.
//!
//! The scripts need cmake on PATH, which runs every CTest case; where there is none the case says
//! so and returns. The import checks need objdump as well (CMAKE_OBJDUMP), and are registered on
//! Windows only, where the programs are PE files: written, not compiled, until the port is built
//! there (docs/rust.md, step 8). Not twinned: `updater_import_check_accepts_a_static_listing` and
//! `updater_import_check_names_the_gcc_runtime` test the script on its own fixtures and no program,
//! and the link probe (tests/update/link_probe.cpp) proves the C++ link line of OpenSSL and zlib,
//! which the Rust has no part of (ed25519-dalek and miniz_oxide are compiled into the programs).

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use support::{Ran, checkout, find_on_path, one_spaced, run, work_folder};

// The 64-digit hexadecimal strings of tests/update/test_key.hpp: the two seeds and the two public
// keys, read as tests/update/CMakeLists.txt reads them (every quoted run of lower-case hexadecimal
// digits, those of 64 kept), and held to there being four.
fn test_keys() -> String {
    let header = std::fs::read_to_string(checkout().join("tests/update/test_key.hpp"))
        .expect("tests/update/test_key.hpp");
    let mut keys: Vec<&str> = Vec::new();
    for (at, _) in header.match_indices('"') {
        let rest = &header[at + 1..];
        let digits = rest
            .bytes()
            .take_while(|&c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            .count();
        if digits == 64 && rest[digits..].starts_with('"') {
            keys.push(&rest[..digits]);
        }
    }
    assert_eq!(
        keys.len(),
        4,
        "tests/update/test_key.hpp should hold four 64-digit keys (two seeds, two public keys); \
         check_no_test_keys.cmake reads them from there"
    );
    keys.join(",")
}

fn script(name: &str) -> PathBuf {
    checkout().join("tests/update").join(name)
}

// `cmake <definitions> -P <script>`, or None, said, where there is no cmake.
fn cmake_script(definitions: &[String], name: &str) -> Option<Ran> {
    let Some(cmake) = find_on_path("cmake") else {
        eprintln!("no cmake on PATH: the CMake script {name} cannot be run here");
        return None;
    };
    let mut command = Command::new(cmake);
    command.args(definitions).arg("-P").arg(script(name));
    Some(run(command, Duration::from_secs(300)))
}

// PASS_REGULAR_EXPRESSION `phrase` (blanks and line breaks as one, katana_phrase) and
// FAIL_REGULAR_EXPRESSION "CMake Error", as CTest judges the cases.
fn expect_passes(ran: &Ran, phrase: &str) {
    let output = ran.output();
    assert!(
        one_spaced(&output).contains(phrase),
        "expected '{phrase}':\n{}",
        ran.describe()
    );
    assert!(!output.contains("CMake Error"), "{}", ran.describe());
}

fn definition(name: &str, value: impl AsRef<Path>) -> String {
    format!("-D{name}={}", value.as_ref().to_string_lossy())
}

// The programs, as a file a path a line (-DPROGRAM_LIST), as the C++ case hands them over.
fn program_list(name: &str, programs: &[&str]) -> PathBuf {
    let list = work_folder(name);
    let mut text = String::new();
    for program in programs {
        text.push_str(program);
        text.push('\n');
    }
    std::fs::write(&list, text).expect("the list of programs");
    list
}

#[test]
fn update_test_keys_are_in_no_shipped_program() {
    let list = program_list(
        "shipped_programs.txt",
        &[env!("CARGO_BIN_EXE_katana_updater")],
    );
    let definitions = [
        definition("PROGRAM_LIST", list.as_path()),
        format!("-DKEYS={}", test_keys()),
        String::from("-DEXPECT=absent"),
    ];
    if let Some(ran) = cmake_script(&definitions, "check_no_test_keys.cmake") {
        expect_passes(&ran, "No test key in");
    }
}

#[test]
fn update_test_keys_check_refuses_a_list_holding_a_program_that_has_them() {
    let list = program_list(
        "a_program_with_test_keys.txt",
        &[env!("CARGO_BIN_EXE_katana_updater_test")],
    );
    let definitions = [
        definition("PROGRAM_LIST", list.as_path()),
        format!("-DKEYS={}", test_keys()),
        String::from("-DEXPECT=absent"),
    ];
    if let Some(ran) = cmake_script(&definitions, "check_no_test_keys.cmake") {
        // Its PASS expression alone: the refusal is a FATAL_ERROR, which CMake prints as an error.
        assert!(
            one_spaced(&ran.output()).contains("A shipped program contains a test key"),
            "{}",
            ran.describe()
        );
    }
}

// "Control: [0-9]+ of 8 forms found", and no "CMake Error".
fn expect_control(program: &str) {
    let definitions = [
        definition("PROGRAM", program),
        format!("-DKEYS={}", test_keys()),
        String::from("-DEXPECT=present"),
    ];
    let Some(ran) = cmake_script(&definitions, "check_no_test_keys.cmake") else {
        return;
    };
    let output = ran.output();
    let found = output.match_indices("Control: ").any(|(at, key)| {
        let rest = &output[at + key.len()..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && rest[digits..].starts_with(" of 8 forms found")
    });
    assert!(found, "{}", ran.describe());
    assert!(!output.contains("CMake Error"), "{}", ran.describe());
}

#[test]
fn update_test_keys_control_is_found_in_katana_update_e2e() {
    expect_control(env!("CARGO_BIN_EXE_katana_update_e2e"));
}

#[test]
fn update_test_keys_control_is_found_in_katana_updater_test() {
    expect_control(env!("CARGO_BIN_EXE_katana_updater_test"));
}

// The helper's imports: `cmake -DOBJDUMP=<objdump> -DPROGRAM=<the program> -P
// check_updater_imports.cmake`, which must find every imported DLL to be the operating system's.
#[cfg(windows)]
fn expect_only_os_dlls(program: &str) {
    let Some(objdump) = find_on_path("objdump") else {
        eprintln!("no objdump on PATH: the import table of {program} is not read here");
        return;
    };
    let definitions = [
        definition("OBJDUMP", objdump.as_path()),
        definition("PROGRAM", program),
    ];
    if let Some(ran) = cmake_script(&definitions, "check_updater_imports.cmake") {
        expect_passes(&ran, "all of them the operating system's");
    }
}

#[cfg(windows)]
#[test]
fn katana_updater_imports_only_os_dlls() {
    expect_only_os_dlls(env!("CARGO_BIN_EXE_katana_updater"));
}

#[cfg(windows)]
#[test]
fn katana_updater_test_imports_only_os_dlls() {
    expect_only_os_dlls(env!("CARGO_BIN_EXE_katana_updater_test"));
}
