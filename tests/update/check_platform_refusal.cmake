# Where a copy of Katana does not replace its own files, the helper program refuses a swap and a
# recovery FOR THE PLATFORM, and writes nothing.
#
#   cmake -DHELPER=<katana_updater or katana_updater_test> -DWORK=<a folder of this test's own>
#         -P check_platform_refusal.cmake
#
# Registered where the system is not Windows (tests/update/CMakeLists.txt): Linux, until its apply
# back end is written (task T18, src/katana_update/fsops_posix.cpp), and macOS for good, where a
# file changed inside Katana.app breaks the seal of the bundle. There the helper is started with
# the two command lines it has for changing an install - the swap, and --recover - and each must
# end
#
#   outcome=refused   (the swap)   or   recover=refused   (--recover)
#   reason=platform
#   exit=13
#   detail=<a sentence>
#
# with the exit code 13, having made no file and no folder anywhere.
#
# Why it matters: before this was held, the helper on these systems answered a swap with
# `root_not_writable` AFTER writing a last.json into the state folder, which the window reads at
# its next start and reports as an update that failed; and it answered --recover with "failed",
# exit 12, the code that tells a person their install may be half swapped.
#
# WHERE THE EXPECTATIONS COME FROM - none is the program's output read back:
#   - 13 is "refused: nothing was touched" in the helper's table of exit codes, which its own
#     --help prints (src/katana_updater/main.cpp, top);
#   - `platform` is the reason the UPDATE verbs give for a system that does not install an
#     update, and what these systems get is settled outside the code: they are told of a release,
#     and an install of it is refused as unsupported, for the platform, changing nothing;
#   - "wrote nothing" is THIS script's listing of its folder after each run (file(GLOB) and
#     if(EXISTS), CMake's own), never the helper's word.
#
# WHEN T18 WRITES THE LINUX BACK END this stays for macOS alone, and holds there whatever that
# back end answers: it is what fails if writing it let the helper swap files inside a bundle.

cmake_minimum_required(VERSION 3.24)

foreach(_needed IN ITEMS HELPER WORK)
    if(NOT DEFINED ${_needed} OR "${${_needed}}" STREQUAL "")
        message(FATAL_ERROR "check_platform_refusal.cmake needs -DHELPER=<the helper program> "
            "and -DWORK=<a folder of this test's own>")
    endif()
endforeach()
# The helper takes absolute folders only on its swap line.
if(NOT IS_ABSOLUTE "${WORK}")
    message(FATAL_ERROR "-DWORK must be an absolute path: ${WORK}")
endif()

# The folder, made new: an install folder that is there and empty (--recover needs one that
# exists), and a stage and a state folder that are NOT there.
file(REMOVE_RECURSE "${WORK}")
file(MAKE_DIRECTORY "${WORK}/install")
set(_root "${WORK}/install")
set(_stage "${WORK}/stage")
set(_state "${WORK}/state")

# What is under WORK and in its install folder, by name.
function(katana_list_work out)
    file(GLOB _inside LIST_DIRECTORIES true RELATIVE "${WORK}" "${WORK}/*" "${WORK}/install/*")
    set(${out} "${_inside}" PARENT_SCOPE)
endfunction()

# Nothing is under WORK but the empty install folder. The three names a helper that went on
# would make first are asked for one by one - the state folder (last.json), the stage folder and
# the install's .update (the lock, the journal, recovery.log) - and then everything is listed.
function(katana_expect_nothing_written after)
    foreach(_made IN ITEMS "${_state}" "${_stage}" "${_root}/.update")
        if(EXISTS "${_made}" OR IS_SYMLINK "${_made}")
            message(FATAL_ERROR "After ${after} there is ${_made}: the helper wrote something on "
                "a system where it changes nothing.")
        endif()
    endforeach()
    katana_list_work(_inside)
    if(NOT "${_inside}" STREQUAL "install")
        message(FATAL_ERROR "After ${after} the test's folder holds more than its empty install "
            "folder: ${_inside}")
    endif()
endfunction()

# The listing can see what it is there to find: a check that cannot see a violation passes as
# quietly as one that finds none. A file put into the install folder must show in it.
file(WRITE "${_root}/probe" "")
katana_list_work(_with_probe)
file(REMOVE "${_root}/probe")
if(NOT "${_with_probe}" STREQUAL "install;install/probe")
    message(FATAL_ERROR "The listing of the test's folder does not show a file put into its "
        "install folder (it shows: ${_with_probe}), so it could not show one the helper made.")
endif()

# Runs the helper and holds it to the four lines and the exit code. `first` is the first line.
function(katana_expect_refusal what first)
    execute_process(COMMAND "${HELPER}" ${ARGN}
        RESULT_VARIABLE _code OUTPUT_VARIABLE _said ERROR_VARIABLE _error TIMEOUT 120)
    set(_report "exit code: ${_code}\nstandard output:\n${_said}\nstandard error:\n${_error}")
    if(NOT "${_code}" STREQUAL "13")
        message(FATAL_ERROR "The helper, asked for ${what}, did not exit with 13 (refused).\n"
            "${_report}")
    endif()
    if(NOT "${_said}" MATCHES
            "^${first}\r?\nreason=platform\r?\nexit=13\r?\ndetail=[^\r\n]+\r?\n$")
        message(FATAL_ERROR "The helper, asked for ${what}, did not print exactly ${first}, "
            "reason=platform, exit=13 and a detail= line.\n${_report}")
    endif()
    katana_expect_nothing_written("${what}")
endfunction()

katana_expect_nothing_written("the folder was made")
katana_expect_refusal("a swap" "outcome=refused"
    --root "${_root}" --stage "${_stage}" --state "${_state}"
    --tx a1b2c3d4 --wait-pid 0 --relaunch none)
katana_expect_refusal("a recovery" "recover=refused" --recover "${_root}")

file(REMOVE_RECURSE "${WORK}")
message(STATUS "The helper refused a swap and a recovery for the platform, and wrote nothing.")
