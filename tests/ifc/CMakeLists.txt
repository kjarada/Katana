# The native IFC module (src/katana_ifc). What the tests expect is worked out
# beside each one - from the schema, from AS 5488, from IfcOpenShell's
# encodings - never read back from what the writer happened to write. The
# files IfcOpenShell itself judges are the cli.ifc_* tests
# (src/katana_app/CMakeLists.txt, tools/check_ifc.py).
katana_add_test_suite(ifc
    LIBS katana_ifc
    SOURCES
        test_step.cpp
        test_classification.cpp
        test_export.cpp
        test_import.cpp
        test_front_end.cpp
)
target_compile_definitions(katana_ifc_tests PRIVATE
    KATANA_IFC_TEST_DATA="${CMAKE_CURRENT_SOURCE_DIR}/data"
    KATANA_SAMPLES="${PROJECT_SOURCE_DIR}/samples")
