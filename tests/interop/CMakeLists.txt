katana_add_test_suite(interop
    LIBS katana_interop katana_commands
    SOURCES
        test_interop.cpp
        test_archive12d.cpp
        test_round_trip.cpp
)
# The 12d archive fixtures live with the format's own tests; the container and
# the file handling tested here read the same ones.
target_compile_definitions(katana_interop_tests PRIVATE
    KATANA_ARCHIVE12D_TEST_DATA="${PROJECT_SOURCE_DIR}/tests/archive12d/data")
