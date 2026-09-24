# DXF read and write, natively: entities, layers and linetypes, with no
# third-party library (docs/dxf.md).
#
# Deliberately NOT part of katana_interop, for the reason the 12d archive
# module is not: reading a DXF needs nothing but the entity model, so this
# builds with -DKATANA_BUILD_IO=OFF, and the sanitizer job runs a hand-written
# parser of untrusted text - which is where one most needs to be run.
add_library(katana_dxf STATIC
    tokenizer.cpp
    codes.cpp
    reader.cpp
    writer.cpp
)
katana_target_defaults(katana_dxf)
target_link_libraries(katana_dxf PUBLIC katana_core katana_geometry katana_entity)
