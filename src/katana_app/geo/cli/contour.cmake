# The CONTOUR verb through katana_cli (docs/terrain.md, "Contours"). Included
# from src/katana_app/CMakeLists.txt when the interop module is built.
set(_contour_samples "${PROJECT_SOURCE_DIR}/samples/gis")

# samples/gis/terrain.asc holds heights from 24.892 to 38.819 (its least and
# greatest values, read from the text grid) on 1.5 m cells, so the whole
# metres inside it are 25 to 38: 14 levels, whichever engine traces them -
# GDAL on the raster, the tracer on the surface made from it, whose vertices
# are the same heights.
add_test(NAME cli.contour_of_the_sample_terrain_reports_levels
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "CONTOUR FILE \"${_contour_samples}/terrain.asc\" interval=1"
            -c "SURFACE FROM FILE \"${_contour_samples}/terrain.asc\" NAME ground"
            -c "CONTOUR SURFACE ground interval=1 layer=tin")
set_tests_properties(cli.contour_of_the_sample_terrain_reports_levels PROPERTIES
    PASS_REGULAR_EXPRESSION "contours method=grid cell=1\\.5 levels=14 count=[0-9]+ major=[0-9]+ minor=[0-9]+ layer=terrain/contours smoothed=no.*contours method=tin cell= levels=14 count=[0-9]+ major=[0-9]+ minor=[0-9]+ layer=tin smoothed=no"
    FAIL_REGULAR_EXPRESSION "error")

# No interval is refused, and the session exits with a failure.
add_test(NAME cli.contour_without_an_interval_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "CONTOUR FILE \"${_contour_samples}/terrain.asc\"")
set_tests_properties(cli.contour_without_an_interval_is_refused PROPERTIES WILL_FAIL TRUE)
