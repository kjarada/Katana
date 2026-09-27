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

# conda-forge's compilers are named by target triple: Apple silicon's toolchain
# starts at Darwin 20 (macOS 11), the Intel one at Darwin 13.4. The machine
# builds for itself, as the setup script installs the toolchain for it.
if(CMAKE_HOST_SYSTEM_PROCESSOR MATCHES "^(arm64|aarch64)$")
    set(_katana_triple "arm64-apple-darwin20.0.0")
    set(_katana_arch "arm64")
else()
    set(_katana_triple "x86_64-apple-darwin13.4.0")
    set(_katana_arch "x86_64")
endif()
set(_katana_cxx "${KATANA_TOOLCHAIN}/bin/${_katana_triple}-clang++")
if(NOT EXISTS "${_katana_cxx}")
    message(FATAL_ERROR
        "No Katana toolchain at ${KATANA_TOOLCHAIN} (${_katana_cxx} is missing). "
        "Install it with\n  python3 tools/setup_linux_toolchain.py\n"
        "or point KATANA_TOOLCHAIN at the prefix it was installed into.")
endif()

set(CMAKE_C_COMPILER "${KATANA_TOOLCHAIN}/bin/${_katana_triple}-clang")
set(CMAKE_CXX_COMPILER "${_katana_cxx}")
set(CMAKE_OSX_ARCHITECTURES "${_katana_arch}" CACHE STRING "")
# The oldest macOS the programs start on: 13, the oldest Qt 6.11 supports
# (conda-forge builds the other libraries for 11.0).
set(CMAKE_OSX_DEPLOYMENT_TARGET "13.0" CACHE STRING "")
# libc++'s availability markup refuses library functions the SYSTEM libc++ of
# the deployment target lacks - floating-point std::from_chars "introduced in
# macOS 26.0" stopped the first CI build. The programs never use the system's:
# they load conda-forge's libc++, which the package carries in lib/
# (cmake/KatanaDeployUnix.cmake.in), so the markup does not apply. This is
# conda-forge's documented remedy (conda-forge.org/docs/maintainer/
# knowledge_base, "Newer C++ features with old SDK").
set(CMAKE_CXX_FLAGS_INIT "-D_LIBCPP_DISABLE_AVAILABILITY")
list(PREPEND CMAKE_PREFIX_PATH "${KATANA_TOOLCHAIN}")

# libc++ and the libraries are conda-forge's, found at link time in the
# prefix's lib/ and at run time through the programs' RPATH.
set(CMAKE_EXE_LINKER_FLAGS_INIT "-L${KATANA_TOOLCHAIN}/lib")
set(CMAKE_SHARED_LINKER_FLAGS_INIT "-L${KATANA_TOOLCHAIN}/lib")
set(CMAKE_BUILD_RPATH "${KATANA_TOOLCHAIN}/lib")
# Installed, in Katana.app: the programs in Contents/MacOS, the libraries in
# Contents/Frameworks (cmake/KatanaDeployUnix.cmake.in).
set(CMAKE_INSTALL_RPATH "@loader_path/../Frameworks")

set(KATANA_FIND_TEST_FRAMEWORKS ON CACHE BOOL
    "Find GoogleTest and Google Benchmark with find_package instead of downloading them")
