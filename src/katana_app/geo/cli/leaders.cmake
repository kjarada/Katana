# The preset leaders through katana_cli (docs/annotation.md, "Presets"): a
# surface made from the sample terrain, a point on it, and leaders reading its
# level and its height above the ground, live. Included from
# src/katana_app/CMakeLists.txt with the other geo/cli files, since SURFACE
# FROM needs the interop module.
#
# samples/gis/terrain.asc's 71st row from the top, 21st column: its centre is
# -5 + 1.5 x 20.5 = 25.75 across and -5 + 1.5 x (90 - 70 - 0.5) = 24.25 up,
# and its value, read straight from the text grid, 27.218. Every cell centre
# is a vertex of the surface (cli.surface_from_the_sample_terrain_then_list:
# points=10800), and a TIN reads a vertex's own level there. A point at RL 30
# is 30 - 27.218 = 2.782 above the ground: +2.782, a fill of 2.782.
set(_leader_samples "${PROJECT_SOURCE_DIR}/samples/gis")

add_test(NAME cli.a_surface_leader_says_how_far_a_point_is_above_the_ground
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "POINT 25.75,24.25" -c "SELECT 1" -c "PROP SET elevation 30"
            -c "SURFACE FROM FILE \"${_leader_samples}/terrain.asc\" NAME ground"
            -c "LEADER #1 30,30 preset=surface surface=ground"
            -c "LEADER #1 30,20 preset=cutfill"
            -c "LEADER #1 20,30 preset=level"
            -c "LEADER LIST")
set_tests_properties(cli.a_surface_leader_says_how_far_a_point_is_above_the_ground PROPERTIES
    PASS_REGULAR_EXPRESSION "created leader id=[0-9]+ text=\\+2\\.782 associative=yes target=1 .*preset=surface surface=ground[\r\n]+.*created leader id=[0-9]+ text=\"FILL 2\\.782\" .*preset=cutfill surface=ground chosen=\"first visible surface\"[\r\n]+.*created leader id=[0-9]+ text=\"RL 30\\.000\" .*preset=level[\r\n]+.*smart=yes template=\\{dsurface\\.ground:\\+\\.3f\\} preset=surface surface=ground .*text=\\+2\\.782"
    FAIL_REGULAR_EXPRESSION "error")

# A preset reading a surface the drawing has not is refused, and the script
# fails rather than leave a leader saying nothing. The refusal is named: a bare
# WILL_FAIL passed on ANY failure - an earlier line of the script refused, or
# katana_cli crashing (cmake -E env turns a crash in its child into an
# ordinary exit 1) - so the test asks for the earlier line's success and the
# LEADER line's own refusal, in either order (the two streams are not ordered
# by the pipe). It does not use WILL_FAIL beside the pattern: ctest would
# invert the pattern's result.
add_test(NAME cli.a_surface_leader_on_no_surface_fails_the_script
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "POINT 25.75,24.25" -c "SELECT 1" -c "PROP SET elevation 30"
            -c "LEADER #1 30,30 preset=surface surface=ground")
set_tests_properties(cli.a_surface_leader_on_no_surface_fails_the_script PROPERTIES
    PASS_REGULAR_EXPRESSION "property elevation set on 1.*error: NotFound: the drawing has no surface of that name|error: NotFound: the drawing has no surface of that name.*property elevation set on 1")

# A surface a leader reads is read by NAME (docs/annotation.md): renamed, the
# leader's template follows it in the same step; removed, the reply carries a
# warning record saying how many leaders will read nothing - and the removal
# stands (UNDO puts it back). The point at RL 30 is 2.782 above the ground, as
# above.
add_test(NAME cli.renaming_a_surface_moves_its_leaders_and_removing_it_warns
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "POINT 25.75,24.25" -c "SELECT 1" -c "PROP SET elevation 30"
            -c "SURFACE FROM FILE \"${_leader_samples}/terrain.asc\" NAME ground"
            -c "LEADER #1 30,30 preset=surface surface=ground"
            -c "SURFACE RENAME ground dem"
            -c "LEADER LIST"
            -c "SURFACE REMOVE dem")
set_tests_properties(cli.renaming_a_surface_moves_its_leaders_and_removing_it_warns PROPERTIES
    PASS_REGULAR_EXPRESSION "renamed surface=ground name=dem corridors=0 roadobjects=0 leaders=1.*template=\{dsurface\.dem:\+\.3f\} preset=surface surface=dem .*text=\+2\.782.*removed surface=dem surfaces=0[\r\n]+warning text=\"leaders=1 read surface dem and will read nothing now\""
    FAIL_REGULAR_EXPRESSION "error")
