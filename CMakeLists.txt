cmake_minimum_required(VERSION 3.24)

project(Katana VERSION 0.2.0 LANGUAGES CXX)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")

set(CMAKE_CXX_STANDARD 23)
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

include(KatanaTargetDefaults)
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
    katana_terrain
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

if(KATANA_BUILD_TESTS)
    add_subdirectory(tests)
endif()

if(KATANA_BUILD_BENCHMARKS)
    add_subdirectory(benchmarks)
endif()

include(KatanaFormat)
