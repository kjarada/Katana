//! The version this library was built as. The C++ build passes project(Katana VERSION ...) to the
//! update library as KATANA_VERSION (src/katana_update/CMakeLists.txt); the port reads the same
//! line of the checkout's CMakeLists.txt, so the version is still written in one place.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    );
    let cmake = manifest.join("../../../CMakeLists.txt");
    println!("cargo:rerun-if-changed={}", cmake.display());
    let text = std::fs::read_to_string(&cmake)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", cmake.display()));
    let version = project_version(&text)
        .unwrap_or_else(|| panic!("{} has no project(Katana VERSION ...)", cmake.display()));
    println!("cargo:rustc-env=KATANA_VERSION={version}");
}

// The word after VERSION inside project(Katana ...). It is not checked here: programVersion is
// the check, and a broken one is its Internal error, as in the C++.
fn project_version(text: &str) -> Option<&str> {
    const OPENING: &str = "project(Katana";
    let start = text.find(OPENING)? + OPENING.len();
    let rest = &text[start..];
    let arguments = &rest[..rest.find(')')?];
    let mut words = arguments.split_ascii_whitespace();
    while let Some(word) = words.next() {
        if word == "VERSION" {
            return words.next();
        }
    }
    None
}
