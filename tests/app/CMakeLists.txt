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
