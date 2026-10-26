cmake_minimum_required(VERSION 3.24)
foreach(_required PROGRAM FIXTURES)
    if(NOT DEFINED ${_required})
        message(FATAL_ERROR "Pass -D${_required}")
    endif()
endforeach()
execute_process(COMMAND "${PROGRAM}"
    -c "CUSTOMISE \"${FIXTURES}/drafting_palette.customisation.json\""
    -c "IMPORT \"${FIXTURES}/drafting_leaders.12da\" LOCAL"
    -c "MODIFY DRAWING WHERE COLOUR=#E08020 SET COLOUR=#E08020 PREVIEW"
    -c "LEADER LIST DRAWING"
    RESULT_VARIABLE result OUTPUT_VARIABLE output ERROR_VARIABLE error TIMEOUT 60)
if(NOT "${result}" STREQUAL "0")
    message(FATAL_ERROR "Archive import failed (${result}):\n${output}\n${error}")
endif()
foreach(expected "entities=4" "read=2 imported=2" "preview: 1 entity matched" "scope=drawing matched=2")
    if(NOT output MATCHES "${expected}")
        message(FATAL_ERROR "Archive import did not report ${expected}:\n${output}")
    endif()
endforeach()
if(output MATCHES "not read: drafting_element|DIFFERENT symbol")
    message(FATAL_ERROR "Archive import still drops leaders or misreports an empty symbol:\n${output}")
endif()
message(STATUS "Archive palette colours and both leaders reached the CLI")
