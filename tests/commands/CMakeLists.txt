# Tests written alongside the customisation managers, globbed so that several
# authors can add a file at once without editing this list.
file(GLOB _commands_customisation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/customisation/*.cpp")

katana_add_test_suite(commands
    LIBS katana_commands
    SOURCES test_commands.cpp test_annotation_tables.cpp
        ${_commands_customisation_tests}
)
