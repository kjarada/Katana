# RASTER ZONAL, RASTER SAMPLE and DRAPE through katana_cli (docs/terrain.md,
# "Statistics by area", "Sampling and drape"). Included from
# src/katana_app/CMakeLists.txt when the interop module is built.
set(_zonal_data "${PROJECT_SOURCE_DIR}/tests/geo/data")

# tests/geo/data/plane.asc is z = 100 + 0.05 x at the centres of its 1 m
# cells. A lot drawn from (10,5) to (30,25) covers columns 10 to 29 and 20
# rows whole: 400 cells, a count of exactly 400, written on the lot (entity
# 1) in place.
add_test(NAME cli.raster_zonal_writes_properties_on_a_drawn_lot
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "RECT 10,5 30,25"
            -c "RASTER ZONAL FILE \"${_zonal_data}/plane.asc\" DRAWING stats=count,mean")
set_tests_properties(cli.raster_zonal_writes_properties_on_a_drawn_lot PROPERTIES
    PASS_REGULAR_EXPRESSION "gis op=zonal seconds=[0-9.]+ cancelled=no.*zones used=1 skipped.open=0 skipped.points=0.*output arg=output kind=vector target=in-place created=0 updated=1 deleted=0 skipped=0.*zone entity=1 count=400 mean=10[01]\\.[0-9]+.*zonal stats=count,mean prefix=zone pixels=fractional zones=1"
    FAIL_REGULAR_EXPRESSION "error")

# Bilinear on the plane is the plane: 100 + 0.05 x 12.3 = 100.615 (read as
# Float32, within 4e-6); x = 50 is off the 40 m raster, so no height.
add_test(NAME cli.raster_sample_is_the_plane_and_nothing_off_it
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "RASTER SAMPLE FILE \"${_zonal_data}/plane.asc\" AT 12.3,7.7 AT 50,10")
set_tests_properties(cli.raster_sample_is_the_plane_and_nothing_off_it PROPERTIES
    PASS_REGULAR_EXPRESSION "sample at=12.3,7.7 z=100\\.61[45][0-9]*.*sample at=50,10 ground=no.*samples method=bilinear count=2 on=1 off=1"
    FAIL_REGULAR_EXPRESSION "error")

# A drawn string draped on the plane: every one of its three vertices gets
# a height, in one step.
add_test(NAME cli.drape_gives_every_vertex_of_a_drawn_string_a_height
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "PLINE 5.5,5 20.25,10 35,25"
            -c "DRAPE FILE \"${_zonal_data}/plane.asc\" DRAWING")
set_tests_properties(cli.drape_gives_every_vertex_of_a_drawn_string_a_height PROPERTIES
    PASS_REGULAR_EXPRESSION "gis op=drape .*target=in-place created=0 updated=1 .*drape method=bilinear entities=1 vertices=3 off=0 entities_off=0 unchanged=0"
    FAIL_REGULAR_EXPRESSION "error")
