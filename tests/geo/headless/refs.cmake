# The Reference Data panel in the real window (docs/interop.md, "Reference
# layers"): its controls build REFS lines and run them through the window's
# one executor, driven headlessly by object name through
# tools/check_screenshot.cmake. Included from tests/CMakeLists.txt when the
# window is built.
#
# The raster (terrain.asc) is id 1 and the cloud (survey_scan.las) id 2. Its
# opacity to 50%, hidden and shown again; Build Overviews pressed with nobody
# to confirm it, so its line goes without CONFIRM and is refused, saying what
# it would write (nothing is written beside the sample); the cloud coloured
# by classification, then removed.
add_test(NAME qt_reference_dock_builds_refs_lines_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>IMPORT \"${PROJECT_SOURCE_DIR}/samples/gis/terrain.asc\"|>IMPORT \"${PROJECT_SOURCE_DIR}/samples/gis/survey_scan.las\"|%ReferenceDataDock|referenceList=terrain|referenceOpacity=50%|!referenceHide|!referenceShow|!referenceOverviews|referenceList=survey_scan|referenceColour=Classification|!referenceRemove|>REFS"
        "-DEXPECT=reference id=1 kind=raster name=terrain [^\r\n]*visible=yes opacity=0\\.5 .*reference id=1 kind=raster name=terrain [^\r\n]*visible=no .*reference id=1 kind=raster name=terrain [^\r\n]*visible=yes .*REFS OVERVIEWS writes [^\r\n]*/terrain\\.asc\\.ovr beside the raster's file; add CONFIRM.*reference id=2 kind=pointcloud name=survey_scan [^\r\n]*color=classification .*removed id=2 kind=pointcloud name=survey_scan.*references rasters=1 clouds=0 missing=0"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/reference_dock_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_reference_dock_builds_refs_lines_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
