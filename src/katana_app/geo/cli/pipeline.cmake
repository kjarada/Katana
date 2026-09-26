# A GDAL pipeline through katana_cli (docs/geoprocessing.md, "Pipelines"):
# the session's own executor, end to end. Included from
# src/katana_app/CMakeLists.txt when the interop module is built.
#
# plane.asc is 40 x 30 cells of 1 m, z = 100 + 0.05x at the cell centres, so
# it spans 100.025 .. 101.975: contours every 0.5 m are 100.5, 101.0 and
# 101.5, each buffered 0.1 m into one area - three, by hand, drawn on the
# layer TO names.
add_test(NAME cli.gdal_pipeline_contours_then_buffer_from_a_raster_to_a_layer
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "GDAL pipeline \"read ! contour --interval 0.5 ! buffer 0.1 ! write\" FROM input FILE \"${PROJECT_SOURCE_DIR}/tests/geo/data/plane.asc\" TO LAYER gis/bands")
set_tests_properties(cli.gdal_pipeline_contours_then_buffer_from_a_raster_to_a_layer PROPERTIES
    PASS_REGULAR_EXPRESSION "gdal algorithm=pipeline policy=safe .*input arg=input source=file .*output arg=output kind=vector target=layer layer=gis/bands created=3"
    FAIL_REGULAR_EXPRESSION "error")
