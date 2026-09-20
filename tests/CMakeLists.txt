# One test executable per module keeps link dependencies honest: the math tests
# cannot accidentally rely on SQLite, the geometry tests cannot rely on Qt.
#
#   katana_add_test_suite(<module> LIBS <targets...> SOURCES <files...>)

function(katana_add_test_suite module)
    cmake_parse_arguments(ARG "" "" "LIBS;SOURCES" ${ARGN})
    set(_target katana_${module}_tests)
    add_executable(${_target} ${ARG_SOURCES})
    target_link_libraries(${_target} PRIVATE ${ARG_LIBS} GTest::gtest_main)
    target_include_directories(${_target} PRIVATE "${CMAKE_CURRENT_FUNCTION_LIST_DIR}")
    katana_target_defaults(${_target})
    katana_register_tests(${_target})
endfunction()

add_test(NAME layering
    COMMAND ${CMAKE_COMMAND} -DKATANA_ROOT=${PROJECT_SOURCE_DIR}
            -P ${PROJECT_SOURCE_DIR}/tools/check_layering.cmake)

# tests/<name>/ holds the suite of module katana_<name>; it is configured only
# when that module is.
file(GLOB _suites LIST_DIRECTORIES true "${CMAKE_CURRENT_SOURCE_DIR}/*")
foreach(_suite IN LISTS _suites)
    get_filename_component(_name "${_suite}" NAME)
    if(EXISTS "${_suite}/CMakeLists.txt" AND TARGET katana_${_name})
        add_subdirectory("${_suite}")
    endif()
endforeach()
