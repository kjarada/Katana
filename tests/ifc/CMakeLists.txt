# The native IFC module (src/katana_ifc). What the tests expect is worked out
# beside each one - from the schema, from AS 5488, from IfcOpenShell's
# encodings - never read back from what the writer happened to write. The
# files IfcOpenShell itself judges are the cli.ifc_* tests
# (src/katana_app/CMakeLists.txt, tools/check_ifc.py) and the road model's,
# below.
katana_add_test_suite(ifc
    LIBS katana_ifc
    SOURCES
        test_step.cpp
        test_classification.cpp
        test_export.cpp
        test_import.cpp
        test_front_end.cpp
        # The road model (src/katana_ifc/road.cpp), a synthetic one: the
        # corridor and road-object builders are not needed to judge it.
        test_road.cpp
)
target_compile_definitions(katana_ifc_tests PRIVATE
    KATANA_IFC_TEST_DATA="${CMAKE_CURRENT_SOURCE_DIR}/data"
    KATANA_SAMPLES="${PROJECT_SOURCE_DIR}/samples")

# The road model's file judged by IfcOpenShell (tools/check_ifc.py), beside the
# cli.ifc_* checks of the drawing's: IfcExportRoad.TheRoadModelIsWrittenForTheValidator
# writes it where KATANA_IFC_ROAD_FILE says, and the check reads it. Only where
# the Python that configured the build can import ifcopenshell - a check of
# the output, never a build requirement.
if(Python3_Interpreter_FOUND)
    execute_process(COMMAND "${KATANA_PYTHON}" -c "import ifcopenshell"
        RESULT_VARIABLE _ifc_road_ifcopenshell_missing OUTPUT_QUIET ERROR_QUIET)
    if(_ifc_road_ifcopenshell_missing EQUAL 0)
        set(_ifc_road_file "${CMAKE_CURRENT_BINARY_DIR}/ifc road model.ifc")
        add_test(NAME ifc.the_road_model_export_is_written
            COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                    "KATANA_IFC_ROAD_FILE=${_ifc_road_file}"
                    $<TARGET_FILE:katana_ifc_tests>
                    --gtest_filter=IfcExportRoad.TheRoadModelIsWrittenForTheValidator)
        set_tests_properties(ifc.the_road_model_export_is_written PROPERTIES
            FIXTURES_SETUP ifc_road_file)
        add_test(NAME ifc.the_road_model_export_is_valid_to_ifcopenshell
            COMMAND "${KATANA_PYTHON}" "${PROJECT_SOURCE_DIR}/tools/check_ifc.py"
                    "${_ifc_road_file}")
        set_tests_properties(ifc.the_road_model_export_is_valid_to_ifcopenshell PROPERTIES
            FIXTURES_REQUIRED ifc_road_file)
    endif()
endif()
