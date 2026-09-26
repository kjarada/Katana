katana_add_test_suite(geometry
    LIBS katana_geometry
    SOURCES
        test_primitives2d.cpp
        test_primitives3d.cpp
        test_intersection.cpp test_spatial_index.cpp
        test_polygon.cpp
        test_spiral2.cpp
        test_chording.cpp
        test_alignment.cpp
        test_profile.cpp
        test_editing.cpp
        test_curves2d.cpp
        test_polyline_vertices.cpp
        test_mesh.cpp
        test_point_batch.cpp
        test_point_splat.cpp
)
