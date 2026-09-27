# Terrain > Analysis > Statistics by Area, Drape and Sample Heights, and
# Viewshed and Line of Sight in the real window (docs/terrain.md,
# "Statistics by area", "Sampling and drape", "Viewshed and line of
# sight"), driven headlessly through tools/check_screenshot.cmake by the
# dialogs' object names. Included from tests/CMakeLists.txt when the window
# is built. Each dialog builds its line and hands it to the window's one
# executor; what it reports is checked against values worked by hand.

# A 40 x 30 m lot drawn on terrain.asc's 1.5 m cells, the whole drawing as
# the zones: fractional coverage counts 1200 / 2.25 = 533.333 cells. The
# mean, 27.8048, is the coverage-weighted mean of the text grid's heights
# over the lot, computed independently from samples/gis/terrain.asc (the
# lot's west, south and north sides lie on cell edges, its east side covers
# column 36 by 2/3): 14829.208 / 533.333.
add_test(NAME qt_statistics_by_area_dialog_writes_the_lot_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DIMPORT=${PROJECT_SOURCE_DIR}/samples/gis/terrain.asc"
        "-DDRIVE=>RECT 10,10 50,40|@terrainZonal|!zonalScopeDrawing|?zonalCommand|!zonalRun|?zonalReply"
        "-DEXPECT=--dialog terrainZonal opened zonalStatsDialog \"Statistics by Area\".*zonalCommand: RASTER ZONAL RASTER 1 DRAWING stats=mean,min,max,count,sum prefix=zone pixels=fractional.*zonalReply: gis op=zonal [^\r\n]*scope arg=zones scope=drawing matched=1 used=1 [^\r\n]*target=in-place created=0 updated=1 [^\r\n]*zone entity=1 mean=27\\.804[78][^\r\n]* count=533\\.333"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/statistics_by_area_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_statistics_by_area_dialog_writes_the_lot_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)

# plane.asc is z = 100 + 0.05 x at its 1 m cell centres, 40 x 30 m. The
# Drape tab gives the drawn string's three vertices a height each; the
# Sample tab reads 100 + 0.05 x 12.3 = 100.615 at (12.3, 7.7) - bilinear on
# a plane is the plane - and nothing at x = 50, off the raster.
add_test(NAME qt_drape_and_sample_dialog_drapes_and_samples_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/geo/data/plane.asc"
        "-DDRIVE=>PLINE 5.5,5 20.25,10 35,25|@terrainDrape|!drapeScopeDrawing|?drapeCommand|!drapeRun|?drapeReply|drapeTabs=Sample|samplePoint=12.3,7.7|!sampleAdd|samplePoint=50,10|!sampleAdd|?sampleCommand|!sampleRun|?sampleReply"
        "-DEXPECT=--dialog terrainDrape opened drapeDialog \"Drape and Sample Heights\".*drapeCommand: DRAPE RASTER 1 DRAWING method=bilinear.*drapeReply: [^\r\n]*target=in-place created=0 updated=1 [^\r\n]*drape method=bilinear entities=1 vertices=3 off=0.*sampleCommand: RASTER SAMPLE RASTER 1 AT 12\\.3,7\\.7 AT 50,10 method=bilinear.*sampleReply: [^\r\n]*sample at=12\\.3,7\\.7 z=100\\.61(4999|5000)[^\r\n]* sample at=50,10 ground=no samples method=bilinear count=2 on=1 off=1"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/drape_and_sample_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_drape_and_sample_dialog_drapes_and_samples_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)

# From the west edge of plane.asc, which rises 1 in 20 to the east, every
# one of its 40 x 30 cells is seen (each lies at a steeper angle from the
# eye than any nearer one on its ray): 1200 cells of 1 m2, one connected
# area drawn on site/view, and a derived reference raster SURFACE LIST
# lists. The Line of Sight tab looks along the row to (39.5, 15.5): the eye
# is 100 + 0.05 x 0.5 + 1.7 = 101.725, the target on the ground at
# 100 + 0.05 x 39.5 = 101.975, and the sight clears the rising plane by
# 1.7 (1 - t) at each station, so it is seen.
add_test(NAME qt_viewshed_and_line_of_sight_dialog_sees_the_plane_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/geo/data/plane.asc"
        "-DDRIVE=@terrainViewshed|viewshedObserver=0.5,15.5|!viewshedAdd|viewshedCurvature=none|viewshedAreas=site/view|?viewshedCommand|!viewshedRun|?viewshedReply|viewshedTabs=Line of Sight|losObserver=0.5,15.5|losTarget=39.5,15.5|losCurvature=none|?losCommand|!losRun|?losReply|>SURFACE LIST"
        "-DEXPECT=--dialog terrainViewshed opened viewshedDialog \"Viewshed and Line of Sight\".*viewshedCommand: RASTER VIEWSHED RASTER 1 OBSERVER 0\\.5,15\\.5 height=1\\.7 target=0 curvature=none areas=site/view.*viewshedReply: [^\r\n]*observer at=0\\.5,15\\.5 visible_cells=1200 [^\r\n]*name=plane-viewshed [^\r\n]*output arg=areas kind=vector target=layer layer=site/view created=1 [^\r\n]*visible_cells=1200 area=1200\\.000.*losCommand: LOS RASTER 1 OBSERVER 0\\.5,15\\.5 TARGET 39\\.5,15\\.5 height=1\\.7 target=0 curvature=none.*losReply: [^\r\n]*sight visible=yes observer=0\\.5,15\\.5 target=39\\.5,15\\.5 distance=39\\.000 observer_z=101\\.725 target_z=101\\.975.*raster id=2 name=plane-viewshed kind=derived width=40 height=30"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/viewshed_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_viewshed_and_line_of_sight_dialog_sees_the_plane_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
