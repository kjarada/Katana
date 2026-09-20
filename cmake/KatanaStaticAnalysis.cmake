# Optional static analysis run as part of compilation.
# Both tools are opt-in and fail the configure step if requested but missing,
# so a "static analysis" build can never silently analyse nothing.

if(KATANA_ENABLE_CLANG_TIDY)
    find_program(KATANA_CLANG_TIDY_EXE NAMES clang-tidy REQUIRED)
    message(STATUS "Katana static analysis: clang-tidy (${KATANA_CLANG_TIDY_EXE})")
endif()

if(KATANA_ENABLE_CPPCHECK)
    find_program(KATANA_CPPCHECK_EXE NAMES cppcheck REQUIRED)
    message(STATUS "Katana static analysis: cppcheck (${KATANA_CPPCHECK_EXE})")
endif()

function(katana_apply_static_analysis target)
    if(KATANA_ENABLE_CLANG_TIDY)
        set_target_properties(${target} PROPERTIES
            CXX_CLANG_TIDY "${KATANA_CLANG_TIDY_EXE};--warnings-as-errors=*")
    endif()
    if(KATANA_ENABLE_CPPCHECK)
        set_target_properties(${target} PROPERTIES
            CXX_CPPCHECK "${KATANA_CPPCHECK_EXE};--enable=warning,performance,portability;--inline-suppr;--error-exitcode=1;--std=c++20")
    endif()
endfunction()
