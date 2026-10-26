cmake_minimum_required(VERSION 3.24)

if(NOT KATANA_ROOT OR NOT WORK)
    message(FATAL_ERROR "Pass -DKATANA_ROOT and -DWORK to check_windows_runtime.cmake")
endif()
file(MAKE_DIRECTORY "${WORK}")
set(ENV{PATH} "$ENV{SystemRoot}/System32;$ENV{SystemRoot}")
set(ENV{QT_QPA_PLATFORM} "offscreen")
foreach(_variable QT_PLUGIN_PATH QT_QPA_PLATFORM_PLUGIN_PATH QML2_IMPORT_PATH
                  GDAL_DATA GDAL_DRIVER_PATH PROJ_DATA PROJ_LIB)
    unset(ENV{${_variable}})
endforeach()

if(DEFINED CLI)
    execute_process(COMMAND "${CLI}" -c "RECT 0,0 10,10" -c "LIST"
        WORKING_DIRECTORY "${WORK}"
        RESULT_VARIABLE _result OUTPUT_VARIABLE _out ERROR_VARIABLE _err TIMEOUT 30)
    if(NOT "${_result}" STREQUAL "0" OR NOT _out MATCHES "area=100")
        message(FATAL_ERROR "The CLI failed without the toolchain on PATH (${_result}):\n${_out}\n${_err}")
    endif()
endif()

if(DEFINED MCP)
    file(WRITE "${WORK}/initialize.jsonl"
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"runtime-test\",\"version\":\"1\"}}}\n")
    execute_process(COMMAND "${MCP}" INPUT_FILE "${WORK}/initialize.jsonl"
        WORKING_DIRECTORY "${WORK}"
        RESULT_VARIABLE _result OUTPUT_VARIABLE _out ERROR_VARIABLE _err TIMEOUT 30)
    if(NOT "${_result}" STREQUAL "0" OR NOT _out MATCHES "\"protocolVersion\""
       OR NOT _out MATCHES "\"serverInfo\"")
        message(FATAL_ERROR "MCP failed without the toolchain on PATH (${_result}):\n${_out}\n${_err}")
    endif()
endif()

if(DEFINED APP)
    foreach(_plugin qwindows qoffscreen)
        get_filename_component(_bin "${APP}" DIRECTORY)
        if(NOT EXISTS "${_bin}/platforms/${_plugin}.dll")
            message(FATAL_ERROR "The desktop runtime is missing platforms/${_plugin}.dll")
        endif()
    endforeach()
    execute_process(COMMAND "${CMAKE_COMMAND}" "-DAPP=${APP}"
        "-DPROJECT=${KATANA_ROOT}/samples/site_plan" "-DOUTPUT=${WORK}/runtime.png"
        -P "${KATANA_ROOT}/tools/check_screenshot.cmake"
        RESULT_VARIABLE _result TIMEOUT 150)
    if(NOT "${_result}" STREQUAL "0")
        message(FATAL_ERROR "The desktop failed without the toolchain on PATH (${_result})")
    endif()
endif()
message(STATUS "Katana starts with only Windows on PATH")
