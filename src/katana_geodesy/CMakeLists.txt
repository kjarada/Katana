# Coordinate reference systems, transformations, units and site calibration
# (PLAN.MD Phase 11). PROJ is a PRIVATE dependency: its headers are included
# only by the sources in this directory and never by include/katana/geodesy
# (Rule 4, verified by tools/check_layering.cmake).
find_package(PROJ CONFIG REQUIRED)

add_library(katana_geodesy STATIC
    units.cpp
    proj_internal.cpp
    coordinate_reference_system.cpp
    coordinate_transformer.cpp
    grid_factors.cpp
    geodesic.cpp
    similarity_transform2d.cpp
)
katana_target_defaults(katana_geodesy)
target_link_libraries(katana_geodesy PUBLIC katana_math PRIVATE PROJ::proj)
