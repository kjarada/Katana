# The GDAL verb through katana_cli (docs/geoprocessing.md, "The GDAL verb"):
# the session's own executor, end to end. Included from
# src/katana_app/CMakeLists.txt when the interop module is built.
set(_gdal_samples "${PROJECT_SOURCE_DIR}/samples/gis")

add_test(NAME cli.gdal_version_names_the_library
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "GDAL VERSION")
set_tests_properties(cli.gdal_version_names_the_library PROPERTIES
    PASS_REGULAR_EXPRESSION "gdal version=3\\.[0-9]+\\.[0-9]+ release=[^ ]+.* algorithms=[0-9]+")

# The sample terrain (120 x 90 cells of 1.5 m) shaded and kept as a derived
# reference raster of the name TO gives, which REFS then lists.
add_test(NAME cli.gdal_hillshade_of_the_sample_terrain_to_a_reference
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "GDAL raster hillshade --zfactor=2 FROM FILE \"${_gdal_samples}/terrain.asc\" TO REFERENCE ground-shade"
            -c "REFS")
set_tests_properties(cli.gdal_hillshade_of_the_sample_terrain_to_a_reference PROPERTIES
    PASS_REGULAR_EXPRESSION "input arg=input source=file .*output arg=output kind=raster target=reference id=1 name=ground-shade raster=120x90 .*persisted=no.*reference id=1 kind=raster name=ground-shade width=120 height=90 "
    FAIL_REGULAR_EXPRESSION "error")

# By hand: a 100 m line buffered 1 m either side with flat caps is a 100 x 2
# rectangle, 200 m2, and LIST says its area.
add_test(NAME cli.gdal_buffer_of_a_drawn_line_creates_two_hundred_square_metres
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "LINE 0,0 100,0"
            -c "GDAL vector buffer --distance=1 --endcap-style=flat FROM DRAWING TO LAYER gis/easement"
            -c "LIST")
set_tests_properties(cli.gdal_buffer_of_a_drawn_line_creates_two_hundred_square_metres PROPERTIES
    PASS_REGULAR_EXPRESSION "scope arg=input scope=drawing matched=1 used=1.*created=1.*Polyline  layer=gis/easement  vertices=4  closed  length=204  area=200"
    FAIL_REGULAR_EXPRESSION "error")

# --config could switch on what runs programs; the line is refused and the
# session exits with a failure.
add_test(NAME cli.gdal_refuses_the_config_token
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "GDAL raster hillshade --config GDAL_ENABLE_EXTERNAL=YES")
set_tests_properties(cli.gdal_refuses_the_config_token PROPERTIES WILL_FAIL TRUE)

# HELP typed as a line names the geo executor's families too (IMPORT and its
# options, RASTER GRID, the GDAL verb), as the window's HELP does
# (docs/geoprocessing.md, "The verb table").
add_test(NAME cli.help_typed_as_a_line_names_the_geo_families
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "HELP")
set_tests_properties(cli.help_typed_as_a_line_names_the_geo_families PROPERTIES
    PASS_REGULAR_EXPRESSION "Geo       GDAL VERSION .*RASTER GRID \\[<scope>\\]"
    FAIL_REGULAR_EXPRESSION "error: ")
