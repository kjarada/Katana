# IMPORT, EXPORT, INFO, REFS and COPC through katana_cli (docs/interop.md,
# "IMPORT, EXPORT, INFO, REFS and COPC on every front end"): the session's
# executor, end to end, replying in records. Included from
# src/katana_app/CMakeLists.txt when the interop module is built.
#
# The samples' facts, by hand: parcels.geojson holds 8 features - the spoil
# heaps a MultiPolygon of two, so 9 entities - spanning (180, 0) to
# (365, 165); terrain.asc is 120 x 90 cells of 1.5 from (-5, -5), so it spans
# to (175, 130); survey_scan.las holds 40 000 points (`pdal info`);
# multiple_tins.12da four tins of 9 points and 8 triangles.
set(_gis_verbs_samples "${PROJECT_SOURCE_DIR}/samples/gis")
set(_gis_verbs_archives "${PROJECT_SOURCE_DIR}/tests/archive12d/data")
set(_gis_verbs_out "${CMAKE_CURRENT_BINARY_DIR}/cli gis verbs")

add_test(NAME cli.gis_verbs_folder
    COMMAND ${CMAKE_COMMAND} -E make_directory "${_gis_verbs_out}")
set_tests_properties(cli.gis_verbs_folder PROPERTIES FIXTURES_SETUP cli_gis_verbs)

# Exported to a GeoPackage and read back into a new drawing: the same nine
# entities over the same extent.
add_test(NAME cli.gis_export_then_import_round_trips_the_parcels
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "IMPORT \"${_gis_verbs_samples}/parcels.geojson\""
            -c "EXPORT \"${_gis_verbs_out}/parcels.gpkg\"" -c "NEW"
            -c "IMPORT \"${_gis_verbs_out}/parcels.gpkg\"")
set_tests_properties(cli.gis_export_then_import_round_trips_the_parcels PROPERTIES
    FIXTURES_REQUIRED cli_gis_verbs
    PASS_REGULAR_EXPRESSION "exported file=\"[^\"]*/parcels\\.gpkg\" kind=vector driver=GPKG features=9 skipped=0.*imported file=\"[^\"]*/parcels\\.gpkg\" kind=vector entities=9 [^\r\n]*bounds=180,0,365,165"
    FAIL_REGULAR_EXPRESSION "error")

# A raster and a point cloud imported are reference layers, which REFS lists
# as records, one a layer, then the count.
add_test(NAME cli.gis_refs_lists_the_reference_layers_as_records
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "IMPORT \"${_gis_verbs_samples}/terrain.asc\""
            -c "IMPORT \"${_gis_verbs_samples}/survey_scan.las\"" -c "REFS")
set_tests_properties(cli.gis_refs_lists_the_reference_layers_as_records PROPERTIES
    PASS_REGULAR_EXPRESSION "reference id=1 kind=raster name=terrain width=120 height=90 bounds=-5,-5,175,130 [^\r\n]*[\r\n]+reference id=2 kind=pointcloud name=survey_scan points=40000 source_points=40000 [^\r\n]*[\r\n]+references rasters=1 clouds=1"
    FAIL_REGULAR_EXPRESSION "error")

# A 12d archive's surfaces are kept by the session, where it once said it
# held none, so the geoprocessing verbs after the import find them by name.
add_test(NAME cli.gis_an_archives_surfaces_are_kept_for_the_verbs_after_it
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "IMPORT \"${_gis_verbs_archives}/multiple_tins.12da\""
            -c "GDAL raster hillshade FROM SURFACE \"TIN SOUTH WEST\" CELL 10 TO REFERENCE shade")
set_tests_properties(cli.gis_an_archives_surfaces_are_kept_for_the_verbs_after_it PROPERTIES
    PASS_REGULAR_EXPRESSION "imported file=\"?[^ ]*/multiple_tins\\.12da\"? kind=archive entities=0 alignments=0 surfaces=4 .*surface name=\"TIN SOUTH WEST\" triangles=8 points=9 bounds=554000,6883000,554100,6883100 zmin=10 zmax=12\\.5 .*input arg=input source=surface name=\"TIN SOUTH WEST\".*output arg=output kind=raster target=reference id=1 name=shade "
    FAIL_REGULAR_EXPRESSION "error|holds no surfaces")

# The same IMPORT line the window runs in
# qt_gis_import_line_gives_the_sessions_records_headless
# (tests/geo/headless/gis_verbs.cmake), with the same expected records: one
# executor, one reply.
add_test(NAME cli.gis_import_line_gives_the_records_the_window_gives
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "RECT 1000,2000 1010,2005"
            -c "IMPORT \"${_gis_verbs_samples}/parcels.geojson\" ALONGSIDE")
set_tests_properties(cli.gis_import_line_gives_the_records_the_window_gives PROPERTIES
    PASS_REGULAR_EXPRESSION "imported file=\"?[^ ]*/parcels\\.geojson\"? kind=vector entities=9 layers=1 features=9 skipped=0 bounds=1000,2000,1185,2165 crs=\"[^\"]*EPSG:32630[^\"]*\"[\r\n]+placed placement=alongside east=820 north=2000 text=\"ALONGSIDE: moved as one piece by 820\\.000,2000\\.000, so its lower-left corner sits on the drawing's, at 1000\\.000,2000\\.000\\.\""
    FAIL_REGULAR_EXPRESSION "error")
