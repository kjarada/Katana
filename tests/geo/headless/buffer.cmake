# GIS > Analysis - GDAL > Buffer... in the real window (docs/geoprocessing.md,
# "V1"), driven headlessly through tools/check_screenshot.cmake: the dialog is
# opened by its menu action, filled by object name, and its Run hands the
# line it shows to the window's one executor - a job the headless run waits
# for. By hand: a 100 m line buffered 1 m either side with flat ends is a
# 100 x 2 rectangle, 200 m2, which LIST then shows on gis/buffer.
add_test(NAME qt_gis_buffer_dialog_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>LINE 0,0 100,0|@gisBuffer|!gisBufferScopeDrawing|gisBufferDistance=1|gisBufferCaps=flat|?gisBufferCommand|!gisBufferRun|?gisBufferStatus|?gisBufferReply|>LIST"
        "-DEXPECT=gisBufferCommand: GIS BUFFER DRAWING distance=1 caps=flat.*gisBufferStatus: Done as one undo step: 1 created, 0 changed, 0 deleted on gis/buffer.*buffer distance=1 side=both caps=flat.* area=200\\.000.*Polyline  layer=gis/buffer  vertices=4  closed  length=204  area=200"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gis_buffer_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
# GIS > Analysis - GDAL > Dissolve...: two 50 x 40 lots sharing an edge,
# merged with REPLACE - one 100 x 40 area of 4000 m2 in their place, and one
# UNDO brings both lots back.
add_test(NAME qt_gis_dissolve_dialog_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>RECT 0,0 50,40|>RECT 50,0 100,40|@gisDissolve|!gisDissolveScopeDrawing|gisDissolveReplace=on|?gisDissolveCommand|!gisDissolveRun|?gisDissolveStatus|>LIST|>UNDO|>LIST"
        "-DEXPECT=gisDissolveCommand: GIS DISSOLVE DRAWING REPLACE.*gisDissolveStatus: Done as one undo step: 1 created, 0 changed, 2 deleted on gis/dissolve.*1 entities.*Polyline  layer=gis/dissolve  .*area=4000.*1 undone.*2 entities"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gis_dissolve_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_gis_buffer_dialog_headless qt_gis_dissolve_dialog_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
