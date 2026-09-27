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

# The same dialog keeps the grid as a named surface when gridToSurface is on:
# the box was disabled though TO SURFACE works on every front end, and
# --fill filled it without a word (docs/terrain.md, "Gridding points to a
# DEM"; docs/headless.md, --fill).
add_test(NAME qt_grid_points_to_dem_dialog_keeps_a_surface_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>IMPORT ${PROJECT_SOURCE_DIR}/tests/geo/data/plane_points.geojson|@terrainGrid|!gridScopeDrawing|gridCell=5|gridName=ground|gridToSurface=on|?gridCommand|!gridRun|>SURFACE LIST"
        "-DEXPECT=gridCommand: RASTER GRID DRAWING method=linear cell=5 NAME ground TO SURFACE ground.*surface name=ground "
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/grid_surface_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
# A field a person cannot type into is refused, not filled: power= is not
# offered to the linear method, so gridPower is disabled.
add_test(NAME qt_fill_of_a_disabled_field_is_refused_headless
    COMMAND katana --dialog terrainGrid --fill gridPower=2
        --screenshot "${CMAKE_CURRENT_BINARY_DIR}/fill_disabled_headless.png")
set_tests_properties(qt_grid_points_to_dem_dialog_keeps_a_surface_headless
    qt_fill_of_a_disabled_field_is_refused_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
set_tests_properties(qt_fill_of_a_disabled_field_is_refused_headless PROPERTIES
    PASS_REGULAR_EXPRESSION "--fill: gridDemDialog's field gridPower is disabled")
