# The headless front ends' shared session and the MCP server over it
# (src/katana_app, docs/mcp.md). The command line's verbs themselves are pinned
# end to end by the cli.* tests in src/katana_app/CMakeLists.txt.
find_package(nlohmann_json CONFIG REQUIRED)
katana_add_test_suite(app
    LIBS katana_app nlohmann_json::nlohmann_json
    SOURCES
        test_mcp_server.cpp
        test_session.cpp
)
# The sample GIS data an import is tried on; and whether the session has the
# GIS module at all, as katana_app decides it (src/katana_app/CMakeLists.txt),
# since without it IMPORT reads only a DXF.
target_compile_definitions(katana_app_tests PRIVATE
    KATANA_GIS_SAMPLES="${PROJECT_SOURCE_DIR}/samples/gis")
if(TARGET katana_interop)
    target_compile_definitions(katana_app_tests PRIVATE KATANA_TEST_WITH_INTEROP=1)
endif()
