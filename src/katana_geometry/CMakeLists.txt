add_library(katana_geometry STATIC
    primitives2d.cpp
    intersection.cpp
    polygon.cpp
    editing.cpp
)
katana_target_defaults(katana_geometry)
target_link_libraries(katana_geometry PUBLIC katana_math)
