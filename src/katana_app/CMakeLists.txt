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
    # that plots fails here saying so. Each stops the batch (exit 1).
    foreach(_verb IN ITEMS "PLOT a.pdf" "PLOTSHEETS" "SNAPSHOT a.png" "ONLINE PROVIDERS"
                           "SCRIPT a.kcs")
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
        "survey file=setup\\.fld format=opcode-field-file parser=1\\.1 read=16 skipped=2 warnings=2[\r\n]+content setups=1 observations=12 points=2 unpositioned=2 features=1 control=0[\r\n]+declared crs=\"Australia/GDA2020, Zone 56\" epsg=none[\r\n]+warning record=20 [^\r\n]+[\r\n]+warning record=21 text=\"opcode 18 \\(circle feature\\) is not one this reader imports\".*Entities: 0 "
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
        "survey file=gnss\\.fld format=opcode-field-file parser=1\\.1 read=35 skipped=0 warnings=2[\r\n]+content setups=0 observations=5 points=1 unpositioned=4 features=2 control=0[\r\n]+declared crs=\"Local grid A\" epsg=none[\r\n]+warning record=0 text=\"the file is not UTF-8[^\r\n]+[\r\n]+warning record=41 text=\"point 'R001' is given a GNSS position here"
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
    # coordinates. IMPORT: the file does not state
    # where S1 is and the reduction computes no resection, so nothing is
    # radiated from it, which the report says; K1, oriented on K2 due east at
    # circle 90 (orientation 0), radiates 103 at 45 deg, 10 m out (zenith 90):
    # 1000 + 10 sin 45 = 1007.071 E, 5000 + 10 cos 45 = 5007.071 N.
    set(_fld_resection "${PROJECT_SOURCE_DIR}/tests/surveyio/data/fld/resection.fld")
    add_test(NAME cli.survey_read_resection_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_fld_resection}\"")
    set_tests_properties(cli.survey_read_resection_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=resection\\.fld format=opcode-field-file parser=1\\.1 read=27 skipped=0 warnings=0[\r\n]+content setups=2 observations=24 points=2 unpositioned=4 features=3 control=0[\r\n]+declared crs=\"Local grid B\" epsg=none[\r\n]+not_carried .*not_carried text=\"no coordinates for the 1 setup\\(s\\) made by resection"
        FAIL_REGULAR_EXPRESSION "error|warning record")
    add_test(NAME cli.survey_import_resection_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_fld_resection}\"" -c "LIST")
    set_tests_properties(cli.survey_import_resection_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=3 layer=survey/points .*reduction_warning text=\"Setup S1 stands on S1, which has no position: nothing was computed from it\\.\".* at 1000,5000[\r\n]+[^\r\n]+ at 1100,5000[\r\n]+[^\r\n]+ at 1007\\.07[01][0-9]*,5007\\.07[01][0-9]*"
        FAIL_REGULAR_EXPRESSION "error")
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
        "survey file=rtk_setup\\.fld format=opcode-field-file parser=1\\.1 read=18 skipped=0 warnings=0[\r\n]+content setups=1 observations=11 points=0 unpositioned=4 features=1 control=0[\r\n]+declared crs=\"Local grid C\" epsg=none"
        FAIL_REGULAR_EXPRESSION "error|warning record")
    add_test(NAME cli.survey_import_rtk_setup_field_file
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_fld_rtk_setup}\"" -c "LIST")
    set_tests_properties(cli.survey_import_rtk_setup_field_file PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=4 layer=survey/points .* at 1000,5000[\r\n]+[^\r\n]+ at 1000,5100[\r\n]+[^\r\n]+ at 1010\\.5,5000[\r\n]+[^\r\n]+ at 1020,4999\\.5[\r\n]"
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
    # E 6070.711 N. P2 at 135 deg and zenith 084 01 60.0 = 84 deg 02 min, 200
    # m, is 200 sin(84 deg 02') = 198.9165 m out, less the curvature and
    # refraction term (1 - k/2) V H / R (reduceSlope; k 0.13, R 6 371 000 m)
    # of 0.6 mm: 198.9159 m at 135 deg, 5140.655 E 5859.345 N. Read as 084 01
    # 00 it would be 6.1 mm nearer (4.3 mm in each coordinate, 5140.6505 E),
    # and P1 read as 044 59 00 lands 2.9 cm away; refused, as they were,
    # neither is radiated and 2 points are drawn, not 4. The READ warning
    # lists the records of the three 60-second words and shows the first as
    # written and as read.
    set(_gsi "${PROJECT_SOURCE_DIR}/tests/surveyio/data/leica/rounded_seconds_gsi16.gsi")
    add_test(NAME cli.survey_read_gsi_reads_sixty_seconds_as_the_next_minute
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY READ \"${_gsi}\"")
    set_tests_properties(cli.survey_read_gsi_reads_sixty_seconds_as_the_next_minute PROPERTIES
        PASS_REGULAR_EXPRESSION
        "survey file=rounded_seconds_gsi16\\.gsi format=leica-gsi parser=1\\.1 read=6 skipped=0 warnings=2[\r\n]+content setups=1 observations=11 points=2 unpositioned=3 features=0 control=0[\r\n]+declared crs=none epsg=none[\r\n]+warning record=6 text=\"word 21 '0000000009000610' is not a sexagesimal angle: the seconds run to 59[^\r\n]*[\r\n]+warning record=4 text=\"3 angle words write 60 seconds, where a sexagesimal angle's seconds run to 59 \\(at records 4, 5; the first, word 21, 044 59 60\\.0, read as 045 00 00\\.0\\)[^\r\n]*read as the next minute, within the writer's rounding of what was measured\""
        FAIL_REGULAR_EXPRESSION "error")
    set(_gsi_p1 "at 5070\\.71[01][0-9]*,6070\\.71[01][0-9]*")
    set(_gsi_p2 "at 5140\\.65[45][0-9]*,5859\\.34[45][0-9]*")
    add_test(NAME cli.survey_import_gsi_radiates_the_shots_whose_angles_wrote_sixty_seconds
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "SURVEY IMPORT \"${_gsi}\" LAYER survey/gsi"
                -c "LIST")
    set_tests_properties(cli.survey_import_gsi_radiates_the_shots_whose_angles_wrote_sixty_seconds
        PROPERTIES
        PASS_REGULAR_EXPRESSION
        "imported job=job-1 entities=4 layer=survey/gsi .*(${_gsi_p1}.*${_gsi_p2}|${_gsi_p2}.*${_gsi_p1})"
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
# observed, the CTRL and TRAV codes; 7 warnings, one for each deleted
# record (the header's coordinate order, option 2, is the format's own
# E-N-Elev and needs none). IMPORT reduces it with the defaults and draws 4
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
        "survey file=traverse\\.sdr format=sokkia-sdr parser=1\\.0 read=40 skipped=7 warnings=7[\r\n]+content setups=2 observations=36 points=2 unpositioned=2 features=2 control=0[\r\n]+declared crs=none epsg=none[\r\n]+warning record=33 text=\"a deleted record \\(marked 'DD'\\), 01NM \\(instrument\\), is not imported\".*warning record=39 text=\"a deleted record \\(marked 'DDDD'\\), 13TS \\(note\\), is not imported\".*Entities: 0 "
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
