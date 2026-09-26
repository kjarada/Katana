# Terrain > Analysis > Slope and Aspect in the real window (docs/terrain.md,
# "Slope and aspect"), driven headlessly through tools/check_screenshot.cmake.
# Included from tests/CMakeLists.txt when the window is built.
#
# The menu item opens the dialog on the imported plane; classes at 2 and 10
# are typed and Run hands the RASTER SLOPE line to the window's one
# executor, which waits for the job headless: the whole plane is one class,
# 1200 m2 (5 % inside, 2.5 % at the corners by GDAL's edge rule), drawn as
# one closed polyline. A typed UNDO takes the area away in one step.
add_test(NAME qt_slope_analysis_dialog_draws_the_class_areas_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/geo/data/plane.asc"
        "-DDRIVE=@terrainSlope|slopeClasses=2,10|?slopeCommand|!slopeRun|?slopeReply|>LIST|>UNDO|>LIST"
        "-DEXPECT=slopeCommand: RASTER SLOPE RASTER 1 unit=percent classes=2,10 areas=terrain/slope.*class name=2-10 from=2 to=10 unit=percent area=1200\\.000 polygons=1.*Polyline +layer=terrain/slope/2-10 +vertices=4 +closed.*[Uu]ndo.*0 entities"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/slope_analysis_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_slope_analysis_dialog_draws_the_class_areas_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
