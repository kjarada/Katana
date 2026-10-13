# No shipped program carries a test key.
#
#   cmake -DPROGRAM_LIST=<file> -DKEYS=<hex>,<hex>,... -DEXPECT=absent  -P check_no_test_keys.cmake
#   cmake -DPROGRAM=<exe>       -DKEYS=<hex>,<hex>,... -DEXPECT=present -P check_no_test_keys.cmake
#
# The programs are given in a FILE, a path a line (-DPROGRAM_LIST), and not on the command
# line, for the reason tools/check_program_tests.cmake gives for its own list: a test whose
# command names katana, katana_cli or katana_mcp is a test that RUNS a program, and must say
# which customisation it starts with. This one only reads bytes.
#
# The update system's central promise (include/katana/update/trust.hpp) is that the trust
# root is a PARAMETER and only the test executables pass a test key, so that no shipped
# binary contains one and nothing in the environment can move which signatures count. A
# promise that nothing checks is a promise about the author's intentions; this checks the
# bytes. KEYS are the 64-digit values of tests/update/test_key.hpp (the CMakeLists beside
# this file reads them from there, so there is one list): the two seeds and the two public
# keys. Each is looked for in the two forms it could take in a binary - as the 32 raw bytes,
# and as the 64 characters of a string literal.
#
# EXPECT=present is the CONTROL: run on a test executable, which does carry them, it fails
# unless the scan finds them. A scan that cannot see a key would pass the "absent" runs
# just as quietly, and a check that cannot fail is not one.

if(NOT (PROGRAM OR PROGRAM_LIST) OR NOT KEYS OR NOT EXPECT)
    message(FATAL_ERROR
        "Pass -DPROGRAM=<exe> or -DPROGRAM_LIST=<file>, -DKEYS=<hex>,<hex>,... and "
        "-DEXPECT=absent|present")
endif()
set(_programs "")
if(PROGRAM_LIST)
    file(STRINGS "${PROGRAM_LIST}" _programs)
endif()
if(PROGRAM)
    list(APPEND _programs "${PROGRAM}")
endif()
if(NOT _programs)
    message(FATAL_ERROR "No program to look at: ${PROGRAM_LIST} names none")
endif()
foreach(_program IN LISTS _programs)
    if(NOT EXISTS "${_program}")
        message(FATAL_ERROR "${_program} does not exist: build it first")
    endif()
endforeach()

string(REPLACE "," ";" _keys "${KEYS}")
foreach(_key IN LISTS _keys)
    string(LENGTH "${_key}" _length)
    if(NOT _length EQUAL 64 OR NOT _key MATCHES "^[0-9a-f]+$")
        message(FATAL_ERROR "'${_key}' is not 64 lower-case hexadecimal digits: the key list is wrong")
    endif()
endforeach()

set(_offences "")
set(_forms 0)
set(_controls_found 0)
foreach(_program IN LISTS _programs)
    file(READ "${_program}" _bytes HEX)   # lower-case hexadecimal, two digits to the byte
    set(_found "")
    set(_forms 0)
    foreach(_key IN LISTS _keys)
        # As raw bytes: the digits of the bytes ARE the key's own hex digits.
        # As text: the bytes of the 64 characters, which `string(HEX)` spells.
        string(HEX "${_key}" _as_text)
        foreach(_form IN ITEMS "raw|${_key}" "text|${_as_text}")
            string(REPLACE "|" ";" _parts "${_form}")
            list(GET _parts 0 _kind)
            list(GET _parts 1 _needle)
            math(EXPR _forms "${_forms} + 1")
            string(FIND "${_bytes}" "${_needle}" _at)
            if(NOT _at EQUAL -1)
                string(SUBSTRING "${_key}" 0 16 _id)
                list(APPEND _found "${_id}... (${_kind})")
            endif()
        endforeach()
    endforeach()
    list(LENGTH _found _count)
    if(EXPECT STREQUAL "absent")
        if(_found)
            list(JOIN _found ", " _list)
            list(APPEND _offences "${_program}: ${_list}")
        else()
            message(STATUS "No test key in ${_program} (${_forms} forms checked).")
        endif()
    elseif(EXPECT STREQUAL "present")
        if(NOT _found)
            message(FATAL_ERROR
                "The control ${_program} should contain the test keys and the scan found none "
                "of the ${_forms} forms: this scan cannot see a key, so its \"absent\" results "
                "prove nothing.")
        endif()
        message(STATUS "Control: ${_count} of ${_forms} forms found in ${_program}, as expected.")
    else()
        message(FATAL_ERROR "EXPECT is absent or present, not '${EXPECT}'")
    endif()
endforeach()

if(_offences)
    list(JOIN _offences "\n  " _list)
    message(FATAL_ERROR "A shipped program contains a test key:\n  ${_list}\nA shipped program "
        "must take its trust from productionTrust() and never link tests/update/test_key.cpp.")
endif()
