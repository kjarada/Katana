# Third-party sources that are built from source (test/benchmark harnesses only).
#
# Sources are cached under third_party/_cache after the first download so that
# additional build directories configure offline and never race on a download.

include(FetchContent)

set(KATANA_THIRD_PARTY_CACHE "${PROJECT_SOURCE_DIR}/third_party/_cache")

# ON takes both harnesses from an installed package (find_package) instead of
# building them from a download. The Linux toolchain file turns it on, since
# its prefix carries both and a cloud session cannot reach github.com; the
# Windows build keeps the pinned sources, so its versions do not move with
# whatever MSYS2 happens to have installed.
option(KATANA_FIND_TEST_FRAMEWORKS
       "Find GoogleTest and Google Benchmark with find_package instead of downloading them" OFF)

function(_katana_fetch name url)
    string(TOUPPER ${name} _upper)
    set(_cached "${KATANA_THIRD_PARTY_CACHE}/${name}")
    if(EXISTS "${_cached}/CMakeLists.txt")
        set(FETCHCONTENT_SOURCE_DIR_${_upper} "${_cached}" CACHE PATH "" FORCE)
    endif()

    FetchContent_Declare(${name} URL ${url})
    FetchContent_MakeAvailable(${name})

    if(NOT EXISTS "${_cached}/CMakeLists.txt")
        file(COPY "${${name}_SOURCE_DIR}/" DESTINATION "${_cached}")
    endif()
endfunction()

function(katana_provide_googletest)
    if(KATANA_FIND_TEST_FRAMEWORKS)
        find_package(GTest CONFIG REQUIRED GLOBAL)
        return()
    endif()
    set(gtest_force_shared_crt ON CACHE BOOL "" FORCE)
    set(INSTALL_GTEST OFF CACHE BOOL "" FORCE)
    _katana_fetch(googletest
        https://github.com/google/googletest/archive/refs/tags/v1.14.0.zip)
endfunction()

function(katana_provide_benchmark)
    if(KATANA_FIND_TEST_FRAMEWORKS)
        find_package(benchmark CONFIG REQUIRED GLOBAL)
        return()
    endif()
    set(BENCHMARK_ENABLE_TESTING OFF CACHE BOOL "" FORCE)
    set(BENCHMARK_ENABLE_GTEST_TESTS OFF CACHE BOOL "" FORCE)
    set(BENCHMARK_ENABLE_INSTALL OFF CACHE BOOL "" FORCE)
    set(BENCHMARK_ENABLE_WERROR OFF CACHE BOOL "" FORCE)
    # Two of Google Benchmark's feature checks ask about what the C++ standard
    # guarantees - std::regex ([re]) and std::chrono::steady_clock
    # ([time.clock.steady]), both C++11, and Katana needs C++23 - and each is a
    # try_run: a program compiled, linked and started. On a machine whose virus
    # scanner inspects every new executable, answering them instead took a
    # fresh configure from 41 s to 28 s (docs/building.md, "Build and test
    # speed"). Benchmark's own cxx_feature_check takes a defined
    # HAVE_<feature> as the answer and adds the same -DHAVE_<feature> a
    # passing check would. The checks whose answer the standard does not give
    # (the POSIX regex engines, pthread affinity) still run.
    set(HAVE_STD_REGEX 1)
    set(HAVE_STEADY_CLOCK 1)
    _katana_fetch(benchmark
        https://github.com/google/benchmark/archive/refs/tags/v1.9.1.zip)
endfunction()
