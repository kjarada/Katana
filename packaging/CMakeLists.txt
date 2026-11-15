# Katana's packages, made from the programs cargo built (docs/release.md,
# "Packaging"). A project of its own beside the test runner (the root
# CMakeLists.txt), and like it compiling nothing: it installs the four
# programs, the samples and the notices, deploys the libraries they load
# beside them, and has CPack make the zip and installer, the tarball or the
# disk image. All of that is cmake/KatanaPackaging.cmake; this only gives it
# the version and the programs' folder.
#
#   cd packaging
#   cmake --preset package -DKATANA_PROGRAMS=<cargo target>/release
#   cmake --build ../build/package --target package     (or --target bundle)
#
# A release adds -DKATANA_REQUIRE_UPDATE_KEYS=ON, -DKATANA_UPDATE_REPO and
# -DKATANA_UPDATE_SEQUENCE (.github/workflows/release.yml), and on Windows
# -DKATANA_RUNTIME_BIN (the MSYS2 environment's bin/) and on Linux and macOS
# -DKATANA_TOOLCHAIN (the prefix of tools/setup_linux_toolchain.py), the
# folders the programs' libraries are taken from.
#
# Why a project of its own and not part of the runner: the runner is
# configured for every test run, against whatever programs were built last,
# and a package is made once, for a release, from programs built for it; and
# the runner registers tests, this installs. Rejected: a Python script doing
# what CPack does - the per-user installer template (cmake/NSIS.template.in),
# the dependency scan (file(GET_RUNTIME_DEPENDENCIES)), the signing between
# staging and archiving and the install record are already written for CPack
# and proven by the C++ releases.

cmake_minimum_required(VERSION 3.24)

# The one place a version is written: project(Katana VERSION ...) of the root
# CMakeLists.txt, which katana_update's build.rs and the release workflow read
# too.
get_filename_component(_katana_root "${CMAKE_CURRENT_SOURCE_DIR}/.." ABSOLUTE)
file(STRINGS "${_katana_root}/CMakeLists.txt" _katana_project REGEX "^project[(]Katana VERSION ")
if(NOT _katana_project MATCHES "^project[(]Katana VERSION ([0-9]+[.][0-9]+([.][0-9]+)?([.][0-9]+)?)[ )]")
    message(FATAL_ERROR "${_katana_root}/CMakeLists.txt has no project(Katana VERSION <x.y.z> ...)")
endif()
project(KatanaPackage VERSION ${CMAKE_MATCH_1} LANGUAGES NONE)

list(APPEND CMAKE_MODULE_PATH "${_katana_root}/cmake")
include("${_katana_root}/cmake/KatanaPackaging.cmake")
