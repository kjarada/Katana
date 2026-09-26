# GIS > Check - GDAL > Check Geometry... in the real window
# (docs/geoprocessing.md, "V4"), driven headlessly through
# tools/check_screenshot.cmake: the bow-tie (30,0) (40,10) (40,0) (30,10)
# crosses itself at (35,5), by hand; the dialog's Run hands its line to the
# window's one executor, the problem fills the table, and a marker is drawn
# on the layer asked for.
add_test(NAME qt_gis_check_dialog_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>PLINE 30,0 40,10 40,0 30,10 CLOSE|@gisCheck|!gisCheckScopeDrawing|gisCheckMarkers=qa/geometry|?gisCheckCommand|!gisCheckRun|?gisCheckStatus|?gisCheckProblems|>LIST"
        "-DEXPECT=gisCheckCommand: GIS CHECK DRAWING markers=qa/geometry.*gisCheckStatus: 1 problems on 1 entities.*gisCheckProblems: self-intersection \\| 1 \\| 35,5 \\| \\| Self-intersection.*2 entities.*Point  layer=qa/geometry"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gis_check_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_gis_check_dialog_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
