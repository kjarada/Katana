# Terrain > Analysis > Terrain Shading in the real window (docs/terrain.md,
# "Shading"), driven headlessly through tools/check_screenshot.cmake.
# Included from tests/CMakeLists.txt when the window is built.
#
# The menu item opens the dialog on the imported raster; Colour relief is
# chosen, Run hands the RASTER SHADE line to the window's one executor,
# which waits for the job headless: the picture is the second reference
# raster, derived, of the DEM's 120 x 90 cells, which a typed SURFACE LIST
# lists - the dialog's line and the typed one reach one reference data.
add_test(NAME qt_terrain_shading_dialog_adds_a_reference_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DIMPORT=${PROJECT_SOURCE_DIR}/samples/gis/terrain.asc"
        "-DDRIVE=@terrainShading|shadingStyle=Colour relief|?shadingCommand|!shadingRun|>SURFACE LIST"
        "-DEXPECT=shadingCommand: RASTER SHADE RASTER 1 style=relief.*ramp name=terrain min=24\\.892 max=38\\.819 from=data.*raster id=2 name=terrain-relief kind=derived width=120 height=90"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/terrain_shading_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_terrain_shading_dialog_adds_a_reference_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
