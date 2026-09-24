# Software renderer (PLAN.MD Phase 15).
#
# Depends on core (for the task pool), math and geometry, and on nothing above
# them: the renderer consumes a DrawList and knows nothing of entities, the
# document or a project (Rule 3). The Vulkan backend of Phase 15 becomes a
# second implementation behind the same DrawList/Camera interface, so nothing
# above this module changes when it lands.
add_library(katana_render STATIC
    framebuffer.cpp
    camera.cpp
    draw_list.cpp
    rasterizer.cpp
)
katana_target_defaults(katana_render)
# The fill and the vertex transform, eight pixels or four vertices a step on a
# processor with AVX2; rasterizer.cpp chooses at run time (docs/performance.md).
katana_add_simd_sources(katana_render AVX2 simd/raster_avx2.cpp)
target_link_libraries(katana_render PUBLIC katana_geometry katana_core)
