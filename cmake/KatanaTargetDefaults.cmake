# Common compile settings applied to every first-party target.
#
#   katana_target_defaults(<target>)
#
# INTERFACE (header-only) targets only receive the include path; warnings are a
# property of the translation unit that includes them.

function(katana_target_defaults target)
    get_target_property(_type ${target} TYPE)
    if(_type STREQUAL "INTERFACE_LIBRARY")
        target_include_directories(${target} INTERFACE "${PROJECT_SOURCE_DIR}/include")
        target_compile_features(${target} INTERFACE cxx_std_${KATANA_CXX_STANDARD})
        return()
    endif()

    target_include_directories(${target} PUBLIC "${PROJECT_SOURCE_DIR}/include")
    target_compile_features(${target} PUBLIC cxx_std_${KATANA_CXX_STANDARD})

    if(MSVC)
        target_compile_options(${target} PRIVATE
            $<$<COMPILE_LANGUAGE:CXX>:/W4 /permissive- /utf-8>)
        if(KATANA_WARNINGS_AS_ERRORS)
            target_compile_options(${target} PRIVATE $<$<COMPILE_LANGUAGE:CXX>:/WX>)
        endif()
    else()
        # Guarded to C++: a target may also carry a Windows resource file, and
        # these flags are handed to EVERY language on the target - windres
        # rejects -Wall outright.
        target_compile_options(${target} PRIVATE
            $<$<COMPILE_LANGUAGE:CXX>:-Wall -Wextra -Wpedantic -Wshadow -Wnon-virtual-dtor
                                     -Woverloaded-virtual -Wformat=2>)
        if(KATANA_WARNINGS_AS_ERRORS)
            target_compile_options(${target} PRIVATE $<$<COMPILE_LANGUAGE:CXX>:-Werror>)
        endif()
        # Deterministic floating point: forbid value-changing optimisations and
        # fused multiply-add contraction so results match across optimisation levels.
        target_compile_options(${target} PRIVATE
            $<$<COMPILE_LANGUAGE:CXX>:-ffp-contract=off -fno-fast-math>)
    endif()

    # libstdc++ container/iterator precondition checks in debug builds.
    target_compile_definitions(${target} PRIVATE $<$<CONFIG:Debug>:_GLIBCXX_ASSERTIONS>)

    katana_apply_sanitizers(${target})
    katana_apply_static_analysis(${target})
endfunction()

# Registers every GoogleTest case of <target> with CTest, one test per case,
# each starting the executable directly:
#
#   katana_discover_tests(<target> [PREFIX <prefix>] [ENVIRONMENT <modifications>...])
#
# ENVIRONMENT is the cases' ENVIRONMENT_MODIFICATION (the runtime's PATH is
# always first), and a TEST_LAUNCHER on the target (xvfb-run for the GPU
# suites on Linux) or a cross-compiling emulator comes before the executable.
# This is the one way the project registers GoogleTest cases;
# katana_register_tests is it with no prefix.
#
# Why not gtest_discover_tests itself. Its PRE_TEST mode, in CMake 4.4 (the
# MSYS2 toolchain's), registers each case as
# `cmake -P GoogleTest/LaunchTest.cmake`, which then starts the test: two
# processes per case, where the Windows toolchain PATH
# used to add a third (a TEST_LAUNCHER of `cmake -E env --modify PATH=...`).
# It also lists every executable again on every ctest run, even `ctest -N`.
# On the owner's machine, whose virus scanner inspects every process start,
# the two cmake.exe hops were most of a trivial case's time and the listing
# 11-51 s of every run (docs/building.md, "Build and test speed", has the
# numbers). Its POST_BUILD mode registers the executable directly, but lists
# the cases while BUILDING, when the toolchain DLLs need not be on PATH, and a
# listing that dies with STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139; Git for
# Windows' bash puts its own older mingw64 first) would then fail the build.
#
# So the listing stays at test time, in a TEST_INCLUDE_FILES script
# (KatanaGoogleTests.cmake.in) that runs CMake's own lister -
# gtest_discover_tests_impl in GoogleTestAddTests.cmake, which POST_BUILD mode
# runs and which the PRE_TEST mode of CMake 3.x ran from exactly such a
# script - with the runtime on PATH for that listing only, and keeps the list
# until the executable is rebuilt. That function is CMake's internal
# interface: a CMake that changes it fails loudly, at listing, never by
# running fewer cases.
#
# The listing has a minute, not the default 5 s: the first run after the build
# has redeployed the GDAL/PDAL runtime loads freshly copied DLLs, which are
# scanned on first load, and listing the 1073-case widget suite took 10.7 s
# then. Listing normally takes a tenth of a second, so a minute costs nothing.
function(katana_discover_tests target)
    cmake_parse_arguments(PARSE_ARGV 1 ARG "" "PREFIX" "ENVIRONMENT")
    set(_katana_target ${target})
    set(_katana_prefix "${ARG_PREFIX}")
    set(_katana_environment "PATH=path_list_prepend:${KATANA_RUNTIME_BIN}")
    foreach(_modification IN LISTS ARG_ENVIRONMENT)
        string(APPEND _katana_environment ";${_modification}")
    endforeach()
    set(_katana_executor "")
    # What gtest_discover_tests puts before the executable, in its order: the
    # target's TEST_LAUNCHER, then, when cross-compiling, its emulator (qemu
    # for the aarch64 build, docs/building.md).
    get_property(_launcher TARGET ${target} PROPERTY TEST_LAUNCHER)
    if(CMAKE_CROSSCOMPILING)
        get_property(_emulator TARGET ${target} PROPERTY CROSSCOMPILING_EMULATOR)
        list(APPEND _launcher ${_emulator})
    endif()
    foreach(_word IN LISTS _launcher)
        string(APPEND _katana_executor " [==[${_word}]==]")
    endforeach()
    set(_katana_tests_file "${CMAKE_CURRENT_BINARY_DIR}/${target}_tests.cmake")
    set(_katana_include_file "${CMAKE_CURRENT_BINARY_DIR}/${target}_include.cmake")
    configure_file("${PROJECT_SOURCE_DIR}/cmake/KatanaGoogleTests.cmake.in"
                   "${CMAKE_CURRENT_BINARY_DIR}/${target}_include.cmake.in" @ONLY)
    file(GENERATE OUTPUT "${_katana_include_file}"
         INPUT "${CMAKE_CURRENT_BINARY_DIR}/${target}_include.cmake.in")
    set_property(DIRECTORY APPEND PROPERTY TEST_INCLUDE_FILES "${_katana_include_file}")
endfunction()

function(katana_register_tests target)
    katana_discover_tests(${target})
endfunction()
