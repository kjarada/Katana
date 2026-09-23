# Import and export between external formats and the domain model
# (PLAN.MD Phase 20). This is the ONLY layer that sees both the external readers
# (katana_gis / katana_pointcloud, implemented by katana_io) and the entity
# model, which is what keeps GDAL and PDAL out of katana_cad.
add_library(katana_interop STATIC
    reference_data.cpp
    import.cpp
    export.cpp
    archive12d.cpp
    dataset_info.cpp
    terrain_io.cpp
)
katana_target_defaults(katana_interop)
# katana_terrain is PUBLIC and named although katana_archive12d brings it too:
# terrain_io.hpp includes tin_surface.hpp, and a dependency a public header
# has should not rest on another library happening to have it.
target_link_libraries(katana_interop
    PUBLIC katana_core katana_geometry katana_terrain katana_entity katana_archive12d katana_io
    PRIVATE katana_commands
)
