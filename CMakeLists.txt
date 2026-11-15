cmake_minimum_required(VERSION 3.24)

# The version is written here and nowhere else: rust/crates/katana_update/build.rs reads it from
# this line into every program, and the update tests hold the programs to it.
project(Katana VERSION 0.3.0 LANGUAGES NONE)

# CMake builds nothing. Katana is built by cargo (rust/, docs/rust.md); CMake is the TEST RUNNER
# of the programs cargo built: it registers every process-level test - the `cli.` lines through
# katana_cli and katana_mcp (tests/cli), the window's headless checks through
# tools/check_screenshot.cmake (tests/), the update programs' tests (tests/update) - and the
# repository's checks that need no build (the docs, Katana Standard, the release tools), and
# `ctest --test-dir <build>` runs them. No compiler is looked for and no library is found.
#
# Rejected: a new runner for the 600-odd process tests. Their definitions, and the scripts they
# run, are the specification the C++ programs were held to and the Rust ones now are; moved to
# another runner they would have to be rewritten, and a rewrite is where an expectation drifts.

list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")

# Python is a convenience, never a requirement: the docs test (tools/check_docs.py), the IFC and
# utility-schema checks, the release tools' tests and the customisation censuses run when it is
# found and are not registered when it is not.
find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    set(KATANA_PYTHON "${Python3_EXECUTABLE}")
else()
    set(KATANA_PYTHON "python")
endif()

# ---------------------------------------------------------------------------
# The programs under test: cargo's release folder.
#
#   KATANA_PROGRAMS     where cargo put the programs: $CARGO_TARGET_DIR/release when the
#                       configure's environment names a CARGO_TARGET_DIR (tools/build_visible.sh
#                       sets one); else, on Linux and macOS, ~/katana-build/<checkout>/cargo/release,
#                       the folder build_visible.sh's WSL phase builds in; else, on Windows,
#                       rust/target/release, cargo's own default for rust/Cargo.toml.
#   KATANA_RUNTIME_BIN  the folder of the libraries the programs load, put first on each test's
#                       PATH: on Windows, MSYS2 UCRT64's bin (GDAL, PDAL, PROJ), where it is
#                       installed; elsewhere the programs' own folder, since cargo writes the
#                       libraries' run-time path into them (build_visible.sh's RUSTFLAGS).
#
# Each program is then KATANA_PROGRAM_<name> - KATANA_PROGRAM_katana, _katana_cli, _katana_mcp,
# _katana_updater, and the three test programs of rust/crates/katana_updater - which is what a
# test names where the C++ build's tests named $<TARGET_FILE:<name>>. A program that is not there
# is not a reason to register fewer tests: a test of it then fails, which is what it should do.
# ---------------------------------------------------------------------------
if(DEFINED ENV{CARGO_TARGET_DIR} AND NOT "$ENV{CARGO_TARGET_DIR}" STREQUAL "")
    set(_katana_default_programs "$ENV{CARGO_TARGET_DIR}/release")
elseif(CMAKE_HOST_WIN32)
    set(_katana_default_programs "${PROJECT_SOURCE_DIR}/rust/target/release")
else()
    get_filename_component(_katana_checkout_name "${PROJECT_SOURCE_DIR}" NAME)
    set(_katana_default_programs "$ENV{HOME}/katana-build/${_katana_checkout_name}/cargo/release")
endif()
set(KATANA_PROGRAMS "${_katana_default_programs}" CACHE PATH
    "The folder of the programs cargo built (cargo's release folder), which the tests run")
file(TO_CMAKE_PATH "${KATANA_PROGRAMS}" KATANA_PROGRAMS)

if(CMAKE_HOST_WIN32 AND EXISTS "C:/msys64/ucrt64/bin")
    set(_katana_default_runtime "C:/msys64/ucrt64/bin")
else()
    set(_katana_default_runtime "${KATANA_PROGRAMS}")
endif()
set(KATANA_RUNTIME_BIN "${_katana_default_runtime}" CACHE PATH
    "The folder of the libraries the programs load, first on every program test's PATH")

if(CMAKE_HOST_WIN32)
    set(KATANA_PROGRAM_SUFFIX ".exe")
else()
    set(KATANA_PROGRAM_SUFFIX "")
endif()
foreach(_katana_program IN ITEMS katana katana_cli katana_mcp katana_updater
                                 katana_updater_test katana_updater_stub katana_update_e2e)
    set(KATANA_PROGRAM_${_katana_program}
        "${KATANA_PROGRAMS}/${_katana_program}${KATANA_PROGRAM_SUFFIX}")
endforeach()
if(NOT EXISTS "${KATANA_PROGRAM_katana_cli}")
    message(STATUS "No katana_cli at ${KATANA_PROGRAM_katana_cli} yet: build with cargo "
                   "(docs/rust.md) before running ctest, or set KATANA_PROGRAMS to the folder "
                   "the programs are in")
endif()

# The families of process tests, each on by default: a runner with no display, say, can leave
# the window's out.
option(KATANA_TEST_CLI "Register the cli. tests, which run katana_cli and katana_mcp" ON)
option(KATANA_TEST_WINDOW "Register the window's headless checks, which run katana" ON)

# The built-in customisations the programs are built with: the files that
# rust/crates/katana_cad/build.rs compiles in when they are there, the default first. The tests
# that depend on which of them a build holds read this list.
set(KATANA_BUILTIN_FILES "")
foreach(_katana_builtin IN ITEMS katana-standard nsw)
    set(_katana_builtin_file
        "${PROJECT_SOURCE_DIR}/resources/customisation/${_katana_builtin}.customisation.json")
    if(EXISTS "${_katana_builtin_file}")
        list(APPEND KATANA_BUILTIN_FILES "${_katana_builtin_file}")
    endif()
endforeach()

# The update system's pinned public keys (cmake/KatanaUpdateKeys.cmake), which
# rust/crates/katana_update/build.rs compiles into the programs. ON makes this configure FAIL
# while they are placeholders, as KATANA_REQUIRE_UPDATE_KEYS in cargo's environment fails the
# build.
option(KATANA_REQUIRE_UPDATE_KEYS "Refuse to configure while the update keys are placeholders" OFF)
include(KatanaUpdateKeys)

enable_testing()
add_subdirectory(tests)
