# The surface edits through katana_cli (docs/surface_editing.md): SURFACE EDIT,
# MERGE, PASTE, CLIP and OFFSET are the interpreter's verbs, on the drawing's
# surfaces, one undo step each. Included from src/katana_app/CMakeLists.txt
# when the interop module is built (geo/cli/*.cmake).
set(_surface_edit_samples "${PROJECT_SOURCE_DIR}/samples/gis")

# samples/gis/terrain.asc as a surface: 10 800 vertices from 24.892 to 38.819
# (cli.surface_from_the_sample_terrain_then_list works the figures). RAISE 1
# moves every vertex by exactly 1: 25.892 to 39.819.
add_test(NAME cli.surface_edit_raise_moves_every_vertex_by_the_amount
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "SURFACE EDIT ground RAISE 1"
            -c "SURFACE INFO ground")
set_tests_properties(cli.surface_edit_raise_moves_every_vertex_by_the_amount PROPERTIES
    PASS_REGULAR_EXPRESSION "surface name=ground operation=raise .*moved=10800 held=0 crossings=0 largest_change=1 changed=yes.*surface name=ground triangles=[0-9]+ points=10800 bounds=-4\\.250,-4\\.250,174\\.250,129\\.250 zmin=25\\.892 zmax=39\\.819"
    FAIL_REGULAR_EXPRESSION "error")

# One undo step: UNDO puts the levels back. The figures are anchored AFTER the
# "1 undone" line: SURFACE FROM's own reply, printed before the edit, already
# says zmin=24.892 zmax=38.819, so an expectation anywhere in the output passed
# whether or not the UNDO restored anything.
add_test(NAME cli.surface_edit_is_one_undo_step
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "SURFACE EDIT ground RAISE 1"
            -c "UNDO"
            -c "SURFACE INFO ground")
set_tests_properties(cli.surface_edit_is_one_undo_step PROPERTIES
    PASS_REGULAR_EXPRESSION "1 undone[\r\n]+surface name=ground triangles=[0-9]+ points=10800 bounds=-4\\.250,-4\\.250,174\\.250,129\\.250 zmin=24\\.892 zmax=38\\.819"
    FAIL_REGULAR_EXPRESSION "error")

# A boundary inside the sample's extent clips it exactly to the square typed:
# its bounds are the square's.
add_test(NAME cli.surface_edit_boundary_clips_to_the_typed_square
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "SURFACE EDIT ground BOUNDARY 10,10 60,10 60,40 10,40"
            -c "SURFACE INFO ground")
set_tests_properties(cli.surface_edit_boundary_clips_to_the_typed_square PROPERTIES
    PASS_REGULAR_EXPRESSION "surface name=ground operation=boundary .*changed=yes.*surface name=ground triangles=[0-9]+ points=[0-9]+ bounds=10\\.000,10\\.000,60\\.000,40\\.000"
    FAIL_REGULAR_EXPRESSION "error")

# OFFSET makes a second surface 2 m down; the first is as it was.
add_test(NAME cli.surface_offset_makes_a_copy_by_name
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "SURFACE OFFSET ground -2 NAME subgrade"
            -c "SURFACE INFO subgrade"
            -c "SURFACE INFO ground")
set_tests_properties(cli.surface_offset_makes_a_copy_by_name PROPERTIES
    PASS_REGULAR_EXPRESSION "surface name=subgrade .*zmin=22\\.892 zmax=36\\.819.*surface name=ground .*zmin=24\\.892 zmax=38\\.819"
    FAIL_REGULAR_EXPRESSION "error")

# HELP SURFACE EDIT names every verb of the family.
add_test(NAME cli.surface_edit_help_names_the_family
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "HELP SURFACE EDIT")
set_tests_properties(cli.surface_edit_help_names_the_family PROPERTIES
    PASS_REGULAR_EXPRESSION "SURFACE EDIT <name> FLIP x,y.*SURFACE PASTE <top> INTO <base>")

# A surface nothing has is refused, and the session exits with a failure.
add_test(NAME cli.surface_edit_of_no_such_surface_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "SURFACE EDIT nothing THIN 0.1")
set_tests_properties(cli.surface_edit_of_no_such_surface_is_refused PROPERTIES WILL_FAIL TRUE)

# A 3D breakline along a row of the lattice the sample's vertices make is
# straight end to end: (49.75, 51.25) is a vertex of the raster's surface in
# the middle of the line from (0.25, 51.25, 40) to (99.25, 51.25, 45), half way
# along it (49.5 of 99), so it takes 40 + 5 / 2 = 42.5. It kept its ground
# level (35.666) while only the two ends moved.
add_test(NAME cli.surface_edit_breakline_through_vertices_is_straight_in_3d
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "SURFACE EDIT ground BREAKLINE 0.25,51.25,40 99.25,51.25,45"
            -c "RASTER SAMPLE SURFACE ground AT 49.75,51.25")
set_tests_properties(cli.surface_edit_breakline_through_vertices_is_straight_in_3d PROPERTIES
    PASS_REGULAR_EXPRESSION "operation=breakline .*changed=yes.*sample at=49\\.75,51\\.25 z=(42\\.5|42\\.4999[0-9]*|42\\.5000[0-9]*)"
    FAIL_REGULAR_EXPRESSION "error")

# Two 3D lines of one edit that cross at different levels are refused, whatever
# order they are named in: (5,5) is 12 on the one and 8 on the other.
add_test(NAME cli.surface_edit_two_crossing_breaklines_at_different_levels_are_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "LAYER NEW base" -c "LAYER SET base"
            -c "PLINE3D 0,0,10 10,0,10 10,10,10 0,10,10 CLOSE"
            -c "SURFACE FROM LAYERS base NAME g"
            -c "LAYER NEW lines" -c "LAYER SET lines"
            -c "PLINE3D 1,5,12 9,5,12" -c "PLINE3D 5,1,8 5,9,8"
            -c "SURFACE EDIT g BREAKLINE FROM LAYERS lines ONLY")
set_tests_properties(cli.surface_edit_two_crossing_breaklines_at_different_levels_are_refused PROPERTIES
    PASS_REGULAR_EXPRESSION "breaklines cross at two different levels")

# A scope that takes no polygon is an answer, not a refusal: the reply says it
# ran nothing (OUTSIDE of no polygon is not the whole surface), and the next
# line still runs.
add_test(NAME cli.surface_edit_scope_that_takes_nothing_is_reported_and_the_session_goes_on
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "LINE 0,0 10,10"
            -c "SURFACE EDIT ground DELETE TRIANGLES OUTSIDE FROM DRAWING"
            -c "SURFACE INFO ground")
set_tests_properties(cli.surface_edit_scope_that_takes_nothing_is_reported_and_the_session_goes_on PROPERTIES
    PASS_REGULAR_EXPRESSION "scope=drawing matched=1 polygons=0 skipped=1[\r\n]+surface name=ground operation=delete_triangles_outside .*changed=no ran=no reason=\"the scope took no polygon\"[\r\n]+surface name=ground triangles=21182 "
    FAIL_REGULAR_EXPRESSION "error")

# A breakline ending where decimal arithmetic puts it on an old breakline: the
# point (14.3, 4.29) is on y = 0.3 x to within 2e-16. The new line, level at 35
# from (40,70) to there, used to cross the old one a few ulps from its end and
# ramp down to the old level (33.75, 32.5, 31.25, 30.23 at the quarter points).
# It is level at 35, the quarter points on it worked out by hand:
# (40 - 25.7 s, 70 - 65.71 s) for s = 1/4, 1/2, 3/4, 0.95 and the end: the
# s = 0.95 point is (15.585, 7.5755), and a point 0.0005 below it is in a
# triangle on the other side of the line, where the surface is 34.97.
set(_z "(35|34\\.9999[0-9]*|35\\.0000[0-9]*)")
add_test(NAME cli.surface_edit_breakline_ending_within_rounding_of_a_breakline_stays_level
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME g"
            -c "SURFACE EDIT g BREAKLINE 0,0,30 100,30,30"
            -c "SURFACE EDIT g BREAKLINE 40,70,35 14.3,4.29,35"
            -c "RASTER SAMPLE SURFACE g AT 33.575,53.5725 AT 27.15,37.145 AT 20.725,20.7175 AT 15.585,7.5755 AT 14.3,4.29")
set_tests_properties(cli.surface_edit_breakline_ending_within_rounding_of_a_breakline_stays_level PROPERTIES
    PASS_REGULAR_EXPRESSION "operation=breakline .*crossings=0 changed=yes.*sample at=33\\.575,53\\.5725 z=${_z}[\r\n]+sample at=27\\.15,37\\.145 z=${_z}[\r\n]+sample at=20\\.725,20\\.7175 z=${_z}[\r\n]+sample at=15\\.585,7\\.5755 z=${_z}[\r\n]+sample at=14\\.3,4\\.29 z=${_z}"
    FAIL_REGULAR_EXPRESSION "error")

# A parallelogram with two decimal corners on that breakline and its edge along
# it: area |AB x AD| = 19.6 * 10 + 5.88 * 3 = 213.64. Refused, a time in four,
# as "the polygon touches or crosses itself" at the breakline's far end (0,0).
add_test(NAME cli.surface_clip_with_an_edge_along_a_breakline_is_cut_exactly
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME g"
            -c "SURFACE EDIT g BREAKLINE 0,0,30 100,30,30"
            -c "SURFACE CLIP g 14.3,4.29 33.9,10.17 30.9,20.17 11.3,14.29 NAME c"
            -c "SURFACE INFO c")
set_tests_properties(cli.surface_clip_with_an_edge_along_a_breakline_is_cut_exactly PROPERTIES
    PASS_REGULAR_EXPRESSION "surface name=c operation=clip .*changed=yes.*surface name=c triangles=[0-9]+ .*area plan=213\\.640 "
    FAIL_REGULAR_EXPRESSION "error")

# A point a hair beyond a rim edge made by a cut is on the surface (RASTER
# SAMPLE says so), so a breakline may end there: (115, 37.5), (80, 65) and
# (45, 92.5) are on the hypotenuse from (150,10) to (10,120) of the triangle
# 10,10 150,10 10,120 in decimal arithmetic (y = 10 + 110 (150 - x) / 140), and
# the first was refused as "off the surface" when the last bit rounded the wrong
# way.
add_test(NAME cli.surface_edit_breakline_may_end_on_a_cut_rim_whichever_way_it_rounds
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME g"
            -c "SURFACE EDIT g BOUNDARY 10,10 150,10 10,120"
            -c "SURFACE EDIT g BREAKLINE 40,30 115,37.5 PREVIEW"
            -c "SURFACE EDIT g BREAKLINE 40,30 80,65 PREVIEW"
            -c "SURFACE EDIT g BREAKLINE 40,30 45,92.5 PREVIEW")
set_tests_properties(cli.surface_edit_breakline_may_end_on_a_cut_rim_whichever_way_it_rounds PROPERTIES
    PASS_REGULAR_EXPRESSION "operation=breakline .*changed=yes preview=yes.*operation=breakline .*changed=yes preview=yes.*operation=breakline .*changed=yes preview=yes"
    FAIL_REGULAR_EXPRESSION "error")

# A polygon that visits a corner twice is no polygon with an inside: refused
# whichever way its lobes run. This one crosses itself at the vertex 40,40, its
# lobes running opposite ways; it was taken, and removed 461.9 m2 from ground
# outside it where the polygon covers 450.
add_test(NAME cli.surface_edit_hole_that_crosses_itself_at_a_vertex_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "SURFACE EDIT ground HOLE 20,20 60,20 40,40 35,50 45,50 40,40")
set_tests_properties(cli.surface_edit_hole_that_crosses_itself_at_a_vertex_is_refused PROPERTIES
    PASS_REGULAR_EXPRESSION "InvalidGeometry: the polygon touches or crosses itself \\[at \\(40, 40\\)\\]")

# A point put in a 20 x 20 hole fills all of it: the plan area is the ground's
# 23829.750 again (23429.750 less the 400 m2 hole, plus the 400 the fans of the
# point cover). Two ears of the hole were left open, 66 m2 of it.
add_test(NAME cli.surface_edit_add_point_in_a_hole_fills_the_whole_hole
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME t"
            -c "SURFACE EDIT t HOLE 50,50 70,50 70,70 50,70"
            -c "SURFACE EDIT t ADD POINT 60,60,30"
            -c "SURFACE INFO t")
set_tests_properties(cli.surface_edit_add_point_in_a_hole_fills_the_whole_hole PROPERTIES
    PASS_REGULAR_EXPRESSION "operation=add_point .*changed=yes.*surface name=t triangles=[0-9]+ .*area plan=23829\\.750 "
    FAIL_REGULAR_EXPRESSION "error")

# Two clips of one surface that abut merge into all of it: the west and east
# halves of the sample at x = 85 (11914.875 each) are 23829.750 together. The
# merge dropped the row of the west half's triangles along the seam (23729.625).
add_test(NAME cli.surface_merge_of_two_abutting_clips_is_all_of_the_surface
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME terrain"
            -c "SURFACE CLIP terrain -10,-10 85,-10 85,140 -10,140 NAME west"
            -c "SURFACE CLIP terrain 85,-10 185,-10 185,140 85,140 NAME east"
            -c "SURFACE MERGE west east NAME merged"
            -c "SURFACE INFO merged")
set_tests_properties(cli.surface_merge_of_two_abutting_clips_is_all_of_the_surface PROPERTIES
    PASS_REGULAR_EXPRESSION "surface name=merged operation=merge .*surface name=merged triangles=[0-9]+ .*area plan=23829\\.750 "
    FAIL_REGULAR_EXPRESSION "error")
