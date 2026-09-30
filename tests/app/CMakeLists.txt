# The headless front ends' shared session and the MCP server over it
# (src/katana_app, docs/mcp.md). The command line's verbs themselves are pinned
# end to end by the cli.* tests in src/katana_app/CMakeLists.txt.
find_package(nlohmann_json CONFIG REQUIRED)
katana_add_test_suite(app
    LIBS katana_app nlohmann_json::nlohmann_json
    SOURCES
        test_mcp_server.cpp
        test_session.cpp
        # SURVEY IMPORT's SETTINGS and SET, read on the document they leave.
        test_survey_verbs.cpp
)
# The sample GIS data an import is tried on; the survey fixtures, the
# import wizard's network among them; and whether the session has the GIS
# module at all, as katana_app decides it (src/katana_app/CMakeLists.txt),
# since without it IMPORT reads only a DXF.
target_compile_definitions(katana_app_tests PRIVATE
    KATANA_GIS_SAMPLES="${PROJECT_SOURCE_DIR}/samples/gis"
    KATANA_SURVEYIO_DATA="${PROJECT_SOURCE_DIR}/tests/surveyio/data"
    KATANA_SURVEY_UI_DATA="${PROJECT_SOURCE_DIR}/tests/qt_widgets/survey/data")
if(TARGET katana_interop)
    target_compile_definitions(katana_app_tests PRIVATE KATANA_TEST_WITH_INTEROP=1)
endif()
