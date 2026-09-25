# The headless front ends. katana_app is the session they share - a Document,
# its CommandInterpreter and the verbs above katana_cad (session.hpp) - and the
# MCP server over it (mcp_server.hpp), so that a line sent by Claude and a line
# typed at katana_cli run the same code.
add_library(katana_app STATIC
    session.cpp session.hpp
    dxf_verbs.cpp dxf_verbs.hpp
    mcp_server.cpp mcp_server.hpp
)
katana_target_defaults(katana_app)
target_include_directories(katana_app PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(katana_app PUBLIC katana_cad PRIVATE katana_archive12d katana_dxf)
find_package(nlohmann_json CONFIG REQUIRED)
target_link_libraries(katana_app PRIVATE nlohmann_json::nlohmann_json)

# Interoperability is optional: with -DKATANA_BUILD_IO=OFF the CLI still builds
# and simply has no IMPORT/EXPORT verbs, which is what the sanitizer job needs.
if(TARGET katana_interop)
    target_link_libraries(katana_app PRIVATE katana_interop)
    target_compile_definitions(katana_app PRIVATE KATANA_WITH_INTEROP=1)
endif()

# Headless command-line front end. Proves the engine is complete without a GUI.
add_executable(katana_cli main.cpp)
katana_target_defaults(katana_cli)
target_link_libraries(katana_cli PRIVATE katana_app)

# The same session served to Claude over the Model Context Protocol (docs/mcp.md).
add_executable(katana_mcp mcp_main.cpp)
katana_target_defaults(katana_mcp)
target_link_libraries(katana_mcp PRIVATE katana_app)
target_compile_definitions(katana_mcp PRIVATE KATANA_VERSION="${PROJECT_VERSION}")

if(KATANA_BUILD_TESTS)
    # End-to-end: draw, save, reopen in a second process, and query.
    set(_project "${CMAKE_CURRENT_BINARY_DIR}/cli-smoke.katana")
    add_test(NAME cli.save
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "LAYER NEW Survey #FF0000" -c "LAYER SET Survey"
                -c "RECT 0,0 30,20" -c "CIRCLE 15,10 5" -c "SAVE \"${_project}\"")
    add_test(NAME cli.reopen
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "OPEN \"${_project}\"" -c "LIST")
    add_test(NAME cli.rejects_bad_input
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "CIRCLE 0,0 -1")
    add_test(NAME cli.cleanup COMMAND ${CMAKE_COMMAND} -E rm -rf "${_project}")

    set_tests_properties(cli.cleanup PROPERTIES FIXTURES_SETUP cli_project)
    set_tests_properties(cli.save PROPERTIES FIXTURES_REQUIRED cli_project FIXTURES_SETUP cli_saved)
    set_tests_properties(cli.reopen PROPERTIES
        FIXTURES_REQUIRED cli_saved
        PASS_REGULAR_EXPRESSION "2 entities.*Polyline.*layer=Survey.*area=600")
    set_tests_properties(cli.rejects_bad_input PROPERTIES WILL_FAIL TRUE)

    # The survey-code verbs, on a mapfile of the tests' own: its shape is the
    # reference mapfile's, its codes are not. REPLACE puts it in place of the
    # compiled-in map, so what is expected does not depend on whether this
    # checkout has the reference customisation. By index: 0 KT* map_data,
    # 1 KX* map_data with no model, 2 KT* a built-in symbol, "cross".
    set(_mapfile "${CMAKE_CURRENT_BINARY_DIR}/cli-codes.mapfile")
    file(WRITE "${_mapfile}" [=[<?xml version="1.0"?>
<xml12d xmlns="http://www.12d.com/schema/xml12d-10.0">
  <map_file>
    <version>11.0</version>
    <map_data>
      <item><key>KT*</key><model>KATANA TEST</model><colour>red</colour><linestyle>0</linestyle><comment>[KT*] Katana test</comment></item>
      <item><key>KX*</key><colour>red</colour></item>
    </map_data>
    <vertex_symbol_data>
      <item><key>KT*</key><symbol_data><style>cross</style><size>2</size></symbol_data></item>
    </vertex_symbol_data>
  </map_file>
</xml12d>
]=])
    set(_replace "CUSTOMISE REPLACE \"${_mapfile}\"")
    add_test(NAME cli.code_explain
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_replace}" -c "CODE EXPLAIN KT01"
                -c "CODE EXPLAIN kt01")
    add_test(NAME cli.mapfile_list
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_replace}" -c "MAPFILE LIST katana")
    # Loaded again WITHOUT replace: merged, so each of its (code, section)
    # groups - KT* map_data, KX* map_data, KT* vertex_symbol_data - replaces
    # itself. A code's loaded rules are gathered at its first rule (so that
    # what the load brought outranks what it left of that code), which makes
    # KX* rule #2: KT* map_data, KT* vertex_symbol_data, KX* map_data.
    add_test(NAME cli.mapfile_check_after_a_merge
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_replace}" -c "CUSTOMISE \"${_mapfile}\""
                -c "MAPFILE CHECK")
    add_test(NAME cli.code_applies_and_reports
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_replace}" -c "POINT 1,1" -c "SELECT ALL"
                -c "PROP SET code KT01" -c "CODE" -c "CODE CENSUS")
    # A QUOTED path whose first word is "replace" is a file, not the REPLACE
    # keyword: taken for the keyword, the file was dropped ("REPLACE needs a
    # file") or, given twice, replaced the loaded map instead of merging. Run
    # beside the file, so that the path's first word really is "replace". By
    # hand: cli-codes.mapfile has 3 rules over 2 keys (KT*, KX*) and this
    # file 1 rule over RM*, so a merge holds 4 rules over 3 keys, and loading
    # the same file twice in one load adds its rule once.
    file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/replace me.mapfile" [=[<?xml version="1.0"?>
<xml12d xmlns="http://www.12d.com/schema/xml12d-10.0">
  <map_file>
    <version>11.0</version>
    <map_data>
      <item><key>RM*</key><model>REPLACE ME</model></item>
    </map_data>
  </map_file>
</xml12d>
]=])
    add_test(NAME cli.a_quoted_path_starting_with_replace_is_loaded_as_a_file
        WORKING_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}"
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "customise replace \"${_mapfile}\""
                -c "CUSTOMISE \"replace me.mapfile\"" -c "CUSTOMISE")
    add_test(NAME cli.the_same_quoted_replace_path_twice_merges_rather_than_replaces
        WORKING_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}"
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_replace}"
                -c "CUSTOMISE \"replace me.mapfile\" \"replace me.mapfile\"" -c "CUSTOMISE")
    set_tests_properties(cli.a_quoted_path_starting_with_replace_is_loaded_as_a_file
        cli.the_same_quoted_replace_path_twice_merges_rather_than_replaces
        PROPERTIES PASS_REGULAR_EXPRESSION "4 survey code rules over 3 distinct codes")
    # A library-only load keeps the map (audit QT-21): the symbol fixture is
    # merged in after the tests' mapfile, and the mapfile's 3 rules over 2
    # codes are still there. By hand, from cli-codes.mapfile above.
    set(_fixture "${PROJECT_SOURCE_DIR}/tests/archive12d/data/customisation")
    add_test(NAME cli.a_library_alone_merged_in_keeps_the_survey_codes
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_replace}"
                -c "CUSTOMISE \"${_fixture}/test_symbols.4d\"" -c "CUSTOMISE")
    set_tests_properties(cli.a_library_alone_merged_in_keeps_the_survey_codes PROPERTIES
        PASS_REGULAR_EXPRESSION "test_symbols\\.4d: style library, [0-9]+ added.*3 survey code rules over 2 distinct codes")

    # A saved project records the customisation files it was drawn with, and
    # a later session that opens it without one of them is told which (a
    # warning; the project still opens). Saved with the symbol fixture merged
    # in, then opened in a second process that never loads it. With the
    # fixture loaded first, nothing is missing.
    set(_recorded "${CMAKE_CURRENT_BINARY_DIR}/cli-recorded.katana")
    add_test(NAME cli.recorded_cleanup COMMAND ${CMAKE_COMMAND} -E rm -rf "${_recorded}")
    add_test(NAME cli.a_saved_project_records_its_customisation
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "CUSTOMISE \"${_fixture}/test_symbols.4d\""
                -c "LINE 0,0 10,0" -c "SAVE \"${_recorded}\"")
    add_test(NAME cli.opening_a_project_names_the_customisation_files_not_loaded
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "OPEN \"${_recorded}\"")
    add_test(NAME cli.opening_a_project_with_its_customisation_loaded_warns_of_nothing
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "CUSTOMISE \"${_fixture}/test_symbols.4d\""
                -c "OPEN \"${_recorded}\"")
    set_tests_properties(cli.recorded_cleanup PROPERTIES FIXTURES_SETUP cli_recorded_clean)
    set_tests_properties(cli.a_saved_project_records_its_customisation PROPERTIES
        FIXTURES_REQUIRED cli_recorded_clean FIXTURES_SETUP cli_recorded)
    set_tests_properties(cli.opening_a_project_names_the_customisation_files_not_loaded PROPERTIES
        FIXTURES_REQUIRED cli_recorded
        PASS_REGULAR_EXPRESSION "opened .*warning: this project was drawn with customisation files that are not loaded: \"test_symbols\\.4d\"[\r\n]")
    set_tests_properties(cli.opening_a_project_with_its_customisation_loaded_warns_of_nothing PROPERTIES
        FIXTURES_REQUIRED cli_recorded
        PASS_REGULAR_EXPRESSION "opened "
        FAIL_REGULAR_EXPRESSION "not loaded")

    # Three sessions on one project of their own. The first saves it with the
    # symbol fixture loaded; the second opens it without the fixture (warned)
    # and saves it unedited; the third, again without it, must still be told:
    # a session that never had the file cannot judge it, so its save keeps
    # the name. The second session rewrote the record from what it had
    # loaded, and the third opened in silence.
    set(_resaved "${CMAKE_CURRENT_BINARY_DIR}/cli-resaved.katana")
    add_test(NAME cli.resaved_cleanup COMMAND ${CMAKE_COMMAND} -E rm -rf "${_resaved}")
    add_test(NAME cli.a_project_is_saved_with_a_symbol_file_loaded
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "CUSTOMISE \"${_fixture}/test_symbols.4d\""
                -c "LINE 0,0 10,0" -c "SAVE \"${_resaved}\"")
    add_test(NAME cli.an_unedited_save_where_a_file_is_missing_succeeds
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "OPEN \"${_resaved}\"" -c "SAVE")
    add_test(NAME cli.a_file_missing_when_the_project_was_saved_is_still_named_on_opening
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "OPEN \"${_resaved}\"")
    set_tests_properties(cli.resaved_cleanup PROPERTIES FIXTURES_SETUP cli_resaved_clean)
    set_tests_properties(cli.a_project_is_saved_with_a_symbol_file_loaded PROPERTIES
        FIXTURES_REQUIRED cli_resaved_clean FIXTURES_SETUP cli_resaved_first)
    set_tests_properties(cli.an_unedited_save_where_a_file_is_missing_succeeds PROPERTIES
        FIXTURES_REQUIRED cli_resaved_first FIXTURES_SETUP cli_resaved_second
        PASS_REGULAR_EXPRESSION "saved[\r\n]")
    set_tests_properties(cli.a_file_missing_when_the_project_was_saved_is_still_named_on_opening
        PROPERTIES FIXTURES_REQUIRED cli_resaved_second
        PASS_REGULAR_EXPRESSION "not loaded: \"test_symbols\\.4d\"[\r\n]")

    # "[\r\n]+" where a line ends: the console writes CRLF on Windows.
    set(_eol "[\r\n]+")
    set_tests_properties(cli.code_explain PROPERTIES PASS_REGULAR_EXPRESSION
        "Code \"KT01\": prefix match, matched${_eol}  model: KATANA TEST  <- rule #0 KT\\* \\(map_data\\).*  symbol: cross, size 2  <- rule #2 KT\\* \\(vertex_symbol_data\\)${_eol}  linestyle \"0\": a plain line${_eol}  symbol \"cross\": drawn by Katana itself${_eol}  colour \"red\": #FF0000${_eol}Code \"kt01\": no rule matches it${_eol}  near miss: key \"KT\\*\" differs only in letter case")
    set_tests_properties(cli.mapfile_list PROPERTIES PASS_REGULAR_EXPRESSION
        "KT\\*  \\[prefix; 2 rules: map_data, vertex_symbol_data\\]  model KATANA TEST; linestyle 0; symbol cross; colour red; \"\\[KT\\*\\] Katana test\"${_eol}1 of 2 codes match \"katana\"")
    # The file's kind is said in the help's words: a survey code file, not a
    # style library.
    set_tests_properties(cli.mapfile_check_after_a_merge PROPERTIES PASS_REGULAR_EXPRESSION
        "cli-codes\\.mapfile: survey code file, 0 added, 3 replaced \\(codes, once for each section\\): \"KT\\*\", \"KX\\*\", \"KT\\*\".*3 rules checked: 0 errors, 1 warning${_eol}  by kind: 1 no model${_eol}  rule #2 KX\\* \\(map_data\\): warning, no model")
    set_tests_properties(cli.code_applies_and_reports PROPERTIES PASS_REGULAR_EXPRESSION
        "1 entity carries a code in \"code\": 1 matched, 0 fallback-only.*Layers created: KATANA TEST${_eol}Styles created: cross${_eol}.*  KT01: 1 entity, prefix, matched; layer 0 -> KATANA TEST; style cross \\(created\\); 1 changed${_eol}Applied as one command.*1 entity carries a code in \"code\", 1 distinct code${_eol}  KT01: 1, prefix, matched, model KATANA TEST")

    # CUSTOMISE alone names the files loaded, in load order - "is my code
    # file loaded?" is what a count could not answer - after the counts. The
    # built-in libraries may come first where this checkout has them, so the
    # line after the heading is not pinned.
    add_test(NAME cli.customise_alone_names_the_files_loaded
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_replace}" -c "CUSTOMISE")
    set_tests_properties(cli.customise_alone_names_the_files_loaded PROPERTIES
        PASS_REGULAR_EXPRESSION
        "3 survey code rules over 2 distinct codes${_eol}Loaded files, in load order:${_eol}.*  \"cli-codes\\.mapfile\", a survey code file")
    # INFO with an entity's id describes the entity, as the interpreter's
    # INFO does, in a build with the GIS module too: there every INFO was
    # once read as INFO <file>. A 10 x 5 rectangle: perimeter 30, area 50.
    add_test(NAME cli.info_with_an_id_describes_the_entity
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "RECT 0,0 10,5" -c "INFO 1")
    set_tests_properties(cli.info_with_an_id_describes_the_entity PROPERTIES
        PASS_REGULAR_EXPRESSION "1  Polyline  layer=0  vertices=4  closed  length=30  area=50"
        FAIL_REGULAR_EXPRESSION "error")

    # STATUS and STATUS JSON: the record katana_mcp's katana_status gives,
    # here as a verb (cad/document_status.hpp). One rectangle, selected, one
    # undo step.
    add_test(NAME cli.status_reports_the_drawing_in_text_and_json
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "RECT 0,0 10,5" -c "SELECT ALL" -c STATUS
                -c "STATUS JSON")
    set_tests_properties(cli.status_reports_the_drawing_in_text_and_json PROPERTIES
        PASS_REGULAR_EXPRESSION
        "Entities: 1  Layers: 1  Alignments: 0${_eol}Current layer: 0${_eol}Selected: 1  Undo steps: 1  Redo steps: 0${_eol}.*  \"entities\": 1,${_eol}.*  \"selected\": 1,${_eol}.*  \"undoSteps\": 1${_eol}}"
        FAIL_REGULAR_EXPRESSION "error")

    # The help names the customisation's files by what they are, in the words
    # the window uses, and no product: what STYLE's linetype takes and what
    # CUSTOMISE loads.
    add_test(NAME cli.the_help_names_the_customisation_files_by_what_they_are
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> --help)
    set_tests_properties(cli.the_help_names_the_customisation_files_by_what_they_are PROPERTIES
        PASS_REGULAR_EXPRESSION "a loaded library linestyle or ByLayer.*CUSTOMISE \\[REPLACE\\] <file> \\[<file>\\.\\.\\.\\]  load style${_eol} +libraries \\(\\.4d\\) and survey code files \\(\\.mapfile\\)"
        FAIL_REGULAR_EXPRESSION "12d (customisation|linestyle|symbol|mapfile|librar)")
endif()

# The sheet verbs with no window (docs/plotting.md, "Sheets on the command
# line"): a drawing laid out on a sheet, renamed, the title block filled and
# the set listed, one fact per line; a refused verb stops the script.
if(KATANA_BUILD_TESTS)
    add_test(NAME cli.sheet_verbs_lay_out_edit_and_list_the_sheets
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "LINE 0,0 300,100" -c "GENERATE fit"
                -c "SHEET RENAME 1 SITE PLAN" -c "TITLEBLOCK organisation \"Example Surveys\""
                -c "SHEETS")
    add_test(NAME cli.a_refused_sheet_verb_stops_the_script
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SHEET NEW PLAN" -c "SHEET REMOVE 2"
                -c "SHEET NEW NEVER REACHED")
    set(_eol "[\r\n]+")
    set_tests_properties(cli.sheet_verbs_lay_out_edit_and_list_the_sheets PROPERTIES
        PASS_REGULAR_EXPRESSION
        "generated 1 sheet: 1.*renamed sheet 1 id=s1 name=\"SITE PLAN\".*organisation=\"Example Surveys\"${_eol}1 sheet${_eol}sheet 1 id=s1 name=\"SITE PLAN\" paper=A3 orientation=landscape frame=a3_landscape legendblock=on views=1${_eol}  view id=vp1 kind=plan scale=1000 rect=")
    set_tests_properties(cli.a_refused_sheet_verb_stops_the_script PROPERTIES
        PASS_REGULAR_EXPRESSION "no sheet 2: the set has 1 sheet"
        FAIL_REGULAR_EXPRESSION "NEVER REACHED")
    # A pass expression makes ctest ignore the exit code, so the exit code of
    # the same refusal is a test of its own: the script must fail.
    add_test(NAME cli.a_refused_sheet_verb_fails_the_script
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SHEET NEW PLAN" -c "SHEET REMOVE 2")
    set_tests_properties(cli.a_refused_sheet_verb_fails_the_script PROPERTIES WILL_FAIL TRUE)
endif()

# UTILITY on the sample investigation (samples/utilities, worked by hand in
# docs/subsurface_utilities.md). E1-3 is a radar pick at 350 mm claimed as
# QL-B: the report must say so. The pothole W1-X1 checks the detection W1-3:
# hypot(0.080, 0.060) = 0.100 in plan; top 19.155 against the detection's
# centre 19.180 + 0.075 = 19.255, so -0.100. The stormwater design crosses
# W1's W1-2 -> W1-3, a QL-B stretch, clear by 0.372 m vertically at the
# drawn levels but not across QL-B's 500 mm - within tolerance, not clear.
if(KATANA_BUILD_TESTS)
    set(_utilities "${PROJECT_SOURCE_DIR}/samples/utilities")
    add_test(NAME cli.utility_report_grades_the_sample_and_names_the_over_claim
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY REPORT \"${_utilities}/schedule.csv\" MINCOVER 0.6")
    add_test(NAME cli.utility_verify_compares_the_pothole_with_its_detection
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY VERIFY \"${_utilities}/schedule.csv\"")
    add_test(NAME cli.utility_clearance_shows_the_crossing_within_tolerance
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY CLEARANCE \"${_utilities}/schedule.csv\" \"${_utilities}/design.csv\" WIDTH 0.375")
    add_test(NAME cli.utility_refuses_an_unknown_option
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY REPORT \"${_utilities}/schedule.csv\" MINCOVR 0.6")
    set_tests_properties(cli.utility_report_grades_the_sample_and_names_the_over_claim PROPERTIES
        PASS_REGULAR_EXPRESSION "4 lines, 14 vertices.*E1 / E1-3: claimed QL-B, the ground penetrating radar evidence supports QL-C")
    set_tests_properties(cli.utility_verify_compares_the_pothole_with_its_detection PROPERTIES
        PASS_REGULAR_EXPRESSION "W1 +W1-3 +W1-X1 +0\\.100 +ok +-0\\.100 +ok")
    set_tests_properties(cli.utility_clearance_shows_the_crossing_within_tolerance PROPERTIES
        PASS_REGULAR_EXPRESSION "W1 W1-2 -> W1-3 +QL-B +within tolerance +-0\\.263 +0\\.300 +0\\.372")
    set_tests_properties(cli.utility_refuses_an_unknown_option PROPERTIES WILL_FAIL TRUE)

    # UTILITY DRAW on the same sample: W1 is three runs (QL-B, the QL-A
    # trench between the potholes, the QL-C stretch past the detected
    # spacing), E1 two, T1 and G1 one each, and a point per vertex - 21
    # entities on 11 layers, the first record carrying the bounds a window
    # frames.
    add_test(NAME cli.utility_draw_adds_the_graded_sample_to_the_drawing
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY DRAW \"${_utilities}/schedule.csv\"" -c "LAYER LIST")
    set_tests_properties(cli.utility_draw_adds_the_graded_sample_to_the_drawing PROPERTIES
        PASS_REGULAR_EXPRESSION
        "utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 bounds=334000\\.000,6250000\\.000,334040\\.000,6250007\\.200${_eol}line id=W1 type=water length=30\\.024 ql_a=1\\.420 ql_b=16\\.102 ql_c=12\\.502 ql_d=0\\.000.*utilities/water/QL-A  #2F80ED  \\(1 entities\\)${_eol}  utilities/water/QL-B  #2F80ED  \\(1 entities\\)${_eol}  utilities/water/QL-C  #2F80ED  \\(1 entities\\)${_eol}  utilities/water/points  #2F80ED  \\(6 entities\\)")

    # A service of one located vertex cannot be graded, and refuses the whole
    # draw by its id: nothing is drawn and the script stops, failing.
    set(_one_vertex "${CMAKE_CURRENT_BINARY_DIR}/cli utility one vertex.csv")
    file(WRITE "${_one_vertex}"
        "line,point,easting,northing,method,h_unc,type\n"
        "W1,W1-1,0,0,EML,0.1,water\n"
        "W1,W1-2,10,0,EML,0.1,\n"
        "W9,W9-1,5,5,EML,0.1,water\n")
    add_test(NAME cli.utility_draw_refuses_a_line_it_cannot_grade
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "UTILITY DRAW \"${_one_vertex}\"" -c "LAYER LIST")
    set_tests_properties(cli.utility_draw_refuses_a_line_it_cannot_grade PROPERTIES
        PASS_REGULAR_EXPRESSION
        "line W9 cannot be graded: a utility line needs at least two vertices; nothing was drawn"
        FAIL_REGULAR_EXPRESSION "utilities drawn|utilities/water")
    add_test(NAME cli.utility_a_refused_draw_fails_the_script
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "UTILITY DRAW \"${_one_vertex}\"")
    set_tests_properties(cli.utility_a_refused_draw_fails_the_script PROPERTIES WILL_FAIL TRUE)

    # UTILITY CHECK against a schema written by hand, as any client's can be
    # (delivery_schema.hpp): the sample gives each service's material once,
    # so the rows after the first are empty - 11 errors. The whole check is
    # shown, and the script fails, so a delivery can be gated on it. The
    # refusal's one line goes to stderr and the check to stdout, which ctest
    # merges in no fixed order, so the check is matched on its own; which
    # stream takes what is pinned in tests/app/test_session.cpp, by
    # Session.ARefusalThatCarriesAReportPrintsTheReportWhereAReportGoes.
    set(_example_schema "${CMAKE_CURRENT_BINARY_DIR}/cli-utility-schema.csv")
    file(WRITE "${_example_schema}"
        "kind,attribute,value,detail,label\n"
        "schema,Example Services Schema,1,,\n"
        "identifier,line,,,\n"
        "field,line,Alphanumerical,Yes,Line\n"
        "field,material,Free Text,Yes,Material\n")
    add_test(NAME cli.utility_check_with_errors_shows_the_whole_check
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY CHECK \"${_utilities}/schedule.csv\" SCHEMA \"${_example_schema}\"")
    set_tests_properties(cli.utility_check_with_errors_shows_the_whole_check PROPERTIES
        PASS_REGULAR_EXPRESSION
        "Delivery schema check against Example Services Schema v1${_eol}14 rows, 4 assets \\(line\\): 11 errors, 0 warnings.*material is empty \\(11 rows")
    add_test(NAME cli.utility_check_with_errors_fails_the_script
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY CHECK \"${_utilities}/schedule.csv\" SCHEMA \"${_example_schema}\"")
    set_tests_properties(cli.utility_check_with_errors_fails_the_script PROPERTIES WILL_FAIL TRUE)

    # UTILITY CHECK against the real TfNSW Utility Schema, when this checkout
    # has the owner's copy (it is theirs to keep, not the repository's - see
    # .gitignore). tools/utility_schema_domains.py makes the schema file, then
    # the TfNSW-shaped sample is checked; its comments list the mistakes left
    # in it on purpose, and each must be found. PASS_REGULAR_EXPRESSION
    # ignores the exit status, which is 1 because there are errors.
    set(_tfnsw_workbook
        "${PROJECT_SOURCE_DIR}/docs/TfNSW Reference Files/Utility-Schema-and-Specification-v1.2.xlsx")
    if(EXISTS "${_tfnsw_workbook}" AND Python3_Interpreter_FOUND)
        set(_tfnsw_schema "${CMAKE_CURRENT_BINARY_DIR}/tfnsw-utility-schema.csv")
        add_test(NAME cli.utility_check_schema_from_the_workbook
            COMMAND "${KATANA_PYTHON}" "${PROJECT_SOURCE_DIR}/tools/utility_schema_domains.py"
                    "${_tfnsw_workbook}" "${_tfnsw_schema}")
        add_test(NAME cli.utility_check_finds_what_the_tfnsw_sample_gets_wrong
            COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                    $<TARGET_FILE:katana_cli>
                    -c "UTILITY CHECK \"${_utilities}/schedule_tfnsw.csv\" SCHEMA \"${_tfnsw_schema}\"")
        set_tests_properties(cli.utility_check_schema_from_the_workbook PROPERTIES
            FIXTURES_SETUP cli_tfnsw_schema
            PASS_REGULAR_EXPRESSION "43 fields, 25 domains, 1084 values \\(schema version 1\\.2\\)")
        set_tests_properties(cli.utility_check_finds_what_the_tfnsw_sample_gets_wrong PROPERTIES
            FIXTURES_REQUIRED cli_tfnsw_schema
            PASS_REGULAR_EXPRESSION "7 rows, 3 assets \\(AssetIdentifier\\): 7 errors, 2 warnings.*TfNSW_ContractOrgCode is mandatory and has no column.*DateInfoObtained \"2026-09-20\" is not a date.*Depth is empty \\(2 rows.*UtilityInstallDate is empty \\(2 rows.*AssetStatus \"In service\" is listed as \"In Service\"")
    endif()
endif()

# A .dxf through the command line both ways, natively - no GDAL, so it runs
# in a build without it. Drawn by hand: a 30 x 20 rectangle (perimeter 100,
# area 600), a circle of radius 5 at (15,10) and a text, on a nested layer;
# exported, a new drawing, imported back. Three layers go out: 0, Survey and
# Survey/Kerb - the last as "Survey$Kerb" with its path beside it.
if(KATANA_BUILD_TESTS)
    set(_dxf "${CMAKE_CURRENT_BINARY_DIR}/cli round trip.dxf")
    add_test(NAME cli.a_drawing_exported_to_dxf_and_imported_back_keeps_its_curves_text_and_layer
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "LAYER NEW Survey/Kerb #FF0000"
                -c "LAYER SET Survey/Kerb" -c "RECT 0,0 30,20" -c "CIRCLE 15,10 5"
                -c "TEXT 5,5 2.5 \"LOT 42\"" -c "EXPORT \"${_dxf}\"" -c "NEW"
                -c "IMPORT \"${_dxf}\"" -c "LIST")
    set_tests_properties(cli.a_drawing_exported_to_dxf_and_imported_back_keeps_its_curves_text_and_layer
        PROPERTIES PASS_REGULAR_EXPRESSION
        "exported 3 entities and 3 layers \\(DXF R2000.*imported 3 entities from cli round trip\\.dxf \\(DXF R2000\\).*Polyline  layer=Survey/Kerb  vertices=4  closed  length=100  area=600.*Circle  layer=Survey/Kerb  centre 15,10  radius=5 .*Text  layer=Survey/Kerb  \"LOT 42\"")
endif()

# INFO, COPC and IMPORT ... LOCAL, on the sample GIS data. Expected values are
# the files' own: terrain.asc's header says 120 x 90 cells of 1.5 from a
# lower-left corner at (-5, -5), so it spans to (175, 130); `pdal info`
# counts 40 000 points in survey_scan.las.
if(KATANA_BUILD_TESTS AND TARGET katana_interop)
    set(_gis "${PROJECT_SOURCE_DIR}/samples/gis")
    set(_copc "${CMAKE_CURRENT_BINARY_DIR}/cli-scan.copc.laz")
    add_test(NAME cli.info_raster
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "INFO ${_gis}/terrain.asc")
    add_test(NAME cli.info_point_cloud
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "INFO ${_gis}/survey_scan.las")
    # Converted, then described: the description must now say COPC, with
    # every point kept.
    add_test(NAME cli.copc_cleanup COMMAND ${CMAKE_COMMAND} -E rm -f "${_copc}")
    add_test(NAME cli.copc_then_info
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "COPC ${_gis}/survey_scan.las ${_copc}"
                -c "INFO ${_copc}")
    # Audit QT-13/14: the raster's LOCAL is refused by name, where it used to
    # be left on the path and reported as a missing file "terrain.asc LOCAL".
    add_test(NAME cli.local_is_refused_for_reference_data
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "IMPORT ${_gis}/terrain.asc LOCAL")
    set_tests_properties(cli.info_raster PROPERTIES PASS_REGULAR_EXPRESSION
        "Raster: 120 x 90 pixels, 1 band.*Pixel size: 1\\.5 x 1\\.5.*Bounds: \\(-5\\.000, -5\\.000\\) to \\(175\\.000, 130\\.000\\)")
    set_tests_properties(cli.info_point_cloud PROPERTIES PASS_REGULAR_EXPRESSION
        "Point cloud: 40,000 points.*COPC: no")
    set_tests_properties(cli.copc_cleanup PROPERTIES FIXTURES_SETUP cli_copc)
    set_tests_properties(cli.copc_then_info PROPERTIES FIXTURES_REQUIRED cli_copc
        PASS_REGULAR_EXPRESSION "converted survey_scan\\.las.*Point cloud: 40,000 points.*COPC: yes")
    set_tests_properties(cli.local_is_refused_for_reference_data PROPERTIES
        PASS_REGULAR_EXPRESSION "LOCAL is not supported for rasters and point clouds")
    # A QUOTED path and LOCAL, as katana_import sends them: LOCAL was taken off
    # and the quotes left on, so the file was looked for by its quoted name.
    # parcels.geojson spans (180, 0) to (365, 165) (its coordinates, by hand),
    # so moved to put that lower-left corner at 0,0 it spans (0, 0) to
    # (185, 165).
    add_test(NAME cli.import_local_of_a_quoted_path_moves_the_data_to_the_origin
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "IMPORT \"${_gis}/parcels.geojson\" LOCAL")
    set_tests_properties(cli.import_local_of_a_quoted_path_moves_the_data_to_the_origin PROPERTIES
        PASS_REGULAR_EXPRESSION "imported [0-9]+ entities from parcels\\.geojson.*extent 0,0 to 185,165"
        FAIL_REGULAR_EXPRESSION "error")
endif()
