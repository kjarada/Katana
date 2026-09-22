# The 12d file formats: the Archive (.12da) - text <-> typed archive <-> the
# domain model (PLAN.MD Phase 20) - and beside it the survey coding files a
# customisation is made of, the linestyle and symbol libraries (.4d) and the
# mapfile (PLAN.MD 20.3). They share an encoding, a lexer and a set of colour
# names, which is why they share a module.
#
# Deliberately NOT part of katana_interop. Reading a 12da needs no third-party
# library, so this module builds with -DKATANA_BUILD_IO=OFF - which puts a
# hand-written parser of untrusted text under the sanitizer job, the one place
# in the codebase that most needs it. Only the zipped container (.12daz) needs
# a library, and that stays in katana_io behind gis/zip_container.hpp.
add_library(katana_archive12d STATIC
    text_encoding.cpp
    text_utilities.cpp
    lexer.cpp
    archive.cpp
    reader.cpp
    writer.cpp
    coverage.cpp
    plan_geometry.cpp
    domain_common.cpp
    domain_import.cpp
    domain_export.cpp
    style_library.cpp
    xml.cpp
    map_file.cpp
)
katana_target_defaults(katana_archive12d)
target_link_libraries(katana_archive12d
    PUBLIC katana_core katana_geometry katana_entity katana_terrain
)

# What is in this file, and what would Katana take from it? See probe_main.cpp.
add_executable(katana_12da_probe probe_main.cpp)
katana_target_defaults(katana_12da_probe)
target_link_libraries(katana_12da_probe PRIVATE katana_archive12d)
