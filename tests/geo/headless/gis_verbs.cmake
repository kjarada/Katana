# IMPORT, EXPORT, REFS in the real window (docs/interop.md, "IMPORT, EXPORT,
# INFO, REFS and COPC on every front end"), driven headlessly through
# tools/check_screenshot.cmake: the executor katana_cli runs, run here as a
# background job the headless run waits for. Included from
# tests/CMakeLists.txt when the window is built.
#
# The samples' facts, by hand: parcels.geojson holds 8 features - the spoil
# heaps a MultiPolygon of two, so 9 entities - spanning (180, 0) to
# (365, 165), in EPSG:32630.
set(_gis_verbs_samples "${PROJECT_SOURCE_DIR}/samples/gis")
set(_gis_verbs_out "${CMAKE_CURRENT_BINARY_DIR}/qt gis verbs")

add_test(NAME qt_gis_verbs_folder
    COMMAND ${CMAKE_COMMAND} -E make_directory "${_gis_verbs_out}")
set_tests_properties(qt_gis_verbs_folder PROPERTIES FIXTURES_SETUP qt_gis_verbs)

# The IMPORT line katana_cli runs in cli.gis_import_line_gives_the_records_the_window_gives
# (src/katana_app/geo/cli/gis_verbs.cmake), typed here, with the same expected
# records: ALONGSIDE puts the parcels' corner (180, 0) on the rectangle's
# (1000, 2000).
add_test(NAME qt_gis_import_line_gives_the_sessions_records_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>RECT 1000,2000 1010,2005|>IMPORT \"${_gis_verbs_samples}/parcels.geojson\" ALONGSIDE"
        "-DEXPECT=imported file=\"?[^ ]*/parcels\\.geojson\"? kind=vector entities=9 layers=1 features=8 skipped=0 bounds=1000,2000,1185,2165 crs=\"[^\"]*EPSG:32630[^\"]*\"[\r\n]+placed placement=alongside east=820 north=2000 text=\"ALONGSIDE: moved as one piece by 820\\.000,2000\\.000, so its lower-left corner sits on the drawing's, at 1000\\.000,2000\\.000\\.\""
        "-DFORBID=Far from|no one to ask"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gis_import_records_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

# The line as a dialog runs it (runVerbLine): the job waited for, the records
# handed back as its reply, and the import one undo step.
add_test(NAME qt_gis_import_through_the_windows_executor_is_one_undo_step_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=<IMPORT \"${_gis_verbs_samples}/parcels.geojson\" LOCAL|>LIST|>UNDO|>LIST"
        "-DEXPECT=--run-line IMPORT [^\r\n]*: ok=yes[\r\n]+  reply: imported file=\"?[^ ]*/parcels\\.geojson\"? kind=vector entities=9 [^\r\n]*bounds=0,0,185,165[^\r\n]*[\r\n]+  reply: placed placement=local east=-180 north=0 .*9 entities.*1 undone.*0 entities"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gis_import_undo_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

# A file given on the window's command line is the IMPORT line File > Import
# makes; REFS then says there is no reference layer, and EXPORT writes the
# nine entities to a GeoPackage in a folder with a blank in its name.
add_test(NAME qt_gis_refs_and_export_after_an_import_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DIMPORT=${_gis_verbs_samples}/parcels.geojson"
        "-DDRIVE=>REFS|>EXPORT \"${_gis_verbs_out}/qt parcels.gpkg\""
        "-DEXPECT=imported file=\"?[^ ]*/parcels\\.geojson\"? kind=vector entities=9 .*references rasters=0 clouds=0.*exported file=\"[^\"]*/qt parcels\\.gpkg\" kind=vector driver=GPKG features=9 skipped=0"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gis_refs_export_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_gis_refs_and_export_after_an_import_headless PROPERTIES
    FIXTURES_REQUIRED qt_gis_verbs)

set_tests_properties(qt_gis_verbs_folder
    qt_gis_import_line_gives_the_sessions_records_headless
    qt_gis_import_through_the_windows_executor_is_one_undo_step_headless
    qt_gis_refs_and_export_after_an_import_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
