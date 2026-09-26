# The GDAL verb in the real window (docs/geoprocessing.md, "The window"),
# driven headlessly through tools/check_screenshot.cmake. Included from
# tests/CMakeLists.txt when the window is built.
#
# Typed on the command line: a 100 m line buffered 1 m either side with flat
# caps is a 100 x 2 rectangle, 200 m2 by hand, on the layer TO names; the
# reply says what the scope took.
add_test(NAME qt_gdal_typed_on_the_command_line_runs_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>LINE 0,0 100,0|>GDAL vector buffer --distance=1 --endcap-style=flat FROM DRAWING TO LAYER gis/buffer|>LIST"
        "-DEXPECT=gdal algorithm=\"vector buffer\" policy=safe seconds=[0-9.]+ cancelled=no.*scope arg=input scope=drawing matched=1 used=1 points=0 lines=1 polygons=0.*output arg=output kind=vector target=layer layer=gis/buffer created=1.*Polyline  layer=gis/buffer  vertices=4  closed  length=204  area=200"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gdal_typed_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
# The same verb as a dialog runs its line, through the window's one executor
# (MainWindow::runVerbLine): a background job the headless run waits for, its
# reply handed back to the caller, and the result one undo step - without TO
# the features go to gis/buffer.
add_test(NAME qt_gdal_line_through_the_windows_executor_is_one_undo_step_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>LINE 0,0 100,0|<GDAL vector buffer distance=1 FROM DRAWING|>LIST|>UNDO|>LIST"
        "-DEXPECT=--run-line GDAL vector buffer distance=1 FROM DRAWING: ok=yes.*reply: output arg=output kind=vector target=layer layer=gis/buffer created=1.*2 entities.*1 undone.*1 entities"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gdal_executor_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_gdal_typed_on_the_command_line_runs_headless
    qt_gdal_line_through_the_windows_executor_is_one_undo_step_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
