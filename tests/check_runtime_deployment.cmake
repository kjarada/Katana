cmake_minimum_required(VERSION 3.24)

foreach(_required KATANA_ROOT WORK COMPILER GENERATOR MAKE_PROGRAM CASE)
    if(NOT DEFINED ${_required} OR "${${_required}}" STREQUAL "")
        message(FATAL_ERROR "Pass -D${_required} to check_runtime_deployment.cmake")
    endif()
endforeach()
if(NOT CASE MATCHES "^(individual_targets|missing_dll|missing_qt)$")
    message(FATAL_ERROR "Unknown runtime deployment case: ${CASE}")
endif()
if(NOT DEFINED DEPLOY_CMAKE_DIR)
    set(DEPLOY_CMAKE_DIR "${KATANA_ROOT}/cmake")
endif()
set(_build "${WORK}/fixture")
file(REMOVE_RECURSE "${_build}")
set(_automatic OFF)
if(CASE STREQUAL "individual_targets")
    set(_automatic ON)
endif()

function(_run)
    execute_process(COMMAND ${ARGV} RESULT_VARIABLE _result TIMEOUT 180)
    if(NOT "${_result}" STREQUAL "0")
        message(FATAL_ERROR "Command failed (${_result}): ${ARGV}")
    endif()
endfunction()

function(_require_dll name)
    if(NOT EXISTS "${_build}/bin/katana_runtime_${name}.dll")
        message(FATAL_ERROR "The executable target did not deploy katana_runtime_${name}.dll")
    endif()
endfunction()

function(_launch program)
    execute_process(
        COMMAND "${CMAKE_COMMAND}" -E env "PATH=$ENV{SystemRoot}/System32;$ENV{SystemRoot}"
                "${_build}/bin/${program}.exe"
        RESULT_VARIABLE _result TIMEOUT 30)
    if(NOT "${_result}" STREQUAL "0")
        message(FATAL_ERROR "${program} failed without the toolchain on PATH (${_result})")
    endif()
endfunction()

_run("${CMAKE_COMMAND}" -S "${KATANA_ROOT}/tests/runtime_deployment" -B "${_build}"
     -G "${GENERATOR}" "-DCMAKE_CXX_COMPILER=${COMPILER}"
     "-DCMAKE_MAKE_PROGRAM=${MAKE_PROGRAM}" -DCMAKE_BUILD_TYPE=Release
     "-DDEPLOY_CMAKE_DIR=${DEPLOY_CMAKE_DIR}" "-DKATANA_DEPLOY_RUNTIME=${_automatic}")
_run("${CMAKE_COMMAND}" --build "${_build}" --config Release --parallel 2 --target katana_cli)

if(CASE STREQUAL "individual_targets")
    _require_dll(middle)
    _require_dll(leaf)
    _launch(katana_cli)
    if(EXISTS "${_build}/bin/katana_mcp.exe")
        message(FATAL_ERROR "Building one executable unexpectedly built the other")
    endif()
    if(EXISTS "${_build}/bin/katana_runtime_extra.dll")
        message(FATAL_ERROR "The unused import library was deployed before the relink case could exercise it")
    endif()

    file(WRITE "${_build}/cli.cpp"
        "extern \"C\" __declspec(dllimport) int middle();\n"
        "extern \"C\" __declspec(dllimport) int extra();\n"
        "int main() { return middle() == 42 && extra() == 7 ? 0 : 1; }\n")
    _run("${CMAKE_COMMAND}" --build "${_build}" --config Release --parallel 2 --target katana_cli)
    _require_dll(extra)
    _launch(katana_cli)

    _run("${CMAKE_COMMAND}" --build "${_build}" --config Release --parallel 2 --target katana_mcp)
    _require_dll(later)
    _launch(katana_mcp)
    file(STRINGS "${_build}/CMakeFiles/katana_runtime.stamp" _recorded)
    foreach(_program katana_cli.exe katana_mcp.exe)
        set(_matching "${_recorded}")
        list(FILTER _matching INCLUDE REGEX "^program=${_program}[|]")
        if(NOT _matching)
            message(FATAL_ERROR "The deployment stamp did not record ${_program}")
        endif()
    endforeach()

    file(REMOVE "${_build}/bin/katana_runtime_leaf.dll")
    _run("${CMAKE_COMMAND}" "-DKATANA_DEPLOY_PREFIX=${_build}"
         -P "${_build}/KatanaDeploy.cmake")
    _require_dll(leaf)
    _launch(katana_cli)

    file(REMOVE_RECURSE "${_build}/bin")
    file(REMOVE "${_build}/CMakeFiles/katana_runtime.stamp")
    _run("${CMAKE_COMMAND}" --build "${_build}" --config Release --parallel 2
         --clean-first --target katana_cli katana_mcp)
    _require_dll(middle)
    _require_dll(leaf)
    _require_dll(extra)
    _require_dll(later)
    _launch(katana_cli)
    _launch(katana_mcp)
else()
    if(CASE STREQUAL "missing_dll")
        file(REMOVE "${_build}/toolchain/bin/katana_runtime_leaf.dll")
        set(_program katana_cli)
        set(_expected "katana_runtime_leaf[.]dll")
    else()
        file(COPY_FILE "${_build}/bin/katana_cli.exe" "${_build}/bin/katana.exe")
        set(_program katana)
        set(_expected "windeployqt")
    endif()
    execute_process(
        COMMAND "${CMAKE_COMMAND}" "-DKATANA_DEPLOY_PREFIX=${_build}"
                "-DKATANA_DEPLOY_EXECUTABLE=${_build}/bin/${_program}.exe"
                -P "${_build}/KatanaDeploy.cmake"
        RESULT_VARIABLE _result OUTPUT_VARIABLE _out ERROR_VARIABLE _err TIMEOUT 120)
    if("${_result}" STREQUAL "0" OR NOT "${_out}\n${_err}" MATCHES "${_expected}")
        message(FATAL_ERROR "An incomplete runtime must fail and name ${_expected}; "
                            "got ${_result}:\n${_out}\n${_err}")
    endif()
    if(EXISTS "${_build}/CMakeFiles/katana_runtime.stamp")
        message(FATAL_ERROR "A failed deployment left a success stamp")
    endif()
endif()
message(STATUS "Windows runtime deployment: ${CASE} passed")
