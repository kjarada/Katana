# Vector fidelity through katana_cli (docs/interop.md, "Fidelity"): what
# IMPORT and EXPORT used to lose without a word, end to end in the session.
# Included from src/katana_app/CMakeLists.txt when the interop module is built.
set(_fidelity_data "${PROJECT_SOURCE_DIR}/tests/geo/data")
set(_fidelity_out "${CMAKE_CURRENT_BINARY_DIR}/cli fidelity")
file(MAKE_DIRECTORY "${_fidelity_out}")

# The project's coordinate system reaches the file: a KML is converted to
# longitude and latitude from it, and read back there, beside the point it
# came from. PROJ's own cs2cs puts
# 330000,6250000 in EPSG:28356 at 151.161906846,-33.876653623. The .kml is
# written by LIBKML, GDAL's own choice for the name (docs/interop.md,
# "Formats"), where a hand-kept table named the older KML driver.
add_test(NAME cli.export_of_a_projected_drawing_to_kml_is_in_longitude_and_latitude
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "CRS SET EPSG:28356" -c "POINT 330000,6250000"
            -c "EXPORT \"${_fidelity_out}/peg.kml\""
            -c "IMPORT \"${_fidelity_out}/peg.kml\"" -c "LIST")
set_tests_properties(cli.export_of_a_projected_drawing_to_kml_is_in_longitude_and_latitude PROPERTIES
    PASS_REGULAR_EXPRESSION "exported 1 features \\(LIBKML\\).*converted from GDA94 / MGA zone 56 \\(EPSG:28356\\) to longitude and latitude.*Point  layer=0  at 330000,6250000.*Point  layer=0  at 151\\.161906846,-33\\.876653623"
    FAIL_REGULAR_EXPRESSION "error")

# Without a project coordinate system there is nothing to convert from: the
# export is refused, and says how to put that right, rather than writing
# placemarks without their geometry.
add_test(NAME cli.export_to_kml_without_a_project_crs_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "POINT 330000,6250000"
            -c "EXPORT \"${_fidelity_out}/refused.kml\"")
set_tests_properties(cli.export_to_kml_without_a_project_crs_is_refused PROPERTIES
    PASS_REGULAR_EXPRESSION "error: InvalidCRS: KML holds longitude and latitude on WGS 84 and nothing else.*CRS SET")

# A lot with a hole, imported (two rings) and exported, is ONE polygon with
# its hole - the defect wrote two, 10400 m2 where the lot is 9600 - in the
# project's coordinate system, which the export used to leave out.
add_test(NAME cli.a_lot_with_a_hole_is_exported_as_one_polygon_with_the_projects_crs
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "CRS SET EPSG:28356"
            -c "IMPORT \"${_fidelity_data}/lot_with_hole.geojson\""
            -c "EXPORT \"${_fidelity_out}/lot.gpkg\"" -c "INFO \"${_fidelity_out}/lot.gpkg\"")
set_tests_properties(cli.a_lot_with_a_hole_is_exported_as_one_polygon_with_the_projects_crs PROPERTIES
    PASS_REGULAR_EXPRESSION "imported 2 entities.*exported 1 features \\(GPKG\\).*Coordinate system: GDA94 / MGA zone 56 \\(EPSG:28356\\).*katana: 1 feature, Polygon"
    FAIL_REGULAR_EXPRESSION "error")
