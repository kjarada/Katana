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
    # The web access and the reprojection the online import rests on
    # (docs/gis_online.md): HTTP through GDAL's own libcurl, XML through its
    # parser, warping and coordinate transformation through GDAL and OGR.
    web_access.cpp
    reproject.cpp
)
# GDAL's algorithm framework behind plain types (include/katana/gis/processing.hpp,
# docs/geoprocessing.md): globbed, one directory, so the geoprocessing lanes
# add files without editing this list. The directory's own headers
# (gdal_registry.hpp, ogr_detail.hpp) are shared with it.
file(GLOB _katana_io_geo CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/geo/*.cpp")
target_sources(katana_io PRIVATE ${_katana_io_geo})
target_include_directories(katana_io PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")
katana_target_defaults(katana_io)
target_link_libraries(katana_io
    PUBLIC katana_core
    PRIVATE GDAL::GDAL pdalcpp
)
