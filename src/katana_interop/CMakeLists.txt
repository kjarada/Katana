# Import and export between external formats and the domain model
# (PLAN.MD Phase 20). This is the ONLY layer that sees both the external readers
# (katana_gis / katana_pointcloud, implemented by katana_io) and the entity
# model, which is what keeps GDAL and PDAL out of katana_cad.
add_library(katana_interop STATIC
    reference_data.cpp
    import.cpp
    export.cpp
)
katana_target_defaults(katana_interop)
target_link_libraries(katana_interop
    PUBLIC katana_core katana_geometry katana_entity katana_io
    PRIVATE katana_commands
)
