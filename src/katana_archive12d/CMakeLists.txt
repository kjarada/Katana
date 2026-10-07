# The archive file format (.12da) - text <-> typed archive <-> the domain
# model (PLAN.MD Phase 20). This is the product's: katana, katana_cli,
# katana_mcp and katana_interop link it.
#
# The older customisation formats - style libraries (.4d) and survey code
# files (.mapfile) - are NOT here any more. Their readers and writers share
# this module's lexer and text utilities, which is why they are in this
# directory, under legacy/, but they are a library of their own
# (katana_legacy_customisation, below) that only the converter and its tests
# link: the program reads one customisation format, the Katana one, and the
# test the_programs_link_none_of_the_legacy_customisation_readers
# (tests/CMakeLists.txt) fails if the older readers ever get into katana,
# katana_cli or katana_mcp.
#
# Deliberately NOT part of katana_interop. Reading a 12da needs no third-party
# library, so this module builds with -DKATANA_BUILD_IO=OFF - which puts a
# hand-written parser of untrusted text under the sanitizer job, the one place
# in the codebase that most needs it. Only the zipped container (.12daz) needs
# a library, and that stays in katana_io behind gis/zip_container.hpp.
add_library(katana_archive12d STATIC
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
)
target_include_directories(katana_archive12d PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

katana_target_defaults(katana_archive12d)
target_link_libraries(katana_archive12d
    PUBLIC katana_core katana_geometry katana_entity katana_terrain
)

# What is in this file, and what would Katana take from it? See probe_main.cpp.
add_executable(katana_12da_probe probe_main.cpp)
katana_target_defaults(katana_12da_probe)
target_link_libraries(katana_12da_probe PRIVATE katana_archive12d)

# The older customisation formats and the converter of them: style libraries
# (.4d) and survey code files (.mapfile) in, one Katana customisation out
# (legacy/convert.hpp; docs/customisation.md, "Converting a customisation from
# the legacy formats"). A developer's tool, like the probe above: built with
# everything else so that it cannot rot, never installed
# (cmake/KatanaPackaging.cmake installs three programs by name, and
# tools/check_install_rules.cmake fails on a fourth), and linked by none of
# katana, katana_cli and katana_mcp (the same test).
#
# The two readers, the two writers, the by-content loader and the tables of
# the older formats' words (in both directions) are a library of their own so
# that the tests call the functions rather than run a program, and so that
# nothing that links katana_archive12d for the .12da archive gets them with
# it. Only katana_customisation_convert and katana_archive12d_tests link it.
add_library(katana_legacy_customisation STATIC
    legacy/convert.cpp
    legacy/customisation.cpp
    legacy/map_file.cpp
    legacy/map_file_writer.cpp
    legacy/style_library.cpp
    legacy/style_library_writer.cpp
)
katana_target_defaults(katana_legacy_customisation)
# The headers are a tool's, not the program's, so they are not under include/:
# the two things that link the library reach them as "convert.hpp",
# "style_library.hpp" and so on, because the directory published is legacy/
# ALONE. This module's own directory is PRIVATE to the library - it is where
# the lexer and the text utilities are, which the readers share with the
# archive reader - and is never handed on.
target_include_directories(katana_legacy_customisation
    PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/legacy"
    PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(katana_legacy_customisation PUBLIC katana_archive12d)

add_executable(katana_customisation_convert legacy/convert_main.cpp)
katana_target_defaults(katana_customisation_convert)
target_link_libraries(katana_customisation_convert PRIVATE katana_legacy_customisation)
if(WIN32)
    # CommandLineToArgvW: the command line as UTF-16, which is the only form
    # of it that holds every file name (convert_main.cpp).
    target_link_libraries(katana_customisation_convert PRIVATE shell32)
endif()
