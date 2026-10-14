katana_add_test_suite(terrain
    LIBS katana_terrain
    SOURCES test_tin_builder.cpp test_tin_surface.cpp test_contours.cpp test_volume.cpp
            test_tiled_terrain.cpp test_super_surface.cpp
            # Volumes (docs/volumes.md).
            test_overlay.cpp test_section_volume.cpp test_volume_methods.cpp
)
