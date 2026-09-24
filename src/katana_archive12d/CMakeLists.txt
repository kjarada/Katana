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
