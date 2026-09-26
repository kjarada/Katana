# The geoprocessing bridge, its bindings and the one executor
# (docs/geoprocessing.md): suite katana_geo_tests. It links katana_app, which
# brings every layer below it, so a lane's tests of its own verbs go here
# beside the foundation's. Globbed, one file per package, so the lanes add
# tests without editing this list.
find_package(nlohmann_json CONFIG REQUIRED)
file(GLOB _geo_test_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/test_*.cpp")
katana_add_test_suite(geo
    LIBS katana_app katana_interop katana_io katana_cad nlohmann_json::nlohmann_json
    SOURCES ${_geo_test_sources}
)
# The fixtures of tests/geo/data and the sample data; both are only ever
# read - a test that needs a file to change copies it first.
target_compile_definitions(katana_geo_tests PRIVATE
    KATANA_GEO_TEST_DATA="${CMAKE_CURRENT_SOURCE_DIR}/data"
    KATANA_GIS_SAMPLES="${PROJECT_SOURCE_DIR}/samples/gis")
