# Terrain > Analysis > Contours in the real window (docs/terrain.md,
# "Contours"), driven headlessly through tools/check_screenshot.cmake.
# Included from tests/CMakeLists.txt when the window is built.
#
# The menu item opens the dialog on the imported raster; Run hands its
# CONTOUR line to the window's one executor, which waits for the job
# headless, and the reply is the verb's: terrain.asc's heights run from
# 24.892 to 38.819 on 1.5 m cells, so the whole metres 25 to 38 are 14
# levels, and major=5 with base 0 makes 25, 30 and 35 the major ones. A typed
# UNDO then takes every contour away in one step: LIST lists nothing (the
# raster is a reference layer, no entity).
add_test(NAME qt_contours_dialog_draws_contours_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DIMPORT=${PROJECT_SOURCE_DIR}/samples/gis/terrain.asc"
        "-DDRIVE=@terrainContours|contourInterval=1|?contourCommand|!contourRun|?contourReply|>LIST|>UNDO|>LIST"
        "-DEXPECT=contourCommand: CONTOUR RASTER 1 interval=1 major=5 layer=terrain/contours.*contours method=grid cell=1\\.5 levels=14 count=[0-9]+ major=[1-9][0-9]* minor=[1-9][0-9]* layer=terrain/contours smoothed=no.*Polyline +layer=terrain/contours/major.*[Uu]ndo.*0 entities"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/contours_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_contours_dialog_draws_contours_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
