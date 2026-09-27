# The DEM tools through katana_cli (docs/terrain.md, "The DEM tools"): the
# session's own executor, end to end, on tests/geo/data/plane.asc - 40 x 30
# cells of 1 m from (0,0). Included from src/katana_app/CMakeLists.txt when the
# interop module is built.
set(_dem_data "${PROJECT_SOURCE_DIR}/tests/geo/data")
set(_dem_scratch "${CMAKE_CURRENT_BINARY_DIR}/dem_cli")
file(MAKE_DIRECTORY "${_dem_scratch}")

# By hand: 0..20 x 0..15 of 1 m cells is 20 x 15, kept as a reference raster
# of the name NAME gives, which REFS then lists.
add_test(NAME cli.raster_clip_of_the_plane_to_an_area_is_twenty_by_fifteen
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "RASTER CLIP FILE \"${_dem_data}/plane.asc\" AREA 0,0,20,15 NAME west"
            -c "REFS")
set_tests_properties(cli.raster_clip_of_the_plane_to_an_area_is_twenty_by_fifteen PROPERTIES
    PASS_REGULAR_EXPRESSION "clip algorithm=\"raster clip\" by=area area=0,0,20,15 seconds=[0-9.]+.*input arg=input source=file .*output arg=output kind=raster target=reference id=1 name=west raster=20x15.*reference id=1 kind=raster name=west width=20 height=15 "
    FAIL_REGULAR_EXPRESSION "error")

# Every cell of the plane holds a value, so its footprint is its extent: a
# closed 40 x 30 rectangle, 140 m round and 1200 m2, which LIST measures.
add_test(NAME cli.raster_footprint_of_the_plane_is_its_extent_rectangle
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "RASTER FOOTPRINT FILE \"${_dem_data}/plane.asc\"" -c "LIST")
set_tests_properties(cli.raster_footprint_of_the_plane_is_its_extent_rectangle PROPERTIES
    PASS_REGULAR_EXPRESSION "output arg=output kind=vector target=layer layer=gis/footprint created=1.*Polyline  layer=gis/footprint  vertices=4  closed  length=140  area=1200"
    FAIL_REGULAR_EXPRESSION "error")

# The plane's west and east halves, cut by GDAL, joined again by a mosaic:
# GDAL's own compare of the mosaic with the whole (cut by the same clip, so the
# same kind of file) finds no difference.
add_test(NAME cli.raster_mosaic_of_the_planes_halves_is_the_whole
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "GDAL raster clip --window=0,0,20,30 FROM FILE \"${_dem_data}/plane.asc\" TO FILE \"${_dem_scratch}/west.tif\" OVERWRITE"
            -c "GDAL raster clip --window=20,0,20,30 FROM FILE \"${_dem_data}/plane.asc\" TO FILE \"${_dem_scratch}/east.tif\" OVERWRITE"
            -c "GDAL raster clip --window=0,0,40,30 FROM FILE \"${_dem_data}/plane.asc\" TO FILE \"${_dem_scratch}/whole.tif\" OVERWRITE"
            -c "RASTER MOSAIC FILE \"${_dem_scratch}/west.tif\" FILE \"${_dem_scratch}/east.tif\" NAME joined"
            -c "GDAL raster compare --skip-binary FROM input RASTER joined FROM reference FILE \"${_dem_scratch}/whole.tif\"")
set_tests_properties(cli.raster_mosaic_of_the_planes_halves_is_the_whole PROPERTIES
    PASS_REGULAR_EXPRESSION "mosaic algorithm=\"raster mosaic\" tiles=2 virtual=yes.*name=joined raster=40x30.*return code=0"
    FAIL_REGULAR_EXPRESSION "error")

# A raster that says nothing of where it is: reprojecting it is refused, not
# guessed, and the session ends with a failure.
add_test(NAME cli.raster_reproject_of_a_raster_with_no_system_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "RASTER REPROJECT FILE \"${_dem_data}/plane.asc\" crs=EPSG:28356")
set_tests_properties(cli.raster_reproject_of_a_raster_with_no_system_is_refused PROPERTIES
    WILL_FAIL TRUE)
