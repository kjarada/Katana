# Every test that runs one of the programs says which built-in customisation
# the program starts with.
#
#   cmake -DCTEST=<ctest> -DBUILD=<build tree> [-DCONFIGURATION=<name>]
#         -DPROGRAM_LIST=<file> -DVARIABLE=KATANA_BUILTIN_CUSTOMISATION
#         -P tools/check_program_tests.cmake
#
# A build may have a customisation compiled in and may have none (the owner's
# has one; a clean checkout and every release build have nothing), and a test
# that left the choice to the build would be two tests. So a test whose
# command names katana, katana_cli or katana_mcp must set VARIABLE
# (cad/customisation_host.hpp, "The seam"), and this names every one that
# does not, and fails.
#
# It is run at TEST time, over the listing ctest itself gives of what it will
# run (`ctest --show-only=json-v1`): every directory's tests, whenever each
# was configured, with the properties they really have - and it judges a test
# by its COMMAND, not by its name. The scan in tests/CMakeLists.txt that GIVES
# the tests the variable goes by name, and only through the directories it
# can see; a check that shared its list could name nothing the scan had not
# just set, and so could not fail. This one shares nothing with it.
#
# What a test must have, the last that names VARIABLE deciding:
#   ENVIRONMENT               VARIABLE=<value>
#   ENVIRONMENT_MODIFICATION  VARIABLE=set:<value>
#   its command line          VARIABLE=<value> or VARIABLE=set:<value>, as
#                             `cmake -E env [--modify]` takes them
# An empty value, `unset:` and `reset:` do not say: they leave the program
# with whatever its build compiled in.
#
# What it cannot see: a program that a test starts from inside a script or an
# executable without naming it on the command line. There is none today.
#
#   -DPROGRAM_LIST=<file>   the programs, a file a line - in a file, since on
#                           this test's own command line they would make it a
#                           test that names a program
#   -DPROGRAMS=<a>|<b>      or given here, '|' between them
#   -DLISTING=<file>        a listing read from a file in the place of ctest's:
#                           how the check is itself checked, on a listing with
#                           tests in it that it must name

cmake_minimum_required(VERSION 3.24)

if(NOT DEFINED VARIABLE OR VARIABLE STREQUAL "")
    message(FATAL_ERROR "check_program_tests.cmake needs -DVARIABLE")
endif()

set(_programs "")
if(DEFINED PROGRAM_LIST)
    file(STRINGS "${PROGRAM_LIST}" _programs)
elseif(DEFINED PROGRAMS)
    string(REPLACE "|" ";" _programs "${PROGRAMS}")
endif()
if(NOT _programs)
    message(FATAL_ERROR "check_program_tests.cmake needs -DPROGRAM_LIST or -DPROGRAMS")
endif()

if(DEFINED LISTING)
    file(READ "${LISTING}" _listing)
    set(_from "${LISTING}")
else()
    foreach(_needed CTEST BUILD)
        if(NOT DEFINED ${_needed})
            message(FATAL_ERROR "check_program_tests.cmake needs -D${_needed}, or -DLISTING")
        endif()
    endforeach()
    set(_configuration "")
    if(DEFINED CONFIGURATION AND NOT CONFIGURATION STREQUAL "")
        set(_configuration -C "${CONFIGURATION}")
    endif()
    execute_process(
        COMMAND "${CTEST}" --show-only=json-v1 ${_configuration}
        WORKING_DIRECTORY "${BUILD}"
        RESULT_VARIABLE _result
        OUTPUT_VARIABLE _listing
        ERROR_VARIABLE _error)
    if(NOT _result EQUAL 0)
        message(FATAL_ERROR "ctest --show-only=json-v1 failed in ${BUILD} (${_result}):\n${_error}")
    endif()
    set(_from "ctest's listing of ${BUILD}")
endif()

string(JSON _total ERROR_VARIABLE _problem LENGTH "${_listing}" tests)
if(_problem)
    message(FATAL_ERROR "${_from} is not a ctest json-v1 listing: ${_problem}")
endif()

# The tests, an element each of a CMake list, so that each is parsed as JSON
# on its own: string(JSON) parses the whole of what it is handed at every
# call, and seven thousand tests asked one by one of an eight-megabyte listing
# is minutes. The array is first written again by string(JSON GET) - this
# CMake's own layout, whatever layout the listing came in: an element begins
# on a line of the array's indent and a brace, and nothing inside it does (it
# is indented further, and a line break in a JSON string is written \n). The
# indent is read from the first element rather than known here. The three
# characters a CMake list reads as its own are set aside meanwhile, and put
# back into each element.
string(JSON _tests GET "${_listing}" tests)
unset(_listing)
string(ASCII 1 _semicolon)
string(ASCII 2 _open)
string(ASCII 3 _close)
string(REPLACE ";" "${_semicolon}" _tests "${_tests}")
string(REPLACE "[" "${_open}" _tests "${_tests}")
string(REPLACE "]" "${_close}" _tests "${_tests}")
set(_indent "")
if(_tests MATCHES "^${_open}\n([ \t]+){\n")
    set(_indent "${CMAKE_MATCH_1}")
    string(REPLACE "\n${_indent}{\n" ";" _tests "${_tests}")
endif()
list(LENGTH _tests _elements)
math(EXPR _elements "${_elements} - 1") # the first is what stands before the first test
if(NOT _elements EQUAL _total)
    # Never a guess: a layout this does not know would otherwise pass as
    # "no test runs a program".
    message(FATAL_ERROR
        "${_from} holds ${_total} tests and ${_elements} were found in it: "
        "string(JSON) no longer writes an array as tools/check_program_tests.cmake reads it")
endif()

# Whether `argument` names the file `program` - whole, not as the start of a
# longer name beside it (bin/katana is not bin/katana_cli, nor a tool whose
# name only begins the same).
function(katana_names_program argument program out)
    set(${out} FALSE PARENT_SCOPE)
    string(LENGTH "${program}" _length)
    set(_rest "${argument}")
    while(TRUE)
        string(FIND "${_rest}" "${program}" _at)
        if(_at LESS 0)
            return()
        endif()
        math(EXPR _after "${_at} + ${_length}")
        string(SUBSTRING "${_rest}" ${_after} 1 _next)
        if(NOT "${_next}" MATCHES "^[A-Za-z0-9_.-]$")
            set(${out} TRUE PARENT_SCOPE)
            return()
        endif()
        string(SUBSTRING "${_rest}" ${_after} -1 _rest)
    endwhile()
endfunction()

# What an entry `VARIABLE=...` says: TRUE when it gives the variable a value.
# `plain` is how ENVIRONMENT writes one (VARIABLE=value), `modification` how
# ENVIRONMENT_MODIFICATION does (VARIABLE=set:value).
function(katana_entry_says entry plain modification out)
    set(${out} "" PARENT_SCOPE) # not about the variable at all
    string(LENGTH "${VARIABLE}=" _length)
    string(FIND "${entry}" "${VARIABLE}=" _at)
    if(NOT _at EQUAL 0)
        return()
    endif()
    string(SUBSTRING "${entry}" ${_length} -1 _value)
    set(_says FALSE)
    if(modification AND _value MATCHES "^set:.")
        set(_says TRUE)
    elseif(plain AND NOT _value STREQUAL "" AND NOT _value MATCHES "^(set|unset|reset):")
        set(_says TRUE)
    endif()
    set(${out} "${_says}" PARENT_SCOPE)
endfunction()

# A Windows path names one file however its letters are cased.
if(CMAKE_HOST_WIN32)
    string(TOLOWER "${_programs}" _programs)
endif()

set(_count 0)
set(_bare "")
set(_first TRUE)
foreach(_test IN LISTS _tests)
    if(_first)
        set(_first FALSE)
        continue()
    endif()
    string(REPLACE "${_semicolon}" ";" _test "${_test}")
    string(REPLACE "${_open}" "[" _test "${_test}")
    string(REPLACE "${_close}" "]" _test "${_test}")
    # Up to the brace that closes it, which is the last line of that indent
    # and a brace: what follows is the comma, or the end of the array.
    string(FIND "${_test}" "\n${_indent}}" _end REVERSE)
    if(_end LESS 0)
        message(FATAL_ERROR "a test of ${_from} does not end as string(JSON) writes one")
    endif()
    string(SUBSTRING "${_test}" 0 ${_end} _test)
    set(_test "{\n${_test}\n}")

    string(JSON _name ERROR_VARIABLE _problem GET "${_test}" name)
    if(_problem)
        message(FATAL_ERROR "a test of ${_from} has no name: ${_problem}")
    endif()
    # No command: a test whose executable was not built. It runs nothing.
    string(JSON _kind ERROR_VARIABLE _problem TYPE "${_test}" command)
    if(_problem OR NOT _kind STREQUAL "ARRAY")
        continue()
    endif()
    string(JSON _arguments LENGTH "${_test}" command)
    if(_arguments EQUAL 0)
        continue()
    endif()

    # `_line` and `_environment` are empty while nothing has named the
    # variable, then TRUE or FALSE as the last entry that named it says.
    set(_runs FALSE)
    set(_line "")
    math(EXPR _last "${_arguments} - 1")
    foreach(_index RANGE ${_last})
        string(JSON _argument GET "${_test}" command ${_index})
        katana_entry_says("${_argument}" TRUE TRUE _entry)
        if(NOT _entry STREQUAL "")
            set(_line "${_entry}")
        endif()
        if(CMAKE_HOST_WIN32)
            string(TOLOWER "${_argument}" _argument)
        endif()
        foreach(_program IN LISTS _programs)
            katana_names_program("${_argument}" "${_program}" _named)
            if(_named)
                set(_runs TRUE)
            endif()
        endforeach()
    endforeach()
    if(NOT _runs)
        continue()
    endif()
    math(EXPR _count "${_count} + 1")

    # The properties are in name order, which is the order ctest applies
    # these two in: ENVIRONMENT, then its modifications.
    set(_environment "")
    string(JSON _properties ERROR_VARIABLE _problem LENGTH "${_test}" properties)
    if(NOT _problem AND _properties GREATER 0)
        math(EXPR _last "${_properties} - 1")
        foreach(_index RANGE ${_last})
            string(JSON _key GET "${_test}" properties ${_index} name)
            if(NOT _key STREQUAL "ENVIRONMENT" AND NOT _key STREQUAL "ENVIRONMENT_MODIFICATION")
                continue()
            endif()
            set(_modification FALSE)
            set(_plain TRUE)
            if(_key STREQUAL "ENVIRONMENT_MODIFICATION")
                set(_modification TRUE)
                set(_plain FALSE)
            endif()
            # A list is an array; one entry may be written as a string.
            string(JSON _kind TYPE "${_test}" properties ${_index} value)
            set(_entries "")
            if(_kind STREQUAL "ARRAY")
                string(JSON _length LENGTH "${_test}" properties ${_index} value)
                if(_length GREATER 0)
                    math(EXPR _stop "${_length} - 1")
                    foreach(_at RANGE ${_stop})
                        string(JSON _one GET "${_test}" properties ${_index} value ${_at})
                        katana_entry_says("${_one}" ${_plain} ${_modification} _entry)
                        if(NOT _entry STREQUAL "")
                            set(_environment "${_entry}")
                        endif()
                    endforeach()
                endif()
            elseif(_kind STREQUAL "STRING")
                string(JSON _one GET "${_test}" properties ${_index} value)
                katana_entry_says("${_one}" ${_plain} ${_modification} _entry)
                if(NOT _entry STREQUAL "")
                    set(_environment "${_entry}")
                endif()
            endif()
        endforeach()
    endif()
    # What the command line names wins: `cmake -E env` sets its own after
    # ctest has set the properties' on the process it starts from.
    set(_says "${_environment}")
    if(NOT _line STREQUAL "")
        set(_says "${_line}")
    endif()
    if(NOT _says)
        list(APPEND _bare "${_name}")
    endif()
endforeach()

if(_bare)
    list(JOIN _bare ", " _named)
else()
    set(_named "none")
endif()
message(STATUS
    "Of ${_count} tests that run a program, those that do not say which built-in customisation they start with: ${_named}.")
if(_bare)
    message(FATAL_ERROR
        "Give each of them ${VARIABLE} in its ENVIRONMENT_MODIFICATION - `set:none` to "
        "start with nothing, or `set:<file>` for a fixture (docs/headless.md, "
        "\"The customisation a run starts with\").")
endif()
if(_count EQUAL 0)
    # The programs are built and no test names one: it is this check that is
    # not reading the listing, and "none" would say the opposite.
    message(FATAL_ERROR "no test of ${_from} names a program (${_programs})")
endif()
