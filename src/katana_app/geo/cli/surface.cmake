# The SURFACE verbs through katana_cli (docs/terrain.md, "Surfaces on every
# front end"): the session keeps its surfaces between lines. Included from
# src/katana_app/CMakeLists.txt when the interop module is built.
set(_surface_samples "${PROJECT_SOURCE_DIR}/samples/gis")

# samples/gis/terrain.asc: 120 x 90 cells of 1.5 m from (-5,-5), every one a
# value (10 800, under the 400 000 cap, so each is a vertex). Its least and
# greatest values, read straight from the text grid, are 24.892 and 38.819,
# and linear interpolation cannot leave the range of its vertices. The cell
# centres run from -5 + 0.75 = -4.25 to -5 + 1.5 x 119.5 = 174.25 across and
# to -5 + 1.5 x 89.5 = 129.25 up.
add_test(NAME cli.surface_from_the_sample_terrain_then_list
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_samples}/terrain.asc\" NAME ground"
            -c "SURFACE LIST")
set_tests_properties(cli.surface_from_the_sample_terrain_then_list PROPERTIES
    PASS_REGULAR_EXPRESSION "sampled stride=1 pixels=10800 nodata=0 points=10800.*surface name=ground triangles=[0-9]+ points=10800 bounds=-4\\.250,-4\\.250,174\\.250,129\\.250 zmin=24\\.892 zmax=38\\.819 .*listed surfaces=1 rasters=0"
    FAIL_REGULAR_EXPRESSION "error")

# The same surface written as a DEM: at the sample's own 1.5 m cell the
# surface's 178.5 x 133.5 m extent is 119 x 89 cells, stored as Float32 by
# default - which GDAL's own raster info reads back (its JSON's band type).
add_test(NAME cli.surface_export_writes_a_float32_dem
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_samples}/terrain.asc\" NAME ground"
            -c "SURFACE EXPORT ground \"${CMAKE_CURRENT_BINARY_DIR}/surface_export_ground.tif\" cell=1.5 OVERWRITE"
            -c "GDAL raster info \"${CMAKE_CURRENT_BINARY_DIR}/surface_export_ground.tif\"")
set_tests_properties(cli.surface_export_writes_a_float32_dem PROPERTIES
    PASS_REGULAR_EXPRESSION "exported file=[^ ]*surface_export_ground\\.tif driver=GTiff surface=ground raster=119x89 cell=1\\.5 type=Float32.*\"type\":\"Float32\""
    FAIL_REGULAR_EXPRESSION "error")

# A surface name nothing has is refused, and the session exits with a failure.
add_test(NAME cli.surface_info_of_no_such_surface_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "SURFACE INFO nothing")
set_tests_properties(cli.surface_info_of_no_such_surface_is_refused PROPERTIES WILL_FAIL TRUE)
