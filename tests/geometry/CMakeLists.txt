katana_add_test_suite(geometry
    LIBS katana_geometry
    SOURCES
        test_primitives2d.cpp
        test_primitives3d.cpp
        test_intersection.cpp test_spatial_index.cpp
        test_polygon.cpp
        test_spiral2.cpp
        test_editing.cpp
)
