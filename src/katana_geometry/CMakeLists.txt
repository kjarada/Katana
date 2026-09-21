add_library(katana_geometry STATIC
    primitives2d.cpp
    spiral2.cpp
    intersection.cpp
    spatial_index.cpp
    polygon.cpp
    editing.cpp
)
katana_target_defaults(katana_geometry)
target_link_libraries(katana_geometry PUBLIC katana_math)
