find_package(GDAL REQUIRED)
find_package(PDAL CONFIG REQUIRED)

# GDAL raster/vector access and PDAL point-cloud access behind Katana interfaces.
# These adapters predate the Result<T> error policy and still report failures by
# throwing; they are converted when Phase 20 (file interoperability) is built.
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
