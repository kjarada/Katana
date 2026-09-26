# The RASTER SLOPE and RASTER ASPECT verbs through katana_cli (docs/terrain.md,
# "Slope and aspect"). Included from src/katana_app/CMakeLists.txt when the
# interop module is built.
set(_slope_data "${PROJECT_SOURCE_DIR}/tests/geo/data")

# tests/geo/data/plane.asc rises 1 in 20 to the east: 5 % everywhere Horn's
# window lies on it, and 2.5 % at its corners (GDAL's edge rule), so with
# breaks at 2 and 10 all 40 x 30 of its 1 m cells are in the one class
# [2, 10): 1200 m2, one region, drawn on terrain/slope/2-10. Its aspect is
# a west face.
add_test(NAME cli.raster_slope_classes_of_a_plane_are_one_area
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "RASTER SLOPE FILE \"${_slope_data}/plane.asc\" classes=2,10"
            -c "RASTER ASPECT FILE \"${_slope_data}/plane.asc\""
            -c "LIST")
set_tests_properties(cli.raster_slope_classes_of_a_plane_are_one_area PROPERTIES
    PASS_REGULAR_EXPRESSION "slope unit=percent method=horn raster=40x30 cell=1.*output arg=areas kind=vector target=layer layer=terrain/slope created=1.*class name=0-2 from=0 to=2 unit=percent area=0\\.000 polygons=0.*class name=2-10 from=2 to=10 unit=percent area=1200\\.000 polygons=1.*aspect convention=azimuth.*name=plane-aspect.*Polyline +layer=terrain/slope/2-10 +vertices=4 +closed"
    FAIL_REGULAR_EXPRESSION "error")
