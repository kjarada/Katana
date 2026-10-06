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
    style_library_writer.cpp
    map_file.cpp
    map_file_writer.cpp
    customisation.cpp
    customisation_merge.cpp
)
# The customisation compiled into the build: its linestyles, symbols and
# survey codes become part of the program rather than something it is given.
# The source is GENERATED, never committed - the customisation is third-party
# material under its own licence and is not in this repository, and a checkout
# without it generates an empty table (see tools/embed_customisation.py).
#
# Its files are read from KATANA_CUSTOMISATION_DIR, git-ignored in the source
# tree by default, in name order: the style libraries linestyles.4d and
# symbols.4d, and the survey code files survey_codes.mapfile and
# survey_codes_names.mapfile (the second read after the first, whose rules win
# a field they both give). A cache variable, so a build can be pointed at a
# customisation kept elsewhere without moving it.
set(KATANA_CUSTOMISATION_DIR "${PROJECT_SOURCE_DIR}/resources/customisation" CACHE PATH
    "Directory of the customisation compiled into the build (style libraries and survey code files); empty or absent builds none")
set(_customisation "${KATANA_CUSTOMISATION_DIR}")
set(_embedded "${CMAKE_CURRENT_BINARY_DIR}/builtin_customisation.cpp")
file(GLOB _customisation_files "${_customisation}/*")
add_custom_command(
    OUTPUT "${_embedded}"
    COMMAND ${CMAKE_COMMAND} -E env
            "${KATANA_PYTHON}" "${PROJECT_SOURCE_DIR}/tools/embed_customisation.py"
            "${_embedded}" "${_customisation}"
    DEPENDS "${PROJECT_SOURCE_DIR}/tools/embed_customisation.py" ${_customisation_files}
    COMMENT "Embedding the built-in customisation"
    VERBATIM)
target_sources(katana_archive12d PRIVATE "${_embedded}")
target_include_directories(katana_archive12d PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

katana_target_defaults(katana_archive12d)
target_link_libraries(katana_archive12d
    PUBLIC katana_core katana_geometry katana_entity katana_terrain
)

# What is in this file, and what would Katana take from it? See probe_main.cpp.
add_executable(katana_12da_probe probe_main.cpp)
katana_target_defaults(katana_12da_probe)
target_link_libraries(katana_12da_probe PRIVATE katana_archive12d)

# The converter of the older customisation formats - style libraries (.4d) and
# survey code files (.mapfile) in, one Katana customisation out (legacy/
# convert.hpp; docs/customisation.md, "Converting a customisation from the
# legacy formats"). A developer's tool, like the probe above: built with
# everything else so that it cannot rot, never installed
# (cmake/KatanaPackaging.cmake installs three programs by name, and this is
# not one), and linked by none of katana, katana_cli and katana_mcp.
#
# What it does is a library of its own, so that the tests call the functions
# rather than run a program, and so that nothing that links katana_archive12d
# for the .12da archive gets the converter with it. The directory is where the
# readers of the older formats are to move when they leave the product; until
# then they are still in katana_archive12d, which this links.
add_library(katana_legacy_customisation STATIC legacy/convert.cpp)
katana_target_defaults(katana_legacy_customisation)
# Its header is a tool's, not the program's, so it is not under include/. The
# two things that link the library reach it as "convert.hpp": the directory
# published is legacy/ ALONE, never this module's own, whose private headers
# (the lexer, the text utilities, the embedded customisation) stay out of
# reach of everything that links this.
target_include_directories(katana_legacy_customisation PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/legacy")
target_link_libraries(katana_legacy_customisation PUBLIC katana_archive12d)

add_executable(katana_customisation_convert legacy/convert_main.cpp)
katana_target_defaults(katana_customisation_convert)
target_link_libraries(katana_customisation_convert PRIVATE katana_legacy_customisation)
if(WIN32)
    # CommandLineToArgvW: the command line as UTF-16, which is the only form
    # of it that holds every file name (convert_main.cpp).
    target_link_libraries(katana_customisation_convert PRIVATE shell32)
endif()
