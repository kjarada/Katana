# RASTER GRID through katana_cli (docs/terrain.md, "Gridding points to a DEM"):
# surveyed points imported with their heights and gridded into a DEM that is
# kept as a reference raster. Included from src/katana_app/CMakeLists.txt when
# the interop module is built.
set(_grid_data "${PROJECT_SOURCE_DIR}/tests/geo/data")

# plane_points.geojson: 121 points every 10 m over 0..100 on the plane
# z = 100 + x/10 + y/20. A linear grid reproduces a plane, so the 5 m cell
# centred on (52.5, 47.5) - column 10, line 10 from the top left - holds
# 100 + 5.25 + 2.375 = 107.625 by hand.
add_test(NAME cli.raster_grid_of_surveyed_points_reproduces_the_plane
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "IMPORT \"${_grid_data}/plane_points.geojson\""
            -c "RASTER GRID DRAWING method=linear cell=5 NAME ground"
            -c "GDAL raster pixel-info --position=10,10 FROM RASTER ground")
set_tests_properties(cli.raster_grid_of_surveyed_points_reproduces_the_plane PROPERTIES
    PASS_REGULAR_EXPRESSION "grid method=linear algorithm=\"vector grid linear\" z=geometry cell=5 extent=0,0,100,100 size=20x20.*scope arg=input scope=drawing matched=121 used=121 points=121.*output arg=output kind=raster target=reference id=1 name=ground raster=20x20.*107\\.625"
    FAIL_REGULAR_EXPRESSION "error")

# A scope that takes nothing is said, and nothing is gridded.
add_test(NAME cli.raster_grid_of_an_empty_scope_says_so
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "IMPORT \"${_grid_data}/plane_points.geojson\""
            -c "RASTER GRID AREA 500,500,600,600 cell=5" -c "REFS")
set_tests_properties(cli.raster_grid_of_an_empty_scope_says_so PROPERTIES
    PASS_REGULAR_EXPRESSION "grid method=linear .* ran=no.*scope arg=input scope=area area=500,500,600,600 matched=0"
    FAIL_REGULAR_EXPRESSION "error|Raster  ")
