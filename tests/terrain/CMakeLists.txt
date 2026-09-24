katana_add_test_suite(terrain
    LIBS katana_terrain
    SOURCES test_tin_builder.cpp test_tin_surface.cpp test_contours.cpp test_volume.cpp
            test_tiled_terrain.cpp test_super_surface.cpp test_terrain_simd.cpp
)

# The suite again with the level forced through the KATANA_SIMD override, as
# tests/CMakeLists.txt does for core and geometry: `scalar` is what a
# processor without AVX2 runs, `avx2` puts every contour and locate test in
# the suite on the kernels. Registered here rather than in that list so the
# terrain kernels stay within this module's files.
if(TARGET katana_terrain_tests)
    foreach(_level scalar avx2)
        add_test(NAME simd_${_level}.terrain COMMAND katana_terrain_tests)
        set_tests_properties(simd_${_level}.terrain PROPERTIES
            ENVIRONMENT_MODIFICATION
                "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};KATANA_SIMD=set:${_level}")
    endforeach()
endif()
