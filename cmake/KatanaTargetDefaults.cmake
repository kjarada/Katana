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
        target_compile_features(${target} INTERFACE cxx_std_23)
        return()
    endif()

    target_include_directories(${target} PUBLIC "${PROJECT_SOURCE_DIR}/include")
    target_compile_features(${target} PUBLIC cxx_std_23)

    if(MSVC)
        target_compile_options(${target} PRIVATE /W4 /permissive- /utf-8)
        if(KATANA_WARNINGS_AS_ERRORS)
            target_compile_options(${target} PRIVATE /WX)
        endif()
    else()
        target_compile_options(${target} PRIVATE
            -Wall -Wextra -Wpedantic -Wshadow -Wnon-virtual-dtor -Woverloaded-virtual -Wformat=2)
        if(KATANA_WARNINGS_AS_ERRORS)
            target_compile_options(${target} PRIVATE -Werror)
        endif()
        # Deterministic floating point: forbid value-changing optimisations and
        # fused multiply-add contraction so results match across optimisation levels.
        target_compile_options(${target} PRIVATE -ffp-contract=off -fno-fast-math)
    endif()

    # libstdc++ container/iterator precondition checks in debug builds.
    target_compile_definitions(${target} PRIVATE $<$<CONFIG:Debug>:_GLIBCXX_ASSERTIONS>)

    katana_apply_sanitizers(${target})
    katana_apply_static_analysis(${target})
endfunction()

# Registers the GoogleTest cases of <target> with CTest. Discovery happens at
# test time so that building never needs the runtime DLLs on PATH.
#
# Discovery RUNS the executable to enumerate its cases, so on Windows it needs
# the toolchain DLLs exactly as running a test does. gtest_discover_tests applies
# PROPERTIES to the discovered tests only, never to the discovery step itself
# (GoogleTestAddTests.cmake simply writes them into the generated test file), so
# ENVIRONMENT_MODIFICATION alone leaves discovery dependent on the ambient PATH.
# Where that PATH lacks the toolchain the executable dies at load time with
# STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139) and the whole suite fails to list,
# which is a failure of the environment rather than of any test.
#
# A TEST_LAUNCHER is threaded through both paths - gtest_discover_tests passes it
# as the discovery TEST_EXECUTOR, and CTest uses it to run each case - so setting
# it fixes discovery and execution together and makes the suite independent of
# the environment it is invoked from.
function(katana_register_tests target)
    if(WIN32)
        set_property(TARGET ${target} PROPERTY TEST_LAUNCHER
            ${CMAKE_COMMAND} -E env --modify
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN}")
    endif()
    gtest_discover_tests(${target}
        DISCOVERY_MODE PRE_TEST
        PROPERTIES ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:${KATANA_RUNTIME_BIN}"
    )
endfunction()
