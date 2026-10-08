# What cmake/KatanaBuiltInDefault.cmake does with the cache entry
# KATANA_BUILTIN_CUSTOMISATION, proved on small projects configured here with
# the real CMake: the behaviour is about what a CACHE holds between two
# configures, which only two configures can show.
#
#   cmake -DKATANA_ROOT=<source tree> -DSCRATCH=<folder to work in>
#         -P tools/check_builtin_default.cmake
#
# Each case is a build folder of its own. A project is two lines (the module,
# and the entry reported back); the project of an EARLIER version is the one
# line that left a FILEPATH entry holding the NSW file, which is all a stale
# build directory is. The five cases:
#
#   a new directory             gets the default, says nothing
#   a stale directory           holding the NSW file is moved to the default,
#                               once, with a line saying so; configured again
#                               it stays and says nothing
#   -D, no type                 a choice, kept, said nothing of, on a new
#                               directory and on the second configure
#   -D, a type, after the move  the entry is settled, so the choice is kept
#   a stale directory holding   a person's own file is not the earlier default
#   another file                and stays
#
# A check that cannot see the stale directory moved would pass as quietly as
# one that moved nothing, so the stale case is the one that must show it.

if(NOT KATANA_ROOT OR NOT SCRATCH)
    message(FATAL_ERROR "usage: cmake -DKATANA_ROOT=<source tree> -DSCRATCH=<folder> -P check_builtin_default.cmake")
endif()

set(_default "/katana/resources/customisation/katana-standard.customisation.json")
set(_nsw "/old/resources/customisation/nsw.customisation.json")
set(_mine "/mine/my.customisation.json")

file(REMOVE_RECURSE "${SCRATCH}")
file(MAKE_DIRECTORY "${SCRATCH}")

# The project as it is now, and as it was when the entry was set plainly.
function(_write_project folder version)
    file(MAKE_DIRECTORY "${folder}")
    if(version STREQUAL "now")
        file(WRITE "${folder}/CMakeLists.txt"
"cmake_minimum_required(VERSION 3.24)
project(builtin_default NONE)
list(APPEND CMAKE_MODULE_PATH \"${KATANA_ROOT}/cmake\")
include(KatanaBuiltInDefault)
katana_declare_builtin_default(\"${_default}\")
message(STATUS \"ENTRY=[\${KATANA_BUILTIN_CUSTOMISATION}]\")
")
    else()
        file(WRITE "${folder}/CMakeLists.txt"
"cmake_minimum_required(VERSION 3.24)
project(builtin_default NONE)
set(KATANA_BUILTIN_CUSTOMISATION \"${_nsw}\" CACHE FILEPATH \"the earlier default\")
")
    endif()
endfunction()

# Configures `case` and returns what the entry was and whether the move was
# said, in the parent's `entry` and `said`. `ARGN` is passed to cmake.
function(_configure case)
    execute_process(
        COMMAND "${CMAKE_COMMAND}" -G Ninja -S "${SCRATCH}/${case}" -B "${SCRATCH}/${case}/build" ${ARGN}
        RESULT_VARIABLE _rc OUTPUT_VARIABLE _out ERROR_VARIABLE _err)
    if(NOT _rc EQUAL 0)
        message(FATAL_ERROR "${case}: configure failed (${_rc})\n${_out}\n${_err}")
    endif()
    string(REGEX MATCH "ENTRY=\\[([^]]*)\\]" _match "${_out}")
    set(entry "${CMAKE_MATCH_1}" PARENT_SCOPE)
    if(_out MATCHES "KATANA_BUILTIN_CUSTOMISATION held the NSW file")
        set(said TRUE PARENT_SCOPE)
    else()
        set(said FALSE PARENT_SCOPE)
    endif()
endfunction()

function(_expect case what entry_now said_now want_entry want_said)
    if(NOT "${entry_now}" STREQUAL "${want_entry}")
        message(FATAL_ERROR "${case}, ${what}: the entry is [${entry_now}], expected [${want_entry}]")
    endif()
    if(NOT said_now STREQUAL want_said)
        message(FATAL_ERROR "${case}, ${what}: the move was said=${said_now}, expected ${want_said}")
    endif()
endfunction()

# 1. A new directory.
_write_project("${SCRATCH}/new" now)
_configure(new)
_expect(new "first configure" "${entry}" "${said}" "${_default}" FALSE)

# 2. A stale directory: configured by the version that set the entry plainly
# to the NSW file, then by this one.
_write_project("${SCRATCH}/stale" before)
_configure(stale)
_expect(stale "the earlier version" "" "${said}" "" FALSE)
_write_project("${SCRATCH}/stale" now)
_configure(stale)
_expect(stale "the first configure of this version" "${entry}" "${said}" "${_default}" TRUE)
_configure(stale)
_expect(stale "the second" "${entry}" "${said}" "${_default}" FALSE)

# 3. A choice with -D and no type: on a new directory it is the entry, and it
# stays through the next configure.
_write_project("${SCRATCH}/chosen" now)
_configure(chosen "-DKATANA_BUILTIN_CUSTOMISATION=${_nsw}")
_expect(chosen "given with -D" "${entry}" "${said}" "${_nsw}" FALSE)
_configure(chosen)
_expect(chosen "configured again" "${entry}" "${said}" "${_nsw}" FALSE)

# 3b. The same on a stale directory: the -D is a choice, not the old value.
_write_project("${SCRATCH}/stale_chosen" before)
_configure(stale_chosen)
_write_project("${SCRATCH}/stale_chosen" now)
_configure(stale_chosen "-DKATANA_BUILTIN_CUSTOMISATION=${_nsw}")
_expect(stale_chosen "given with -D over a stale entry" "${entry}" "${said}" "${_nsw}" FALSE)

# 4. After the move, a typed -D is a choice too: the entry is settled.
_configure(stale "-DKATANA_BUILTIN_CUSTOMISATION:FILEPATH=${_nsw}")
_expect(stale "typed, after the move" "${entry}" "${said}" "${_nsw}" FALSE)

# 5. A stale directory that holds a person's own file is not the earlier
# default, and keeps it.
_write_project("${SCRATCH}/mine" before)
_configure(mine "-DKATANA_BUILTIN_CUSTOMISATION=${_mine}")
_write_project("${SCRATCH}/mine" now)
_configure(mine)
_expect(mine "a file of the person's own" "${entry}" "${said}" "${_mine}" FALSE)

message(STATUS "KatanaBuiltInDefault: the earlier default is moved once, a choice is kept")
