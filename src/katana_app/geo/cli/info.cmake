# INFO as data through katana_cli (docs/interop.md, "Dataset information"):
# records read from GDAL's own info JSON, the JSON itself, statistics and a
# folder. Included from src/katana_app/CMakeLists.txt when the interop module
# is built.
#
# By hand: terrain.asc is 120 x 90 Float32 cells with no-data -9999 and
# values 24.892 to 38.819 (read from the text; GDAL's Float32 copies of them
# print more digits); samples/gis holds three datasets - parcels.geojson,
# terrain.asc and survey_scan.las; lots.geojson is GeoJSON.
set(_info_samples "${PROJECT_SOURCE_DIR}/samples/gis")

add_test(NAME cli.info_stats_gives_the_sample_terrains_range
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "INFO \"${_info_samples}/terrain.asc\" STATS")
set_tests_properties(cli.info_stats_gives_the_sample_terrains_range PROPERTIES
    PASS_REGULAR_EXPRESSION "band band=1 type=Float32 nodata=-9999 min=24\\.89[0-9]* max=38\\.81[0-9]* mean=[0-9.]+ stddev=[0-9.]+ "
    FAIL_REGULAR_EXPRESSION "error")

add_test(NAME cli.info_of_a_folder_names_each_dataset_in_it
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "INFO \"${_info_samples}\"")
set_tests_properties(cli.info_of_a_folder_names_each_dataset_in_it PROPERTIES
    PASS_REGULAR_EXPRESSION "dataset file=[^ ]*/samples/gis kind=folder driver= crs= datasets=3.*found file=[^ ]*/parcels\\.geojson driver=GeoJSON.*found file=[^ ]*/terrain\\.asc driver=AAIGrid.*found file=[^ ]*/survey_scan\\.las driver=readers\\.las"
    FAIL_REGULAR_EXPRESSION "error")

add_test(NAME cli.info_json_is_gdals_own
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "INFO \"${PROJECT_SOURCE_DIR}/tests/geo/data/lots.geojson\" JSON")
set_tests_properties(cli.info_json_is_gdals_own PROPERTIES
    PASS_REGULAR_EXPRESSION "\\{\"path\":\"[^\"]*/lots\\.geojson\",\"raster\":null,\"vector\":\\{\"description\":\"[^\"]*/lots\\.geojson\",\"driverShortName\":\"GeoJSON\""
    FAIL_REGULAR_EXPRESSION "error")
