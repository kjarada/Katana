# The Linux toolchain: GCC, CMake's find_package roots and the libraries'
# run-time path, all from the one prefix tools/setup_linux_toolchain.py
# installs (docs/building.md, "Linux"). The linux-* presets name this file;
# without them:
#
#   cmake -S . -B build/linux -G Ninja \
#         -DCMAKE_TOOLCHAIN_FILE=cmake/toolchains/katana-linux.cmake
#
# The prefix is $KATANA_TOOLCHAIN, or /opt/katana-toolchain. A prefix that is
# missing fails here, saying how to make it, rather than falling back to the
# system compiler: a distribution's GCC 14 configures cleanly and then fails
# in half a dozen files for reasons that look like defects in Katana.

if(DEFINED ENV{KATANA_TOOLCHAIN} AND NOT "$ENV{KATANA_TOOLCHAIN}" STREQUAL "")
    set(KATANA_TOOLCHAIN "$ENV{KATANA_TOOLCHAIN}")
else()
    set(KATANA_TOOLCHAIN "/opt/katana-toolchain")
endif()

set(_katana_cxx "${KATANA_TOOLCHAIN}/bin/x86_64-conda-linux-gnu-g++")
if(NOT EXISTS "${_katana_cxx}")
    message(FATAL_ERROR
        "No Katana toolchain at ${KATANA_TOOLCHAIN} (${_katana_cxx} is missing). "
        "Install it with\n  python3 tools/setup_linux_toolchain.py\n"
        "or point KATANA_TOOLCHAIN at the prefix it was installed into.")
endif()

set(CMAKE_C_COMPILER "${KATANA_TOOLCHAIN}/bin/x86_64-conda-linux-gnu-gcc")
set(CMAKE_CXX_COMPILER "${_katana_cxx}")
list(PREPEND CMAKE_PREFIX_PATH "${KATANA_TOOLCHAIN}")

# Programs built here load the prefix's libstdc++, Qt, GDAL and PROJ, which are
# newer than the system's; the run-time path finds them without an
# LD_LIBRARY_PATH, as the Windows build tree finds its DLLs beside it.
set(CMAKE_BUILD_RPATH "${KATANA_TOOLCHAIN}/lib")
set(CMAKE_INSTALL_RPATH "${KATANA_TOOLCHAIN}/lib")

# GoogleTest and Google Benchmark come from the prefix too, so a Linux
# configure needs no download (cmake/KatanaThirdParty.cmake).
set(KATANA_FIND_TEST_FRAMEWORKS ON CACHE BOOL
    "Find GoogleTest and Google Benchmark with find_package instead of downloading them")
