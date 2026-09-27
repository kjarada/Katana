# The GDAL Toolbox's Pipeline tab in the real window (docs/geoprocessing.md,
# "Pipelines"), driven headlessly through tools/check_screenshot.cmake.
# Included from tests/CMakeLists.txt when the window is built.
#
# The plane (z = 100 + 0.05x over 0..40, 100.025 .. 101.975) read from its
# file, its pipeline typed into the tab's text - which the steps then follow -
# and run: contours every 0.5 m (100.5, 101.0, 101.5) each buffered 0.1 m,
# three areas on gis/bands by hand.
add_test(NAME qt_gdal_toolbox_pipeline_tab_runs_its_steps_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=@gdalToolbox|gdalToolboxTabs=Pipeline|gdalPipelineKind=File|gdalPipelineFile=${PROJECT_SOURCE_DIR}/tests/geo/data/plane.asc|gdalPipelineText=read ! contour --interval=0.5 ! buffer --distance=0.1 ! write|gdalPipelineOutputName=gis/bands|?gdalPipelineSteps|?gdalPipelineCommand|!gdalPipelineRun|?gdalPipelineReply|>LIST"
        "-DEXPECT=gdalPipelineSteps: read ; contour +--interval=0.5 ; buffer +--distance=0.1 ; write.*gdalPipelineCommand: GDAL pipeline \"read ! contour --interval=0.5 ! buffer --distance=0.1 ! write\" FROM input FILE [^\r\n]*plane.asc TO LAYER gis/bands.*gdalPipelineReply: gdal algorithm=pipeline policy=safe.*layer=gis/bands created=3.*3 entities"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gdal_pipeline_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_gdal_toolbox_pipeline_tab_runs_its_steps_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
