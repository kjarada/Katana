# Terrain > Surface From Raster and GIS > Export Surface as DEM in the real
# window (docs/terrain.md, "Surfaces on every front end"), driven headlessly
# through tools/check_screenshot.cmake. Included from tests/CMakeLists.txt when
# the window is built.
#
# The menu item opens the Surface From dialog on the imported raster; Run
# hands its SURFACE FROM line to the window's one executor, which waits for
# the job headless. Then a typed SURFACE LIST lists the surface: the dialog's
# line and the typed one reach one store. The range is terrain.asc's own,
# read from the text grid: 24.892 to 38.819 (cli.surface_* say why).
add_test(NAME qt_surface_from_raster_menu_runs_the_surface_line_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DIMPORT=${PROJECT_SOURCE_DIR}/samples/gis/terrain.asc"
        "-DDRIVE=@surfaceFromRaster|?surfaceFromCommand|!surfaceFromRun|>SURFACE LIST"
        "-DEXPECT=surfaceFromCommand: SURFACE FROM RASTER 1.*surface name=terrain triangles=[0-9]+ points=10800 .*zmin=24\\.892 zmax=38\\.819 .*surface name=terrain .*raster id=1 name=terrain .*listed surfaces=1 rasters=1"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/surface_from_raster_menu_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
# GIS > Export Surface as DEM: the dialog builds the SURFACE EXPORT line from
# its fields and runs it; the grid is 119 x 89 at the sample's 1.5 m cell.
add_test(NAME qt_export_surface_dialog_writes_the_dem_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DIMPORT=${PROJECT_SOURCE_DIR}/samples/gis/terrain.asc"
        "-DDRIVE=>SURFACE FROM RASTER 1 NAME ground|@exportSurfaceDem|surfaceRasterCell=1.5|surfaceRasterFile=${CMAKE_CURRENT_BINARY_DIR}/export_surface_dialog.tif|surfaceRasterOverwrite=on|?surfaceRasterCommand|!surfaceRasterRun"
        "-DEXPECT=surfaceRasterCommand: SURFACE EXPORT ground [^ ]*export_surface_dialog\\.tif cell=1\\.5 type=Float32 OVERWRITE.*exported file=[^ ]*export_surface_dialog\\.tif driver=GTiff surface=ground raster=119x89"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/export_surface_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_surface_from_raster_menu_runs_the_surface_line_headless
    qt_export_surface_dialog_writes_the_dem_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
