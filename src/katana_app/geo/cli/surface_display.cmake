# SURFACE LEGEND and SURFACE PLAN through katana_cli (docs/surface_display.md,
# "On the command line"). Included from src/katana_app/CMakeLists.txt with the
# geoprocessing tests (geo/cli/*.cmake): the surface is made from the sample
# DEM by the geoprocessing SURFACE FROM, which only that build has.
set(_display_samples "${PROJECT_SOURCE_DIR}/samples/gis")

# samples/gis/terrain.asc holds heights from 24.892 to 38.819 (its least and
# greatest values, read from the text grid; cli.contour_* and
# qt_surface_from_raster_headless hold the surface to the same), and the
# built-in Elevation Bands cuts the surface's own range into ten bands on the
# terrain ramp: the first from 24.892 to 24.892 + 13.927 / 10 = 26.2847, in the
# ramp's first colour (40, 70, 140) = #28468C.
add_test(NAME cli.surface_legend_lists_the_bands_of_a_surfaces_style
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_display_samples}/terrain.asc\" NAME ground"
            -c "SURFACE STYLE ground \"Elevation Bands\""
            -c "SURFACE LEGEND ground")
set_tests_properties(cli.surface_legend_lists_the_bands_of_a_surfaces_style PROPERTIES
    PASS_REGULAR_EXPRESSION "legend surface=\"ground\" style=\"Elevation Bands\" view=plan faces=elevation kind=bands entries=10.*entry index=1 colour=#28468C from=24\\.892 to=26\\.284"
    FAIL_REGULAR_EXPRESSION "error")

# The built-in Contours (1 m) over the same heights: the whole metres 25 to 38,
# 14 levels, as cli.contour_of_the_sample_terrain_reports_levels counts them;
# looked at whole through the plan view's 1600 x 1000.
add_test(NAME cli.surface_plan_says_what_the_plan_view_draws_of_a_surface
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_display_samples}/terrain.asc\" NAME ground"
            -c "SURFACE STYLE ground Contours"
            -c "SURFACE PLAN ground")
set_tests_properties(cli.surface_plan_says_what_the_plan_view_draws_of_a_surface PROPERTIES
    PASS_REGULAR_EXPRESSION "plan surface=\"ground\" style=\"Contours\" visible=yes pixels=1600,1000 [^\r\n]* levels=14 [^\r\n]*[\r\n]+scope=surface"
    FAIL_REGULAR_EXPRESSION "error")

# A surface the drawing has not got is refused, and the session fails.
add_test(NAME cli.surface_plan_of_no_such_surface_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE PLAN nothing")
set_tests_properties(cli.surface_plan_of_no_such_surface_is_refused PROPERTIES WILL_FAIL TRUE)
