//! katana_updater, the helper that ships: the program of this crate's library with the keys pinned
//! at build time (src/lib.rs says what it does; src/katana_updater/main.cpp is the C++).

use std::process::ExitCode;

fn main() -> ExitCode {
    katana_updater::run_program(katana_updater::Build::SHIPPED)
}
