# GIS OVERLAY through katana_cli (docs/geoprocessing.md, "V2"): the session's
# own executor, end to end. Included from src/katana_app/CMakeLists.txt when
# the interop module is built.
set(_overlay_data "${PROJECT_SOURCE_DIR}/tests/geo/data")

# Two 50 x 40 lots and a 4 m corridor across both: 4 x 50 = 200 m2 of
# corridor on each lot, by hand, a row each.
add_test(NAME cli.gis_overlay_of_a_corridor_on_two_lots_gives_two_hundred_square_metres_each
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "LAYER NEW lots" -c "LAYER SET lots"
            -c "RECT 0,0 50,40" -c "RECT 50,0 100,40"
            -c "LAYER NEW corridor" -c "LAYER SET corridor" -c "RECT -10,18 110,22"
            -c "GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor")
set_tests_properties(cli.gis_overlay_of_a_corridor_on_two_lots_gives_two_hundred_square_metres_each PROPERTIES
    PASS_REGULAR_EXPRESSION "scope arg=method scope=layers layers=corridor .*matched=1.*created=2.*row entity=1 [^\n]*with=3 area=200\\.000.*row entity=2 [^\n]*with=3 area=200\\.000.*overlay operation=intersection features=2 area=400\\.000"
    FAIL_REGULAR_EXPRESSION "error")

# The same corridor from tests/geo/data/lots.geojson, by where=.
add_test(NAME cli.gis_overlay_with_a_file_reads_its_corridor
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "RECT 0,0 50,40" -c "RECT 50,0 100,40"
            -c "GIS OVERLAY intersection DRAWING WITH FILE \"${_overlay_data}/lots.geojson\" where=\"kind='corridor'\"")
set_tests_properties(cli.gis_overlay_with_a_file_reads_its_corridor PROPERTIES
    PASS_REGULAR_EXPRESSION "input arg=method source=file .*overlay operation=intersection features=2 area=400\\.000"
    FAIL_REGULAR_EXPRESSION "error")
