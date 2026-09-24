# Terrain engine (PLAN.MD Phase 14).
#
# CGAL is an implementation detail of this module (PLAN.MD Rule 4): it is linked
# PRIVATE and included by cdt_backend.cpp only. CGAL is header-only; its imported
# target brings GMP/MPFR and the Boost headers.
find_package(CGAL REQUIRED)

add_library(katana_terrain STATIC
    cdt_backend.cpp
    point_merge.cpp
    tin_surface.cpp
    tin_builder.cpp
    contours.cpp
    volume.cpp
    tiled_terrain.cpp
    super_surface.cpp
)
katana_target_defaults(katana_terrain)
# Contour level ranges and the containment test of locate(), four triangles
# per step (docs/terrain.md, "SIMD").
katana_add_simd_sources(katana_terrain AVX2 simd/terrain_avx2.cpp)
target_link_libraries(katana_terrain PUBLIC katana_geometry PRIVATE CGAL::CGAL)
