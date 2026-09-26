# GIS HULL and GIS CLIP through katana_cli (docs/geoprocessing.md, "V3"):
# the session's own executor, end to end. Included from
# src/katana_app/CMakeLists.txt when the interop module is built.

# The corners and centre of a 10 x 10 square: its hull is the square,
# 100 m2, the centre inside.
add_test(NAME cli.gis_hull_of_a_squares_corners_and_centre_is_the_square
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "POINT 0,0" -c "POINT 10,0" -c "POINT 10,10"
            -c "POINT 0,10" -c "POINT 5,5" -c "GIS HULL DRAWING" -c "LIST")
set_tests_properties(cli.gis_hull_of_a_squares_corners_and_centre_is_the_square PROPERTIES
    PASS_REGULAR_EXPRESSION "hull kind=convex ratio= holes=no points=5 area=100\\.000.*Polyline  layer=gis/hull  vertices=4  closed  length=40  area=100"
    FAIL_REGULAR_EXPRESSION "error")

# A 100 m line through a box from x = 10 to 40, cut in place: 30 m stays on
# the line's own id.
add_test(NAME cli.gis_clip_in_place_leaves_thirty_metres_of_the_line
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "LINE 0,0 100,0"
            -c "LAYER NEW site" -c "LAYER SET site" -c "RECT 10,-5 40,5"
            -c "GIS CLIP LAYERS 0 BY LAYERS site REPLACE" -c "LIST")
set_tests_properties(cli.gis_clip_in_place_leaves_thirty_metres_of_the_line PROPERTIES
    PASS_REGULAR_EXPRESSION "target=in-place created=0 updated=1 deleted=0.*clip mode=replace features=1 whole=0 cut=1 outside=0.*1  Line  layer=0 .*length=30"
    FAIL_REGULAR_EXPRESSION "error")
