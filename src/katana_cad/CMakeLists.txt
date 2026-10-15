add_library(katana_cad STATIC
    document.cpp
    document_status.cpp
    project_crs.cpp
    import_placement.cpp
    selection.cpp
    view_transform.cpp
    snapping.cpp
    command_interpreter.cpp
    section.cpp
    dashing.cpp
    hatching.cpp
    corridor.cpp
    grading.cpp
    symbols.cpp
    style_drawing.cpp
    survey_coding.cpp
    survey_import.cpp
    survey_tools.cpp
    parcel.cpp
    alignment_report.cpp
    plot.cpp
    dimension_draw.cpp
    scene.cpp
    scene_zoom.cpp
    viewport_layout.cpp
    layer_overrides.cpp
    view_set.cpp
    view_link.cpp
    view_verbs.cpp
    scope_verbs.cpp
    property_outline.cpp
)
# The interactive tools (include/katana/cad/interactive_tool.hpp) are globbed:
# each family is written by its own author, often at the same time as the
# others, and a list here would make every one of them edit this file. A file
# cannot join the build unseen and do nothing - tools/families.hpp lists every
# family by name, and a missing one is a link error.
file(GLOB _cad_tool_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.cpp")
target_sources(katana_cad PRIVATE ${_cad_tool_sources} interactive_tool.cpp)
# The customisation managers' logic (style and symbol catalogues, usage, purge,
# the code table): globbed for the same reason, several authors at once.
file(GLOB _cad_customisation_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/customisation/*.cpp")
target_sources(katana_cad PRIVATE ${_cad_customisation_sources})
# Sheets, the plot frame and the sheet generators (include/katana/cad/plotting/,
# docs/plotting.md): globbed for the same reason - the sheet model, its painter
# and its editor are written by different authors at the same time.
file(GLOB _cad_plotting_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/plotting/*.cpp")
target_sources(katana_cad PRIVATE ${_cad_plotting_sources})
# The drawing system - grips, vertex editing, precision input, the curve
# pieces (include/katana/cad/drawing/, docs/drawing.md): globbed for the
# same reason, written beside the annotation and plotting work.
file(GLOB _cad_drawing_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/drawing/*.cpp")
target_sources(katana_cad PRIVATE ${_cad_drawing_sources})
# The drawing's surfaces - their styles, edits and volumes (terrain/,
# docs/terrain.md, docs/surface_display.md, docs/surface_editing.md,
# docs/volumes.md) - and the road model (road/ and bim/, docs/road_design.md,
# docs/road_objects.md): globbed for the same reason, written side by side.
file(GLOB _cad_terrain_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/terrain/*.cpp")
file(GLOB _cad_road_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/road/*.cpp"
     "${CMAKE_CURRENT_SOURCE_DIR}/bim/*.cpp")
target_sources(katana_cad PRIVATE ${_cad_terrain_sources} ${_cad_road_sources})
# The built-in plot frame is compiled in with #embed (plotting/frame.cpp) from
# the committed resources/plot_frames/, so the frame is part of the program and
# a missing file is a build error, not a blank title block.
set_source_files_properties("${CMAKE_CURRENT_SOURCE_DIR}/plotting/frame.cpp" PROPERTIES
    COMPILE_OPTIONS "--embed-dir=${PROJECT_SOURCE_DIR}/resources/plot_frames")
# The built-in customisations: Katana customisation files compiled in with
# #embed, one table of them (customisation/builtin_customisation.cpp,
# customisation/builtin_table.hpp). Two cache variables say which:
#
#   KATANA_BUILTIN_CUSTOMISATION      THE built-in, the one a program starts with
#                                     when nothing says otherwise: a file
#   KATANA_ALTERNATE_CUSTOMISATIONS   the others, a ;-separated list of files,
#                                     which a person picks in Settings or with
#                                     CUSTOMISE BUILTIN <name>
#
# The files are in the repository, so a clean checkout and every CI build have
# both. The default, Katana Standard, was made for Katana: what it names, draws
# and words is Katana's own, and its survey code keys, with what each key
# stands for and the kinds of rule it has, are the alternate's, kept on the
# owner's decision so that the same field data works with either
# (docs/katana_standard.md). The alternate is third-party material tracked on
# the owner's decision of 2026-10-08 (docs/customisation.md, "Why the built-in
# is in the repository"). Unlike the plot frame's,
# a file's absence is still no build error: a path that names no file is not
# offered, which is how a build is made without one. No default and no
# alternate compiles a program with no built-in at all.
#
# NOTE: a cache keeps what it was given. A build directory configured before
# Katana Standard became the default has KATANA_BUILTIN_CUSTOMISATION set to the
# earlier default, and would offer Katana Standard nowhere. The first configure
# that sees it MOVES it to the new default, once, and says so
# (cmake/KatanaBuiltInDefault.cmake); a -D given on the command line is a
# choice and is never moved.
include(KatanaBuiltInDefault)
katana_declare_builtin_default(
    "${PROJECT_SOURCE_DIR}/resources/customisation/katana-standard.customisation.json")
set(KATANA_ALTERNATE_CUSTOMISATIONS
    "${PROJECT_SOURCE_DIR}/resources/customisation/nsw.customisation.json"
    CACHE STRING "Katana customisation files compiled into the program besides the default, a ;-separated list; one that does not exist is not offered")
# For a release job: the files are tracked, so the absence of the default means
# it was deleted or the path is wrong, and is then a configure error and not a
# program that draws plain lines. (The alternates are optional by nature.)
option(KATANA_REQUIRE_BUILTIN_CUSTOMISATION
    "Fail the configure when KATANA_BUILTIN_CUSTOMISATION does not exist, to guard a release job against the tracked file being deleted or a wrong path" OFF)

# Whether a path is a file the table can embed, and if so which: "" when it is
# not, else its real path, with '/'. WHETHER it is there is asked of the file
# itself. A glob is no judge of that: the path is a PATTERN to it, so a checkout
# under "work [2026]" matched nothing though the file was there - no built-in,
# without a word, and KATANA_REQUIRE_BUILTIN_CUSTOMISATION failing for a file
# in plain sight - while a DIRECTORY of that name matched, and broke the #embed.
#
# The glob stays for the one thing it is for: CONFIGURE_DEPENDS re-runs the
# configure when what it finds changes, so a file put there after the configure
# is picked up by the next build instead of waiting for someone to remember to
# configure again. The characters a pattern would read are bracketed, each
# standing for itself, so that it watches exactly this path.
function(_katana_builtin_candidate path result)
    set(${result} "" PARENT_SCOPE)
    if(path STREQUAL "")
        return()
    endif()
    string(REGEX REPLACE "([][*?])" "[\\1]" _pattern "${path}")
    file(GLOB _watched CONFIGURE_DEPENDS "${_pattern}")
    if(NOT EXISTS "${path}" OR IS_DIRECTORY "${path}")
        return()
    endif()
    get_filename_component(_real "${path}" REALPATH)
    # The path goes into a C string literal in the table: a quote, a backslash
    # or a line break in it cannot be written there.
    if(_real MATCHES "[\"\\\\\n\r]")
        message(STATUS "Built-in customisation not offered (its path holds a quote, a backslash or a line break): ${path}")
        return()
    endif()
    set(${result} "${_real}" PARENT_SCOPE)
endfunction()

set(_katana_builtin_files "")
set(_katana_builtin_has_default FALSE)
_katana_builtin_candidate("${KATANA_BUILTIN_CUSTOMISATION}" _katana_default_file)
if(_katana_default_file)
    list(APPEND _katana_builtin_files "${_katana_default_file}")
    set(_katana_builtin_has_default TRUE)
    message(STATUS "Built-in customisation (default): ${_katana_default_file}")
elseif(KATANA_REQUIRE_BUILTIN_CUSTOMISATION)
    message(FATAL_ERROR
        "KATANA_REQUIRE_BUILTIN_CUSTOMISATION is ON and the built-in customisation is not there:\n"
        "  ${KATANA_BUILTIN_CUSTOMISATION}\n"
        "Put the file there, or set KATANA_BUILTIN_CUSTOMISATION to where it is.")
else()
    message(STATUS "Built-in customisation (default): none (${KATANA_BUILTIN_CUSTOMISATION} is not a file)")
endif()
foreach(_katana_alternate IN LISTS KATANA_ALTERNATE_CUSTOMISATIONS)
    _katana_builtin_candidate("${_katana_alternate}" _katana_alternate_file)
    if(NOT _katana_alternate_file)
        message(STATUS "Built-in customisation (alternate) not offered, not a file: ${_katana_alternate}")
    elseif(_katana_alternate_file IN_LIST _katana_builtin_files)
        message(STATUS "Built-in customisation (alternate) is already offered: ${_katana_alternate_file}")
    else()
        list(APPEND _katana_builtin_files "${_katana_alternate_file}")
        message(STATUS "Built-in customisation (alternate): ${_katana_alternate_file}")
    endif()
endforeach()

# The table: ONE generated translation unit holding every file's bytes, so the
# number of built-ins is no limit of the source and a file is embedded once.
# Each file is named by its absolute path, so two files of one name in two
# folders do not collide (a --embed-dir would). It is rewritten only when its
# text changes, so a configure that changes nothing rebuilds nothing; the
# compiler's dependency file names every embedded file, so editing one rebuilds
# the table.
set(_katana_table_text "// Generated by src/katana_cad/CMakeLists.txt: the built-in customisations\n// this build compiles in (customisation/builtin_table.hpp). Do not edit.\n\n")
string(APPEND _katana_table_text
    "#include \"${CMAKE_CURRENT_SOURCE_DIR}/customisation/builtin_table.hpp\"\n\n"
    "namespace katana::cad::detail {\n\nnamespace {\n\n"
    "// #embed is C++26; the pragma keeps a C++23 build (KATANA_CXX_STANDARD) from\n"
    "// failing -Werror on it.\n"
    "#pragma GCC diagnostic push\n"
    "#if defined(__clang__)\n#pragma clang diagnostic ignored \"-Wc23-extensions\"\n"
    "#else\n#pragma GCC diagnostic ignored \"-Wc++26-extensions\"\n#endif\n\n")
set(_katana_table_entries "")
set(_katana_table_index 0)
foreach(_katana_file IN LISTS _katana_builtin_files)
    get_filename_component(_katana_file_name "${_katana_file}" NAME)
    # An empty file is no customisation, and no array either: one zero byte
    # stands for it, which the reader refuses in so many words.
    string(APPEND _katana_table_text
        "constexpr unsigned char kBytes${_katana_table_index}[] = {\n"
        "#embed \"${_katana_file}\" if_empty(0)\n};\n\n")
    string(APPEND _katana_table_entries
        "        {\"${_katana_file_name}\", {reinterpret_cast<const char*>(kBytes${_katana_table_index}), "
        "sizeof kBytes${_katana_table_index}}},\n")
    math(EXPR _katana_table_index "${_katana_table_index} + 1")
endforeach()
string(APPEND _katana_table_text "#pragma GCC diagnostic pop\n\n} // namespace\n\n")
if(_katana_builtin_files)
    string(APPEND _katana_table_text
        "std::span<const CompiledInFile> compiledInFiles()\n{\n"
        "    static const CompiledInFile files[] = {\n${_katana_table_entries}    };\n"
        "    return files;\n}\n\n")
else()
    string(APPEND _katana_table_text
        "std::span<const CompiledInFile> compiledInFiles() { return {}; }\n\n")
endif()
if(_katana_builtin_has_default)
    string(APPEND _katana_table_text "bool compiledInHasDefault() { return true; }\n")
else()
    string(APPEND _katana_table_text "bool compiledInHasDefault() { return false; }\n")
endif()
string(APPEND _katana_table_text "\n} // namespace katana::cad::detail\n")
set(_katana_table_file "${CMAKE_CURRENT_BINARY_DIR}/builtin_customisation_table.cpp")
set(_katana_table_before "")
if(EXISTS "${_katana_table_file}")
    file(READ "${_katana_table_file}" _katana_table_before)
endif()
if(NOT _katana_table_before STREQUAL _katana_table_text)
    file(WRITE "${_katana_table_file}" "${_katana_table_text}")
endif()
target_sources(katana_cad PRIVATE "${_katana_table_file}")
# For the tests, which register a check only where the build has what it
# needs (tests/CMakeLists.txt): how many built-ins this build compiles in, and
# which files, the default first.
list(LENGTH _katana_builtin_files _katana_builtin_count)
set(KATANA_BUILTIN_COUNT "${_katana_builtin_count}" CACHE INTERNAL
    "How many built-in customisations this build compiles in")
set(KATANA_BUILTIN_FILES "${_katana_builtin_files}" CACHE INTERNAL
    "The files of the built-in customisations this build compiles in, the default first")
# Survey jobs (survey_job*.cpp) are globbed so the files implementing them can
# be added without editing this list.
file(GLOB _cad_survey_job_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/survey_job*.cpp")
target_sources(katana_cad PRIVATE ${_cad_survey_job_sources})
# The annotation system (include/katana/cad/annotation/, docs/annotation.md):
# text layout, labels and their placer, leaders, the associative update and
# auto-labelling. Globbed like the sheets, so its parts can be added and
# written at the same time without each editing this list.
file(GLOB _cad_annotation_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/annotation/*.cpp")
target_sources(katana_cad PRIVATE ${_cad_annotation_sources})
# The AS 5488 subsurface utility verbs and the drawing they make
# (include/katana/cad/utilities/, docs/subsurface_utilities.md): here, over
# katana_survey's grading, so the window, katana_cli and katana_mcp share them.
target_sources(katana_cad PRIVATE utilities/utility_drawing.cpp utilities/utility_data.cpp
                                  utilities/utility_verbs.cpp)
# The geoprocessing packages' domain edits that need no GDAL - a surface
# built from the drawing, a drape (include/katana/cad/geo/,
# docs/geoprocessing.md): globbed, so the packages add files without editing
# this list. Empty until one does.
file(GLOB _cad_geo_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/geo/*.cpp")
target_sources(katana_cad PRIVATE ${_cad_geo_sources})
katana_target_defaults(katana_cad)
# The scene build's kernels (docs/performance.md, "SIMD: kernels chosen at run
# time"): lighting, colour and edge fades of dense TINs and meshes.
katana_add_simd_sources(katana_cad AVX2 simd/scene_avx2.cpp NEON simd/scene_neon.cpp)
# katana_geodesy for the survey tools' coordinate converter (survey_tools.cpp):
# the one survey tool whose answer is PROJ's, reached through the transformer
# that refuses a ballpark datum shift. tools/check_layering.cmake already lets
# cad see geodesy; PROJ itself stays behind katana_geodesy's src/ (Rule 4).
target_link_libraries(katana_cad PUBLIC katana_commands katana_storage katana_terrain katana_render katana_survey
                      katana_geodesy)
# The sheet set is stored as JSON (plotting/sheet_json.cpp) and the frame is
# read from JSON, through the same library the entity serialisation uses. Kept
# PRIVATE: no public header names it (tools/check_layering.cmake, Rule 4).
find_package(nlohmann_json CONFIG REQUIRED)
target_link_libraries(katana_cad PRIVATE nlohmann_json::nlohmann_json)
