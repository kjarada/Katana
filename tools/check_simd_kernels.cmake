# The guard on SIMD kernel files (cmake/KatanaSimd.cmake says why they exist).
#
#   cmake -DKATANA_ROOT=<source dir> -P tools/check_simd_kernels.cmake
#       Sources. Every src/**/*_avx2.cpp includes only <immintrin.h>,
#       <cstddef>, <cstdint> and a *_kernels.hpp beside it; that header includes
#       only <cstddef>/<cstdint> and holds declarations only (no braces but
#       extern "C" and namespace blocks), so nothing from it can be emitted as
#       AVX2 code. And no build file turns on OpenMP threading, nor does any
#       source use an OpenMP pragma other than `omp simd`: core::TaskPool is the
#       one thread pool, because OpenMP reductions run in no fixed order.
#
#   cmake -DOBJECTS=<a|b|...> -DNM=<nm> -DOBJDUMP=<objdump> -P tools/check_simd_kernels.cmake
#       Objects: what the compiler really produced for each kernel file.
#       - Every code symbol visible to the linker is a katana_avx2_ entry. Any
#         other is an out-of-line copy of some inline function (a COMDAT), and
#         the linker may give THAT copy to baseline code: the inline-copy hazard.
#       - No static-initialiser section (.ctors / .init_array): an initialiser
#         in a kernel file runs at program start on every machine, AVX2 or not.
#       - No fused multiply-add instruction: the kernels promise results equal
#         to their scalar references, which multiply and then add.
#       Each object must also define at least one entry, so an empty or
#       mis-built object cannot pass by having nothing in it.

set(_violations "")

if(DEFINED OBJECTS)
    if(NOT NM OR NOT OBJDUMP)
        message(FATAL_ERROR "Pass -DNM=<nm> and -DOBJDUMP=<objdump> with -DOBJECTS")
    endif()
    string(REPLACE "|" ";" _objects "${OBJECTS}")
    if(NOT _objects)
        message(FATAL_ERROR "OBJECTS is empty: no kernel object to check")
    endif()
    foreach(_object IN LISTS _objects)
        get_filename_component(_name "${_object}" NAME)
        if(NOT EXISTS "${_object}")
            list(APPEND _violations "${_name}: object not built")
            continue()
        endif()

        execute_process(COMMAND "${NM}" --defined-only "${_object}"
            OUTPUT_VARIABLE _symbols RESULT_VARIABLE _nm_result ERROR_VARIABLE _nm_error)
        if(NOT _nm_result EQUAL 0)
            list(APPEND _violations "${_name}: nm failed: ${_nm_error}")
            continue()
        endif()
        string(REPLACE "\n" ";" _lines "${_symbols}")
        set(_entries 0)
        foreach(_line IN LISTS _lines)
            # "<address> <type> <name>". Upper-case T or W is code the linker
            # can see; lower-case t is local and cannot be shared.
            if(_line MATCHES "^[0-9a-fA-F]+ ([TW]) (.+)$")
                set(_symbol "${CMAKE_MATCH_2}")
                if(_symbol MATCHES "^katana_avx2_[a-z0-9_]+$")
                    math(EXPR _entries "${_entries} + 1")
                else()
                    list(APPEND _violations
                        "${_name}: defines '${_symbol}' with external linkage - only katana_avx2_ entries may be visible (inline-copy hazard)")
                endif()
            endif()
        endforeach()
        if(_entries EQUAL 0)
            list(APPEND _violations "${_name}: defines no katana_avx2_ entry")
        endif()

        execute_process(COMMAND "${OBJDUMP}" -h "${_object}"
            OUTPUT_VARIABLE _sections RESULT_VARIABLE _h_result)
        if(NOT _h_result EQUAL 0)
            list(APPEND _violations "${_name}: objdump -h failed")
        elseif(_sections MATCHES "[ \t](\\.ctors|\\.init_array)")
            list(APPEND _violations
                "${_name}: has a static initialiser (${CMAKE_MATCH_1}) - it would run AVX2 code at start-up on every machine")
        endif()

        execute_process(COMMAND "${OBJDUMP}" -d --no-show-raw-insn "${_object}"
            OUTPUT_VARIABLE _code RESULT_VARIABLE _d_result)
        if(NOT _d_result EQUAL 0)
            list(APPEND _violations "${_name}: objdump -d failed")
        elseif(_code MATCHES "[ \t](vf(n)?m(add|sub)[0-9a-z]*)")
            list(APPEND _violations
                "${_name}: contains ${CMAKE_MATCH_1} - a fused multiply-add rounds once where the scalar reference rounds twice")
        endif()
    endforeach()
    if(_violations)
        list(JOIN _violations "\n  " _text)
        message(FATAL_ERROR "SIMD kernel object violations:\n  ${_text}")
    endif()
    list(LENGTH _objects _count)
    message(STATUS "SIMD kernel objects passed: ${_count} checked.")
    return()
endif()

if(NOT KATANA_ROOT)
    message(FATAL_ERROR "Pass -DKATANA_ROOT=<source dir>, or -DOBJECTS=... to check objects")
endif()

set(_allowed_system "immintrin.h;cstddef;cstdint")

file(GLOB_RECURSE _kernels "${KATANA_ROOT}/src/*_avx2.cpp")
foreach(_kernel IN LISTS _kernels)
    file(RELATIVE_PATH _rel "${KATANA_ROOT}" "${_kernel}")
    get_filename_component(_dir "${_kernel}" DIRECTORY)
    file(STRINGS "${_kernel}" _includes REGEX "^[ \t]*#[ \t]*include")
    foreach(_line IN LISTS _includes)
        if(_line MATCHES "#[ \t]*include[ \t]*<([^>]+)>")
            list(FIND _allowed_system "${CMAKE_MATCH_1}" _index)
            if(_index EQUAL -1)
                list(APPEND _violations "${_rel}: includes <${CMAKE_MATCH_1}> - a kernel file may include only <immintrin.h>, <cstddef> and <cstdint>")
            endif()
        elseif(_line MATCHES "#[ \t]*include[ \t]*\"([A-Za-z0-9_]+_kernels\\.hpp)\"")
            set(_header "${_dir}/${CMAKE_MATCH_1}")
            if(NOT EXISTS "${_header}")
                list(APPEND _violations "${_rel}: includes \"${CMAKE_MATCH_1}\", which is not beside it")
                continue()
            endif()
            # The declarations header: its own includes, and no definitions.
            file(RELATIVE_PATH _hrel "${KATANA_ROOT}" "${_header}")
            file(STRINGS "${_header}" _header_lines)
            foreach(_hline IN LISTS _header_lines)
                string(REGEX REPLACE "//.*$" "" _code "${_hline}")
                if(_code MATCHES "#[ \t]*include[ \t]*[<\"]([^>\"]+)[>\"]")
                    if(NOT CMAKE_MATCH_1 STREQUAL "cstddef" AND NOT CMAKE_MATCH_1 STREQUAL "cstdint")
                        list(APPEND _violations "${_hrel}: includes ${CMAKE_MATCH_1} - a kernel declarations header may include only <cstddef> and <cstdint>")
                    endif()
                endif()
                if(_code MATCHES "{" AND NOT _code MATCHES "^[ \t]*(extern \"C\"|namespace[ \t]+[A-Za-z0-9_:]+)[ \t]*{[ \t]*$")
                    list(APPEND _violations "${_hrel}: '${_hline}' - a kernel declarations header holds declarations only")
                endif()
            endforeach()
        else()
            list(APPEND _violations "${_rel}: '${_line}' - a kernel file may include only <immintrin.h>, <cstddef>, <cstdint> and its *_kernels.hpp")
        endif()
    endforeach()
endforeach()

# OpenMP: -fopenmp-simd (a vectorisation hint with no runtime) is allowed;
# anything that starts OpenMP threads is not.
file(GLOB_RECURSE _build_files "${KATANA_ROOT}/CMakeLists.txt" "${KATANA_ROOT}/cmake/*.cmake")
list(FILTER _build_files EXCLUDE REGEX "/(build|third_party)/")
foreach(_file IN LISTS _build_files)
    file(RELATIVE_PATH _rel "${KATANA_ROOT}" "${_file}")
    file(STRINGS "${_file}" _lines REGEX "openmp|OpenMP")
    foreach(_line IN LISTS _lines)
        if(_line MATCHES "^[ \t]*#")
            continue()
        endif()
        if(_line MATCHES "-fopenmp($|[^-])|OpenMP::|find_package\\([ \t]*OpenMP")
            list(APPEND _violations "${_rel}: '${_line}' - no OpenMP threading, core::TaskPool is the one pool")
        endif()
    endforeach()
endforeach()
file(GLOB_RECURSE _code_files "${KATANA_ROOT}/src/*.cpp" "${KATANA_ROOT}/src/*.hpp" "${KATANA_ROOT}/include/*.hpp")
foreach(_file IN LISTS _code_files)
    file(STRINGS "${_file}" _lines REGEX "#[ \t]*pragma[ \t]+omp")
    foreach(_line IN LISTS _lines)
        if(NOT _line MATCHES "#[ \t]*pragma[ \t]+omp[ \t]+simd")
            file(RELATIVE_PATH _rel "${KATANA_ROOT}" "${_file}")
            list(APPEND _violations "${_rel}: '${_line}' - only '#pragma omp simd' is allowed")
        endif()
    endforeach()
endforeach()

if(_violations)
    list(JOIN _violations "\n  " _text)
    message(FATAL_ERROR "SIMD kernel source violations:\n  ${_text}")
endif()
list(LENGTH _kernels _count)
message(STATUS "SIMD kernel sources passed: ${_count} kernel files.")
