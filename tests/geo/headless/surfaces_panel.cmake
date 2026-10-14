# Terrain > Surfaces Panel in the real window (docs/terrain.md, "Surfaces in
# the drawing"), driven headlessly through tools/check_screenshot.cmake.
# Included from tests/CMakeLists.txt when the window is built.
#
# A surface is made from the imported sample DEM; the panel, opened from its
# Terrain item, picks it and copies it with its Copy button (a SURFACE COPY
# line through the window's one executor); a typed SURFACE HIDE hides the
# first. SURFACE LIST then lists the two as the drawing has them: the copy
# named "ground (2)", as an import names a second of a name, and shown; the
# first hidden. Both are drawn in the default style, Standard.
add_test(NAME qt_the_surfaces_panel_copies_the_picked_surface_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DIMPORT=${PROJECT_SOURCE_DIR}/samples/gis/terrain.asc"
        "-DDRIVE=>SURFACE FROM RASTER 1 NAME ground|*terrainSurfacesPanel|%SurfacesDock|surfacesTable=ground|!surfacesCopy|>SURFACE HIDE ground|>SURFACE LIST"
        "-DEXPECT=surface name=ground triangles=[0-9]+ points=10800 [^\r\n]* style=Standard visible=no[\r\n]+surface name=\"ground \\(2\\)\" triangles=[0-9]+ points=10800 [^\r\n]* style=Standard visible=yes.*listed surfaces=2 rasters=1"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/surfaces_panel_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_the_surfaces_panel_copies_the_picked_surface_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
