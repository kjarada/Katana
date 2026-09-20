# Third-party sources that are built from source (test/benchmark harnesses only).
#
# Sources are cached under third_party/_cache after the first download so that
# additional build directories configure offline and never race on a download.

include(FetchContent)

set(KATANA_THIRD_PARTY_CACHE "${PROJECT_SOURCE_DIR}/third_party/_cache")

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
    set(gtest_force_shared_crt ON CACHE BOOL "" FORCE)
    set(INSTALL_GTEST OFF CACHE BOOL "" FORCE)
    _katana_fetch(googletest
        https://github.com/google/googletest/archive/refs/tags/v1.14.0.zip)
endfunction()

function(katana_provide_benchmark)
    set(BENCHMARK_ENABLE_TESTING OFF CACHE BOOL "" FORCE)
    set(BENCHMARK_ENABLE_GTEST_TESTS OFF CACHE BOOL "" FORCE)
    set(BENCHMARK_ENABLE_INSTALL OFF CACHE BOOL "" FORCE)
    set(BENCHMARK_ENABLE_WERROR OFF CACHE BOOL "" FORCE)
    _katana_fetch(benchmark
        https://github.com/google/benchmark/archive/refs/tags/v1.9.1.zip)
endfunction()
