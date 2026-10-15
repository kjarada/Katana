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
# ramp's first colour (40, 70, 140) = #28468C. GDAL reads the text grid as
# Float32 (docs/terrain.md, "One surface record"), and the legend's from= and
# to= are exact, so they are the Float32 values: float32(24.892) =
# 24.892000198364258, float32(38.819) = 38.819000244140625, and the first band
# ends at 24.892000198364258 + (38.819000244140625 - 24.892000198364258) / 10 =
# 26.284700202941895 (worked with Python's struct, '<f').
add_test(NAME cli.surface_legend_lists_the_bands_of_a_surfaces_style
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_display_samples}/terrain.asc\" NAME ground"
            -c "SURFACE STYLE ground \"Elevation Bands\""
            -c "SURFACE LEGEND ground")
set_tests_properties(cli.surface_legend_lists_the_bands_of_a_surfaces_style PROPERTIES
    PASS_REGULAR_EXPRESSION "legend surface=\"ground\" style=\"Elevation Bands\" view=plan faces=elevation kind=bands entries=10.*entry index=1 colour=#28468C from=24\\.892000198364258 to=26\\.284700202941895"
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

# A surface the drawing has not got is refused, and the session fails: named,
# not a bare WILL_FAIL, which passed on any failure (docs/testing.md).
add_test(NAME cli.surface_plan_of_no_such_surface_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE PLAN nothing")
set_tests_properties(cli.surface_plan_of_no_such_surface_is_refused PROPERTIES
    PASS_REGULAR_EXPRESSION "error: NotFound: no surface of that name; SURFACE LIST lists them \\[nothing\\]")

# A point is not a view: an AREA with no width and no height cannot be framed,
# and was answered with every count 0 - the reply of a point the surface does
# not cover. The terrain covers (5, 5): -4.25 to 174.25 across, -4.25 to 129.25 up.
add_test(NAME cli.surface_plan_of_an_area_that_is_a_point_is_refused_not_answered_with_zeros
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_display_samples}/terrain.asc\" NAME ground"
            -c "SURFACE PLAN ground AREA 5,5,5,5")
set_tests_properties(cli.surface_plan_of_an_area_that_is_a_point_is_refused_not_answered_with_zeros PROPERTIES
    PASS_REGULAR_EXPRESSION "error: InvalidArgument: AREA has no width and no height")

# The most bands a style may have is 100 (kMaximumSurfaceBands); the verb took
# a million, and the 3D view built 643 triangles a band on this terrain.
add_test(NAME cli.surfacestyle_with_more_bands_than_the_maximum_is_refused_naming_bands
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACESTYLE NEW Many FROM \"Elevation Bands\" bands=101")
set_tests_properties(cli.surfacestyle_with_more_bands_than_the_maximum_is_refused_naming_bands PROPERTIES
    PASS_REGULAR_EXPRESSION "error: InvalidArgument: [^\r\n]*bands must be 2 to 100")

# A plot is warned of a surface it will print without its contours. An interval
# of 0.0005 over the terrain's 13.9 units of relief crosses its 21 182
# triangles more than four million times (SURFACE PLAN says so as problem=), so
# the plan painter refuses the contours and a plot would be without them; the
# preflight check says it before the paper, from a sheet whose plan (1:1000
# about (85, 62), a window 385 x 250 m) reaches the terrain, which covers -4.25
# to 174.25 across and -4.25 to 129.25 up. The finding is a record: the
# severity, the code, the sheet and view, a message naming the surface, its
# style and why, and the surface again as subject=.
add_test(NAME cli.sheets_check_warns_of_a_surface_whose_contours_are_too_fine_to_plot
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_display_samples}/terrain.asc\" NAME ground"
            -c "SURFACESTYLE NEW Fine FROM Contours interval=0.0005"
            -c "SURFACE STYLE ground Fine"
            -c "SHEET NEW PLAN"
            -c "VIEW ADD 1 plan scale=1000 centre=85,62"
            -c "SHEETS CHECK")
set_tests_properties(cli.sheets_check_warns_of_a_surface_whose_contours_are_too_fine_to_plot PROPERTIES
    PASS_REGULAR_EXPRESSION "warning plan\\.surface-style 1 vp1 message=\"[^\r\n]*the surface ground \\(style Fine\\) without part of its style: contours: an interval of 0\\.000500 crosses this surface's triangles more than 4000000 times[^\r\n]* subject=\"ground\""
    FAIL_REGULAR_EXPRESSION "error:")

# The same plan with an interval the terrain can trace says nothing of it.
add_test(NAME cli.sheets_check_says_nothing_of_a_surface_whose_contours_can_be_traced
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_display_samples}/terrain.asc\" NAME ground"
            -c "SURFACE STYLE ground Contours"
            -c "SHEET NEW PLAN"
            -c "VIEW ADD 1 plan scale=1000 centre=85,62"
            -c "SHEETS CHECK")
set_tests_properties(cli.sheets_check_says_nothing_of_a_surface_whose_contours_can_be_traced PROPERTIES
    PASS_REGULAR_EXPRESSION "checked 1 sheet"
    FAIL_REGULAR_EXPRESSION "plan\\.surface-style;error:")
