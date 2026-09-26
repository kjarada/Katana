# RASTER GRID in the real window (docs/terrain.md, "Gridding points to a
# DEM"), driven headlessly through tools/check_screenshot.cmake. Included from
# tests/CMakeLists.txt when the window is built.
#
# Terrain > DEM > Grid Points to DEM, filled and run as a person would: the
# 121 points of plane_points.geojson (every 10 m over 0..100 on
# z = 100 + x/10 + y/20) gridded at 5 m, 20 x 20 cells by hand, into the
# reference raster the dialog names, which REFS then lists; its reply shows
# what the scope took. The menu offers the item under Terrain > DEM.
add_test(NAME qt_grid_points_to_dem_dialog_grids_the_drawing_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>IMPORT ${PROJECT_SOURCE_DIR}/tests/geo/data/plane_points.geojson|?terrainDemMenu|@terrainGrid|!gridScopeDrawing|gridCell=5|gridName=ground|?gridCommand|!gridRun|?gridReply|>REFS"
        "-DEXPECT=terrainDemMenu: [^\r\n]*Grid Points to DEM.*--dialog terrainGrid opened gridDemDialog \"Grid Points to DEM\".*gridCommand: RASTER GRID DRAWING method=linear cell=5 NAME ground.*gridReply: grid method=linear algorithm=\"vector grid linear\" z=geometry cell=5 extent=0,0,100,100 size=20x20.*scope arg=input scope=drawing matched=121 used=121 points=121.*output arg=output kind=raster target=reference id=1 name=ground raster=20x20.*reference id=1 kind=raster name=ground width=20 height=20 "
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/grid_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_grid_points_to_dem_dialog_grids_the_drawing_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
