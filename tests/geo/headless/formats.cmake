# Every driver in the real window (docs/interop.md, "Formats"), driven
# headlessly through tools/check_screenshot.cmake. Included from
# tests/CMakeLists.txt when the window is built.
#
# GIS > Processing - GDAL > Formats: the choices make the FORMATS line the
# dialog shows, the table is that line's records, selecting FlatGeobuf shows
# its options (SPATIAL_INDEX, gdal.org/drivers/vector/flatgeobuf.html), and
# Run in Command Line hands the line to the window's one executor, whose
# records reach the log; FORMATS typed on the command line answers there too.
add_test(NAME qt_the_formats_dialog_shows_the_formats_line_it_runs_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=@gisFormats|gisFormatsKind=Vector|gisFormatsCapability=Writes|gisFormatsFilter=fgb|?gisFormatsCommand|?gisFormatsCount|?gisFormatsTable|gisFormatsTable=FlatGeobuf|?gisFormatsOptions|!gisFormatsRun|>FORMATS OPTIONS FlatGeobuf"
        "-DEXPECT=gisFormatsCommand: FORMATS VECTOR WRITE fgb[\r\n].*gisFormatsCount: 1 format \\(GDAL 3\\.[0-9.]+\\)[\r\n].*gisFormatsTable: FlatGeobuf \\| FlatGeobuf \\| vector \\| vector \\| vector \\| fgb \\| yes[\r\n].*gisFormatsOptions: [^\r\n]*SPATIAL_INDEX.*format driver=FlatGeobuf kind=vector read=vector write=vector extensions=fgb.*option driver=FlatGeobuf list=layer_creation name=SPATIAL_INDEX type=boolean default=YES"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/formats_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

# The window's IMPORT routes a file by what it holds: a .zip, which GDAL
# does not open as it is, by the one dataset inside it. The archive is made
# from the fixture when the build is configured.
set(_formats_headless "${CMAKE_CURRENT_BINARY_DIR}/formats_headless")
file(MAKE_DIRECTORY "${_formats_headless}")
execute_process(
    COMMAND ${CMAKE_COMMAND} -E tar cf "${_formats_headless}/lots.zip" --format=zip lots.geojson
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}/tests/geo/data")
add_test(NAME qt_import_of_a_zip_opens_the_one_dataset_inside_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>IMPORT \"${_formats_headless}/lots.zip\"|>LIST"
        "-DEXPECT=Imported 3 entities from lots\\.zip.*Polyline  layer=lots [^\r\n]*area=2000"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/formats_zip_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_the_formats_dialog_shows_the_formats_line_it_runs_headless
    qt_import_of_a_zip_opens_the_one_dataset_inside_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
