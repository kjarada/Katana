# Every driver through katana_cli (docs/interop.md, "Formats"): FORMATS reads
# GDAL's own registry, and a dataset inside an archive is imported through
# it. Included from src/katana_app/CMakeLists.txt when the interop module is
# built.
set(_formats_data "${PROJECT_SOURCE_DIR}/tests/geo/data")
set(_formats_out "${CMAKE_CURRENT_BINARY_DIR}/cli formats")
file(MAKE_DIRECTORY "${_formats_out}")

# FORMATS VECTOR WRITE is what EXPORT can write layers with, from the
# registry: FlatGeobuf, which the hand-kept table of ten extensions could not
# write, and GeoPackage, which holds rasters and vectors and writes both
# (gdal.org/drivers/vector/gpkg.html). A raster-only format (GeoTIFF) writes
# no layers and is not listed. FORMATS OPTIONS gives a driver's declared
# options, GeoPackage's LIST_ALL_TABLES among its open options.
add_test(NAME cli.formats_lists_writable_vector_drivers
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "FORMATS VECTOR WRITE" -c "FORMATS OPTIONS GPKG")
set_tests_properties(cli.formats_lists_writable_vector_drivers PROPERTIES
    PASS_REGULAR_EXPRESSION "format driver=FlatGeobuf kind=vector read=vector write=vector extensions=fgb vsi=yes.*format driver=GPKG kind=raster,vector read=raster,vector write=raster,vector extensions=gpkg,gpkg\\.zip.*listed formats=[0-9]+ kind=vector capability=write filter= gdal=3\\.[0-9.]+.*option driver=GPKG list=open name=LIST_ALL_TABLES type=string-select default=AUTO scope=vector choices=AUTO,YES,NO"
    FAIL_REGULAR_EXPRESSION "driver=GTiff|error")

# A zipped shapefile - EXPORT writes one for .shp.zip, GDAL's own name for it
# - imported by naming the shapefile inside, as GDAL names it:
# /vsizip/<archive>/<member>. The member is katana.shp because EXPORT names
# its one layer "katana". GdalDataset::open refused every such path as a
# missing file. UNDO takes the first import away, so LIST is what came out of
# the archive: lots.geojson's two 50 x 40 lots and its 120 x 4 corridor.
add_test(NAME cli.import_of_a_zipped_shapefile_by_vsizip
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "CRS SET EPSG:28356"
            -c "IMPORT \"${_formats_data}/lots.geojson\""
            -c "EXPORT \"${_formats_out}/lots.shp.zip\"" -c "UNDO"
            -c "IMPORT \"/vsizip/${_formats_out}/lots.shp.zip/katana.shp\"" -c "LIST")
set_tests_properties(cli.import_of_a_zipped_shapefile_by_vsizip PROPERTIES
    PASS_REGULAR_EXPRESSION "exported file=\"[^\"]*/lots\\.shp\\.zip\" kind=vector driver=\"ESRI Shapefile\" features=3 .*imported file=\"/vsizip/[^\"]*/lots\\.shp\\.zip/katana\\.shp\" kind=vector entities=3 .*3 entities.*area=2000.*area=2000.*area=480"
    FAIL_REGULAR_EXPRESSION "error")

# A plain .zip, which GDAL does not open as it is, is imported by the one
# dataset inside it; the archive is made from the fixture when the build is
# configured.
execute_process(
    COMMAND ${CMAKE_COMMAND} -E tar cf "${_formats_out}/lots.zip" --format=zip lots.geojson
    WORKING_DIRECTORY "${_formats_data}")
add_test(NAME cli.import_of_a_zip_opens_the_one_dataset_inside
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "IMPORT \"${_formats_out}/lots.zip\"" -c "LIST")
set_tests_properties(cli.import_of_a_zip_opens_the_one_dataset_inside PROPERTIES
    PASS_REGULAR_EXPRESSION "imported file=\"[^\"]*/lots\\.zip\" kind=vector entities=3 .*3 entities.*Polyline  layer=lots [^\r\n]*area=2000"
    FAIL_REGULAR_EXPRESSION "error")
