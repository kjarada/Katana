cmake_minimum_required(VERSION 3.24)

project(Katana VERSION 0.2.0 LANGUAGES CXX)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")

# Python is used at build time to embed the built-in customisation into the binary
# (tools/embed_customisation.py). Optional: without it the generated table is
# empty and Katana draws plain lines.
find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    set(KATANA_PYTHON "${Python3_EXECUTABLE}")
else()
    set(KATANA_PYTHON "python")
endif()

# The C++ standard the whole project is built to. A variable rather than a
# constant so that a build can be made to another standard and MEASURED
# against this one rather than argued about - see docs/performance.md.
set(KATANA_CXX_STANDARD "26" CACHE STRING "C++ standard (23 or 26)")
set_property(CACHE KATANA_CXX_STANDARD PROPERTY STRINGS 23 26)

set(CMAKE_CXX_STANDARD ${KATANA_CXX_STANDARD})
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE RelWithDebInfo CACHE STRING "Build type" FORCE)
endif()

option(KATANA_BUILD_TESTS "Build unit and integration tests" ON)
option(KATANA_BUILD_BENCHMARKS "Build the benchmark executable" ON)
option(KATANA_BUILD_QT_APP "Build the Qt 6 desktop application" ON)
option(KATANA_BUILD_IO "Build the GDAL/PDAL interoperability module" ON)
option(KATANA_WARNINGS_AS_ERRORS "Treat compiler warnings as errors" ON)
option(KATANA_ENABLE_SANITIZERS "Instrument with ASan/UBSan where the toolchain supports it" OFF)
option(KATANA_ENABLE_CLANG_TIDY "Run clang-tidy during compilation (requires clang-tidy)" OFF)
option(KATANA_ENABLE_CPPCHECK "Run cppcheck during compilation (requires cppcheck)" OFF)
# Compile through ccache when it is installed (MSYS2:
# mingw-w64-ucrt-x86_64-ccache), unless the configure names a launcher of its
# own: a fresh build tree of a checkout, or a branch switched away from and
# back, then compiles only what the cache has not seen. docs/building.md,
# "Build and test speed", has what a cold and a warm cache measured.
#
# Without ccache's base_dir, so a second worktree does not share the first
# one's entries. base_dir makes ccache hand the compiler paths relative to the
# build tree, and __FILE__ with them: the survey-format and customisation
# tests that find their fixtures beside __FILE__ then looked in the wrong
# place, and 160 cases failed.
option(KATANA_CCACHE "Compile through ccache when it is found" ON)
if(KATANA_CCACHE AND NOT CMAKE_CXX_COMPILER_LAUNCHER)
    find_program(KATANA_CCACHE_PROGRAM ccache)
    if(KATANA_CCACHE_PROGRAM)
        set(CMAKE_CXX_COMPILER_LAUNCHER "${KATANA_CCACHE_PROGRAM}")
        message(STATUS "Compiling through ${KATANA_CCACHE_PROGRAM} (KATANA_CCACHE=OFF to stop)")
    endif()
endif()
# Who publishes the build: the company in katana.exe's version resource, the
# installer's publisher, and the name the packages are signed under
# (cmake/KatanaSign.cmake, docs/release.md "Signing").
set(KATANA_PUBLISHER "Jarada" CACHE STRING "The publisher named in the programs, installer and signatures")

# Where the reference customisation is kept in the legacy formats, the ones
# the Katana customisation format is to take the place of: two style libraries
# and two survey code files, with their colour table in a `support` folder
# beside them. It is third-party material under its author's own licence, so
# the folder is git-ignored - it is on the owner's machine and in no clone -
# and nothing committed needs it. Only the converter of those formats
# (src/katana_archive12d/legacy; docs/customisation.md, "Converting a
# customisation from the legacy formats") is run on it, and only the test of
# what the converter makes of it reads it: that test skips when the folder is
# absent or holds no file in those formats, and FAILS when it holds some and
# they are not the four. Defined here, above the modules and the tests,
# because both the converter's module and its test suite are to see one value.
# The owner's folder is named for the program its files came from; the name is
# assembled so that no tracked file spells it (CLAUDE.md, section 9).
string(CONCAT _katana_reference_folder "1" "2d Refrence Files")
set(KATANA_REFERENCE_CUSTOMISATION_DIR "${PROJECT_SOURCE_DIR}/docs/${_katana_reference_folder}" CACHE PATH
    "Folder of the reference customisation in the legacy formats; the converter's reference test reads it and skips when it is absent or holds no such file")

# ---------------------------------------------------------------------------
# Where things are built.
#
# CMake's default mirrors the source tree inside the build tree, which is how
# the application came to be at build/release/src/katana_qt/katana_qt_app.exe -
# a path that reads as though the executable were in the sources, and that
# nobody could be expected to find. Everything a person runs now lands in
# <build>/bin, with the tests and benchmarks one level down so that bin itself
# holds only what ships:
#
#   <build>/bin/katana.exe          the application
#   <build>/bin/katana_cli.exe      the command-line front end
#   <build>/bin/tests/              one test executable per module
#   <build>/bin/benchmarks/
#   <build>/lib/                    static libraries
#
# Set before any add_subdirectory, because the variables initialise each
# target's property at the moment the target is created. `cmake --install`
# (or the `bundle` target) turns bin/ into a self-contained folder; see
# cmake/KatanaPackaging.cmake.
# ---------------------------------------------------------------------------
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/lib")
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/lib")
set(KATANA_TEST_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin/tests")
set(KATANA_BENCHMARK_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin/benchmarks")

include(GNUInstallDirs)
include(KatanaTargetDefaults)
include(KatanaSimd)
include(KatanaSanitizers)
include(KatanaStaticAnalysis)
include(KatanaThirdParty)

# Directory holding the toolchain runtime DLLs (MinGW). Tests need it on PATH.
get_filename_component(KATANA_RUNTIME_BIN "${CMAKE_CXX_COMPILER}" DIRECTORY)

if(KATANA_BUILD_TESTS)
    include(CTest)
    enable_testing()
    katana_provide_googletest()
    include(GoogleTest)
endif()

# ---------------------------------------------------------------------------
# Modules, lowest layer first. The allowed dependency direction is recorded in
# tools/check_layering.cmake and verified by the `layering` test.
# ---------------------------------------------------------------------------
set(KATANA_MODULES
    katana_core
    katana_math
    katana_geometry
    katana_entity
    katana_commands
    katana_storage
    katana_geodesy
    katana_survey
    katana_surveyio
    katana_terrain
    katana_archive12d
    katana_dxf
    katana_ifc
    katana_render
    katana_cad
)
if(KATANA_BUILD_IO)
    # katana_io first: katana_interop builds on it.
    list(APPEND KATANA_MODULES katana_io katana_interop)
endif()
if(KATANA_BUILD_QT_APP)
    list(APPEND KATANA_MODULES katana_qt)
endif()
list(APPEND KATANA_MODULES katana_app)

# Restricts the configured modules (and their test suites), e.g.
#   -DKATANA_MODULE_FILTER="katana_core;katana_math;katana_geometry;katana_terrain"
# so that one module can be built in isolation while others are in flux.
set(KATANA_MODULE_FILTER "" CACHE STRING "If non-empty, only these modules are configured")

foreach(module IN LISTS KATANA_MODULES)
    if(KATANA_MODULE_FILTER AND NOT module IN_LIST KATANA_MODULE_FILTER)
        continue()
    endif()
    if(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/src/${module}/CMakeLists.txt")
        add_subdirectory(src/${module})
    endif()
endforeach()

# A filtered configuration must still be a COMPLETE one.
#
# CMake turns a link against a target that does not exist into a bare -lname and
# says nothing, so excluding a module that another included module depends on
# configures cleanly, builds every object file, and then fails at LINK time with
# "ld returned 1" and no indication of why. That happened: a build directory
# carried a KATANA_MODULE_FILTER from before katana_cad depended on
# katana_terrain and katana_render, and the failure looked like a code
# regression rather than a stale cache.
#
# PLAN.MD section 36 forbids silent failures, so the graph is checked here and
# named precisely.
set(_katana_missing "")
foreach(module IN LISTS KATANA_MODULES)
    if(NOT TARGET ${module})
        continue()
    endif()
    set(_deps "")
    foreach(_property LINK_LIBRARIES INTERFACE_LINK_LIBRARIES)
        get_target_property(_value ${module} ${_property})
        if(_value)
            list(APPEND _deps ${_value})
        endif()
    endforeach()
    foreach(_dep IN LISTS _deps)
        # Only first-party modules; third-party names are found by find_package.
        if(_dep MATCHES "^katana_" AND NOT TARGET ${_dep})
            list(APPEND _katana_missing "${module} needs ${_dep}")
        endif()
    endforeach()
endforeach()
if(_katana_missing)
    list(REMOVE_DUPLICATES _katana_missing)
    list(JOIN _katana_missing "
  " _text)
    message(FATAL_ERROR
        "KATANA_MODULE_FILTER excludes a module that another one depends on:
  ${_text}
"
        "Add the missing modules to KATANA_MODULE_FILTER, or clear it to configure them all. "
        "Without this check the configure succeeds and the LINK fails with no explanation.")
endif()

if(KATANA_BUILD_TESTS)
    add_subdirectory(tests)
endif()

if(KATANA_BUILD_BENCHMARKS)
    add_subdirectory(benchmarks)
endif()

include(KatanaFormat)
include(KatanaPackaging)
