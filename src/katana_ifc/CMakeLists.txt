# IFC 4.3 export (IFC4X3_ADD2), natively: the drawing, its alignments, its
# surfaces and an AS 5488 subsurface utility investigation (docs/ifc.md).
#
# Deliberately NOT part of katana_interop, for the reason the DXF and 12d
# modules are not: writing IFC needs nothing but the domain model, so this
# builds with -DKATANA_BUILD_IO=OFF. It sees survey for the AS 5488 grading
# of the services it writes, and terrain for the surfaces.
add_library(katana_ifc STATIC
    step.cpp
    step_file.cpp
    builder.cpp
    classification.cpp
    alignment.cpp
    utilities.cpp
    drawing.cpp
    surfaces.cpp
    export.cpp
)
katana_target_defaults(katana_ifc)
target_link_libraries(katana_ifc
    PUBLIC katana_core katana_geometry katana_entity katana_survey
    PRIVATE katana_terrain)
