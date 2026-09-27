# GIS BUFFER and GIS DISSOLVE through katana_cli (docs/geoprocessing.md,
# "V1"): the session's own executor, end to end. Included from
# src/katana_app/CMakeLists.txt when the interop module is built.

# By hand: a 100 m line buffered 1 m either side with flat ends is a 100 x 2
# rectangle, 200 m2, which the reply totals and LIST measures.
add_test(NAME cli.gis_buffer_of_a_drawn_line_creates_two_hundred_square_metres
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "LINE 0,0 100,0"
            -c "GIS BUFFER DRAWING distance=1 caps=flat"
            -c "LIST")
set_tests_properties(cli.gis_buffer_of_a_drawn_line_creates_two_hundred_square_metres PROPERTIES
    PASS_REGULAR_EXPRESSION "gis op=buffer seconds=[0-9.]+ cancelled=no.*scope arg=input scope=drawing matched=1 used=1.*output arg=output kind=vector target=layer layer=gis/buffer created=1.*buffer distance=1 .*area=200\\.000.*Polyline  layer=gis/buffer  vertices=4  closed  length=204  area=200"
    FAIL_REGULAR_EXPRESSION "error")

# Two 50 x 40 lots sharing an edge dissolve into one 100 x 40 area: 4000 m2.
add_test(NAME cli.gis_dissolve_of_two_adjacent_lots_is_one_area_of_four_thousand_square_metres
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "RECT 0,0 50,40" -c "RECT 50,0 100,40"
            -c "GIS DISSOLVE DRAWING TO LAYER superlot"
            -c "LIST")
set_tests_properties(cli.gis_dissolve_of_two_adjacent_lots_is_one_area_of_four_thousand_square_metres PROPERTIES
    PASS_REGULAR_EXPRESSION "dissolve by= keep=none areas=2 replace=no groups=1 area=4000\\.000.*layer=superlot .*area=4000"
    FAIL_REGULAR_EXPRESSION "error")

# Without a distance there is nothing to buffer by: refused, and the session
# exits with a failure.
add_test(NAME cli.gis_buffer_without_a_distance_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "LINE 0,0 100,0" -c "GIS BUFFER DRAWING caps=flat")
set_tests_properties(cli.gis_buffer_without_a_distance_is_refused PROPERTIES WILL_FAIL TRUE)
