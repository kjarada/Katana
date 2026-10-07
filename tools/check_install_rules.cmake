# Checks the install rules a configure generated, without installing anything.
#
#   cmake -DKATANA_ROOT=<source dir> -DKATANA_BINARY=<build dir> -P tools/check_install_rules.cmake
#
# 1. Every SOURCE file an install rule names exists. `cmake --install` does
#    not skip a missing one: it stops there, after the programs are copied and
#    before the runtime DLLs are, and leaves a tree that does not start. A
#    top-level file was deleted from the repository while a rule still
#    installed it, and every bundle and package failed that way.
# 2. Nothing is installed from the build's third-party inputs: the folder the
#    built-in customisation is compiled from (resources/customisation) and the
#    reference files under docs/. The built-in is tracked, but it is compiled
#    INTO the programs and travels inside them, so no copy of the file is
#    handed on beside them; the reference files are kept on the owner's
#    machine and are not ours to hand on. A bundle is handed on.
#
# 3. The only programs installed are katana, katana_cli and katana_mcp. A
#    program that is installed is one that is handed on, and the converter of
#    the older customisation formats (katana_customisation_convert) carries the
#    readers of those formats: it is built with everything else so that it
#    cannot rot, and is never to ship. The rule names the PROGRAMS, so a
#    developer's tool added next to it is refused until somebody decides it
#    should ship - and says so here.
#
# Reads <build dir>/cmake_install.cmake, where each rule is one
# `file(INSTALL ... FILES "<path>" ...)` line, and the scripts it includes
# (one for each directory that installs anything).

if(NOT KATANA_ROOT OR NOT KATANA_BINARY)
    message(FATAL_ERROR "Pass -DKATANA_ROOT=<source dir> -DKATANA_BINARY=<build dir>")
endif()
set(_script "${KATANA_BINARY}/cmake_install.cmake")
if(NOT EXISTS "${_script}")
    message(FATAL_ERROR "${_script} does not exist: configure the build first")
endif()

# Every `TYPE EXECUTABLE` rule of the top script and the scripts it includes,
# in the order they are met. Followed through the `include(...)` lines CMake
# wrote, not by searching the build tree: only what `cmake --install` would run.
set(KATANA_INSTALLED_PROGRAMS katana katana_cli katana_mcp)
list(JOIN KATANA_INSTALLED_PROGRAMS ", " _allowed_names)
set(_pending "${_script}")
set(_seen "")
set(_programs "")
set(_violations "")
while(_pending)
    list(POP_FRONT _pending _current)
    if(_current IN_LIST _seen OR NOT EXISTS "${_current}")
        continue()
    endif()
    list(APPEND _seen "${_current}")
    file(STRINGS "${_current}" _lines REGEX "include[(]\"|TYPE EXECUTABLE")
    foreach(_line IN LISTS _lines)
        if(_line MATCHES "^[ \t]*include[(]\"([^\"]+)\"[)]")
            list(APPEND _pending "${CMAKE_MATCH_1}")
        elseif(_line MATCHES " TYPE EXECUTABLE .*FILES (\"[^)]*)")
            string(REGEX MATCHALL "\"[^\"]+\"" _quoted "${CMAKE_MATCH_1}")
            foreach(_path IN LISTS _quoted)
                string(REPLACE "\"" "" _path "${_path}")
                get_filename_component(_program "${_path}" NAME_WE)
                list(APPEND _programs "${_program}")
                if(NOT _program IN_LIST KATANA_INSTALLED_PROGRAMS)
                    list(APPEND _violations
                        "installs the program ${_program}, which is not one of ${_allowed_names}")
                endif()
            endforeach()
        endif()
    endforeach()
endwhile()
list(LENGTH _programs _program_count)
if(_program_count EQUAL 0)
    message(FATAL_ERROR "No install rule for a program was found in ${_script}, or in the "
                        "scripts it includes: the check would pass with nothing to check")
endif()

file(TO_CMAKE_PATH "${KATANA_ROOT}" _root)
file(STRINGS "${_script}" _rules REGEX "file[(]INSTALL ")
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
list(REMOVE_DUPLICATES _programs)
list(JOIN _programs ", " _program_names)
message(STATUS "Install rules checked: ${_checked} source paths, all present, none third-party. "
               "Programs installed: ${_program_names}.")
