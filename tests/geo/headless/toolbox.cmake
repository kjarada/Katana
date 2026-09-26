# GIS > GDAL Toolbox in the real window (docs/geoprocessing.md, "The
# toolbox"), driven headlessly through tools/check_screenshot.cmake. Included
# from tests/CMakeLists.txt when the window is built.
#
# A 100 m line drawn, the toolbox opened from the GIS menu, "buffer" searched
# for - which chooses vector buffer - and its form filled as a person would:
# 1 m either side with flat caps, drawn on gis/buffer. The line the dialog
# shows is the line it runs; by hand the buffer is a 100 x 2 rectangle,
# 204 m round and 200 m2, which LIST measures.
add_test(NAME qt_gdal_toolbox_buffers_a_drawn_line_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>LINE 0,0 100,0|?gisMenu|@gdalToolbox|gdalToolboxSearch=buffer|gdalArg.distance=1|gdalArg.endcap-style=flat|gdalOutput.name=gis/buffer|?gdalToolboxCommand|!gdalToolboxRun|?gdalToolboxReply|>LIST"
        "-DEXPECT=gisMenu: [^\r\n]*GDAL Toolbox.*--dialog gdalToolbox opened gdalToolboxDialog \"GDAL Toolbox\".*gdalToolboxCommand: GDAL vector buffer --distance=1 --endcap-style=flat FROM DRAWING TO LAYER gis/buffer.*gdalToolboxReply: gdal algorithm=\"vector buffer\" policy=safe.*scope arg=input scope=drawing matched=1 used=1.*output arg=output kind=vector target=layer layer=gis/buffer created=1.*Polyline  layer=gis/buffer  vertices=4  closed  length=204  area=200"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gdal_toolbox_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_gdal_toolbox_buffers_a_drawn_line_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
