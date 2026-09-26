# RASTER VIEWSHED and LOS through katana_cli (docs/terrain.md, "Viewshed and
# line of sight"). Included from src/katana_app/CMakeLists.txt when the
# interop module is built.
set(_viewshed_data "${PROJECT_SOURCE_DIR}/tests/geo/data")

# tests/geo/data/plane.asc rises 1 in 20 to the east. Along any ray from an
# observer at its west edge the ground rises linearly, so each cell further
# out lies at a steeper angle from the eye than any nearer one: every one of
# its 40 x 30 cells of 1 m2 is seen - 1200 cells, 1200 m2.
add_test(NAME cli.raster_viewshed_of_a_rising_plane_sees_every_cell
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "RASTER VIEWSHED FILE \"${_viewshed_data}/plane.asc\" OBSERVER 0.5,15.5 curvature=none")
set_tests_properties(cli.raster_viewshed_of_a_rising_plane_sees_every_cell PROPERTIES
    PASS_REGULAR_EXPRESSION "gis op=viewshed .*observer at=0.5,15.5 visible_cells=1200.*name=plane-viewshed.*viewshed observers=1 height=1.7 target=0 max= curvature=0 visible_cells=1200 area=1200\\.000"
    FAIL_REGULAR_EXPRESSION "error")

# A 1 m wall 20 m from the observer, on flat ground: 60 x 31 cells of 1 m,
# 0 but for the column x = 20..21. Read bilinearly the wall rises from 0 at
# x = 19.5 to 1 at 20.5; the sight from 1.7 m over (0.5, 15.5) to the ground
# at (30.5, 15.5) is 1.7 x (1 - (x - 0.5) / 30) high - 0.567 at x = 20.5,
# under the wall's 1 - so the first station hidden, half a cell apart, is
# 20.5, given to the millimetre as a computed place: 20.500,15.500.
set(_wall "${CMAKE_CURRENT_BINARY_DIR}/cli_viewshed_wall.asc")
set(_wall_text "ncols 60\nnrows 31\nxllcorner 0\nyllcorner 0\ncellsize 1\n")
set(_wall_row "")
foreach(_column RANGE 0 59)
    if(_column EQUAL 20)
        string(APPEND _wall_row "1")
    else()
        string(APPEND _wall_row "0")
    endif()
    if(_column LESS 59)
        string(APPEND _wall_row " ")
    endif()
endforeach()
foreach(_row RANGE 1 31)
    string(APPEND _wall_text "${_wall_row}\n")
endforeach()
file(WRITE "${_wall}" "${_wall_text}")
add_test(NAME cli.los_over_a_wall_is_blocked_at_the_wall
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "LOS FILE \"${_wall}\" OBSERVER 0.5,15.5 TARGET 30.5,15.5 curvature=none")
set_tests_properties(cli.los_over_a_wall_is_blocked_at_the_wall PROPERTIES
    PASS_REGULAR_EXPRESSION "gis op=los .*sight visible=no observer=0.5,15.5 target=30.5,15.5 distance=30\\.000 observer_z=1\\.700 target_z=0\\.000 .*blocked_at=20\\.500,15\\.500 .*step=0\\.5 curvature=0"
    FAIL_REGULAR_EXPRESSION "error")
