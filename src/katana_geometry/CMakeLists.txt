add_library(katana_geometry STATIC
    primitives2d.cpp
    spiral2.cpp
    chording.cpp
    alignment.cpp
    profile.cpp
    intersection.cpp
    spatial_index.cpp
    polygon.cpp
    editing.cpp
    curves2d.cpp
    curve_offset.cpp
    polyline_vertices.cpp
    mesh.cpp
    point_batch.cpp
    point_splat.cpp
)
katana_target_defaults(katana_geometry)
katana_add_simd_sources(katana_geometry
    AVX2 simd/points_avx2.cpp simd/splat_avx2.cpp
    NEON simd/points_neon.cpp simd/splat_neon.cpp)
target_link_libraries(katana_geometry PUBLIC katana_math)
