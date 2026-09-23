find_package(GDAL REQUIRED)
find_package(PDAL CONFIG REQUIRED)

# GDAL raster/vector access and PDAL point-cloud access behind Katana interfaces.
# Failures cross the interface as Result<T> / Status: whatever GDAL reports and
# PDAL throws is caught and converted inside these files, and nothing throws
# out of them (docs/interop.md, "Layering").
add_library(katana_io STATIC
    gdal_adapter.cpp
    point_cloud_engine.cpp
    zip_container.cpp
)
katana_target_defaults(katana_io)
target_link_libraries(katana_io
    PUBLIC katana_core
    PRIVATE GDAL::GDAL pdalcpp
)
