# Headless command-line front end. Proves the engine is complete without a GUI.
add_executable(katana_cli main.cpp dxf_verbs.cpp dxf_verbs.hpp)
katana_target_defaults(katana_cli)
target_link_libraries(katana_cli PRIVATE katana_cad katana_archive12d katana_dxf)

# Interoperability is optional: with -DKATANA_BUILD_IO=OFF the CLI still builds
# and simply has no IMPORT/EXPORT verbs, which is what the sanitizer job needs.
if(TARGET katana_interop)
    target_link_libraries(katana_cli PRIVATE katana_interop)
    target_compile_definitions(katana_cli PRIVATE KATANA_WITH_INTEROP=1)
endif()

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
endif()
