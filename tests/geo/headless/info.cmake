# INFO as data in the real window (docs/interop.md, "Dataset information"),
# driven headlessly through tools/check_screenshot.cmake: typed, and run as a
# dialog runs its line (runVerbLine), both through the executor katana_cli
# runs. Included from tests/CMakeLists.txt when the window is built.
#
# By hand: terrain.asc is 120 x 90 Float32 cells, no-data -9999, values
# 24.892 to 38.819 (read from the text); lots.geojson one layer, "lots".
add_test(NAME qt_info_typed_and_run_as_a_dialog_runs_it_gives_the_records_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>INFO \"${PROJECT_SOURCE_DIR}/samples/gis/terrain.asc\" STATS|<INFO \"${PROJECT_SOURCE_DIR}/tests/geo/data/lots.geojson\" JSON"
        "-DEXPECT=raster width=120 height=90 bands=1 [^\r\n]*[\r\n]+band band=1 type=Float32 nodata=-9999 min=24\\.89[0-9]* max=38\\.81[0-9]* .*--run-line INFO [^\r\n]*: ok=yes[\r\n]+  reply: \\{\"path\":\"[^\"]*/lots\\.geojson\",\"raster\":null,\"vector\":\\{"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/info_records_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_info_typed_and_run_as_a_dialog_runs_it_gives_the_records_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
