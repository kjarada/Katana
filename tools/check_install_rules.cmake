# Checks the install rules a configure generated, without installing anything.
#
#   cmake -DKATANA_ROOT=<source dir> -DKATANA_BINARY=<build dir> -P tools/check_install_rules.cmake
#
# 1. Every SOURCE file an install rule names exists. `cmake --install` does
#    not skip a missing one: it stops there, after the programs are copied and
#    before the runtime DLLs are, and leaves a tree that does not start. A
#    top-level file was deleted from the repository while a rule still
#    installed it, and every bundle and package failed that way.
# 2. Nothing is installed from the build's third-party inputs: the folders the
#    built-in customisation is compiled from (resources/customisation) and the
#    reference files under docs/. They are kept on the owner's machine and are
#    not ours to hand on; a bundle is handed on.
#
# Reads <build dir>/cmake_install.cmake, where each rule is one
# `file(INSTALL ... FILES "<path>" ...)` line.

if(NOT KATANA_ROOT OR NOT KATANA_BINARY)
    message(FATAL_ERROR "Pass -DKATANA_ROOT=<source dir> -DKATANA_BINARY=<build dir>")
endif()
set(_script "${KATANA_BINARY}/cmake_install.cmake")
if(NOT EXISTS "${_script}")
    message(FATAL_ERROR "${_script} does not exist: configure the build first")
endif()

file(TO_CMAKE_PATH "${KATANA_ROOT}" _root)
file(STRINGS "${_script}" _rules REGEX "file[(]INSTALL ")
set(_violations "")
set(_checked 0)
foreach(_rule IN LISTS _rules)
    # The quoted paths after FILES, up to the first keyword that follows them.
    if(NOT _rule MATCHES " FILES (\"[^)]*)")
        continue()
    endif()
    string(REGEX MATCHALL "\"[^\"]+\"" _quoted "${CMAKE_MATCH_1}")
    foreach(_path IN LISTS _quoted)
        string(REPLACE "\"" "" _path "${_path}")
        # Built files (the programs) exist only after a build; a rule for one
        # is the build's to satisfy. Only what is taken from the sources is
        # checked here.
        string(FIND "${_path}" "${_root}/" _at)
        if(NOT _at EQUAL 0)
            continue()
        endif()
        file(RELATIVE_PATH _rel "${_root}" "${_path}")
        if(_rel MATCHES "^build/")
            continue()
        endif()
        math(EXPR _checked "${_checked} + 1")
        if(NOT EXISTS "${_path}")
            list(APPEND _violations "installs ${_rel}, which does not exist")
        endif()
        if(_rel MATCHES "^(resources/customisation|docs)(/|$)" OR _rel MATCHES "^(resources|docs)/?$")
            list(APPEND _violations "installs ${_rel}, a third-party input of the build")
        endif()
    endforeach()
endforeach()

if(_checked EQUAL 0)
    message(FATAL_ERROR "No install rule taking files from ${_root} was found in ${_script}")
endif()
if(_violations)
    list(JOIN _violations "\n  " _text)
    message(FATAL_ERROR "Install rule problems:\n  ${_text}")
endif()
message(STATUS "Install rules checked: ${_checked} source paths, all present, none third-party.")
