# IFC 4.3 exchange (IFC4X3_ADD2 out; IFC2X3, IFC4 and IFC4X3 in), natively:
# the drawing, its alignments, its surfaces and an AS 5488 subsurface utility
# investigation (docs/ifc.md), and what the two front ends share about it
# (front_end.cpp: the IMPORT/EXPORT grammar and the files an export names).
#
# Deliberately NOT part of katana_interop, for the reason the DXF and .12da
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
    step_reader.cpp
    import_alignment.cpp
    import.cpp
    front_end.cpp
)
katana_target_defaults(katana_ifc)
# commands for one thing, the import as the single undoable step both front
# ends execute (import.hpp), as katana_dxf has it; terrain for the surfaces an
# import returns.
target_link_libraries(katana_ifc
    PUBLIC katana_core katana_geometry katana_entity katana_survey katana_terrain katana_commands)
