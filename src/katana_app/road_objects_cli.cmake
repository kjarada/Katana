# The ROADOBJECT verbs through katana_cli (docs/road_objects.md): the
# interpreter's own family, end to end. Included from
# src/katana_app/CMakeLists.txt, where katana_cli and the test variables are.
#
# The site every test stands on: alignment Main, straight from (0,0) to
# (100,0) with a design profile level at 10, so station s, offset o is (s, o).
set(_road_site -c "ALIGN NEW Main 0,0 100,0" -c "ALIGN DESIGN Main 0,10 100,10")

# A W-beam barrier from 10 to 60 at offset 3: 50 m long, posts every 1.905 m
# - 26 whole spacings make 27 to 49.53, and the end post 28 - its rail's top
# 787 mm above the ground (docs/road_objects.md has the sources).
add_test(NAME cli.roadobject_barrier_along_an_alignment_is_placed_and_measured
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "ROADOBJECT ADD barrier ALIGNMENT Main station=10 to=60 offset=3"
            -c "ROADOBJECT LIST")
set_tests_properties(cli.roadobject_barrier_along_an_alignment_is_placed_and_measured PROPERTIES
    PASS_REGULAR_EXPRESSION "added id=barrier-1 kind=barrier placement=alignment alignment=Main station=10 to=60 offset=3 heights=profile layer=road/objects/barrier objects=1 length=50 height=0.787 count=28.*roadobjects count=1 problems=0"
    FAIL_REGULAR_EXPRESSION "error")

# A culvert across Main at 50, skewed 30 degrees: its axis is the left normal
# turned 30 degrees, (-0.5, 0.8660254), so its 20 m put the inlet at
# (55, -8.660254) and the outlet at (45, 8.660254); the 900 mm pipe's 100 mm
# wall under 600 mm of cover on level 10 sets both inverts at 8.4.
add_test(NAME cli.roadobject_skewed_culvert_has_its_ends_where_the_skew_puts_them
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "ROADOBJECT ADD culvert ALIGNMENT Main station=50 skew=30"
            -c "ROADOBJECT INFO culvert-1")
set_tests_properties(cli.roadobject_skewed_culvert_has_its_ends_where_the_skew_puts_them PROPERTIES
    PASS_REGULAR_EXPRESSION "inlet=55,-8[.]660254,8[.]4 outlet=45,8[.]660254,8[.]4.*part id=object/culvert-1/barrel class=IfcPipeSegment type=CULVERT [^\r\n]*closed=yes"
    FAIL_REGULAR_EXPRESSION "error|closed=no")

# A station off the alignment is refused, naming it, and nothing is placed.
add_test(NAME cli.roadobject_refuses_a_station_off_the_alignment
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "ROADOBJECT ADD sign ALIGNMENT Main station=150 offset=4")
set_tests_properties(cli.roadobject_refuses_a_station_off_the_alignment PROPERTIES
    PASS_REGULAR_EXPRESSION "station 150 is off alignment Main [(]0 to 100[)]")

# One undo step: UNDO takes the sign and the layer made for it away.
add_test(NAME cli.roadobject_add_is_one_undo_step
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4" -c "UNDO"
            -c "ROADOBJECT LIST")
set_tests_properties(cli.roadobject_add_is_one_undo_step PROPERTIES
    PASS_REGULAR_EXPRESSION "added id=sign-1.*roadobjects count=0 problems=0"
    FAIL_REGULAR_EXPRESSION "error")

# Saved with the project and opened again in a second process: the objects
# are the drawing's, under its "road_objects" key.
set(_road_project "${CMAKE_CURRENT_BINARY_DIR}/cli-road-objects.katana")
add_test(NAME cli.roadobject_cleanup COMMAND ${CMAKE_COMMAND} -E rm -rf "${_road_project}")
add_test(NAME cli.roadobject_saved_with_the_project
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "ROADOBJECT ADD light ALIGNMENT Main station=0 to=60 spacing=30 offset=6"
            -c "SAVE \"${_road_project}\"")
add_test(NAME cli.roadobject_opened_again_with_the_project
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "OPEN \"${_road_project}\"" -c "ROADOBJECT LIST")
set_tests_properties(cli.roadobject_cleanup PROPERTIES FIXTURES_SETUP cli_road_project)
set_tests_properties(cli.roadobject_saved_with_the_project PROPERTIES
    FIXTURES_REQUIRED cli_road_project FIXTURES_SETUP cli_road_saved
    FAIL_REGULAR_EXPRESSION "error")
# Three poles at 0, 30 and 60, each a pole and a lantern.
set_tests_properties(cli.roadobject_opened_again_with_the_project PROPERTIES
    FIXTURES_REQUIRED cli_road_saved
    PASS_REGULAR_EXPRESSION "roadobjects count=1 problems=0[\r\n]+object id=light-1 kind=light placement=alignment alignment=Main station=0 to=60 offset=6 spacing=30 [^\r\n]*objects=6"
    FAIL_REGULAR_EXPRESSION "error")
