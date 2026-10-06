# Runs katana_customisation_convert as a person would and checks what a caller
# of the program relies on: the exit code, the file, the report on standard
# output, the reason on standard error.
#
#   cmake -DCASE=<fixture|options|usage|unicode> -DPROGRAM=<the converter>
#         -DFIXTURE=<test_symbols.4d> -DTWIN=<test_symbols.customisation.json>
#         -DSCRATCH=<a directory of this case's own>
#         -P check_convert_program.cmake
#
# What the converter DOES is tested through its functions
# (customisation/test_convert.cpp). This is only the program around them, and
# it is the one place that part is tested at all: that every option of the
# command line reaches the function it is for - the functions' tests pass with
# an option wired to the wrong member, or to none - and that the results reach
# the caller. A case a test, each in a directory of its own:
#
#   fixture  a fixture converted to its hand-written twin; a missing --name
#   options  every option given, each seen in the report and in the file
#   usage    an unknown option, an option without its value, --help
#   unicode  a name, a word, a file and an output outside ASCII

foreach(_needed CASE PROGRAM FIXTURE TWIN SCRATCH)
    if(NOT DEFINED ${_needed})
        message(FATAL_ERROR "check_convert_program.cmake needs -D${_needed}=...")
    endif()
endforeach()

file(REMOVE_RECURSE "${SCRATCH}")
file(MAKE_DIRECTORY "${SCRATCH}")

# Runs the program with the arguments given; leaves _code, _out and _err.
# The program writes UTF-8, and is read as that whatever the console is set to.
macro(run_converter)
    execute_process(
        COMMAND "${PROGRAM}" ${ARGN}
        RESULT_VARIABLE _code OUTPUT_VARIABLE _out ERROR_VARIABLE _err
        ENCODING UTF8)
endmacro()

# A whole line of the report on standard output.
function(expect_line _line)
    string(REPLACE "\r\n" "\n" _lines "\n${_out}")
    string(FIND "${_lines}" "\n${_line}\n" _at)
    if(_at EQUAL -1)
        message(FATAL_ERROR "the report has no line ${_line}:\n${_out}\n${_err}")
    endif()
endfunction()

function(expect_in _text _piece _what)
    string(FIND "${_text}" "${_piece}" _at)
    if(_at EQUAL -1)
        message(FATAL_ERROR "${_what} does not hold ${_piece}:\n${_text}")
    endif()
endfunction()

# A failure as a caller sees one: exit code 1, the reason on standard error,
# and no report.
function(expect_failure _reason _what)
    if(NOT _code EQUAL 1)
        message(FATAL_ERROR "${_what}: the program exited with ${_code}, not 1:\n${_err}")
    endif()
    expect_in("${_err}" "${_reason}" "${_what}: standard error")
    if(NOT "${_out}" STREQUAL "")
        message(FATAL_ERROR "${_what}: a conversion that failed still printed a report:\n${_out}")
    endif()
endfunction()

if(CASE STREQUAL "fixture")
    # ---- a conversion ----------------------------------------------------------------------------
    set(_output "${SCRATCH}/made/here/test_symbols.customisation.json")
    run_converter(--name test_symbols -o "${_output}" "${FIXTURE}")
    if(NOT _code EQUAL 0)
        message(FATAL_ERROR "the conversion of the fixture exited with ${_code}:\n${_err}")
    endif()
    # Counted by hand from the fixture, whose head comment carries the same
    # counts: one file of four definitions, every one a symbol because the
    # file's name says so, three of them `mode vertex`, and no rules.
    foreach(_line "name=\"test_symbols\"" "files=1" "definitions=4" "symbols=4" "linestyles=0"
                  "at_vertices=3" "definitions_replaced=0" "rules=0" "keys=0" "warnings=0")
        expect_line("${_line}")
    endforeach()
    if(NOT EXISTS "${_output}")
        message(FATAL_ERROR "the conversion exited with 0 and wrote no ${_output}")
    endif()
    # The file is the hand-written twin. Line ends apart: a checkout on Windows
    # may give the twin CRLF (core.autocrlf), and the converter's own are
    # checked next.
    execute_process(
        COMMAND "${CMAKE_COMMAND}" -E compare_files --ignore-eol "${_output}" "${TWIN}"
        RESULT_VARIABLE _differs)
    if(NOT _differs EQUAL 0)
        message(FATAL_ERROR "${_output} is not ${TWIN}")
    endif()
    # Its lines end in a line feed alone, on every platform: no byte of it is a
    # carriage return (0d), looked for at a byte boundary of the hexadecimal text.
    file(READ "${_output}" _hex HEX)
    if(_hex MATCHES "^(..)*0d")
        message(FATAL_ERROR "${_output} holds a carriage return; its lines must end in a line feed alone")
    endif()
    # Written beside itself first and then put in its place: nothing is left there.
    if(EXISTS "${_output}.partial")
        message(FATAL_ERROR "the conversion left ${_output}.partial behind")
    endif()

    # ---- a failure -------------------------------------------------------------------------------
    file(REMOVE "${_output}")
    run_converter(-o "${_output}" "${FIXTURE}")
    expect_failure("the customisation has no name: give one with --name <name>" "without --name")
    if(EXISTS "${_output}")
        message(FATAL_ERROR "a conversion that failed still wrote ${_output}")
    endif()

elseif(CASE STREQUAL "options")
    # Small inputs written here, and what each option makes of them worked out
    # beside it. The word stripped (ACME) and the word removed (VND) differ, so
    # that neither option can pass for the other.
    file(WRITE "${SCRATCH}/lines.4d"
        "// Site lines\nworldstyle \"ACME Kerb\" { group \"ACME Roads\" move 0 0 draw 1 0 }\n")
    file(WRITE "${SCRATCH}/more.4d"
        "// More lines\nworldstyle \"OLD Wall\" { move 0 0 draw 2 0 }\n")
    file(WRITE "${SCRATCH}/codes.mapfile"
        "<map_file><map_data><item><key>KB*</key><colour>site blue</colour>"
        "<linestyle>ACME Kerb</linestyle><comment>Kerb to VND standard, REV 2</comment>"
        "<group>ACME ROADS</group></item></map_data></map_file>\n")
    file(WRITE "${SCRATCH}/colours.4d" "0 112 255 21 \"site_blue\"\n")
    set(_output "${SCRATCH}/site.customisation.json")
    run_converter(
        --name Site
        --description "The site's own codes"
        --notice-from "${SCRATCH}/lines.4d" --notice-from "${SCRATCH}/more.4d"
        --colours "${SCRATCH}/colours.4d"
        --strip-leading-word ACME --strip-leading-word OLD
        --remove-word VND --remove-word REV
        -o "${_output}"
        "${SCRATCH}/lines.4d" "${SCRATCH}/codes.mapfile" "${SCRATCH}/more.4d")
    if(NOT _code EQUAL 0)
        message(FATAL_ERROR "the conversion with every option exited with ${_code}:\n${_err}")
    endif()
    foreach(_line
            "name=\"Site\""              # --name
            "files=3"                    # the three files
            "definitions=2" "symbols=0" "rules=1"
            "colours_resolved=1"         # --colours: site_blue is the rule's site blue
            "colour_resolved=\"site blue\""
            "colours_unresolved=0"
            "names_renamed=2"            # --strip-leading-word, both: ACME Kerb, OLD Wall
            "groups_renamed=1"           # ACME Roads
            "references_renamed=1"       # the rule's ACME Kerb
            "rule_groups_renamed=1"      # ACME ROADS
            "comments_changed=1"         # --remove-word
            "unresolved_references=0"    # the rule still names the definition it named
            "notice_lines=2"             # --notice-from, both
            "warnings=0")
        expect_line("${_line}")
    endforeach()
    file(READ "${_output}" _text)
    foreach(_piece
            "\"name\": \"Site\""
            "\"description\": \"The site's own codes\""
            "\"Site lines\"" "\"More lines\""
            "\"site blue\": \"#0070FF\""            # 0, 112, 255
            "{\"name\": \"Kerb\", \"group\": \"Roads\""
            "{\"name\": \"Wall\""
            "\"linestyle\": \"Kerb\""
            "\"group\": \"ROADS\""
            "\"comment\": \"Kerb to standard, 2\"")  # both words gone, and nothing else
        expect_in("${_text}" "${_piece}" "${_output}")
    endforeach()

    # Without -o the same conversion prints the same report and writes nothing.
    file(REMOVE "${_output}")
    run_converter(--name Site "${SCRATCH}/lines.4d" "${SCRATCH}/codes.mapfile")
    if(NOT _code EQUAL 0)
        message(FATAL_ERROR "the conversion without -o exited with ${_code}:\n${_err}")
    endif()
    expect_line("definitions=1")
    expect_line("names_renamed=0")
    if(_out MATCHES "(^|\n)output=")
        message(FATAL_ERROR "without -o the report still names an output:\n${_out}")
    endif()
    if(EXISTS "${_output}")
        message(FATAL_ERROR "without -o the program still wrote ${_output}")
    endif()

    # After `--` a name that begins with a dash is a file, not an option.
    run_converter(--name Site -- "-not-an-option.4d")
    expect_failure("-not-an-option.4d" "a file after --")
    expect_failure("cannot open this file" "a file after --")

elseif(CASE STREQUAL "usage")
    run_converter(--nam Site "${FIXTURE}")
    expect_failure("--nam is not an option" "an unknown option")
    expect_failure("usage: katana_customisation_convert" "an unknown option")

    run_converter("${FIXTURE}" --name)
    expect_failure("--name is not followed by its value" "--name without its value")

    run_converter(--name Site "${FIXTURE}" -o)
    expect_failure("-o is not followed by its value" "-o without its value")

    # No file at all, with a name: the conversion's own refusal, passed on.
    run_converter(--name Site)
    expect_failure("there is no file to convert" "no file")

    run_converter(--help)
    if(NOT _code EQUAL 0)
        message(FATAL_ERROR "--help exited with ${_code}:\n${_err}")
    endif()
    expect_in("${_out}" "usage: katana_customisation_convert --name <name>" "the help")
    expect_in("${_out}" "--strip-leading-word" "the help")

elseif(CASE STREQUAL "unicode")
    # A small o with a stroke (U+00F8) and the capital (U+00D8), by their UTF-8
    # bytes so that this script stays ASCII. On Windows the program is handed
    # them as UTF-16 and must not read its arguments in the ANSI code page: a
    # name so read is refused as "not UTF-8", a word so read strips nothing,
    # and a path so read is not found.
    string(ASCII 195 184 _o)
    string(ASCII 195 152 _O)
    set(_library "${SCRATCH}/linjer_${_o}.4d")
    set(_output "${SCRATCH}/ut_${_o}.json")
    file(WRITE "${_library}" "// Kerb ${_O}100\nworldstyle \"${_O} Kerb\" { move 0 0 draw 1 0 }\n")
    run_converter(
        --name "S${_o}ndre"
        --description "Kerb ${_O}100"
        --notice-from "${_library}"
        --strip-leading-word "${_O}"
        -o "${_output}"
        "${_library}")
    if(NOT _code EQUAL 0)
        message(FATAL_ERROR "the conversion with names outside ASCII exited with ${_code}:\n${_err}")
    endif()
    expect_line("name=\"S${_o}ndre\"")
    expect_line("definitions=1")
    expect_line("names_renamed=1")   # the word is the capital, and the name began with it
    expect_line("notice_lines=1")
    expect_line("warnings=0")        # the library is UTF-8: nothing is inferred
    if(NOT EXISTS "${_output}")
        message(FATAL_ERROR "the conversion exited with 0 and wrote no ${_output}")
    endif()
    file(READ "${_output}" _text)
    expect_in("${_text}" "\"name\": \"S${_o}ndre\"" "${_output}")
    expect_in("${_text}" "\"description\": \"Kerb ${_O}100\"" "${_output}")
    expect_in("${_text}" "\"Kerb ${_O}100\"" "${_output}")
    expect_in("${_text}" "{\"name\": \"Kerb\"" "${_output}")

    # A file that is not there is named as it was typed, and is exit code 1 -
    # never an exception out of main.
    run_converter(--name Site "${SCRATCH}/ikke_${_o}.4d")
    expect_failure("ikke_${_o}.4d" "a missing file outside ASCII")

else()
    message(FATAL_ERROR "check_convert_program.cmake: CASE=${CASE} is not fixture, options, usage or unicode")
endif()
