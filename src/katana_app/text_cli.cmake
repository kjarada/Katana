# Names and paths outside ASCII through the real katana_cli and katana_mcp
# (docs/headless.md, "Text and names"); included from CMakeLists.txt beside this
# file, where the two programs and KATANA_RUNTIME_BIN are.
#
# One folder, named with an accented letter (in the Windows ANSI code page), a
# letter in none of the Western ones and a Han character, is made by the first
# test and used by the rest, in order (text_cli_check.cmake builds each
# character from its UTF-8 bytes, so every file stays ASCII). Before the
# programs read their command line wide, the first two ended in a
# filesystem_error, the third was "cannot read script", the fourth refused the
# name and the fifth ended before it answered `initialize`.
#
# They cannot drive a CONSOLE (there is none under ctest): the console's own
# line reading (ReadConsoleW, main.cpp) is the one part checked by reading. What
# it hands on is the UTF-16 to UTF-8 conversion that tests/update/
# test_contract.cpp checks case by case, and what a line that is not UTF-8
# becomes is the pipe test here.
set(_text_cli_work "${CMAKE_CURRENT_BINARY_DIR}/cli_non_ascii")
set(_text_cli_check "${CMAKE_CURRENT_SOURCE_DIR}/text_cli_check.cmake")

# One check step as a test. Each requires the one before it, which made the
# folder (or the script, or the project) that it needs.
function(katana_text_cli_test name step previous)
    add_test(NAME ${name}
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                ${CMAKE_COMMAND}
                "-DCLI=$<TARGET_FILE:katana_cli>" "-DMCP=$<TARGET_FILE:katana_mcp>"
                "-DWORK=${_text_cli_work}" "-DSTEP=${step}" -P "${_text_cli_check}")
    set_tests_properties(${name} PROPERTIES FIXTURES_SETUP text_cli_${step} TIMEOUT 120)
    if(NOT previous STREQUAL "")
        set_tests_properties(${name} PROPERTIES FIXTURES_REQUIRED text_cli_${previous})
    endif()
endfunction()

katana_text_cli_test(cli.non_ascii_path_is_saved_to_from_a_c_line save "")
katana_text_cli_test(cli.non_ascii_path_is_opened_from_a_c_line open save)
katana_text_cli_test(cli.non_ascii_script_path_and_names_in_it_are_read script open)
katana_text_cli_test(cli.ansi_lines_piped_in_are_read_as_windows_1252_names pipe script)
katana_text_cli_test(cli.mcp_opens_a_non_ascii_project_path_and_runs_a_script_there mcp pipe)
