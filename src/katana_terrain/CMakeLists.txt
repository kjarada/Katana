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
    # The named surfaces of a drawing (docs/terrain.md, "Surfaces in the
    # drawing").
    surface_store.cpp
    # A surface as bytes, as a project keeps it (same chapter).
    tin_codec.cpp
    # A sight line over the ground, for the LOS verb (docs/terrain.md).
    line_of_sight.cpp
    # Volumes (docs/volumes.md): the overlay of two surfaces, the difference
    # and envelope surfaces made of it, and volumes from cross sections.
    overlay.cpp
    section_volume.cpp
    # Editing a surface, and surfaces made of others (docs/surface_editing.md).
    tin_patch.cpp
    tin_edit.cpp
    tin_thin.cpp
    # The named colour ramps, the one table that a surface's display style and
    # RASTER SHADE / RASTER SLOPE both paint with (docs/surface_display.md,
    # "The ramps").
    colour_ramps.cpp
)
katana_target_defaults(katana_terrain)
target_link_libraries(katana_terrain PUBLIC katana_geometry PRIVATE CGAL::CGAL)
