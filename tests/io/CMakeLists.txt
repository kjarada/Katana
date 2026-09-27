# GDAL for the test that reads the configuration katana_io gave it.
find_package(GDAL REQUIRED)

katana_add_test_suite(io
    LIBS katana_io GDAL::GDAL
    SOURCES test_gdal_pdal_adapters.cpp
)
