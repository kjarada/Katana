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
# The fixtures are this family's own (cli_road_objects_*): a fixture of the road
# model's tests under the same name made a failed save here skip that family's
# read-back, and the reverse, and -R of one pulled in the other's setup.
set_tests_properties(cli.roadobject_cleanup PROPERTIES FIXTURES_SETUP cli_road_objects_project)
set_tests_properties(cli.roadobject_saved_with_the_project PROPERTIES
    FIXTURES_REQUIRED cli_road_objects_project FIXTURES_SETUP cli_road_objects_saved
    FAIL_REGULAR_EXPRESSION "error")
# Three poles at 0, 30 and 60, each a pole and a lantern.
set_tests_properties(cli.roadobject_opened_again_with_the_project PROPERTIES
    FIXTURES_REQUIRED cli_road_objects_saved
    PASS_REGULAR_EXPRESSION "roadobjects count=1 problems=0[\r\n]+object id=light-1 kind=light placement=alignment alignment=Main station=0 to=60 offset=6 spacing=30 [^\r\n]*objects=6"
    FAIL_REGULAR_EXPRESSION "error")

# The review of 2026-10-15 (docs/road_objects.md).

# A wall's length is its length once: 50 m of the default wall is 50 m long
# whatever its footing, which is 50 m long too (it said 100), with 25 + 24 =
# 49 m3 of concrete; and a light's count is its poles: poles at 0, 30 and 60.
add_test(NAME cli.roadobject_totals_count_a_walls_length_and_a_lights_poles_once
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "ROADOBJECT ADD wall ALIGNMENT Main station=10 to=60 offset=5"
            -c "ROADOBJECT ADD light ALIGNMENT Main station=0 to=60 spacing=30 offset=6"
            -c "ROADOBJECT QUANTITIES")
set_tests_properties(cli.roadobject_totals_count_a_walls_length_and_a_lights_poles_once PROPERTIES
    PASS_REGULAR_EXPRESSION "added id=wall-1 [^\r\n]*objects=2 length=50 height=2 grossarea=100 grossvolume=49 width=1.2 depth=0.4.*added id=light-2 [^\r\n]*objects=6 count=3 height=10.*total kind=light objects=1 count=3 height=10.*total kind=wall objects=1 length=50 height=2"
    FAIL_REGULAR_EXPRESSION "error|length=100|count=6")

# A closed polyline is stored with its first vertex again at the end: a guide
# post at each corner is four, not five.
add_test(NAME cli.roadobject_posts_along_a_closed_line_stand_once_at_each_corner
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "PLINE 0,0 10,0 10,10 0,10 CLOSE" -c "SELECT ALL" -c "PROP SET elevation 5.5"
            -c "ROADOBJECT ADD guidepost ALONG #1"
            -c "ROADOBJECT ADD guidepost ALONG #1 spacing=10")
set_tests_properties(cli.roadobject_posts_along_a_closed_line_stand_once_at_each_corner PROPERTIES
    PASS_REGULAR_EXPRESSION "added id=guidepost-1 [^\r\n]*points=5 source=#1 [^\r\n]*objects=4 count=4.*added id=guidepost-2 [^\r\n]*spacing=10 [^\r\n]*objects=4 count=4"
    FAIL_REGULAR_EXPRESSION "error|count=5")

# One undo takes the layers the ADD made with it, parents and all: "road",
# "road/objects" and "road/objects/sign". LAYER LIST after the UNDO names only
# layer 0.
add_test(NAME cli.roadobject_undo_takes_the_parent_layers_it_made_too
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4"
            -c "UNDO" -c "LAYER LIST")
set_tests_properties(cli.roadobject_undo_takes_the_parent_layers_it_made_too PROPERTIES
    PASS_REGULAR_EXPRESSION "undone"
    FAIL_REGULAR_EXPRESSION "error|undone.*road")

# An id is never given twice, undo included: the sign is sign-3 after the copy
# it follows is undone.
add_test(NAME cli.roadobject_ids_are_not_given_twice_after_an_undo
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4"
            -c "ROADOBJECT COPY barrier-1 station=30" -c "UNDO"
            -c "ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4")
set_tests_properties(cli.roadobject_ids_are_not_given_twice_after_an_undo PROPERTIES
    PASS_REGULAR_EXPRESSION "undone.*added id=sign-3 "
    FAIL_REGULAR_EXPRESSION "error|id=sign-2")

# spacing= and crossfall= where they apply to nothing are refused, naming why.
add_test(NAME cli.roadobject_refuses_spacing_on_a_barrier
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 spacing=5")
set_tests_properties(cli.roadobject_refuses_spacing_on_a_barrier PROPERTIES
    PASS_REGULAR_EXPRESSION "spacing= is for guide posts, signs and light poles; a barrier takes postspacing=")

# LIST takes no scope: a LAYERS scope is refused, not answered with every object.
add_test(NAME cli.roadobject_list_takes_no_scope
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4"
            -c "ROADOBJECT LIST LAYERS road/objects/sign")
set_tests_properties(cli.roadobject_list_takes_no_scope PROPERTIES
    PASS_REGULAR_EXPRESSION "ROADOBJECT LIST acts on road objects by id and takes no scope or filter"
    FAIL_REGULAR_EXPRESSION "roadobjects count")

# A renamed surface is followed by the road objects standing on it, in the one
# undo step: the sign still stands on its ground and nothing is a problem.
add_test(NAME cli.surface_rename_is_followed_by_the_road_objects_standing_on_it
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_site}
            -c "SURFACE FROM FILE \"${CMAKE_SOURCE_DIR}/samples/gis/terrain.asc\" NAME ground"
            -c "ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4 surface=ground"
            -c "SURFACE RENAME ground eg" -c "ROADOBJECT PROBLEMS" -c "ROADOBJECT LIST")
set_tests_properties(cli.surface_rename_is_followed_by_the_road_objects_standing_on_it PROPERTIES
    PASS_REGULAR_EXPRESSION "renamed surface=ground name=eg corridors=0 roadobjects=1.*problems=0.*roadobjects count=1 problems=0[\r\n]+object id=sign-1 [^\r\n]*surface=eg"
    FAIL_REGULAR_EXPRESSION "error|does not exist")
