# The macOS toolchain: conda-forge's clang and libc++, CMake's find_package
# roots and the libraries' run-time path, all from the one prefix
# tools/setup_linux_toolchain.py installs on a Mac (docs/building.md, "macOS").
# The macos-* presets name this file. The counterpart of katana-linux.cmake;
# why clang and not GCC is in the setup script.
#
# The prefix is $KATANA_TOOLCHAIN, or /opt/katana-toolchain.

if(DEFINED ENV{KATANA_TOOLCHAIN} AND NOT "$ENV{KATANA_TOOLCHAIN}" STREQUAL "")
    set(KATANA_TOOLCHAIN "$ENV{KATANA_TOOLCHAIN}")
else()
    set(KATANA_TOOLCHAIN "/opt/katana-toolchain")
endif()

set(_katana_cxx "${KATANA_TOOLCHAIN}/bin/arm64-apple-darwin20.0.0-clang++")
if(NOT EXISTS "${_katana_cxx}")
    message(FATAL_ERROR
        "No Katana toolchain at ${KATANA_TOOLCHAIN} (${_katana_cxx} is missing). "
        "Install it with\n  python3 tools/setup_linux_toolchain.py\n"
        "or point KATANA_TOOLCHAIN at the prefix it was installed into.")
endif()

set(CMAKE_C_COMPILER "${KATANA_TOOLCHAIN}/bin/arm64-apple-darwin20.0.0-clang")
set(CMAKE_CXX_COMPILER "${_katana_cxx}")
set(CMAKE_OSX_ARCHITECTURES "arm64" CACHE STRING "")
# The oldest macOS the programs start on. conda-forge builds its libraries for
# 11.0; 13.3 is the first whose libc++ availability markup allows the C++17
# floating-point std::to_chars and std::from_chars the code uses - the
# markup is checked at compile time even though the libc++ that runs is the
# bundled one.
set(CMAKE_OSX_DEPLOYMENT_TARGET "13.3" CACHE STRING "")
list(PREPEND CMAKE_PREFIX_PATH "${KATANA_TOOLCHAIN}")

# libc++ and the libraries are conda-forge's, found at link time in the
# prefix's lib/ and at run time through the programs' RPATH.
set(CMAKE_EXE_LINKER_FLAGS_INIT "-L${KATANA_TOOLCHAIN}/lib")
set(CMAKE_SHARED_LINKER_FLAGS_INIT "-L${KATANA_TOOLCHAIN}/lib")
set(CMAKE_BUILD_RPATH "${KATANA_TOOLCHAIN}/lib")
# Installed, lib/ beside bin/ (cmake/KatanaDeployUnix.cmake.in).
set(CMAKE_INSTALL_RPATH "@loader_path/../lib")

set(KATANA_FIND_TEST_FRAMEWORKS ON CACHE BOOL
    "Find GoogleTest and Google Benchmark with find_package instead of downloading them")
