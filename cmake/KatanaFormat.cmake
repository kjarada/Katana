# `format`       rewrites first-party sources with clang-format.
# `format-check` fails if any file would change (for CI).

find_program(KATANA_CLANG_FORMAT_EXE NAMES clang-format)

file(GLOB_RECURSE _katana_format_sources CONFIGURE_DEPENDS
    "${PROJECT_SOURCE_DIR}/include/*.hpp"
    "${PROJECT_SOURCE_DIR}/src/*.hpp"
    "${PROJECT_SOURCE_DIR}/src/*.cpp"
    "${PROJECT_SOURCE_DIR}/tests/*.cpp"
    "${PROJECT_SOURCE_DIR}/tests/*.hpp"
    "${PROJECT_SOURCE_DIR}/benchmarks/*.cpp"
)

if(KATANA_CLANG_FORMAT_EXE)
    add_custom_target(format
        COMMAND ${KATANA_CLANG_FORMAT_EXE} -i ${_katana_format_sources}
        COMMENT "Formatting sources with clang-format"
        VERBATIM)
    add_custom_target(format-check
        COMMAND ${KATANA_CLANG_FORMAT_EXE} --dry-run --Werror ${_katana_format_sources}
        COMMENT "Checking formatting with clang-format"
        VERBATIM)
else()
    set(_msg "clang-format was not found; install it to use the format targets.")
    add_custom_target(format
        COMMAND ${CMAKE_COMMAND} -E echo "${_msg}"
        COMMAND ${CMAKE_COMMAND} -E false)
    add_custom_target(format-check
        COMMAND ${CMAKE_COMMAND} -E echo "${_msg}"
        COMMAND ${CMAKE_COMMAND} -E false)
endif()
