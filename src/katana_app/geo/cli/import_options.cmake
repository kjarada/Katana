# IMPORT's options through katana_cli (docs/interop.md, "Import options"): the
# words that say what of a file is read, end to end in the session. Included
# from src/katana_app/CMakeLists.txt when the interop module is built.
#
# tests/geo/data/lots.geojson, by hand: EPSG:28356; A and B are kind=lot,
# 50 x 40 each, side by side from (0, 0); C is kind=corridor, 120 x 4, across
# both at y = 18..22.
set(_import_options_data "${PROJECT_SOURCE_DIR}/tests/geo/data")

# where= goes to the driver: kind = 'lot' is A and B, two of the three.
add_test(NAME cli.import_where_imports_only_the_matching_features
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "IMPORT \"${_import_options_data}/lots.geojson\" where=\"kind = 'lot'\"")
set_tests_properties(cli.import_where_imports_only_the_matching_features PROPERTIES
    PASS_REGULAR_EXPRESSION "imported file=[^\r\n]*lots\\.geojson\"? kind=vector entities=2 [^\r\n]*[\r\n]+matched features=2 of=3"
    FAIL_REGULAR_EXPRESSION "error")

# PREVIEW reads with the filters and imports nothing: the box (60, 5) -
# (90, 15) lies in B alone.
add_test(NAME cli.import_preview_counts_what_an_area_takes_and_imports_nothing
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "IMPORT \"${_import_options_data}/lots.geojson\" AREA 60,5,90,15 PREVIEW")
set_tests_properties(cli.import_preview_counts_what_an_area_takes_and_imports_nothing PROPERTIES
    PASS_REGULAR_EXPRESSION "scope scope=area area=60,5,90,15 matched=0 box=60,5,90,15 clip=no[\r\n]+import file=[^\r\n]*lots\\.geojson\"? kind=vector preview=yes features=1 of=3 entities=1 layers=1"
    FAIL_REGULAR_EXPRESSION "error|imported ")

# An open option is checked against the driver's own list: GDAL only warns of
# one it does not know and reads on without it.
add_test(NAME cli.import_with_an_unknown_open_option_is_refused_by_name
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "IMPORT \"${_import_options_data}/lots.geojson\" oo=NO_SUCH_OPTION=YES")
set_tests_properties(cli.import_with_an_unknown_open_option_is_refused_by_name PROPERTIES
    PASS_REGULAR_EXPRESSION "error: InvalidArgument: GeoJSON has no open option NO_SUCH_OPTION")

# crs=adopt: a drawing with no coordinate system takes the file's.
add_test(NAME cli.import_crs_adopt_sets_the_projects_coordinate_system
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "IMPORT \"${_import_options_data}/lots.geojson\" crs=adopt" -c "CRS")
set_tests_properties(cli.import_crs_adopt_sets_the_projects_coordinate_system PROPERTIES
    PASS_REGULAR_EXPRESSION "crs adopted=yes id=EPSG:28356.*crs id=EPSG:28356"
    FAIL_REGULAR_EXPRESSION "error")
