cmake_minimum_required(VERSION 3.24)
# An archive's AutoCAD Colour Index names draw in the index's colour, and the
# one name nothing defines is said once, in one line (docs/interop.md,
# "Colour names"). indexed_colours.12da is invented: of its five strings,
# "ACAD 5" is blue (index 5) and "acad 010" red (index 10, the first hue at
# full brightness), by the published index; the other three have no colour of
# their own ("vis something" twice, "acad 000" ByBlock).
foreach(_required PROGRAM FIXTURES)
    if(NOT DEFINED ${_required})
        message(FATAL_ERROR "Pass -D${_required}")
    endif()
endforeach()
execute_process(COMMAND "${PROGRAM}"
    -c "IMPORT \"${FIXTURES}/indexed_colours.12da\" LOCAL"
    -c "MODIFY DRAWING WHERE COLOUR=#0000FF SET COLOUR=#0000FF PREVIEW"
    -c "MODIFY DRAWING WHERE COLOUR=#FF0000 SET COLOUR=#FF0000 PREVIEW"
    RESULT_VARIABLE result OUTPUT_VARIABLE output ERROR_VARIABLE error TIMEOUT 60)
if(NOT "${result}" STREQUAL "0")
    message(FATAL_ERROR "Archive import failed (${result}):\n${output}\n${error}")
endif()
if(NOT output MATCHES "entities=5")
    message(FATAL_ERROR "Archive import did not report entities=5:\n${output}")
endif()
# One entity is blue and one red: the two MODIFY previews match one each.
string(REGEX MATCHALL "preview: 1 entity matched" _previews "${output}")
list(LENGTH _previews _preview_count)
if(NOT _preview_count EQUAL 2)
    message(FATAL_ERROR "Expected one blue and one red entity:\n${output}")
endif()
# The colour line: once, naming the unknown name once and no index name.
# Counted by its opening words only: the line itself holds a ';', which a
# CMake list would split on.
string(REGEX MATCHALL "[0-9]+ colour names? (is|are) not a standard name" _heads "${output}")
list(LENGTH _heads _line_count)
if(NOT _line_count EQUAL 1)
    message(FATAL_ERROR "Expected one line for the colour names:\n${output}")
endif()
string(REGEX MATCH "[0-9]+ colour names? (is|are) not a standard name[^\n]*" _lines "${output}")
# The reply is key=value records, so the line's quotes arrive escaped (\"):
# the names are matched without them.
foreach(expected "^1 colour name is not" "vis something" "customisation's [^ ]*colours")
    if(NOT _lines MATCHES "${expected}")
        message(FATAL_ERROR "The colour line does not say ${expected}:\n${_lines}")
    endif()
endforeach()
string(REGEX MATCHALL "vis something" _named "${_lines}")
list(LENGTH _named _named_count)
if(NOT _named_count EQUAL 1
        OR _lines MATCHES "acad 001|acad 005|acad 010|acad 000|ACAD 5|Vis_Something")
    message(FATAL_ERROR "The colour line names an index name, or one name twice:\n${_lines}")
endif()
message(STATUS "Index colour names reached the CLI, and the unknown name was said once")
