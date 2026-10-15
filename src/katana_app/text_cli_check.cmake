# Names and paths outside ASCII, through the real katana_cli and katana_mcp
# (docs/headless.md, "Text and names"), run by the cli.non_ascii_* tests that
# text_cli.cmake registers:
#
#   cmake -DCLI=<katana_cli> -DMCP=<katana_mcp> -DWORK=<scratch folder> -DSTEP=<step> -P text_cli_check.cmake
#
# The programs are started with a command line that holds characters beyond
# the Windows ANSI code page, which is the case that failed: argv holds that
# code page's version of the command line, so an accented letter was one byte
# no path conversion accepts (the program ended in a filesystem_error), and a
# Polish or Chinese letter was already a '?' (the folder was "not found").
#
# This file stays ASCII: each character is built from its UTF-8 bytes, which
# CMake hands to the child process as UTF-8 text (it converts to the wide
# command line on Windows) - the same text a person's shell would.
#   U+00E9  e-acute         C3 A9        in Windows-1252 (the ANSI page of the
#                                         Western locales), 0xE9
#   U+0141  L with stroke   C5 81        in no Western code page
#   U+6D4B  a Han character E6 B5 8B     in no Western code page
foreach(_required CLI MCP WORK STEP)
    if(NOT DEFINED ${_required})
        message(FATAL_ERROR "${_required} must be given")
    endif()
endforeach()
string(ASCII 195 169 _e_acute)
string(ASCII 197 129 _l_stroke)
string(ASCII 230 181 139 _han)
string(ASCII 233 _ansi_e_acute) # the single byte 0xE9: Windows-1252, and no UTF-8

set(_folder "${WORK}/Jos${_e_acute}-${_l_stroke}ukasz-${_han}")
set(_project "${_folder}/site")
set(_script "${_folder}/run-${_e_acute}.kcs")

# Runs a program; the output (both streams) and the exit code. FATAL_ERROR with
# everything the program said when the code is not `expect`.
function(run_program expect out_var)
    set(_stdin "")
    if(NOT "${_input}" STREQUAL "")
        set(_stdin INPUT_FILE "${_input}")
    endif()
    execute_process(COMMAND ${ARGN}
        RESULT_VARIABLE _code OUTPUT_VARIABLE _out ERROR_VARIABLE _err
        ${_stdin} ENCODING UTF8)
    if(NOT _code STREQUAL "${expect}")
        message(FATAL_ERROR
            "exit code ${_code}, expected ${expect}\n--- stdout ---\n${_out}\n--- stderr ---\n${_err}")
    endif()
    set(${out_var} "${_out}\n${_err}" PARENT_SCOPE)
endfunction()

function(must_contain text what)
    string(FIND "${text}" "${what}" _at)
    if(_at EQUAL -1)
        message(FATAL_ERROR "expected to find '${what}' in:\n${text}")
    endif()
endfunction()

function(must_not_contain text what)
    string(FIND "${text}" "${what}" _at)
    if(NOT _at EQUAL -1)
        message(FATAL_ERROR "did not expect '${what}' in:\n${text}")
    endif()
endfunction()

if(STEP STREQUAL "save")
    # A project under a folder named with all three, from a -c line.
    file(REMOVE_RECURSE "${WORK}")
    file(MAKE_DIRECTORY "${_folder}")
    set(_input "")
    run_program(0 _out "${CLI}" -c "RECT 0,0 30,20" -c "SAVE \"${_project}\"")
    must_contain("${_out}" "saved to")
    if(NOT EXISTS "${_project}/project.db")
        message(FATAL_ERROR "no project.db under ${_project}:\n${_out}")
    endif()
elseif(STEP STREQUAL "open")
    # ... opened again by a -c line (the case that ended the program: OPEN of a
    # path with one accented letter in it).
    set(_input "")
    run_program(0 _out "${CLI}" -c "OPEN \"${_project}\"" -c "LIST")
    must_contain("${_out}" "1 entities")
    must_not_contain("${_out}" "error")
elseif(STEP STREQUAL "script")
    # A script file with such a name, given as the argument, and a script whose
    # lines carry a name of the same kind.
    file(WRITE "${_script}" "RECT 0,0 5,5\nLAYER NEW Caf${_e_acute}\nLIST\nLAYER LIST\n")
    set(_input "")
    run_program(0 _out "${CLI}" "${_script}")
    must_contain("${_out}" "1 entities")
    must_contain("${_out}" "Caf${_e_acute}")
    must_not_contain("${_out}" "error")
elseif(STEP STREQUAL "pipe")
    # Standard input is a file of bytes, as a pipe from another program is: one
    # line in the Windows-1252 that program writes (0xE9), one in UTF-8. Both
    # are names by the end, and neither ends the session or is refused.
    set(_input "${WORK}/ansi_input.txt")
    file(WRITE "${_input}"
        "LAYER NEW Pente${_ansi_e_acute}\nLAYER NEW Caf${_e_acute}\nLAYER LIST\nSAVE \"${_folder}/piped\"\nQUIT\n")
    run_program(0 _out "${CLI}")
    must_contain("${_out}" "Pente${_e_acute}")
    must_contain("${_out}" "Caf${_e_acute}")
    must_contain("${_out}" "saved to")
    must_not_contain("${_out}" "error")
    # ... and the layer table of the saved project is UTF-8 text, so a JSON
    # reply of it cannot throw.
    set(_input "")
    run_program(0 _out "${CLI}" -c "OPEN \"${_folder}/piped\"" -c "LAYER LIST")
    must_contain("${_out}" "Pente${_e_acute}")
elseif(STEP STREQUAL "mcp")
    # katana_mcp --project <that folder>, as an MCP client is configured, answers
    # initialize; and katana_run_script finds the script by its UTF-8 path.
    set(_input "${WORK}/mcp_input.txt")
    set(_initialize "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"check\",\"version\":\"1\"}}}")
    set(_run "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"katana_run_script\",\"arguments\":{\"path\":\"${_script}\"}}}")
    file(WRITE "${_input}" "${_initialize}\n${_run}\n")
    run_program(0 _out "${MCP}" --project "${_project}")
    must_contain("${_out}" "serving MCP on stdio")
    must_contain("${_out}" "\"protocolVersion\"")
    # The script (written by the "script" step) ran: its LIST said one entity
    # more than the project's, and nothing was refused.
    must_not_contain("${_out}" "\"isError\":true")
    must_contain("${_out}" "entities")
else()
    message(FATAL_ERROR "STEP is save, open, script, pipe or mcp, not '${STEP}'")
endif()
