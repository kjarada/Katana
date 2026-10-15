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

# The road model goes out with an IFC export, and NOROADMODEL leaves it out
# (docs/ifc.md, "Road model"): a corridor is the one thing that gives the model
# objects, so both directions are asked of the same drawing. The cli.ifc_*
# test of the word runs the sample scenario, which has no corridor and so has no
# road record whichever way the word goes; these run the corridor above. The
# default reply holds the record (`road elements=N skipped=0 roads=1 parts=P`,
# N and P above zero: the urban street's courses, kerbs and batters in a road
# with parts) and the road and its parts as classes; with the word, the reply
# goes straight from `counts` to the classes - the alignment and its stations
# only - and no class of the road is among them.
add_test(NAME cli.ifc_export_writes_a_corridors_road_model_by_default
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_setup}
            -c "EXPORT \"${CMAKE_CURRENT_BINARY_DIR}/cli road default.ifc\" PREVIEW")
set_tests_properties(cli.ifc_export_writes_a_corridors_road_model_by_default PROPERTIES
    PASS_REGULAR_EXPRESSION
    "ifc previewed file=\"cli road default\\.ifc\" [^\n]*\ncounts [^\n]*\nroad elements=[1-9][0-9]* skipped=0 roads=1 parts=[1-9][0-9]*\nclass name=IfcAlignment.*class name=IfcRoad count=1\n.*class name=IfcRoadPart count=[1-9]")
add_test(NAME cli.ifc_noroadmodel_leaves_a_corridors_road_model_out
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_setup}
            -c "EXPORT \"${CMAKE_CURRENT_BINARY_DIR}/cli road left out.ifc\" NOROADMODEL PREVIEW")
set_tests_properties(cli.ifc_noroadmodel_leaves_a_corridors_road_model_out PROPERTIES
    PASS_REGULAR_EXPRESSION
    "ifc previewed file=\"cli road left out\\.ifc\" [^\n]*\ncounts alignments=1 [^\n]*\nclass name=IfcAlignment"
    FAIL_REGULAR_EXPRESSION "road elements=|class name=IfcRoad|class name=IfcCourse")

# One line sets several components - and the Road Template Manager's Apply
# sends one for its whole grid - so it is ONE undo step: the standard rural
# two-lane's 3.5 m lanes (Austroads) are 3.25 m and 3.75 m, and a single UNDO
# puts both back to 3.5.
add_test(NAME cli.several_template_components_are_set_by_one_line_and_one_undo_step
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "ROADTEMPLATE DEFAULTS"
            -c "ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 right:lane width=3.75"
            -c "UNDO" -c "ROADTEMPLATE INFO \"rural two-lane\"")
set_tests_properties(cli.several_template_components_are_set_by_one_line_and_one_undo_step
    PROPERTIES
    PASS_REGULAR_EXPRESSION
    "side=left name=lane kind=lane width=3[.]25.*side=right name=lane kind=lane width=3[.]75.*1 undone.*side=left name=lane kind=lane width=3[.]5 .*side=right name=lane kind=lane width=3[.]5 "
    FAIL_REGULAR_EXPRESSION "error")

# A value the template refuses in any group refuses the line whole; the script
# stops there.
add_test(NAME cli.a_refused_template_component_value_stops_the_script
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "ROADTEMPLATE DEFAULTS"
            -c "ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 right:lane width=0"
            -c "ROADTEMPLATE INFO NEVER REACHED")
set_tests_properties(cli.a_refused_template_component_value_stops_the_script PROPERTIES
    PASS_REGULAR_EXPRESSION "the width must be positive"
    FAIL_REGULAR_EXPRESSION "NEVER REACHED")

# The same for courses: a lane's 50 mm AC14 and 200 mm base become 60 mm and
# 300 mm by one line, and one UNDO puts both back.
add_test(NAME cli.several_template_courses_are_set_by_one_line_and_one_undo_step
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "ROADTEMPLATE DEFAULTS"
            -c "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 base thickness=0.3"
            -c "UNDO" -c "ROADTEMPLATE INFO \"rural two-lane\"")
set_tests_properties(cli.several_template_courses_are_set_by_one_line_and_one_undo_step PROPERTIES
    PASS_REGULAR_EXPRESSION
    "component=left:lane name=AC14 thickness=0[.]06.*component=left:lane name=base thickness=0[.]3 .*1 undone.*component=left:lane name=AC14 thickness=0[.]05 .*component=left:lane name=base thickness=0[.]2 "

# Removing a name that both sides have, from a template whose right side was made
# first (stored right before left), takes both and does not crash: the indices
# were erased in the wrong order and the second read past the end.
add_test(NAME cli.removing_a_name_from_both_sides_of_a_template_made_right_side_first
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "ROADTEMPLATE NEW t" -c "ROADTEMPLATE ADD t right lane"
            -c "ROADTEMPLATE ADD t left lane" -c "ROADTEMPLATE REMOVE t lane"
            -c "ROADTEMPLATE INFO t")
set_tests_properties(cli.removing_a_name_from_both_sides_of_a_template_made_right_side_first
    PROPERTIES
    PASS_REGULAR_EXPRESSION "side=left name=lane removed.*side=right name=lane removed.*template name=t left=0 right=0 courses=0"
    FAIL_REGULAR_EXPRESSION "error")

# A bench every tenth of a millimetre is refused at once, naming the batter, and
# the script stops there; it took minutes and gigabytes to build.
add_test(NAME cli.a_bench_too_small_to_build_is_refused_at_once
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "ROADTEMPLATE DEFAULTS"
            -c "ROADTEMPLATE SET \"rural two-lane\" batter benchheight=0.0001 benchwidth=0.0001"
            -c "ROADMODEL NEVER REACHED")
set_tests_properties(cli.a_bench_too_small_to_build_is_refused_at_once PROPERTIES
    PASS_REGULAR_EXPRESSION "left:batter: a bench of 1e-04 m every 1e-04 m of rise would make more than 100 benches"
    FAIL_REGULAR_EXPRESSION "NEVER REACHED")

# The divided road's median kerb falls to the carriageway: its lip is at 2.3 m and
# 100 mm under the median, and the lanes start there (-0.205 after the first).
add_test(NAME cli.the_divided_roads_lanes_start_under_its_median
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "ROADTEMPLATE DEFAULTS"
            -c "ROADTEMPLATE SECTION \"divided road\"")
set_tests_properties(cli.the_divided_roads_lanes_start_under_its_median PROPERTIES
    PASS_REGULAR_EXPRESSION "point code=left/kerb/face offset=2[.]150 level=0[.]000.*point code=left/kerb/lip offset=2[.]300 level=-0[.]100.*point code=left/lane1/outer offset=5[.]800 level=-0[.]205"
    FAIL_REGULAR_EXPRESSION "error")

# Renaming a corridor carries the strings it drew, so its next STRINGS replaces
# them; a second set is not drawn beside them.
add_test(NAME cli.a_renamed_corridor_replaces_its_strings_instead_of_doubling_them
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_setup} -c "CORRIDOR STRINGS Main"
            -c "CORRIDOR RENAME Main Road1" -c "CORRIDOR STRINGS Road1")
set_tests_properties(cli.a_renamed_corridor_replaces_its_strings_instead_of_doubling_them
    PROPERTIES
    PASS_REGULAR_EXPRESSION "strings created=15 replaced=0 corridor=Main.*corridor renamed from=Main to=Road1 strings=15.*strings created=15 replaced=15 corridor=Road1"
    FAIL_REGULAR_EXPRESSION "error")

# A section of a corridor outside its stations is refused, naming them; and a
# corridor that could not be built says it has no range, not station 0 to 0.
add_test(NAME cli.a_section_outside_the_corridors_stations_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_setup} -c "CORRIDOR SECTION Main 150")
set_tests_properties(cli.a_section_outside_the_corridors_stations_is_refused PROPERTIES
    PASS_REGULAR_EXPRESSION "station 150 is outside corridor Main's stations 0 to 100")
add_test(NAME cli.a_corridor_that_could_not_be_built_has_no_range
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_setup} -c "ALIGN DELETE road"
            -c "CORRIDOR BUILD Main")
set_tests_properties(cli.a_corridor_that_could_not_be_built_has_no_range PROPERTIES
    PASS_REGULAR_EXPRESSION "built corridor=Main from= to= runs=0 sections=0 incomplete=0 objects=0 cut= fill= net= problems=1"
    FAIL_REGULAR_EXPRESSION "error")

# CORRIDOR QUANTITIES csv= replaces a file only with OVERWRITE, as VOLUME's does:
# written once and refused the second time (the script stops there), then written
# again with the word.
set(_road_csv "${CMAKE_CURRENT_BINARY_DIR}/cli-road-quantities.csv")
add_test(NAME cli.road_csv_cleanup COMMAND ${CMAKE_COMMAND} -E rm -f "${_road_csv}")
add_test(NAME cli.quantities_csv_is_replaced_only_with_overwrite
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_setup}
            -c "CORRIDOR QUANTITIES Main csv=\"${_road_csv}\""
            -c "CORRIDOR QUANTITIES Main csv=\"${_road_csv}\"")
add_test(NAME cli.quantities_csv_is_replaced_when_overwrite_is_given
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_road_setup}
            -c "CORRIDOR QUANTITIES Main csv=\"${_road_csv}\" OVERWRITE")
set_tests_properties(cli.road_csv_cleanup PROPERTIES FIXTURES_SETUP cli_road_csv)
set_tests_properties(cli.quantities_csv_is_replaced_only_with_overwrite PROPERTIES
    FIXTURES_REQUIRED cli_road_csv FIXTURES_SETUP cli_road_csv_written
    PASS_REGULAR_EXPRESSION "csv file=[^\r\n]*cli-road-quantities[.]csv rows=20.*AlreadyExists: the file is already there; OVERWRITE replaces it")
set_tests_properties(cli.quantities_csv_is_replaced_when_overwrite_is_given PROPERTIES
    FIXTURES_REQUIRED cli_road_csv_written
    PASS_REGULAR_EXPRESSION "csv file=[^\r\n]*cli-road-quantities[.]csv rows=20"
    FAIL_REGULAR_EXPRESSION "error")
