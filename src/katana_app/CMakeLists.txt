# The headless front ends. katana_app is the session they share - a Document,
# its CommandInterpreter and the verbs above katana_cad (session.hpp) - and the
# MCP server over it (mcp_server.hpp), so that a line sent by Claude and a line
# typed at katana_cli run the same code.
add_library(katana_app STATIC
    session.cpp session.hpp
    dxf_verbs.cpp dxf_verbs.hpp
    ifc_verbs.cpp ifc_verbs.hpp
    import_records.cpp import_records.hpp
    survey_verbs.cpp survey_verbs.hpp
    mcp_server.cpp mcp_server.hpp
)
katana_target_defaults(katana_app)
target_include_directories(katana_app PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(katana_app PUBLIC katana_cad PRIVATE katana_archive12d katana_dxf katana_ifc
                                                         katana_survey katana_surveyio)
# The version an exported file names as the program that wrote it.
set_source_files_properties(ifc_verbs.cpp PROPERTIES
    COMPILE_DEFINITIONS "KATANA_VERSION=\"${PROJECT_VERSION}\"")
find_package(nlohmann_json CONFIG REQUIRED)
target_link_libraries(katana_app PRIVATE nlohmann_json::nlohmann_json)

# Interoperability is optional: with -DKATANA_BUILD_IO=OFF the CLI still builds
# and simply has no IMPORT/EXPORT verbs, which is what the sanitizer job needs.
if(TARGET katana_interop)
    target_link_libraries(katana_app PRIVATE katana_interop)
    target_compile_definitions(katana_app PRIVATE KATANA_WITH_INTEROP=1)
    # The geoprocessing executor, the GDAL verb and the MCP geoprocessing
    # tools (docs/geoprocessing.md): GDAL's, so only with the interop module.
    # Globbed, one directory, so the geoprocessing lanes add their verb
    # modules without editing this list.
    file(GLOB _katana_app_geo CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/geo/*.cpp")
    target_sources(katana_app PRIVATE ${_katana_app_geo})
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

    # VERTEX INSERT with a point OF the polyline (docs/drawing.md, "What a
    # vertex tool acts on"): "#1@4,0.3" is the place on it nearest (4,0.3),
    # ON the line, as the window's Insert Vertex puts a click; "#1.s1" with
    # after=1 is segment 1's middle. By hand on (0,0) (10,0) (10,10): the
    # first is (4,0), the new vertex 1; segment 1 is (10,0)-(10,10), and its
    # middle (10,5) the new vertex 2.
    add_test(NAME cli.vertex_insert_takes_a_point_on_the_polyline
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "PLINE 0,0 10,0 10,10" -c "VERTEX INSERT 1 #1@4,0.3"
                -c "VERTEX LIST 1")
    set_tests_properties(cli.vertex_insert_takes_a_point_on_the_polyline PROPERTIES
        PASS_REGULAR_EXPRESSION "vertices=4 [^\r\n]* inserted=1.*index=1 x=4 y=0 z=none"
        FAIL_REGULAR_EXPRESSION "error")
    add_test(NAME cli.vertex_insert_takes_a_segments_middle
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "PLINE 0,0 10,0 10,10"
                -c "VERTEX INSERT 1 #1.s1 after=1" -c "VERTEX LIST 1")
    set_tests_properties(cli.vertex_insert_takes_a_segments_middle PROPERTIES
        PASS_REGULAR_EXPRESSION "vertices=4 [^\r\n]* inserted=2.*index=2 x=10 y=5 z=none"
        FAIL_REGULAR_EXPRESSION "error")
    # "#1@4,0.3,101.5": on the line at (4,0), with 101.5 its height - what
    # x,y,z typed at the window's Insert Vertex does.
    add_test(NAME cli.vertex_insert_on_the_line_takes_a_height
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "PLINE 0,0 10,0 10,10"
                -c "VERTEX INSERT 1 #1@4,0.3,101.5" -c "VERTEX LIST 1")
    set_tests_properties(cli.vertex_insert_on_the_line_takes_a_height PROPERTIES
        PASS_REGULAR_EXPRESSION "inserted=1.*index=1 x=4 y=0 z=101[.]5 "
        FAIL_REGULAR_EXPRESSION "error")
    # STRAIGHTEN walks forward from the first number, as it always has, so an
    # existing line does what it did. On the hexagon (0,0) (10,0) (20,0)
    # (20,10) (10,10) (0,10), from 3 to 1 forward is 4, 5 and 0, leaving
    # (10,0) (20,0) (20,10); side=short takes the window's side, the one with
    # fewer between - vertex 2 alone - leaving five.
    add_test(NAME cli.straighten_walks_forward_and_takes_the_short_side_when_asked
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "PLINE 0,0 10,0 20,0 20,10 10,10 0,10 CLOSE"
                -c "PLINE 0,0 10,0 20,0 20,10 10,10 0,10 CLOSE" -c "STRAIGHTEN 1 3 1"
                -c "STRAIGHTEN 2 3 1 side=short" -c "VERTEX LIST 1")
    set_tests_properties(cli.straighten_walks_forward_and_takes_the_short_side_when_asked PROPERTIES
        PASS_REGULAR_EXPRESSION "id=1 [^\r\n]*vertices=3.*id=2 [^\r\n]*vertices=5.*index=0 x=10 y=0 .*index=1 x=20 y=0 .*index=2 x=20 y=10 "
        FAIL_REGULAR_EXPRESSION "error")

    # The survey-code verbs, on a mapfile of the tests' own: its shape is the
    # reference mapfile's, its codes are not. REPLACE puts it in place of the
    # compiled-in map, so what is expected does not depend on whether this
    # checkout has the reference customisation. By index: 0 KT* feature,
    # 1 KX* feature with no layer, 2 KT* a built-in symbol, "cross". (The
    # file is a survey code file, so its elements are that file's: <map_data>
    # is what the replies call a feature rule and <model> its layer.)
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
    # groups - KT* feature, KX* feature, KT* symbol - replaces
    # itself. A code's loaded rules are gathered at its first rule (so that
    # what the load brought outranks what it left of that code), which makes
    # KX* rule #2: KT* feature, KT* symbol, KX* feature.
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
    # A rule's section and fields are in the Katana customisation format's
    # words (feature, symbol, layer), which the replies give since the survey
    # code file's own (map_data, vertex_symbol_data, model) left them: the four
    # expressions below were changed word for word from the table "The words
    # a rule is shown by" in docs/customisation.md, not from what a run printed.
    set(_eol "[\r\n]+")
    set_tests_properties(cli.code_explain PROPERTIES PASS_REGULAR_EXPRESSION
        "Code \"KT01\": prefix match, matched${_eol}  layer: KATANA TEST  <- rule #0 KT\\* \\(feature\\).*  symbol: cross, size 2  <- rule #2 KT\\* \\(symbol\\)${_eol}  linestyle \"0\": a plain line${_eol}  symbol \"cross\": drawn by Katana itself${_eol}  colour \"red\": #FF0000${_eol}Code \"kt01\": no rule matches it${_eol}  near miss: key \"KT\\*\" differs only in letter case")
    set_tests_properties(cli.mapfile_list PROPERTIES PASS_REGULAR_EXPRESSION
        "KT\\*  \\[prefix; 2 rules: feature, symbol\\]  layer KATANA TEST; linestyle 0; symbol cross; colour red; \"\\[KT\\*\\] Katana test\"${_eol}1 of 2 codes match \"katana\"")
    # The file's kind is said in the help's words: a survey code file, not a
    # style library.
    set_tests_properties(cli.mapfile_check_after_a_merge PROPERTIES PASS_REGULAR_EXPRESSION
        "cli-codes\\.mapfile: survey code file, 0 added, 3 replaced \\(codes, once for each section\\): \"KT\\*\", \"KX\\*\", \"KT\\*\".*3 rules checked: 0 errors, 1 warning${_eol}  by kind: 1 no layer${_eol}  rule #2 KX\\* \\(feature\\): warning, no layer")
    set_tests_properties(cli.code_applies_and_reports PROPERTIES PASS_REGULAR_EXPRESSION
        "1 entity carries a code in \"code\": 1 matched, 0 fallback-only.*Layers created: KATANA TEST${_eol}Styles created: cross${_eol}.*  KT01: 1 entity, prefix, matched; layer 0 -> KATANA TEST; style cross \\(created\\); 1 changed${_eol}Applied as one command.*1 entity carries a code in \"code\", 1 distinct code${_eol}  KT01: 1, prefix, matched, layer KATANA TEST")

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
    # INFO #1 is the entity whatever files the working directory holds: run
    # beside files called 1 and #1 (a mistyped shell 2>1 makes the first), it
    # was once read as INFO <file> - "no importer reads files named ''" -
    # and so were katana_mcp's katana_describe_entity and the window's Entity
    # Information, which send it. INFO 1 there is the file, as it was.
    set(_info_dir "${CMAKE_CURRENT_BINARY_DIR}/cli_info_beside_files")
    file(WRITE "${_info_dir}/1" "")
    file(WRITE "${_info_dir}/#1" "")
    add_test(NAME cli.info_hash_id_is_the_entity_beside_a_file_of_that_name
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "RECT 0,0 10,5" -c "INFO #1"
        WORKING_DIRECTORY "${_info_dir}")
    set_tests_properties(cli.info_hash_id_is_the_entity_beside_a_file_of_that_name PROPERTIES
        PASS_REGULAR_EXPRESSION "1  Polyline  layer=0  vertices=4  closed  length=30  area=50"
        FAIL_REGULAR_EXPRESSION "error")
    # A line whose first non-blank is '#' is a note, indented or not, in a
    # file with Windows line ends - the rule File > Run Script keeps. An
    # indented note was once a command here, and stopped the script the
    # window ran through.
    set(_notes "${CMAKE_CURRENT_BINARY_DIR}/cli_scripts/indented notes.kcs")
    file(WRITE "${_notes}"
        "# a note\r\nRECT 0,0 10,5\r\n\r\n  # an indented note\r\n\t# and a tabbed one\r\nCIRCLE 5,5 1\r\nLIST\r\n")
    add_test(NAME cli.an_indented_note_in_a_script_runs_nothing
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> "${_notes}")
    set_tests_properties(cli.an_indented_note_in_a_script_runs_nothing PROPERTIES
        PASS_REGULAR_EXPRESSION "rectangle created${_eol}circle created${_eol}2 entities"
        FAIL_REGULAR_EXPRESSION "error")
    # The window's own verbs are refused by name, saying where they run,
    # rather than as unknown commands: a script written for File > Run Script
    # that plots fails here saying so. Each stops the batch (exit 1). GRID and
    # EXAGGERATION were unknown commands here until 2026-09-30; VIEWS and ZOOM
    # the interpreter refuses itself, having no window to act on
    # (cad/view_verbs.hpp).
    foreach(_verb IN ITEMS "PLOT a.pdf" "PLOTSHEETS" "SNAPSHOT a.png" "ONLINE PROVIDERS"
                           "SCRIPT a.kcs" "GRID ON" "EXAGGERATION 2" "VIEWS LINK 1,2"
                           "ZOOM IN")
        string(REGEX REPLACE " .*" "" _name "${_verb}")
        string(TOLOWER "${_name}" _name)
        add_test(NAME cli.the_windows_${_name}_verb_is_refused_saying_where_it_runs
            COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                    $<TARGET_FILE:katana_cli> -c "${_verb}" -c "POINT 1,1")
        set_tests_properties(cli.the_windows_${_name}_verb_is_refused_saying_where_it_runs
            PROPERTIES
            PASS_REGULAR_EXPRESSION "error: Unsupported: [A-Z]+ is the desktop window's"
            FAIL_REGULAR_EXPRESSION "unknown command|point created")
    endforeach()
    # And the refusal fails the batch, as any refused line does.
    add_test(NAME cli.a_window_verb_in_a_batch_fails_it
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "PLOT a.pdf")
    set_tests_properties(cli.a_window_verb_in_a_batch_fails_it PROPERTIES WILL_FAIL TRUE)

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

    # An alignment's PIs read back exactly, rewritten as one line, and its
    # setting-out table a record a station with the key stations named - what
    # Terrain > Alignment Manager runs and shows (docs/cad.md). R = 50 round
    # the right angle at (100,0) puts the TC at chainage 50 and the CT at
    # 50 + 25 pi = 128.540.
    add_test(NAME cli.align_pis_rewrites_the_pis_and_stations_names_the_key_stations
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "ALIGN NEW road 0,0 100,0 100,100"
                -c "ALIGN PIS road 0,0 100,0,50 100,100" -c "ALIGN PIS road"
                -c "ALIGN STATIONS road 1000")
    # PARCEL as Survey > Parcel Report runs it: the report the dialog shows,
    # and LABEL naming the layer the labels went on (4 courses and the area).
    add_test(NAME cli.parcel_reports_and_labels_naming_the_layer
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "PLINE 0,0 100,0 100,50 0,50 CLOSE" -c "PARCEL 1"
                -c "PARCEL 1 LABEL 2")
    set_tests_properties(cli.parcel_reports_and_labels_naming_the_layer PROPERTIES
        PASS_REGULAR_EXPRESSION "area 5000[.]000 m2 [(]0[.]500 ha[)], perimeter 300[.]000 m.*5 labels created on layer 0, the current layer"
        FAIL_REGULAR_EXPRESSION "error")
    # HATCH SET and STYLE NEW with a field, as the Hatch Patterns tab runs
    # them: the pattern edited to a crosshatch, a style made hatching with it.
    add_test(NAME cli.hatch_set_edits_a_pattern_and_a_style_is_made_with_it
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "HATCH NEW brick 45 0.25"
                -c "HATCH SET brick 45 0.5 135 0.5" -c "STYLE NEW paving HATCH brick"
                -c "HATCH LIST" -c "STYLE LIST")
    set_tests_properties(cli.hatch_set_edits_a_pattern_and_a_style_is_made_with_it PROPERTIES
        PASS_REGULAR_EXPRESSION "hatch pattern brick updated [(]2 families[)].*style paving created.*brick  [[]45 deg @ 0[.]5[]]  [[]135 deg @ 0[.]5[]].*paving  [^\n]*hatch=brick"
        FAIL_REGULAR_EXPRESSION "error")
    set_tests_properties(cli.align_pis_rewrites_the_pis_and_stations_names_the_key_stations
        PROPERTIES
        PASS_REGULAR_EXPRESSION "pi index=1 x=100 y=0 radius=50 spiral_in=0 spiral_out=0.*station=50[.]000 x=50[.]000 y=0[.]000 direction=0[.]000 azimuth=90[^ ]+ radius=50[.]000 turn=left key=TC.*station=128[.]540 x=100[.]000 y=50[.]000 direction=90[.]000 [^ ]+ radius=straight key=CT.*key=end"
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

    # LIST takes no words: LIST LAYER x once listed the whole drawing, as
    # though it had filtered it. Refused with its usage; LIST alone lists.
    add_test(NAME cli.list_refuses_words_it_does_not_take
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "LINE 0,0 10,0" -c "LIST" -c "LIST LAYER 0")
    set_tests_properties(cli.list_refuses_words_it_does_not_take PROPERTIES
        PASS_REGULAR_EXPRESSION "1 entities.*error: InvalidArgument: usage: LIST [(]every entity; INFO id describes one[)]")

    # HELP typed as a line is the session's whole help, as --help and
    # katana_help are: CUSTOMISE, IFC and the window-only verbs are
    # the session's, not the interpreter's, and were missing from it. ? is
    # HELP's alias.
    add_test(NAME cli.help_typed_as_a_line_is_the_whole_session_help
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "HELP" -c "?")
    set_tests_properties(cli.help_typed_as_a_line_is_the_whole_session_help PROPERTIES
        PASS_REGULAR_EXPRESSION "Customise CUSTOMISE \\[REPLACE\\].*IFC       IMPORT <file\\.ifc>.*Window    PLOT, PLOTSHEETS.*Customise CUSTOMISE \\[REPLACE\\].*IFC       IMPORT <file\\.ifc>.*Window    PLOT, PLOTSHEETS"
        FAIL_REGULAR_EXPRESSION "error: ")
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
    # GENERATE fit rotate=on, what the Sheets editor's "Rotate the drawing to
    # fill the sheet" writes: a 450 x 20 m strip along 45 degrees (its
    # corners by hand, to a millimetre) is 1:2000 square and 1:1250 turned
    # 3.62 degrees (docs/plotting.md, "In the editor").
    add_test(NAME cli.generate_fit_rotate_turns_the_plan_to_fill_the_sheet
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "LINE -152.028,-166.170 166.170,152.028"
                -c "LINE 166.170,152.028 152.028,166.170" -c "LINE 152.028,166.170 -166.170,-152.028"
                -c "LINE -166.170,-152.028 -152.028,-166.170" -c "GENERATE fit rotate=on legend=off"
                -c "VIEW LIST")
    set_tests_properties(cli.generate_fit_rotate_turns_the_plan_to_fill_the_sheet PROPERTIES
        PASS_REGULAR_EXPRESSION "generated 1 sheet: 1.*view id=vp1 kind=plan rect=[^\r\n]* scale=1250 centre=[^\r\n]* rotation=3\\.6[12]"
        FAIL_REGULAR_EXPRESSION "error")
    # SHEETS APPEND, what the Sheets editor's Sheet Set > Append Sheets From
    # File runs: a set saved, the sheet renamed, and the saved set appended
    # after it, renumbered s2 (docs/plotting.md, "The Sheet Set menu and Page
    # Setup").
    set(_append_file "${CMAKE_CURRENT_BINARY_DIR}/cli append set.json")
    add_test(NAME cli.sheets_append_puts_another_sets_sheets_after_these
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SHEET NEW NORTH" -c "SHEETS SAVE \"${_append_file}\""
                -c "SHEET RENAME 1 COVER" -c "SHEETS APPEND \"${_append_file}\"" -c "SHEETS")
    set_tests_properties(cli.sheets_append_puts_another_sets_sheets_after_these PROPERTIES
        PASS_REGULAR_EXPRESSION "appended 1 sheet from \"[^\"]*cli append set\\.json\": 2.*2 sheets${_eol}sheet 1 id=s1 name=\"COVER\"[^\r\n]*${_eol}sheet 2 id=s2 name=\"NORTH\""
        FAIL_REGULAR_EXPRESSION "error")
endif()

# The annotation styles with no window (docs/annotation.md): the lines the
# window's style managers send, typed. A SET of three fields undone by ONE
# UNDO: INFO reads the defaults back (tables.hpp: DIMTXT 2.5, closed filled
# arrows, model units).
if(KATANA_BUILD_TESTS)
    add_test(NAME cli.a_dimension_style_set_of_several_fields_is_one_undo_step
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "DIMSTYLE NEW site"
                -c "DIMSTYLE SET site TEXT 3.5 HEAD Tick PAPER on" -c "DIMSTYLE INFO site"
                -c "UNDO" -c "DIMSTYLE INFO site")
    set_tests_properties(cli.a_dimension_style_set_of_several_fields_is_one_undo_step PROPERTIES
        PASS_REGULAR_EXPRESSION
        "name=site text=3\\.5 [^\r\n]*head=Tick [^\r\n]*paper=on layers=0 reads=10\\.000[\r\n]+.*name=site text=2\\.5 [^\r\n]*head=ClosedFilled [^\r\n]*paper=off layers=0"
        FAIL_REGULAR_EXPRESSION "error")
    # The line Annotate > Edit Text sends, typed: the words over two lines, a
    # style and a justification in ONE TEXTEDIT, which one UNDO takes back.
    # Notes is 3.5 mm on paper, 3.5 m at the default 1:1000.
    add_test(NAME cli.textedit_changes_several_keys_of_a_text_in_one_step
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "TEXTSTYLE NEW Notes paper=3.5"
                -c "TEXT 10,20 2.5 old"
                -c "TEXTEDIT 1 text=\"PIT 12\\nIL 10.50\" style=Notes justify=MC"
                -c "INFO 1" -c "UNDO" -c "INFO 1")
    set_tests_properties(cli.textedit_changes_several_keys_of_a_text_in_one_step PROPERTIES
        PASS_REGULAR_EXPRESSION
        "1  Text  layer=0  \"PIT 12.nIL 10\\.50\"  height=3\\.5  style=Notes  justify=MC.*1  Text  layer=0  \"old\"  height=2\\.5[\r\n]"
        FAIL_REGULAR_EXPRESSION "error")
    # The rules tab's lines, typed: a rule added switched off with its type
    # labels nothing even named (auto_label.cpp, ruleMatches); switched on,
    # it is previewed and run by name while another rule waits. One label a
    # target: the rectangle once for its area (auto_label.hpp).
    add_test(NAME cli.chosen_label_rules_are_previewed_and_run_by_name
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "LABELSTYLE DEFAULTS" -c "RECT 0,0 20,20"
                -c "AUTOLABEL RULE ADD lots style=\"Lot Area\" type=Polyline enabled=off"
                -c "AUTOLABEL RULE ADD sides style=\"Bearing Distance\""
                -c "AUTOLABEL PREVIEW lots" -c "AUTOLABEL RULE SET lots enabled=on"
                -c "AUTOLABEL PREVIEW lots" -c "AUTOLABEL RUN lots" -c "AUTOLABEL RULE LIST")
    set_tests_properties(cli.chosen_label_rules_are_previewed_and_run_by_name PROPERTIES
        PASS_REGULAR_EXPRESSION
        "preview created=0 kept=0 removed=0 skipped=0[\r\n]+.*preview created=1 kept=0 removed=0 skipped=0[\r\n]+rule=lots labels=1[\r\n]+autolabel created=1 kept=0 removed=0 skipped=0[\r\n]+rule=lots labels=1[\r\n]+.*name=lots style=\"Lot Area\" layer=\"\" code=\"\" type=Polyline labellayer=\"\" enabled=on"
        FAIL_REGULAR_EXPRESSION "error")
endif()

# Smart leaders with no window (docs/annotation.md, "Smart leaders"): a
# leader reading a point's attribute is saved; a second process opens it,
# changes the attribute and lists the leader, whose note is read afresh off
# the point (9.5 to two places is "9.50"). An exact place along - the end of
# a square's closing side, 4 x 100 = 400 along - and balloons numbered again
# from 10 across the sheet. A leader that would say nothing stops the script
# and fails it.
if(KATANA_BUILD_TESTS)
    set(_leaders "${CMAKE_CURRENT_BINARY_DIR}/cli-leaders.katana")
    add_test(NAME cli.leaders_cleanup COMMAND ${CMAKE_COMMAND} -E rm -rf "${_leaders}")
    add_test(NAME cli.a_smart_leader_is_saved_with_the_point_it_reads
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "POINT 0,0" -c "SELECT 1" -c "PROP SET invert 10.25"
                -c "LEADER #1 5,5 template=\"IL {prop.invert:.2f}\"" -c "SAVE \"${_leaders}\"")
    add_test(NAME cli.a_reopened_smart_leader_reads_its_note_off_the_point_afresh
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "OPEN \"${_leaders}\"" -c "SELECT 1"
                -c "PROP SET invert 9.5" -c "LEADER LIST")
    add_test(NAME cli.a_leader_is_put_exactly_along_and_balloons_are_numbered_again
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "PLINE 0,0 100,0 100,100 0,100 C"
                -c "LEADER #1.along3:1 -20,-20 template=\"CH {chainage:.0f}\""
                -c "BALLOON 30,0 35,5" -c "BALLOON 10,20 15,25"
                -c "BALLOON RENUMBER start=10 order=x")
    add_test(NAME cli.a_leader_that_would_say_nothing_stops_the_script
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "POINT 0,0"
                -c "LEADER #1 5,5 template=\"IL {prop.invert:.2f}\""
                -c "LEADER 0,0 1,1 text=NEVER_REACHED")
    add_test(NAME cli.a_leader_that_would_say_nothing_fails_the_script
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "POINT 0,0"
                -c "LEADER #1 5,5 template=\"IL {prop.invert:.2f}\"")
    set_tests_properties(cli.leaders_cleanup PROPERTIES FIXTURES_SETUP cli_leaders)
    set_tests_properties(cli.a_smart_leader_is_saved_with_the_point_it_reads PROPERTIES
        FIXTURES_REQUIRED cli_leaders FIXTURES_SETUP cli_leaders_saved
        PASS_REGULAR_EXPRESSION "created leader id=2 text=\"IL 10.25\" associative=yes target=1")
    set_tests_properties(cli.a_reopened_smart_leader_reads_its_note_off_the_point_afresh PROPERTIES
        FIXTURES_REQUIRED cli_leaders_saved
        PASS_REGULAR_EXPRESSION "id=2 .*target=1 anchor=position smart=yes .*text=\"IL 9.50\"")
    set_tests_properties(cli.a_leader_is_put_exactly_along_and_balloons_are_numbered_again PROPERTIES
        PASS_REGULAR_EXPRESSION "created leader id=2 text=\"CH 400\" associative=yes target=1 anchor=\"along 3 1\".*balloons=2 renumbered=2")
    set_tests_properties(cli.a_leader_that_would_say_nothing_stops_the_script PROPERTIES
        PASS_REGULAR_EXPRESSION "would say nothing"
        FAIL_REGULAR_EXPRESSION "NEVER_REACHED")
    set_tests_properties(cli.a_leader_that_would_say_nothing_fails_the_script PROPERTIES
        WILL_FAIL TRUE)
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

    # The same verbs on what is drawn (docs/subsurface_utilities.md, "Drawing
    # data"), by the scope words MODIFY takes. The sample drawn and reported
    # from the drawing is the sample's report, after the record of what the
    # scope took: its 7 runs and 14 points, 4 lines, all of each.
    add_test(NAME cli.utility_report_of_the_drawing_is_the_report_of_the_schedule_drawn
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY DRAW \"${_utilities}/schedule.csv\""
                -c "UTILITY REPORT DRAWING MINCOVER 0.6")
    set_tests_properties(cli.utility_report_of_the_drawing_is_the_report_of_the_schedule_drawn
        PROPERTIES PASS_REGULAR_EXPRESSION
        "scope=drawing matched=21 lines=4 completed=0 ignored=0${_eol}AS 5488 subsurface utility investigation: 4 lines, 14 vertices.*E1 / E1-3: claimed QL-B, the ground penetrating radar evidence supports QL-C")
    # Written back as a schedule, which reads again as the sample does.
    set(_scheduled "${CMAKE_CURRENT_BINARY_DIR}/cli utility scheduled.csv")
    add_test(NAME cli.utility_schedule_writes_the_drawing_back_as_a_schedule_that_reads_again
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY DRAW \"${_utilities}/schedule.csv\""
                -c "UTILITY SCHEDULE \"${_scheduled}\" DRAWING"
                -c "UTILITY REPORT \"${_scheduled}\" MINCOVER 0.6")
    set_tests_properties(cli.utility_schedule_writes_the_drawing_back_as_a_schedule_that_reads_again
        PROPERTIES PASS_REGULAR_EXPRESSION
        "utilities scheduled path=\"[^\"]*cli utility scheduled\\.csv\" lines=4 vertices=14${_eol}scope=drawing matched=21 lines=4 completed=0 ignored=0${_eol}AS 5488 subsurface utility investigation: 4 lines, 14 vertices.*E1 / E1-3: claimed QL-B, the ground penetrating radar evidence supports QL-C")
    # REGRADE at a 20 m detected spacing: W1's 12.5 m past the pothole is
    # traced now, QL-B, as DRAW at that spacing draws it; the other three
    # services grade as they did, but every point in scope takes the 20 m
    # it was graded with (utility.spacing), so all four lines change. A
    # regrade then told nothing grades each line at the 20 m it now carries,
    # not the default 10 m: nothing changes.
    add_test(NAME cli.utility_regrade_grades_what_is_drawn_again
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY DRAW \"${_utilities}/schedule.csv\""
                -c "UTILITY REGRADE LAYERS utilities SPACING 20"
                -c "UTILITY REGRADE LAYERS utilities")
    set_tests_properties(cli.utility_regrade_grades_what_is_drawn_again PROPERTIES
        PASS_REGULAR_EXPRESSION
        "utilities regraded changed=4 lines=4 vertices=14 segments=10 entities=21 layers=10 bounds=334000\\.000,6250000\\.000,334040\\.000,6250007\\.200${_eol}scope=layers layers=utilities sublayers=yes matched=21 lines=4 completed=0 ignored=0${_eol}line id=W1 type=water length=30\\.024 ql_a=1\\.420 ql_b=28\\.604 ql_c=0\\.000 ql_d=0\\.000.*utilities regraded changed=0 lines=4 .*line id=W1 type=water length=30\\.024 ql_a=1\\.420 ql_b=28\\.604 ql_c=0\\.000 ql_d=0\\.000")
    # VIEW is the window's plan view: headless it is refused, naming the
    # window typed in, which takes the gas main's run and two points.
    add_test(NAME cli.utility_view_is_refused_headless_naming_area
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY DRAW \"${_utilities}/schedule.csv\"" -c "UTILITY REPORT VIEW")
    set_tests_properties(cli.utility_view_is_refused_headless_naming_area PROPERTIES
        PASS_REGULAR_EXPRESSION
        "VIEW is the window's plan view, and there is none here; give the window instead: AREA x0,y0,x1,y1")
    add_test(NAME cli.utility_area_takes_what_lies_in_the_window
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY DRAW \"${_utilities}/schedule.csv\""
                -c "UTILITY VERIFY AREA 333990,6250006.5,334050,6250008")
    set_tests_properties(cli.utility_area_takes_what_lies_in_the_window PROPERTIES
        PASS_REGULAR_EXPRESSION
        "scope=area area=333990,6250006\\.5,334050,6250008 matched=3 lines=1 completed=0 ignored=0")

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

# UTILITY DRAW of what an import left in the drawing (docs/subsurface_utilities.md,
# "Services from surveyed and imported geometry"). located_services.geojson is
# the sample investigation as a GIS delivers it - plan strings, points shot on
# the ground, the services' and the points' attributes under the GIS's own
# names - and FIELDS reads those as the schedule's columns. It must draw what
# schedule.csv draws: W1 as worked by hand in the doc, and a report of 4
# lines and 14 vertices naming E1-3's over-claim.
if(KATANA_BUILD_TESTS AND TARGET katana_interop)
    set(_eol "[\r\n]+")
    set(_utilities "${PROJECT_SOURCE_DIR}/samples/utilities")
    set(_located_fields "line=ASSET_ID,point=PT_ID,type=ASSET_TYPE,owner=OWNER,material=MATERIAL,diameter_mm=DIA_MM,status=STATUS,config=CONFIG,description=DESC,method=METHOD,h_unc=H_UNC,v_unc=V_UNC,ql=QL,level=LEVEL,level_ref=LEVEL_REF,path=PATH,verifies=VERIFIES")
    add_test(NAME cli.utility_draw_takes_the_services_an_import_left_in_the_drawing
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "IMPORT \"${_utilities}/located_services.geojson\""
                -c "UTILITY DRAW DRAWING FIELDS ${_located_fields}"
                -c "UTILITY REPORT LAYERS utilities MINCOVER 0.6")
    set_tests_properties(cli.utility_draw_takes_the_services_an_import_left_in_the_drawing
        PROPERTIES PASS_REGULAR_EXPRESSION
        "utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 bounds=334000\\.000,6250000\\.000,334040\\.000,6250007\\.200${_eol}scope=drawing matched=18 lines=4 points=14 loose=0 drawn=0 ignored=0${_eol}line id=W1 type=water length=30\\.024 ql_a=1\\.420 ql_b=16\\.102 ql_c=12\\.502 ql_d=0\\.000.*scope=layers layers=utilities sublayers=yes matched=21 lines=4 completed=0 ignored=0${_eol}AS 5488 subsurface utility investigation: 4 lines, 14 vertices.*E1 / E1-3: claimed QL-B, the ground penetrating radar evidence supports QL-C"
        FAIL_REGULAR_EXPRESSION "error")
    # Nothing says how W1's first vertex was located: the draw is refused by
    # that vertex, nothing is drawn, and the script fails.
    add_test(NAME cli.utility_draw_of_geometry_with_no_method_is_refused_by_the_vertex
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "IMPORT \"${_utilities}/located_services.geojson\""
                -c "UTILITY DRAW DRAWING FIELDS line=ASSET_ID,point=PT_ID" -c "LAYER LIST")
    set_tests_properties(cli.utility_draw_of_geometry_with_no_method_is_refused_by_the_vertex
        PROPERTIES PASS_REGULAR_EXPRESSION
        "line W1: vertex W1-1 \\(#[0-9]+\\) says no method it was located by; give METHOD"
        FAIL_REGULAR_EXPRESSION "utilities drawn|utilities/water")
    add_test(NAME cli.utility_draw_of_geometry_refused_fails_the_script
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "IMPORT \"${_utilities}/located_services.geojson\""
                -c "UTILITY DRAW DRAWING FIELDS line=ASSET_ID,point=PT_ID")
    set_tests_properties(cli.utility_draw_of_geometry_refused_fails_the_script PROPERTIES
        WILL_FAIL TRUE)
endif()

# A .ifc through the command line both ways (docs/ifc.md). samples/ifc/
# scenario.txt draws MC01 and a kerb, a fence, a pit, a point and a text;
# with the sample schedule, the export holds the alignment, 4 services of 10
# graded segments - 6 of them in 3D, as tests/ifc/test_export.cpp works out -
# 14 located points and the 5 entities, and read back into a new drawing it
# is the 29 objects and MC01's 4 PIs again. The sample written to a delivery
# schema (schedule_tfnsw.csv), with the schema of tests/ifc: W-0001's 3 points, E-0001's 2 and
# G-0001's 2 make 4 segments, in 3D where a depth is recorded at both ends -
# all but G-0001's, which is from the records.
if(KATANA_BUILD_TESTS)
    set(_ifc "${CMAKE_CURRENT_BINARY_DIR}/cli site.ifc")
    set(_ifc_delivery "${CMAKE_CURRENT_BINARY_DIR}/cli delivery.ifc")
    add_test(NAME cli.ifc_the_scenario_and_the_sample_services_exported_and_imported_back
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> "${PROJECT_SOURCE_DIR}/samples/ifc/scenario.txt"
                -c "EXPORT \"${_ifc}\" UTILITIES \"${PROJECT_SOURCE_DIR}/samples/utilities/schedule.csv\""
                -c "NEW" -c "IMPORT \"${_ifc}\"" -c "ALIGN LIST")
    set_tests_properties(cli.ifc_the_scenario_and_the_sample_services_exported_and_imported_back
        PROPERTIES FIXTURES_SETUP cli_ifc_file PASS_REGULAR_EXPRESSION
        "ifc exported file=\"cli site\\.ifc\" schema=IFC4X3_ADD2 instances=[0-9]+ bytes=[0-9]+\ncounts alignments=1 services=4 segments=10 segments_3d=6 located_points=14 entities_written=5 entities_skipped=0 surfaces=0.*ifc imported file=\"cli site\\.ifc\" schema=IFC4X3_ADD2 crs=\"EPSG:7856\" entities=29 alignments=1 surfaces=0.*MC01  4 PIs  1000\\.000 to 1339\\.016")
    add_test(NAME cli.ifc_a_delivery_schedule_exported_with_its_schema
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "EXPORT \"${_ifc_delivery}\" UTILITIES \"${PROJECT_SOURCE_DIR}/samples/utilities/schedule_tfnsw.csv\" SCHEMA \"${PROJECT_SOURCE_DIR}/tests/ifc/data/delivery_schema.csv\" NODRAWING")
    set_tests_properties(cli.ifc_a_delivery_schedule_exported_with_its_schema
        PROPERTIES FIXTURES_SETUP cli_ifc_delivery_file PASS_REGULAR_EXPRESSION
        "counts alignments=0 services=3 segments=4 segments_3d=3 located_points=7 entities_written=0 ")
    # A project's own rules (samples/ifc/classification_rules.csv), tried
    # before the defaults: a point on a lighting layer is a column, not a
    # survey annotation.
    add_test(NAME cli.ifc_a_projects_rules_classify_what_the_defaults_would_not
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "LAYER NEW Lighting/Poles" -c "LAYER SET Lighting/Poles"
                -c "POINT 10,10" -c "LAYER NEW Drainage/Headwalls" -c "LAYER SET Drainage/Headwalls"
                -c "POINT 30,10"
                -c "EXPORT \"${CMAKE_CURRENT_BINARY_DIR}/cli rules.ifc\" RULES \"${PROJECT_SOURCE_DIR}/samples/ifc/classification_rules.csv\"")
    set_tests_properties(cli.ifc_a_projects_rules_classify_what_the_defaults_would_not PROPERTIES
        PASS_REGULAR_EXPRESSION "\nclass name=IfcColumn count=1\nclass name=IfcDistributionChamberElement count=1\nclass name=IfcDistributionSystem count=1\n")
    # The sample schedule drawn (UTILITY DRAW) and the drawing exported: the
    # services the schedule's own export writes, class for class. The draw's
    # reply gives the runs - W1 three (QL-B, QL-A, QL-C), E1 two (QL-B,
    # QL-C), T1 and G1 one each - and its 14 located vertices; so W1's three
    # and G1's one are pipes, E1's two are conduits (IfcCableCarrierSegment,
    # its "4 x 100 mm conduits"), T1's one a cable, each vertex a survey
    # annotation, and one system a service.
    set(_ifc_drawn "${CMAKE_CURRENT_BINARY_DIR}/cli drawn services.ifc")
    add_test(NAME cli.ifc_a_drawn_services_plan_goes_out_by_service
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "UTILITY DRAW \"${PROJECT_SOURCE_DIR}/samples/utilities/schedule.csv\""
                -c "EXPORT \"${_ifc_drawn}\"")
    set_tests_properties(cli.ifc_a_drawn_services_plan_goes_out_by_service PROPERTIES
        FIXTURES_SETUP cli_ifc_drawn_file PASS_REGULAR_EXPRESSION
        "entities_written=21 entities_skipped=0 surfaces=0\nclass name=IfcAnnotation count=14\nclass name=IfcCableCarrierSegment count=2\nclass name=IfcCableSegment count=1\nclass name=IfcDistributionSystem count=4\nclass name=IfcPipeSegment count=4\n")
    add_test(NAME cli.ifc_refuses_an_unknown_option
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "EXPORT \"${_ifc}\" UTILITIS x.csv")
    set_tests_properties(cli.ifc_refuses_an_unknown_option PROPERTIES WILL_FAIL TRUE)
    # Everything File > Export IFC and Import IFC can choose is a word an
    # agent can type (docs/ifc.md, "Using it"). PREVIEW answers with the
    # objects and writes nothing: the file named is never made. With
    # NOALIGNMENTS the scenario is its 5 entities and no IfcAlignment. INFO
    # describes it; IMPORT ... NOELEMENTS TAKECRS brings nothing but gives
    # the new drawing the file's EPSG:7856, as its own step; IFC RULES writes
    # the defaults.
    set(_ifc_never "${CMAKE_CURRENT_BINARY_DIR}/cli never written.ifc")
    file(REMOVE "${_ifc_never}")
    add_test(NAME cli.ifc_every_choice_of_the_dialogs_is_a_word_of_the_line
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> "${PROJECT_SOURCE_DIR}/samples/ifc/scenario.txt"
                -c "EXPORT \"${_ifc_never}\" PREVIEW"
                -c "EXPORT \"${CMAKE_CURRENT_BINARY_DIR}/cli choices.ifc\" NOALIGNMENTS NOSURFACES"
                -c "INFO \"${CMAKE_CURRENT_BINARY_DIR}/cli choices.ifc\""
                -c "NEW"
                -c "IMPORT \"${CMAKE_CURRENT_BINARY_DIR}/cli choices.ifc\" NOELEMENTS TAKECRS"
                -c "IMPORT \"${CMAKE_CURRENT_BINARY_DIR}/cli choices.ifc\" TOLERANCE 0.01 KEEPCRS"
                -c "CRS"
                -c "IFC RULES \"${CMAKE_CURRENT_BINARY_DIR}/cli default rules.csv\"")
    set_tests_properties(cli.ifc_every_choice_of_the_dialogs_is_a_word_of_the_line PROPERTIES
        PASS_REGULAR_EXPRESSION
        "ifc previewed file=\"cli never written\\.ifc\" schema=IFC4X3_ADD2 instances=[0-9]+\ncounts alignments=1 .*object from=\"layer Survey/Kerb\" count=1 class=\"IfcKerb\" predefined=\"NOTDEFINED\" object_type=\"\" system=\"\" why=\"rule kerb\".*ifc exported file=\"cli choices\\.ifc\" schema=IFC4X3_ADD2 instances=[0-9]+ bytes=[0-9]+\ncounts alignments=0 services=0 segments=0 segments_3d=0 located_points=0 entities_written=5 .*ifc described file=\"cli choices\\.ifc\" schema=IFC4X3_ADD2 crs=\"EPSG:7856\" entities=5 alignments=0 .*ifc imported file=\"cli choices\\.ifc\" schema=IFC4X3_ADD2 crs=\"EPSG:7856\" entities=0 alignments=0 .*note text=\"the project is now in EPSG:7856, the file's coordinate system\".*ifc imported file=\"cli choices\\.ifc\" schema=IFC4X3_ADD2 crs=\"EPSG:7856\" entities=5 .*crs id=EPSG:7856 .*ifc rules file=\"cli default rules\\.csv\" rules=[0-9]+")
    # PREVIEW wrote nothing: the file it named does not exist.
    add_test(NAME cli.ifc_preview_writes_no_file
        COMMAND ${CMAKE_COMMAND} -E rm "${_ifc_never}")
    set_tests_properties(cli.ifc_preview_writes_no_file PROPERTIES WILL_FAIL TRUE
        DEPENDS cli.ifc_every_choice_of_the_dialogs_is_a_word_of_the_line)
    # What would be contradictory is refused by name, and fails the script.
    add_test(NAME cli.ifc_selected_with_nothing_selected_is_refused
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> "${PROJECT_SOURCE_DIR}/samples/ifc/scenario.txt"
                -c "EXPORT \"${CMAKE_CURRENT_BINARY_DIR}/cli selected.ifc\" SELECTED")
    set_tests_properties(cli.ifc_selected_with_nothing_selected_is_refused PROPERTIES
        WILL_FAIL TRUE)
    add_test(NAME cli.ifc_takecrs_with_local_is_refused
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "IMPORT \"${_ifc}\" LOCAL TAKECRS")
    set_tests_properties(cli.ifc_takecrs_with_local_is_refused PROPERTIES WILL_FAIL TRUE)

    # Both files judged by IfcOpenShell (tools/check_ifc.py): the schema, its
    # EXPRESS rules, no proxy, and the alignment's geometry against its
    # business logic. Only where the Python that configured the build can
    # import ifcopenshell - a check of the output, never a build requirement.
    if(Python3_Interpreter_FOUND)
        execute_process(COMMAND "${KATANA_PYTHON}" -c "import ifcopenshell"
            RESULT_VARIABLE _ifcopenshell_missing OUTPUT_QUIET ERROR_QUIET)
        if(_ifcopenshell_missing EQUAL 0)
            add_test(NAME cli.ifc_the_scenario_export_is_valid_to_ifcopenshell
                COMMAND "${KATANA_PYTHON}" "${PROJECT_SOURCE_DIR}/tools/check_ifc.py" "${_ifc}")
            set_tests_properties(cli.ifc_the_scenario_export_is_valid_to_ifcopenshell
                PROPERTIES FIXTURES_REQUIRED cli_ifc_file)
            add_test(NAME cli.ifc_the_delivery_export_is_valid_to_ifcopenshell
                COMMAND "${KATANA_PYTHON}" "${PROJECT_SOURCE_DIR}/tools/check_ifc.py" "${_ifc_delivery}")
            set_tests_properties(cli.ifc_the_delivery_export_is_valid_to_ifcopenshell
                PROPERTIES FIXTURES_REQUIRED cli_ifc_delivery_file)
            add_test(NAME cli.ifc_the_drawn_services_export_is_valid_to_ifcopenshell
                COMMAND "${KATANA_PYTHON}" "${PROJECT_SOURCE_DIR}/tools/check_ifc.py" "${_ifc_drawn}")
            set_tests_properties(cli.ifc_the_drawn_services_export_is_valid_to_ifcopenshell
                PROPERTIES FIXTURES_REQUIRED cli_ifc_drawn_file)
        endif()
    endif()
endif()

# A .dxf through the command line both ways, natively - no GDAL, so it runs
# in a build without it. Drawn by hand: a 30 x 20 rectangle (perimeter 100,
# area 600), a circle of radius 5 at (15,10) and a text, on a nested layer;
# exported, a new drawing, imported back. Three layers go out: 0, Survey and
# Survey/Kerb - the last as "Survey$Kerb" with its path beside it. The replies
# are the records every build writes (import_records.hpp), with GDAL or
# without.
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
        "exported file=\"[^\"]*/cli round trip\\.dxf\" kind=dxf driver=DXF format=\"DXF R2000\" entities=3 skipped=0 layers=3 .*imported file=\"[^\"]*/cli round trip\\.dxf\" kind=dxf format=\"DXF R2000\" entities=3 layers=3 .*Polyline  layer=Survey/Kerb  vertices=4  closed  length=100  area=600.*Circle  layer=Survey/Kerb  centre 15,10  radius=5 .*Text  layer=Survey/Kerb  \"LOT 42\"")
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
        "dataset file=[^ ]*/terrain\\.asc kind=raster driver=AAIGrid [^\r\n]*[\r\n]+raster width=120 height=90 bands=1 georeferenced=yes cell=1\\.5,1\\.5 bounds=-5,-5,175,130")
    set_tests_properties(cli.info_point_cloud PROPERTIES PASS_REGULAR_EXPRESSION
        "pointcloud points=40000 [^\r\n]*copc=no")
    set_tests_properties(cli.copc_cleanup PROPERTIES FIXTURES_SETUP cli_copc)
    set_tests_properties(cli.copc_then_info PROPERTIES FIXTURES_REQUIRED cli_copc
        PASS_REGULAR_EXPRESSION "converted source=[^ ]*/survey_scan\\.las file=[^ ]*/cli-scan\\.copc\\.laz format=copc.*pointcloud points=40000 [^\r\n]*copc=yes")
    set_tests_properties(cli.local_is_refused_for_reference_data PROPERTIES
        PASS_REGULAR_EXPRESSION "LOCAL is not supported for rasters and point clouds")
    # A QUOTED path and LOCAL, as katana_import sends them: LOCAL was taken off
    # and the quotes left on, so the file was looked for by its quoted name.
    # parcels.geojson spans (180, 0) to (365, 165) (its coordinates, by hand),
    # so moved to put that lower-left corner at 0,0 it spans (0, 0) to
    # (185, 165). Its 8 features, the spoil heaps a MultiPolygon of two, are 9
    # entities.
    add_test(NAME cli.import_local_of_a_quoted_path_moves_the_data_to_the_origin
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "IMPORT \"${_gis}/parcels.geojson\" LOCAL")
    set_tests_properties(cli.import_local_of_a_quoted_path_moves_the_data_to_the_origin PROPERTIES
        PASS_REGULAR_EXPRESSION "imported file=\"?[^ ]*/parcels\\.geojson\"? kind=vector entities=9 layers=1 [^\r\n]*bounds=0,0,185,165[^\r\n]*[\r\n]+placed placement=local east=-180 north=0 "
        FAIL_REGULAR_EXPRESSION "error")
    # ALONGSIDE puts the parcels' corner (180, 0) on the rectangle's
    # (1000, 2000); OFFSET moves it by exactly 10,-20.5. Each says its move.
    add_test(NAME cli.import_alongside_and_offset_place_the_data_and_say_the_move
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "RECT 1000,2000 1010,2005"
                -c "IMPORT \"${_gis}/parcels.geojson\" ALONGSIDE" -c UNDO
                -c "IMPORT ${_gis}/parcels.geojson OFFSET=10,-20.5")
    set_tests_properties(cli.import_alongside_and_offset_place_the_data_and_say_the_move PROPERTIES
        PASS_REGULAR_EXPRESSION "bounds=1000,2000,1185,2165[^\r\n]*[\r\n]+placed placement=alongside east=820 north=2000 text=\"ALONGSIDE: moved as one piece by 820\\.000,2000\\.000.*bounds=190,-20\\.5,375,144\\.5[^\r\n]*[\r\n]+placed placement=offset east=10 north=-20\\.5 text=\"OFFSET: moved as one piece by 10\\.000,-20\\.500"
        FAIL_REGULAR_EXPRESSION "error")
endif()

# Labels by hand and their layout, as an agent types them (docs/annotation.md,
# "The verbs"). Two numbered points at one place (1, 2) labelled in a style
# that may not move its labels: the first label (3) is placed and the second
# (4) finds no room, and LABEL LAYOUT names it on a line of its own. Pinned
# elsewhere with LABEL SET, both are placed - the pinned one counted as
# displaced, since it is away from its own place with a leader back to its
# point (label_layout.cpp gives a dragged label that candidate).
if(KATANA_BUILD_TESTS)
    add_test(NAME cli.label_layout_names_the_label_that_found_no_room
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "LABELSTYLE NEW pt kind=point text={point} displace=off"
                -c "POINT 0,0" -c "SELECT 1" -c "PROP SET point P1"
                -c "POINT 0,0" -c "SELECT 2" -c "PROP SET point P2"
                -c "LABEL 1 2 style=pt" -c "LABEL LAYOUT" -c "LABEL SET 4 at=50,50"
                -c "LABEL LAYOUT")
    set_tests_properties(cli.label_layout_names_the_label_that_found_no_room PROPERTIES
        PASS_REGULAR_EXPRESSION
        "considered=2 placed=1 displaced=0 suppressed=1 orphaned=0[\r\n]+label=3 piece=0 [^\r\n]*[\r\n]+label=4 piece=0 suppressed=yes.*updated label id=4.*considered=2 placed=2 displaced=1 suppressed=0 orphaned=0")
endif()

# The geoprocessing verbs' cli.* tests, one file per package of the
# geoprocessing plan (geo/cli/*.cmake), so the lanes add tests without editing
# this file. Included here, where katana_cli and the test variables are.
if(KATANA_BUILD_TESTS AND TARGET katana_interop)
    file(GLOB _katana_geo_cli_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/geo/cli/*.cmake")
    foreach(_geo_cli_test IN LISTS _katana_geo_cli_tests)
        include("${_geo_cli_test}")
    endforeach()
endif()

# The properties as a tree, one level a line (property_outline.hpp; the
# Properties panel reads the same outline). A point given two vertices'
# attributes and a group: the top level has the group, its own value and the
# vertex branch; UNDER vertex lists vertex 2 before vertex 10 (numbers, not
# text); DRAWING with a second point says where the two differ.
if(KATANA_BUILD_TESTS)
    add_test(NAME cli.prop_tree_reads_the_attributes_one_level_at_a_time
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "POINT 0,0" -c "SELECT 1"
                -c "PROP SET vertex/10/QL C" -c "PROP SET vertex/2/QL A"
                -c "PROP SET Asset/Owner Council" -c "PROP SET code KERB"
                -c "PROP TREE" -c "PROP TREE UNDER vertex"
                -c "POINT 5,5" -c "SELECT 2" -c "PROP SET code POLE" -c "PROP TREE DRAWING")
    set_tests_properties(cli.prop_tree_reads_the_attributes_one_level_at_a_time PROPERTIES
        PASS_REGULAR_EXPRESSION
        "scope=selection matched=1 under=\"\" entries=3 from=0 shown=3[\r\n]+path=Asset children=1 values=1[\r\n]+path=code value=KERB type=text[\r\n]+path=vertex children=2 values=2[\r\n]+scope=selection matched=1 under=vertex entries=2 from=0 shown=2[\r\n]+path=vertex/2 children=1 values=1[\r\n]+path=vertex/10 children=1 values=1.*scope=drawing matched=2 under=\"\" entries=3 from=0 shown=3[\r\n]+path=Asset children=1 values=1[\r\n]+path=code varies=yes holders=2"
        FAIL_REGULAR_EXPRESSION "error")
endif()

# SURVEY READ and SURVEY IMPORT (survey_verbs.hpp) on the hand-built field
# file tests/surveyio/data/fld/setup.fld. READ: 16 records read and 2 skipped
# (record 20, a 07 one value short, and record 21, opcode 18, a circle feature
# the reader does not import), 1 setup, 12
# observations (4 pointings of 3), CP1 and CP2 entered, 101 and 102 observed,
# the one EB/01 feature, and the system its header comments name. IMPORT
# reduces it with the defaults and draws 4 points on LAYER, one undo step:
# worked by hand from CP1 (1000 E, 5000 N) oriented on CP2 due north at circle
# 0, shot 101 (45 deg, zenith 95 deg, 50 m) is 50 sin 95 = 49.810 m out at
# 45 deg, so at 1035.221 E 5035.221 N; shot 102 is face right (zenith 275
# deg), so its circle reading 90.5 deg is 270.5 deg on face left, 30 sin 85 =
# 29.886 m out: 970.115 E 5000.261 N. The regexes hold those to the
# millimetre, which is the error budget of that hand working: the reduction
# also means the orientation with the check measurement to CP2 (circle 0.001
# deg, 1.8" on the mean, 0.44 mm across at 50 m) and applies curvature and
# refraction, which the working leaves out.
if(KATANA_BUILD_TESTS)
    set(_fld "${PROJECT_SOURCE_DIR}/tests/surveyio/data/fld/setup.fld")
    add_test(NAME cli.survey_read_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_fld}\"" -c "STATUS")
    set_tests_properties(cli.survey_read_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=setup\\.fld format=opcode-field-file parser=1\\.2 read=16 skipped=2 warnings=2[\r\n]+content setups=1 observations=12 points=2 unpositioned=2 features=1 control=0[\r\n]+declared crs=\"Australia/GDA2020, Zone 56\" epsg=none[\r\n]+warning record=20 [^\r\n]+[\r\n]+warning record=21 text=\"opcode 18 \\(circle feature\\) is not one this reader imports\".*Entities: 0 "
        FAIL_REGULAR_EXPRESSION "error")
    add_test(NAME cli.survey_import_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_fld}\" LAYER survey/fld"
                -c "LIST" -c "UNDO" -c "STATUS")
    set_tests_properties(cli.survey_import_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=4 layer=survey/fld .*layer=survey/fld  at 1000,5000[\r\n]+[^\r\n]+at 1000,5100[\r\n]+[^\r\n]+at 1035\\.22[01][0-9]*,5035\\.22[01][0-9]*[\r\n]+[^\r\n]+at 970\\.11[45][0-9]*,5000\\.26[01][0-9]*.*Entities: 0 "
        FAIL_REGULAR_EXPRESSION "error")
    # A file that is not there, and an option SURVEY does not have: refused.
    add_test(NAME cli.survey_refuses_a_missing_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${CMAKE_CURRENT_BINARY_DIR}/no such.fld\"")
    set_tests_properties(cli.survey_refuses_a_missing_file PROPERTIES WILL_FAIL TRUE)
    add_test(NAME cli.survey_refuses_an_unknown_option
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_fld}\" LAYER x")
    set_tests_properties(cli.survey_refuses_an_unknown_option PROPERTIES WILL_FAIL TRUE)
    # An RTK job, tests/surveyio/data/fld/gnss.fld: each coordinate written
    # " 2" (a blank before the opcode) with its date and time in the column
    # after it, and its receiver's "GNSS Solution" in an attribute group (124,
    # 125); a keyed-in 02; a bare 20. Detected without FORMAT; all 35 records
    # read (the version line and lines 11-37 and 41-47), two warnings - the
    # file is Windows-1252 (record 0), and R001's second GNSS position, at
    # record 41, differs from its first - no setup; CM1, keyed in, the one
    # point with coordinates; R001 to R004 placed by their 5 GNSS positions;
    # 2 features (KB 1 closed by the 20, KB 1 again after it); the system the
    # header names. IMPORT draws the 5 points where the file puts them, X the
    # easting, R001 by its first position: the reduction's entered point CM1
    # first, then the GNSS ones in the order the file names them.
    set(_fld_gnss "${PROJECT_SOURCE_DIR}/tests/surveyio/data/fld/gnss.fld")
    add_test(NAME cli.survey_read_gnss_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_fld_gnss}\"")
    set_tests_properties(cli.survey_read_gnss_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=gnss\\.fld format=opcode-field-file parser=1\\.2 read=35 skipped=0 warnings=2[\r\n]+content setups=0 observations=5 points=1 unpositioned=4 features=2 control=0[\r\n]+declared crs=\"Local grid A\" epsg=none[\r\n]+warning record=0 text=\"the file is not UTF-8[^\r\n]+[\r\n]+warning record=41 text=\"point 'R001' is given a GNSS position here"
        FAIL_REGULAR_EXPRESSION "error")
    add_test(NAME cli.survey_import_gnss_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_fld_gnss}\"" -c "LIST")
    set_tests_properties(cli.survey_import_gnss_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=5 layer=survey/points .* at 500000,6200000[\r\n]+[^\r\n]+ at 500010,6200020[\r\n]+[^\r\n]+ at 500020,6200020[\r\n]+[^\r\n]+ at 500020,6200030[\r\n]+[^\r\n]+ at 500040,6200040[\r\n]"
        FAIL_REGULAR_EXPRESSION "error")
    # A resection, tests/surveyio/data/fld/resection.fld: 128 sets up on S1,
    # whose block ("5" and "7" without their zero) runs to the 129, then
    # shots with 16, 42, 43 and 71, a coded check (06), and a 03 on K1.
    # READ: all 27 records and nothing to warn of - the two offsets are
    # applied to the shots they follow - 2 setups, 24 observations (8
    # pointings of 3), K1 and K2 entered, S1, 101, 102 and 103 unpositioned,
    # the EB 01, NS 01 and PF strings, and what the file does not carry: S1's
    # coordinates. IMPORT: the file does not state where S1 is, so the
    # reduction resects it (docs/survey.md, "The reduction's resection") from
    # its block - the pointings before the 129, K1 on both faces and K2 on
    # face left - and drew only K1, K2 and 103 before it could. By hand: S1
    # is 100 m from K1 and 141.421 m (100 sqrt 2 = 141.4214) from K2, which
    # are 100 m apart due east of each other, so S1 is 100 m due north of K1
    # - the mirror point south of K1 is ruled out by the readings, K2 being
    # read 45 degrees left of K1 - at 1000 E, 5100 N; K1 read at 180, its
    # azimuth, so the orientation is 0. K2's 141.421 is 0.36 mm short of that,
    # and the least squares of the four observations (worked apart by the
    # script of tests/survey/test_reduction_resection.cpp's model) puts S1 at
    # 1000.00033 E, 5100.00000 N, orientation 0.525". From there 101 is read
    # at 90, 50 m out, its tangential offset 0.25 m to the right making it
    # hypot(50, 0.25) m at 90 + atan(0.25 / 50): 1050.00033 E, 5099.74987 N;
    # 102 at 95, its radial offset making it 50.5 m: 1050.30815 E,
    # 5095.59851 N. The check after the 129 (06: K2 at 135.001, 141.420) is a
    # check of S1, a misclosure, not one of its pointings (checks=1). The
    # fixture's heights were written for the reader and disagree by 1.1 m:
    # 20.049 from K1 and 21.149 from K2, weighted by the zeniths (a pair at
    # 100 m, one face at 141.4 m) and the target heights' 2 mm - 2.249 and
    # 2.869 mm - give 20.46773 and, with one redundancy, both flagged at
    # |w| = 1.0993 m / hypot(2.249, 2.869) mm = 301.57, each with a
    # resection_residual record. K1, oriented on K2 due east at circle 90
    # (orientation 0), still radiates 103 at 45 deg, 10 m out (zenith 90):
    # 1000 + 10 sin 45 = 1007.071 E, 5000 + 10 cos 45 = 5007.071 N.
    set(_fld_resection "${PROJECT_SOURCE_DIR}/tests/surveyio/data/fld/resection.fld")
    add_test(NAME cli.survey_read_resection_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_fld_resection}\"")
    set_tests_properties(cli.survey_read_resection_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=resection\\.fld format=opcode-field-file parser=1\\.2 read=27 skipped=0 warnings=0[\r\n]+content setups=2 observations=24 points=2 unpositioned=4 features=3 control=0[\r\n]+declared crs=\"Local grid B\" epsg=none[\r\n]+not_carried .*not_carried text=\"no coordinates for the 1 setup\\(s\\) made by resection"
        FAIL_REGULAR_EXPRESSION "error|warning record")
    add_test(NAME cli.survey_import_resection_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_fld_resection}\"" -c "LIST")
    set_tests_properties(cli.survey_import_resection_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=6 layer=survey/points reduction_warnings=4[\r\n]+reduction method=[a-z]+ adjustments=[0-9]+ rejected=[0-9]+[\r\n]+reduction_warning text=\"height difference at S1: K1 is flagged by the Baarda w, critical 3\\.29 \\(value -301\\.57\\).*resection setup=S1 point=S1 from=K1,K2 northing=5100\\.0000[0-9]* easting=1000\\.00032[0-9]* height=20\\.46772[0-9]* [^\r\n]* redundancy=1\\+1 flagged=2 dilution=[0-9.]+ weak=0 checks=1 file_offset=none radiated_from=none radiation_offset=none unapplied_scale_factor=none[\r\n]+resection_residual setup=S1 observation=\"height difference at S1: K1\" [^\r\n]* state=flagged[\r\n]+resection_residual setup=S1 observation=\"height difference at S1: K2\" [^\r\n]* state=flagged[\r\n]+6 entities[\r\n]+[^\r\n]+ at 1000,5000[\r\n]+[^\r\n]+ at 1100,5000[\r\n]+[^\r\n]+ at 1000\\.00032[0-9]*,5100(\\.0000[0-9]*)?[\r\n]+[^\r\n]+ at 1050\\.00032[0-9]*,5099\\.74987[0-9]*[\r\n]+[^\r\n]+ at 1050\\.30814[0-9]*,5095\\.59850[0-9]*[\r\n]+[^\r\n]+ at 1007\\.07106[0-9]*,5007\\.07106[0-9]*"
        FAIL_REGULAR_EXPRESSION "error|has no position")
    # A free station in the structure of a field controller's resections,
    # tests/surveyio/data/fld/free_station.fld, every name and number
    # invented: M1, M2 and M3 entered; 128 sets up on FS1 (instrument height
    # 0); each mark read on both faces with its target height (1.600, 1.600,
    # 1.800) and level sights; 129; the controller's residual comments; M1
    # restated; a check (06) to M1; two shots. READ: 25 records, no warning;
    # 1 setup, 27 observations (9 pointings of 3), 3 points, FS1, 201 and 202
    # unpositioned, the EB 01 string. IMPORT resects FS1 - by hand at 300000 E,
    # 6200000 N, which the readings were made from: M1 100 m at azimuth
    # atan2(60, 80) = 36.869897646, M2 100 m at 233.130102354, M3 200 m at
    # 143.130102354 degrees, each read 12.5 less (the circle's turn) to 8
    # decimals - and radiates 201 at reading 102.5 (azimuth 115), 50 m:
    # 300000 + 50 sin 115 = 300045.31539 E, 6200000 + 50 cos 115 = 6199978.86909
    # N, and 202 at 192.5 (205), 30 m: 299987.32145 E, 6199972.81077 N. Its
    # height is the weighted mean of what the block's marks give with
    # curvature and refraction (level sights, instrument height 0): M1 and M2
    # 18.401 + 1.600 - 100^2 x 6.8278e-8 = 20.000317, M3 18.203 + 1.800 -
    # 200^2 x 6.8278e-8 = 20.000269, weighted 1 / ((S x 3"/sqrt 2)^2 +
    # (2 mm)^2) - the target height's 2 mm; the instrument's is common to all
    # and here 0 - so 2.249 mm at 100 m and 2.869 mm at 200 m: 20.00030586.
    # Redundancy 3 in plan (3 readings and 3 distances for N, E and the
    # orientation) and 2 in height; the check (06) to M1 after the 129 is a
    # check of FS1 (checks=1), not one of its pointings. Its precision is the
    # a-priori one (the readings fit exactly, so the global test does not fail
    # above), with no mark's centring or height (instrument height 0): by a
    # separate script of the textbook model (normal equations of the three
    # readings, weighted 1 / ((3"/sqrt 2)^2 + (1 mm / D)^2), and three
    # distances, weighted 1 / (hypot(2 mm, 2 ppm S)^2 + (1 mm)^2)), sN 1.142423
    # and sE 1.152938 mm; the height's, 1 / sqrt(sum 1 / sigma^2) over 2.248933,
    # 2.248933 and 2.868937 mm, 1.390860 mm.
    set(_fld_free_station "${PROJECT_SOURCE_DIR}/tests/surveyio/data/fld/free_station.fld")
    add_test(NAME cli.survey_read_free_station_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_fld_free_station}\"")
    set_tests_properties(cli.survey_read_free_station_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=free_station\\.fld format=opcode-field-file parser=1\\.2 read=25 skipped=0 warnings=0[\r\n]+content setups=1 observations=27 points=3 unpositioned=3 features=1 control=0[\r\n]+declared crs=\"Local grid D\" epsg=none"
        FAIL_REGULAR_EXPRESSION "error|warning record")
    add_test(NAME cli.survey_import_free_station_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_fld_free_station}\"" -c "LIST")
    set_tests_properties(cli.survey_import_free_station_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=6 layer=survey/points reduction_warnings=2[\r\n]+.*resection setup=FS1 point=FS1 from=M1,M2,M3 northing=(6200000|6199999\\.9999)[0-9.]* easting=(300000|299999\\.9999)[0-9.]* height=20\\.0003058[0-9]* sigma_n=0\\.001142422[0-9]* sigma_e=0\\.001152937[0-9]* sigma_h=0\\.001390860[0-9]* redundancy=3\\+2 flagged=0 dilution=[0-9.]+ weak=0 checks=1 file_offset=none radiated_from=none radiation_offset=none unapplied_scale_factor=none[\r\n]+6 entities[\r\n]+[^\r\n]+ at 300060,6200080[\r\n]+[^\r\n]+ at 299920,6199940[\r\n]+[^\r\n]+ at 300120,6199840[\r\n]+[^\r\n]+ at 300000,6200000[\r\n]+[^\r\n]+ at 300045\\.3153[0-9]*,6199978\\.869[0-9]*[\r\n]+[^\r\n]+ at 299987\\.3214[0-9]*,6199972\\.8107[0-9]*"
        FAIL_REGULAR_EXPRESSION "error|has no position|flagged by")
    # A setup on RTK marks, tests/surveyio/data/fld/rtk_setup.fld: A (1000,
    # 5000) and B (1000, 5100), each written " 2" with its receiver's
    # solution, a setup on A reading 30 degrees on B, and two shots at circle
    # 120, 10 m and 20 m out and level, each followed by an offset. READ: all
    # 18 records (the version line, 100, two coordinates with a group of
    # three records each, 03, 05, 04, 05, 07, 42, 07, 43), nothing to warn
    # of; 11 observations - the setup's 9 and the marks' GNSS positions - and
    # no point with coordinates of its own. IMPORT: A and B are GNSS
    # positions, which the reduction places before any setup, so the setup is
    # oriented on B, due north: 0 - 30 = -30 degrees, and circle 120 is due
    # east. 101 is 10 m east and its radial offset 0.5 m further out: 1010.5,
    # 5000; 102 is 20 m east and its tangential offset 0.5 m to the right
    # looking east, south: 1020, 4999.5. Were the marks calculated points,
    # which the reduction does not place, they would not orient it: the
    # circle taken as set would put 101 at 1008.66, 4995.
    set(_fld_rtk_setup "${PROJECT_SOURCE_DIR}/tests/surveyio/data/fld/rtk_setup.fld")
    add_test(NAME cli.survey_read_rtk_setup_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_fld_rtk_setup}\"")
    set_tests_properties(cli.survey_read_rtk_setup_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=rtk_setup\\.fld format=opcode-field-file parser=1\\.2 read=18 skipped=0 warnings=0[\r\n]+content setups=1 observations=11 points=0 unpositioned=4 features=1 control=0[\r\n]+declared crs=\"Local grid C\" epsg=none"
        FAIL_REGULAR_EXPRESSION "error|warning record")
    add_test(NAME cli.survey_import_rtk_setup_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_fld_rtk_setup}\"" -c "LIST")
    set_tests_properties(cli.survey_import_rtk_setup_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=4 layer=survey/points .* at 1000,5000[\r\n]+[^\r\n]+ at 1000,5100[\r\n]+[^\r\n]+ at 1010\\.5,5000[\r\n]+[^\r\n]+ at 1020,4999\\.5[\r\n]"
        FAIL_REGULAR_EXPRESSION "error|backsight has no position")
    # Two setups with two backsights to different points each,
    # tests/surveyio/data/fld/two_backsights.fld. A backsight orients its
    # setup where it reads a horizontal circle to a point with a position, or
    # states an azimuth; the first that can names the setup's backsight, and
    # the other is read as a measurement of its point. K (1000, 5000) first
    # backsights R3, entered due north, at circle 30, then R4, which nothing
    # places, at circle 120, 50 m out, and shoots 502 at circle 120, 10 m,
    # level; set up on K again, it first backsights R5, which nothing places,
    # at circle 0, 40 m out, then R3 at 30, and shoots 503 at circle 210,
    # 10 m. READ: all 13 records, and two warnings - each backsight to a point
    # nothing places (records 12 and 15) is read as a measurement of it.
    # IMPORT: R3 orients both setups, 0 - 30 = -30 degrees: circle 120 is due
    # east, so R4 is at 1050, 5000 and 502 at 1010, 5000; circle 0 is 330, so
    # R5 is at 1000 - 40 sin 30 = 980, 5000 + 40 cos 30 = 5034.641; circle 210
    # is due south, so 503 is at 1000, 4990. The last backsight naming it had
    # taken R4 for the first setup's with R3's circle, the reduction taking
    # that circle for a grid azimuth less the reading on R4 (30 - 120 = -90):
    # 502 at 1005, 5008.66. The first naming it took R5 for the second's,
    # its circle taken for a grid azimuth: 503 at 995, 4991.34.
    set(_fld_two_backsights "${PROJECT_SOURCE_DIR}/tests/surveyio/data/fld/two_backsights.fld")
    add_test(NAME cli.survey_read_two_backsights_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_fld_two_backsights}\"")
    set_tests_properties(cli.survey_read_two_backsights_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=two_backsights\\.fld format=opcode-field-file parser=1\\.2 read=13 skipped=0 warnings=2[\r\n]+content setups=2 observations=18 points=2 unpositioned=4 features=2 control=0[\r\n]+declared crs=\"Local grid G\" epsg=none[\r\n]+warning record=12 text=\"a backsight to 'R4', where setup K backsights 'R3' \\(record 11\\), the first that can orient it, with a horizontal circle, and coordinates the file gives: this one is read as a measurement of its point\"[\r\n]+warning record=15 text=\"a backsight to 'R5', where setup K \\(2\\) backsights 'R3' \\(record 16\\), the first that can orient it, with a horizontal circle, and coordinates the file gives: this one is read as a measurement of its point\""
        FAIL_REGULAR_EXPRESSION "error")
    add_test(NAME cli.survey_import_two_backsights_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_fld_two_backsights}\"" -c "LIST")
    set_tests_properties(cli.survey_import_two_backsights_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=6 layer=survey/points .* at 1000,5000[\r\n]+[^\r\n]+ at 1000,5100[\r\n]+[^\r\n]+ at 1050,5000[\r\n]+[^\r\n]+ at 1010,5000[\r\n]+[^\r\n]+ at 980,5034\\.641[0-9]*[\r\n]+[^\r\n]+ at 1000,4990[\r\n]"
        FAIL_REGULAR_EXPRESSION "error|backsight has no position")
    # The same verbs on the hand-built GSI-16 job
    # tests/surveyio/data/leica/rounded_seconds_gsi16.gsi, whose angle words
    # write 60 seconds where a writer rounded up without carrying the minute
    # (docs/survey.md, "Leica GSI"). READ: 6 records; 11 observations - every
    # shot's circle reading, zenith angle and distance but P3's circle
    # reading of 090 00 61.0, the one word refused; and 2 warnings, that
    # refusal and one for the three 60-second words, each read as the next
    # minute. IMPORT, worked by hand: STN1 (5000 E, 6000 N) is oriented on
    # CP01, keyed in due north, at circle 0. P1 at circle 044 59 60.0 = 45 deg
    # and zenith 089 59 60.0 = 90 deg, 100 m, is 100 m out at 45 deg: 5070.711
    # E 6070.711 N. P2 at 135 deg and zenith 086 27 60.0 = 86 deg 28 min, 200
    # m, is 200 sin(86 deg 28') = 199.6198 m out, less the curvature and
    # refraction term (1 - k/2) V H / R (reduceSlope; k 0.13, R 6 371 000 m)
    # of 0.4 mm: 199.6195 m at 135 deg, 5141.1523 E 5858.8477 N (worked with
    # a 75-digit pi). Read as 086 27 00 it would be 3.6 mm nearer (2.5 mm in
    # each coordinate, 5141.1497 E), and P1 read as 044 59 00 lands 2.9 cm
    # away; refused, as they were, neither is radiated and 2 points are drawn,
    # not 4. The READ warning lists the records of the three 60-second words
    # and shows the first as written and as read. A ';' in these expressions
    # is written "\\;": bare, CMake splits the expression there into a list of
    # alternatives, and the test passes on either fragment - these three did,
    # with the parser version wrong.
    set(_gsi "${PROJECT_SOURCE_DIR}/tests/surveyio/data/leica/rounded_seconds_gsi16.gsi")
    add_test(NAME cli.survey_read_gsi_reads_sixty_seconds_as_the_next_minute
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_gsi}\"")
    set_tests_properties(cli.survey_read_gsi_reads_sixty_seconds_as_the_next_minute PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=rounded_seconds_gsi16\\.gsi format=leica-gsi parser=1\\.2 read=6 skipped=0 warnings=2[\r\n]+content setups=1 observations=11 points=2 unpositioned=3 features=0 control=0[\r\n]+declared crs=none epsg=none[\r\n]+warning record=6 text=\"word 21 '0000000009000610' is not a sexagesimal angle: the seconds run to 59[^\r\n]*[\r\n]+warning record=4 text=\"3 angle words write 60 seconds, where a sexagesimal angle's seconds run to 59 \\(at records 4, 5\\; the first, word 21, 044 59 60\\.0, read as 045 00 00\\.0\\)[^\r\n]*read as the next minute, within the writer's rounding of what was measured\""
        FAIL_REGULAR_EXPRESSION "error")
    set(_gsi_p1 "at 5070\\.71[01][0-9]*,6070\\.71[01][0-9]*")
    set(_gsi_p2 "at 5141\\.1522[0-9]*,5858\\.8477[0-9]*")
    add_test(NAME cli.survey_import_gsi_radiates_the_shots_whose_angles_wrote_sixty_seconds
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_gsi}\" LAYER survey/gsi"
                -c "LIST")
    set_tests_properties(cli.survey_import_gsi_radiates_the_shots_whose_angles_wrote_sixty_seconds
        PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=4 layer=survey/gsi .*(${_gsi_p1}.*${_gsi_p2}|${_gsi_p2}.*${_gsi_p1})"
        FAIL_REGULAR_EXPRESSION "error")
    # A negative vertical reading, on the hand-built GSI-8 job
    # tests/surveyio/data/leica/negative_vertical_gsi8.gsi: no zenith angle is
    # negative - it is a damaged word, or an instrument's vertical angle
    # setting (GSI ONLINE's SET 44) reaching the file (docs/survey.md, "Leica
    # GSI"). READ: 5 records; 8 observations - every shot's circle reading,
    # zenith angle and distance but P1's V of -005 00 00.0, refused at record
    # 4 - and that one warning, with a note on what it suggests of the file's
    # other vertical readings. IMPORT, worked by hand: STN1 (5000 E, 6000 N)
    # is oriented on CP01, keyed in due north, at circle 0. P2 at circle -045
    # 00 00.0, the direction 315 deg, and zenith 90 deg, 100 m, is 100 m out
    # at 315 deg (reduceSlope's curvature and refraction term is a product
    # with SD cos z, zero at 90 deg): 5000 - 100 sin 45 deg = 4929.2893
    # E, 6000 + 100 cos 45 deg = 6070.7107 N. P1, with no zenith angle, is not
    # radiated: 3 points are drawn. Wrapped onto the circle, as the reader once
    # did, its V was 355 00 00.0, a face-right zenith of 5 deg, and P1 was
    # drawn 100 sin 5 deg = 8.7156 m out at 225 deg: 4993.8372 E 5993.8372 N,
    # which the FAIL expression refuses.
    set(_gsi_negative_v
        "${PROJECT_SOURCE_DIR}/tests/surveyio/data/leica/negative_vertical_gsi8.gsi")
    add_test(NAME cli.survey_read_gsi_refuses_a_negative_vertical_reading_at_its_record
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_gsi_negative_v}\"")
    set_tests_properties(cli.survey_read_gsi_refuses_a_negative_vertical_reading_at_its_record
        PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=negative_vertical_gsi8\\.gsi format=leica-gsi parser=1\\.2 read=5 skipped=0 warnings=1[\r\n]+content setups=1 observations=8 points=2 unpositioned=2 features=0 control=0[\r\n]+declared crs=none epsg=none[\r\n]+warning record=4 text=\"word 22 '-00500000' is negative, which no zenith angle is \\(a damaged word, or an instrument whose vertical angle setting - GSI ONLINE's SET 44[^\r\n]*\\; that value was not read\".*not_carried text=\"GSI does not record the vertical angle setting: word 22 was read as a zenith angle, the instruments' usual setting\\; 1 reading was refused as negative, which no zenith angle is \\(the first at record 4\\)"
        FAIL_REGULAR_EXPRESSION "error")
    add_test(NAME cli.survey_import_gsi_draws_no_point_from_a_negative_vertical_reading
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_gsi_negative_v}\" LAYER survey/gsi"
                -c "LIST")
    set_tests_properties(cli.survey_import_gsi_draws_no_point_from_a_negative_vertical_reading
        PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=3 layer=survey/gsi .*at 4929\\.2893[0-9]*,6070\\.7106[0-9]*"
        FAIL_REGULAR_EXPRESSION "error|at 4993\\.8")
    # A shot's own horizontal distance and height difference (32, 33), on the
    # hand-built GSI-8 job tests/surveyio/data/leica/derived_distances_gsi8.gsi
    # (docs/survey.md, "Leica GSI"). Each shot's slope distance is 80.000 m,
    # and its 32 and 33 are what an instrument computes for that at a zenith
    # of 094 30 00.0: 80 sin 94.5 deg = 79.753 m, 80 cos 94.5 deg = -6.277 m.
    # Q1's V, -004 30 00.0, is refused as negative and Q2 has none, so neither
    # slope distance can be reduced to horizontal; Q3's V is read. Worked by
    # hand (a 75-digit pi): STN1 (5000 E, 6000 N) is oriented on CP01, keyed
    # in due north, at circle 0. Q1 at 52 deg and Q2 at 142 deg are radiated
    # by their own 79.753 m, a recorded horizontal distance and used as it
    # is: 5062.8462 E 6049.1008 N and 5049.1008 E 5937.1538 N. Q3 at 232 deg
    # is radiated by its slope distance and zenith angle: 80 sin 94.5 deg =
    # 79.75339 m, plus the curvature and refraction term (1 - k/2) V H / R of
    # +0.07 mm (V = 80 cos 94.5 deg is negative), 79.75346 m - 4937.1534 E
    # 5950.8989 N, 0.4 mm from where its own 32 would put it (4937.1538 E).
    # The import says that Q3's 32 and 33 were not carried, and that Q1's and
    # Q2's slope distances were not. Parser 1.1 dropped Q1's and Q2's 32 and
    # 33 as derived too, said so of 3 blocks "beside the measured slope
    # distance and zenith angle", and drew 3 points, not 5.
    set(_gsi_derived "${PROJECT_SOURCE_DIR}/tests/surveyio/data/leica/derived_distances_gsi8.gsi")
    add_test(NAME cli.survey_import_gsi_radiates_a_shot_with_no_zenith_angle_by_its_own_horizontal_distance
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_gsi_derived}\" LAYER survey/gsi"
                -c "LIST")
    set_tests_properties(cli.survey_import_gsi_radiates_a_shot_with_no_zenith_angle_by_its_own_horizontal_distance
        PROPERTIES
        PASS_REGULAR_EXPRESSION
        "parser=1\\.2 read=6 skipped=0 warnings=1[\r\n]+content setups=1 observations=12 .*not_carried text=\"1 block's instrument-computed horizontal distance \\(32\\) or height difference \\(33\\) beside a zenith angle and a distance that were read: not carried[^\r\n]*[\r\n]+not_carried text=\"2 blocks have a slope distance \\(31\\) and no zenith angle that was read \\(none recorded, or one refused at its record\\; the first at record 4\\)[^\r\n]*[\r\n]+settings file= set=0 differ=0[\r\n]+imported job=job-1 entities=5 layer=survey/gsi .*at 5062\\.8462[0-9]*,6049\\.1008[0-9]*[\r\n]+[^\r\n]*at 5049\\.1008[0-9]*,5937\\.1537[0-9]*[\r\n]+[^\r\n]*at 4937\\.1534[0-9]*,5950\\.8988[0-9]*"
        FAIL_REGULAR_EXPRESSION "error")
    # CRS SET with a WKT - GDA2020 / MGA zone 56 as OGC WKT1, its names in
    # double quotes and one of them holding blanks - as an agent or a script
    # passes it: the verb once lost the quotes with the line's tokens, and
    # PROJ refused the WKT "InvalidCRS: not a coordinate system"
    # (test_project_crs.cpp). A WKT whose names hold no blank - an ESRI .prj's
    # GDA2020_MGA_Zone_56 - PROJ reads even without its quotes, so it could
    # not show the fault.
    add_test(NAME cli.crs_set_takes_a_wkt_with_its_quotes
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "CRS SET PROJCS[\"GDA2020 / MGA zone 56\",GEOGCS[\"GDA2020\",DATUM[\"Geocentric_Datum_of_Australia_2020\",SPHEROID[\"GRS 1980\",6378137,298.257222101]],PRIMEM[\"Greenwich\",0],UNIT[\"degree\",0.0174532925199433]],PROJECTION[\"Transverse_Mercator\"],PARAMETER[\"latitude_of_origin\",0],PARAMETER[\"central_meridian\",153],PARAMETER[\"scale_factor\",0.9996],PARAMETER[\"false_easting\",500000],PARAMETER[\"false_northing\",10000000],UNIT[\"metre\",1],AUTHORITY[\"EPSG\",\"7856\"]]")
    set_tests_properties(cli.crs_set_takes_a_wkt_with_its_quotes PROPERTIES
        PASS_REGULAR_EXPRESSION "crs id=EPSG:7856 name=\"GDA2020 / MGA zone 56\" kind=\"projected\" units=metre"
        FAIL_REGULAR_EXPRESSION "error")
endif()

# SURVEY READ and SURVEY IMPORT on the hand-built Sokkia SDR33 file
# tests/surveyio/data/sdr/traverse.sdr. READ: 40 records read and 7 skipped
# (lines 33 to 39, deleted with "DD" and "DDDD"), 2 setups, 36 observations
# (12 face 1 and face 2 records of 3), CP1 and CP2 keyed in, T1 and T2
# observed, the CTRL and TRAV codes; 8 warnings: one at the first record
# that states coordinates (8), since the header's coordinate order, option
# 2, is read east first where no Sokkia file shows the order, and one for
# each deleted record. IMPORT reduces it with the defaults and draws 4
# points on LAYER, one undo step. Worked by hand: from CP1 (500000 E,
# 5000000 N) oriented on CP2 due north at circle 0, T1 is at circle 90,
# zenith 87 on both faces, the mean of its four slope distances 150.0015 m;
# 150.0015 sin 87 = 149.7959282 m east, less the curvature and refraction
# term (1 - k/2) V H / R with the reduction's own k = 0.13
# (kDefaultRefractionCoefficient: the file's 0.14 is the field book's, for
# its reduced views, and would move T1 by a micrometre) and R = 6371000 m:
# 0.935 x 7.8505 x 149.7959 / 6371000 = 0.1726 mm, so 500149.7957556 E,
# 5000000 N. From T1, its circle zeroed on CP1 (the azimuth back to CP1 is
# 270), T2 at circle 90 is at azimuth 90 + 270 - 0 = 360, due north, zenith
# 91, 80.001 m: 80.001 sin 91 = 79.9888155 m north, plus the same term,
# 0.935 x 1.39622 x 79.98882 / 6371000 = 0.0164 mm (V is negative):
# 5000079.9888319 N. LIST writes 12 significant digits, so the regexes hold
# the easting to a micrometre and the northing to ten; without the curvature
# and refraction term T1 would be at 500149.795928.
if(KATANA_BUILD_TESTS)
    set(_sdr "${PROJECT_SOURCE_DIR}/tests/surveyio/data/sdr/traverse.sdr")
    add_test(NAME cli.survey_read_sokkia_sdr
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_sdr}\"" -c "STATUS")
    set_tests_properties(cli.survey_read_sokkia_sdr PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=traverse\\.sdr format=sokkia-sdr parser=1\\.0 read=40 skipped=7 warnings=8[\r\n]+content setups=2 observations=36 points=2 unpositioned=2 features=2 control=0[\r\n]+declared crs=none epsg=none[\r\n]+warning record=8 text=\"the header's coordinate order option is 2, east-north-elevation, and the coordinates are read easting first[^\r\n]*check a known point\"[\r\n]+warning record=33 text=\"a deleted record \\(marked 'DD'\\), 01NM \\(instrument\\), is not imported\".*warning record=39 text=\"a deleted record \\(marked 'DDDD'\\), 13TS \\(note\\), is not imported\".*Entities: 0 "
        FAIL_REGULAR_EXPRESSION "error")
    add_test(NAME cli.survey_import_sokkia_sdr
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_sdr}\" LAYER survey/sdr"
                -c "LIST" -c "UNDO" -c "STATUS")
    set_tests_properties(cli.survey_import_sokkia_sdr PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=4 layer=survey/sdr .*layer=survey/sdr  at 500000,5000000[\r\n]+[^\r\n]+at 500000,5000100[\r\n]+[^\r\n]+at 500149\\.795756,5000000[\r\n]+[^\r\n]+at 500149\\.795756,5000079\\.98883[\r\n].*Entities: 0 "
        FAIL_REGULAR_EXPRESSION "error")
endif()

# SURVEY IMPORT's reduction settings (survey_verbs.hpp, docs/survey.md "The
# reduction settings of SURVEY IMPORT"): SET's items and a SETTINGS file,
# both the settings' text form. The file is
# tests/surveyio/data/sdr/traverse_without_coordinates.sdr - traverse.sdr
# with its two 08 coordinate records taken out - so without control it
# places nothing; FORWARD puts CP1 on the drawing at 500000 E 5000000 N
# 100 Z (1 m due north of 500000,4999999), and control=CP1;drawing;fixed;...
# holds it there. Worked by hand (the working is repeated, with the heights,
# in tests/app/test_survey_verbs.cpp): setup CP1 is oriented on the 07's
# azimuth 0 less its reading 0 on CP2, the backsight, which nothing else
# places. CP2, four pointings of 100.006 m at zenith 89.36977 (face right
# 270.63023), is H = 100.006 sin 89.36977 = 99.9999502 m due north less the
# curvature and refraction term (1 - k/2) V H / R = 0.935 x 1.1000027 x
# 99.99995 / 6371000 = 0.0161 mm, k = 0.13 and R = 6 371 000 m the
# reduction's own: 5000099.9999340 N. T1 and T2 are where
# cli.survey_import_sokkia_sdr has them, worked there: 500149.7957556 E
# 5000000 N and 500149.7957556 E 5000079.9888319 N - T2's setup is oriented
# on CP1 at 270 by the coordinates, as it was by the file's. CP1 itself is
# the drawing's point and is not drawn again: 3 points. A job is numbered
# with the next entity number (cad's newJobId), so the job numbers below
# count what the lines before made. With
# curvature_refraction=false the terms go: CP2 5000099.9999502 N, T1
# 500000 + 150.0015 sin 87 = 500149.7959282 E, T2 5000079.9888155 N. LIST
# writes 12 significant digits. Each refusal is followed by STATUS, which
# must not run: katana_cli stops at the first failing line.
if(KATANA_BUILD_TESTS)
    set(_sdr_bare "${PROJECT_SOURCE_DIR}/tests/surveyio/data/sdr/traverse_without_coordinates.sdr")
    set(_settings "${PROJECT_SOURCE_DIR}/tests/app/data/survey_settings")
    set(_place_cp1 "FORWARD 500000,4999999,100 0 1 0 CP1")
    # ';' separates a CMake list's items, so a control line's are written
    # $<SEMICOLON> in a command (a '\;' reaches the program as it is); in a
    # regex, where a ';' would make two regexes either of which passes the
    # test, '.' stands for it.
    set(_s "$<SEMICOLON>")
    set(_hold_cp1 "control=CP1${_s}drawing${_s}fixed${_s}0${_s}fixed${_s}0${_s}fixed${_s}0")
    set(_held_cp1 "setting key=control value=CP1.drawing.fixed.0.fixed.0.fixed.0[\r\n]")
    set(_held_points "layer=0  at 500000,5000000[\r\n]+[^\r\n]+layer=survey/points  at 500000,5000099\\.99993[\r\n]+[^\r\n]+at 500149\\.795756,5000000[\r\n]+[^\r\n]+at 500149\\.795756,5000079\\.98883[\r\n]")
    # The file alone first: it places nothing. The three reduction warnings
    # of the held import are the humidity, the atmospheric correction and
    # the orientation on the file's azimuth; CP1 is not among points "named
    # with no coordinates", since the drawing gives it a position.
    add_test(NAME cli.survey_import_holds_a_point_of_the_drawing
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_sdr_bare}\""
                -c "${_place_cp1}" -c "SURVEY IMPORT \"${_sdr_bare}\" SET ${_hold_cp1}"
                -c "LIST")
    set_tests_properties(cli.survey_import_holds_a_point_of_the_drawing PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=0 .*settings file= set=1 differ=1[\r\n]+${_held_cp1}imported job=job-3 entities=3 layer=survey/points reduction_warnings=3[\r\n].*${_held_points}"
        FAIL_REGULAR_EXPRESSION "error")
    add_test(NAME cli.survey_import_takes_its_settings_from_a_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_place_cp1}"
                -c "SURVEY IMPORT \"${_sdr_bare}\" SETTINGS \"${_settings}/hold_cp1.txt\""
                -c "LIST")
    set_tests_properties(cli.survey_import_takes_its_settings_from_a_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "settings file=hold_cp1\\.txt set=0 differ=1[\r\n]+${_held_cp1}settings_warning text=\"line 8: setting 'later\\.setting' is not known to this version and was ignored\"[\r\n]+imported job=job-2 entities=3 .*${_held_points}"
        FAIL_REGULAR_EXPRESSION "error")
    add_test(NAME cli.survey_import_set_changes_a_key_the_settings_file_leaves
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_place_cp1}"
                -c "SURVEY IMPORT \"${_sdr_bare}\" SETTINGS \"${_settings}/hold_cp1.txt\" SET curvature_refraction=false"
                -c "LIST")
    set_tests_properties(cli.survey_import_set_changes_a_key_the_settings_file_leaves PROPERTIES
        PASS_REGULAR_EXPRESSION
        "settings file=hold_cp1\\.txt set=1 differ=2[\r\n]+setting key=curvature_refraction value=false[\r\n]+${_held_cp1}.*at 500000,5000099\\.99995[\r\n]+[^\r\n]+at 500149\\.795928,5000000[\r\n]+[^\r\n]+at 500149\\.795928,5000079\\.98882[\r\n]"
        FAIL_REGULAR_EXPRESSION "error")
    # Every refusal the options document, each in the words of the parser,
    # the validation or the reduction that makes it. Named arguments, not a
    # table: a list would split the control lines at their ';'.
    function(katana_survey_refusal name options said)
        add_test(NAME cli.survey_import_refuses_${name}
            COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                    $<TARGET_FILE:katana_cli> -c "${_place_cp1}"
                    -c "SURVEY IMPORT \"${_sdr_bare}\" ${options}" -c "STATUS")
        set_tests_properties(cli.survey_import_refuses_${name} PROPERTIES
            PASS_REGULAR_EXPRESSION "error: ${said}"
            FAIL_REGULAR_EXPRESSION "Entities:|imported job")
    endfunction()
    katana_survey_refusal(an_unknown_setting "SET bogus=1"
        "InvalidArgument: 'bogus' is not a reduction setting; the settings are atmospheric, .*, iterations\\.max, control \\[SET bogus=1\\]")
    katana_survey_refusal(a_setting_that_is_not_a_number "SET refraction.k=abc"
        "ParseFailure: line 2 of the reduction settings: 'abc' is not a number \\(refraction\\.k\\) \\[SET refraction\\.k=abc\\]")
    katana_survey_refusal(a_setting_outside_its_range "SET refraction.k=2"
        "InvalidArgument: the coefficient of refraction must lie between -1 and 1 \\[SET refraction\\.k=2\\]")
    katana_survey_refusal(settings_from_a_newer_katana "SETTINGS \"${_settings}/newer_version.txt\""
        "Unsupported: these reduction settings were written by a newer Katana \\(settings version 2\\); this one reads up to version 1 \\[SETTINGS [^\r\n]*newer_version\\.txt\\]")
    katana_survey_refusal(a_settings_file_that_is_not_there "SETTINGS \"${_settings}/no such settings.txt\""
        "FileImportFailure: the settings file cannot be read \\[[^\r\n]*no such settings\\.txt: ")
    katana_survey_refusal(a_control_point_the_drawing_does_not_have
        "SET control=CP9${_s}drawing${_s}fixed${_s}0${_s}fixed${_s}0${_s}free${_s}0"
        "NotFound: Control point CP9 is not on the drawing, so it cannot be held\\.")
    katana_survey_refusal(a_control_point_the_file_gives_no_coordinates
        "SET control=CP1${_s}file${_s}fixed${_s}0${_s}fixed${_s}0${_s}free${_s}0"
        "NotFound: Control point CP1 is not a point with coordinates in the file, so it cannot be held\\.")
    katana_survey_refusal(a_set_with_no_items "SET"
        "InvalidArgument: SET needs at least one <key>=<value>; usage: ")
    katana_survey_refusal(a_setting_given_twice "SET atmospheric=none atmospheric=auto"
        "InvalidArgument: 'atmospheric' is given twice \\[SET atmospheric=auto\\]")
    katana_survey_refusal(a_control_point_held_twice
        "SET ${_hold_cp1} control=CP1${_s}drawing${_s}free${_s}0${_s}free${_s}0${_s}fixed${_s}0"
        "InvalidArgument: control point 'CP1' is listed twice")
    # CP1 drawn twice, 100 km apart: the reduction would hold whichever was
    # drawn first, so the import refuses to hold it and names both places,
    # to a tenth of a millimetre (cad::reduceForDrawing).
    add_test(NAME cli.survey_import_refuses_a_point_the_drawing_has_at_two_places
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_place_cp1}"
                -c "FORWARD 600000,4999999,100 0 1 0 CP1"
                -c "SURVEY IMPORT \"${_sdr_bare}\" SET ${_hold_cp1}" -c "STATUS")
    set_tests_properties(cli.survey_import_refuses_a_point_the_drawing_has_at_two_places
        PROPERTIES
        PASS_REGULAR_EXPRESSION
        "error: InvalidArgument: Control point CP1 is on the drawing 2 times, at E 500000\\.0000 N 5000000\\.0000 Z 100\\.0000 and E 600000\\.0000 N 5000000\\.0000 Z 100\\.0000, so which one to hold is not known. rename or delete all but one\\."
        FAIL_REGULAR_EXPRESSION "Entities:|imported job")
    # After the import, where CP1 was held - the drawing's entity 1, the
    # point FORWARD made - and what the adjustment made of the data: nothing
    # adjusted and nothing rejected, as the defaults have it. A number is the
    # shortest text that reads back exactly, so 5 000 000 is 5e+06.
    add_test(NAME cli.survey_import_says_where_it_held_and_what_it_adjusted
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "${_place_cp1}"
                -c "SURVEY IMPORT \"${_sdr_bare}\" SET ${_hold_cp1}")
    set_tests_properties(cli.survey_import_says_where_it_held_and_what_it_adjusted PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-2 entities=3 layer=survey/points reduction_warnings=3[\r\n]+held id=CP1 from=drawing entity=1 northing=5e\\+06 easting=5e\\+05 height=100[\r\n]+reduction method=radiation adjustments=0 rejected=0[\r\n]"
        FAIL_REGULAR_EXPRESSION "error")
    # A network, the import wizard's own fixture: two setups, on A and on B,
    # each with a direction and a distance to the other and to P and Q. The
    # network takes a setup's directions as angles from its backsight: 4
    # angles and 6 distances, 10 observations; A and B held in plan, the
    # unknowns are P and Q, 4; a redundancy of 6 (worked in
    # tests/app/test_survey_verbs.cpp, which holds the variance factor and
    # the global test to the reduction's own report).
    add_test(NAME cli.survey_import_says_what_a_network_adjustment_made_of_the_data
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "SURVEY IMPORT \"${PROJECT_SOURCE_DIR}/tests/qt_widgets/survey/data/network_gsi8.gsi\" SET adjustment.method=network control=A${_s}file${_s}fixed${_s}0${_s}fixed${_s}0${_s}free${_s}0 control=B${_s}file${_s}fixed${_s}0${_s}fixed${_s}0${_s}free${_s}0")
    set_tests_properties(cli.survey_import_says_what_a_network_adjustment_made_of_the_data
        PROPERTIES
        PASS_REGULAR_EXPRESSION
        "reduction method=\"network, horizontal\" adjustments=1 rejected=[0-9]+[\r\n]+adjustment method=\"network least squares \\(horizontal\\)\" observations=10 unknowns=4 redundancy=6 variance_factor=[0-9][0-9.e+-]* global_test=(passed|failed) flagged=[0-9]+ rejected=[0-9]+[\r\n]"
        FAIL_REGULAR_EXPRESSION "error")
endif()
