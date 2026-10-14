# The road model's cli.* tests (docs/road_design.md): katana_cli runs the
# interpreter's ROADTEMPLATE, CORRIDOR and ROADMODEL verbs end to end, and a
# project keeps the road model across two processes. Included from
# src/katana_app/CMakeLists.txt, where katana_cli and the test variables are.

# A 100 m straight along +x with a level design at 10, the standard
# templates, and a corridor of the urban street on it (no target surface).
set(_road_setup
    -c "ALIGN NEW road 0,0 100,0" -c "ALIGN DESIGN road 0,10 100,10" -c "ROADTEMPLATE DEFAULTS"
    -c "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"")

# 100 m of the urban street's 3.3 m lane, by hand: its 50 mm of AC14 is 3.3 x
# 0.05 x 100 = 16.5 m3 a side over 330 m2; its kerb and gutter runs the 100 m
# and is 0.3 + 0.025 + 0.15 = 0.475 m wide.
add_test(NAME cli.a_corridor_reports_each_course_and_kerb_with_its_quantities
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_setup} -c "CORRIDOR QUANTITIES Main")
set_tests_properties(cli.a_corridor_reports_each_course_and_kerb_with_its_quantities PROPERTIES
    PASS_REGULAR_EXPRESSION
    "object id=corridor/Main/left/lane/AC14/1 kind=pavement name=\"Main left lane AC14\" Length=100[.]000 Width=3[.]300 Depth=0[.]050 GrossArea=330[.]000 GrossVolume=16[.]500.*object id=corridor/Main/left/kerb/1 kind=kerb name=\"Main left kerb\" Length=100[.]000 Width=0[.]475"
    FAIL_REGULAR_EXPRESSION "error")

# The strings: the centreline and seven a side - the lane's and the parking's
# edges, the gutter's invert, the kerb's face and back, the footpath's and the
# verge's edges - as 3D polylines on road/strings.
add_test(NAME cli.corridor_strings_are_polylines_on_their_own_layer
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_setup} -c "CORRIDOR STRINGS Main" -c "LIST")
set_tests_properties(cli.corridor_strings_are_polylines_on_their_own_layer PROPERTIES
    PASS_REGULAR_EXPRESSION
    "strings created=15 replaced=0 corridor=Main layer=road/strings.*Polyline +layer=road/strings"
    FAIL_REGULAR_EXPRESSION "error")

# Saved, then opened by another process: the templates, the corridor with its
# superelevation, and the road model made again from them.
set(_road_project "${CMAKE_CURRENT_BINARY_DIR}/cli-road.katana")
add_test(NAME cli.road_cleanup COMMAND ${CMAKE_COMMAND} -E rm -rf "${_road_project}")
add_test(NAME cli.a_road_model_is_saved_with_the_project
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_setup}
            -c "CORRIDOR SUPERELEVATION Main ADD 40 3 -3" -c "SAVE \"${_road_project}\"")
add_test(NAME cli.a_saved_road_model_is_read_back_by_another_process
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "OPEN \"${_road_project}\"" -c "ROADTEMPLATE LIST"
            -c "CORRIDOR INFO Main" -c "ROADMODEL PROBLEMS")
set_tests_properties(cli.road_cleanup PROPERTIES FIXTURES_SETUP cli_road_project)
set_tests_properties(cli.a_road_model_is_saved_with_the_project PROPERTIES
    FIXTURES_REQUIRED cli_road_project FIXTURES_SETUP cli_road_saved)
set_tests_properties(cli.a_saved_road_model_is_read_back_by_another_process PROPERTIES
    FIXTURES_REQUIRED cli_road_saved
    PASS_REGULAR_EXPRESSION
    "templates=3.*template name=\"urban street\".*corridor name=Main alignment=road template=\"urban street\".*superelevation station=40 left=3 right=-3.*road model objects=[1-9][0-9]* problems=1"
    FAIL_REGULAR_EXPRESSION "error")

# A corridor on an alignment with no design profile is refused, naming what
# gives it one; the script stops there and fails.
add_test(NAME cli.a_corridor_needs_a_design_profile
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "ALIGN NEW road 0,0 100,0" -c "ROADTEMPLATE DEFAULTS"
            -c "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\""
            -c "ROADMODEL NEVER REACHED")
set_tests_properties(cli.a_corridor_needs_a_design_profile PROPERTIES
    PASS_REGULAR_EXPRESSION "has no design profile: ALIGN DESIGN gives it one"
    FAIL_REGULAR_EXPRESSION "NEVER REACHED")
add_test(NAME cli.a_refused_corridor_fails_the_script
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "ALIGN NEW road 0,0 100,0" -c "ROADTEMPLATE DEFAULTS"
            -c "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"")
set_tests_properties(cli.a_refused_corridor_fails_the_script PROPERTIES WILL_FAIL TRUE)
