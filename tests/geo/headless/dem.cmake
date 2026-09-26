# The DEM tools in the real window (docs/terrain.md, "The DEM tools"), driven
# headlessly through tools/check_screenshot.cmake. Included from
# tests/CMakeLists.txt when the window is built.
#
# Terrain > DEM > DEM Tools, as a person fills it: the plane (40 x 30 cells of
# 1 m) clipped to 0..20 x 0..15 - 20 x 15 cells by hand - into the reference
# raster west, which the Footprint tab then takes from the window's reference
# rasters and draws: a closed 20 x 15 rectangle, 70 m round and 300 m2 by
# hand, as LIST measures it.
add_test(NAME qt_dem_tools_clip_then_footprint_through_the_dialog_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=?terrainDemMenu|@terrainDemTools|demToolsTabs=Clip|demClipKind=File|demClipFile=${PROJECT_SOURCE_DIR}/tests/geo/data/plane.asc|demClipArea=0,0,20,15|demClipName=west|?demClipCommand|!demClipRun|?demClipReply|demToolsTabs=Footprint|demFootprintKind=Reference raster|?demFootprintCommand|!demFootprintRun|>LIST"
        "-DEXPECT=terrainDemMenu: [^\r\n]*DEM Tools.*--dialog terrainDemTools opened demToolsDialog \"DEM Tools\".*demClipCommand: RASTER CLIP FILE [^\r\n]*plane.asc AREA 0,0,20,15 NAME west.*demClipReply: clip algorithm=\"raster clip\" by=area area=0,0,20,15.*name=west raster=20x15.*demFootprintCommand: RASTER FOOTPRINT RASTER 1.*Polyline  layer=gis/footprint  vertices=4  closed  length=70  area=300"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/dem_tools_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_dem_tools_clip_then_footprint_through_the_dialog_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
