# Holds cmake/KatanaUpdateKeys.cmake to stating the one policy it needs itself:
#
#   cmake -DKEYS=<cmake/KatanaUpdateKeys.cmake> [-DMUTATE=<name>] -P check_keys_policy.cmake
#
# The file's header gives two uses, included from the root CMakeLists.txt and run as a script
# (cmake -DKATANA_REQUIRE_UPDATE_KEYS=ON -P cmake/KatanaUpdateKeys.cmake). A script starts with no
# policy set, and a CMake older than 4.0 - the release workflow's first job runs under 3.31.6 - then
# does not know `if(<value> IN_LIST <list>)` and stops at it. The file therefore says
# cmake_policy(SET CMP0057 NEW) before its first IN_LIST.
#
# Why this reads text and does not run the file: CMake 4 sets every policy older than 3.5 itself,
# CMP0057 (3.3) among them, so on the CMake that builds this project the file runs bare with or
# without its line and a run could not tell the two apart. The text can. update_keys_* in
# tests/update/CMakeLists.txt run the file for what it does; this holds what a newer CMake hides.
#
# Comments are whole lines only, as in check_nsis_script.cmake: a word in a comment is not an
# instruction, and the header of the file says IN_LIST in one.
#
# -DMUTATE=policy_removed        the line is taken out
# -DMUTATE=policy_after_the_use  the line is moved to the end of the file, after the IN_LIST
# -DMUTATE=policy_the_wrong_way  the line sets CMP0057 OLD
# -DMUTATE=policy_in_a_comment   the line is only a comment
# each of which the check must refuse, by name (a check that cannot see a fault passes as quietly as one
# that finds none).

if(NOT KEYS OR NOT EXISTS "${KEYS}")
    message(FATAL_ERROR "Pass -DKEYS=<cmake/KatanaUpdateKeys.cmake>")
endif()
file(READ "${KEYS}" _text)
string(REPLACE "\r\n" "\n" _text "${_text}")

set(_policy "cmake_policy(SET CMP0057 NEW)")
if(MUTATE)
    string(FIND "${_text}" "${_policy}" _present)
    if(_present EQUAL -1)
        message(FATAL_ERROR "mutation ${MUTATE} does not apply: the file has no  ${_policy}")
    endif()
    if(MUTATE STREQUAL "policy_removed")
        string(REPLACE "${_policy}" "" _text "${_text}")
    elseif(MUTATE STREQUAL "policy_after_the_use")
        string(REPLACE "${_policy}" "" _text "${_text}")
        string(APPEND _text "\n${_policy}\n")
    elseif(MUTATE STREQUAL "policy_the_wrong_way")
        string(REPLACE "${_policy}" "cmake_policy(SET CMP0057 OLD)" _text "${_text}")
    elseif(MUTATE STREQUAL "policy_in_a_comment")
        string(REPLACE "${_policy}" "# ${_policy}" _text "${_text}")
    else()
        message(FATAL_ERROR "unknown mutation '${MUTATE}'")
    endif()
endif()

# Whole-line comments out.
string(REGEX REPLACE "(^|\n)[ \t]*#[^\n]*" "\\1" _code "${_text}")

string(FIND "${_code}" "IN_LIST" _use)
if(_use EQUAL -1)
    message(FATAL_ERROR
        "${KEYS} no longer uses IN_LIST, so this check has nothing to hold: take the policy line and "
        "this check out together, and say so in docs/release.md, \"Not done\"")
endif()
string(FIND "${_code}" "${_policy}" _set)
string(FIND "${_code}" "cmake_policy(SET CMP0057 OLD)" _set_old)
if(NOT _set_old EQUAL -1)
    message(FATAL_ERROR "${KEYS} sets CMP0057 to OLD, which is the behaviour that stops at IN_LIST")
endif()
if(_set EQUAL -1)
    message(FATAL_ERROR
        "${KEYS} does not say  ${_policy}  as an instruction: run as a script under a CMake older than "
        "4.0 it stops at its first IN_LIST, because a script has no policy set")
endif()
if(_set GREATER _use)
    message(FATAL_ERROR
        "${KEYS} says  ${_policy}  after its first IN_LIST: the policy has to be set before the "
        "instruction that needs it")
endif()
message(STATUS "Update keys file: states CMP0057 before its first IN_LIST, so it runs as a script under any CMake")
