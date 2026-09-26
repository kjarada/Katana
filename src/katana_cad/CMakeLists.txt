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
    viewport_layout.cpp
    layer_overrides.cpp
    view_set.cpp
    scope_verbs.cpp
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
# The built-in plot frame is compiled in with #embed (plotting/frame.cpp) from
# the committed resources/plot_frames/, so the frame is part of the program and
# a missing file is a build error, not a blank title block.
set_source_files_properties("${CMAKE_CURRENT_SOURCE_DIR}/plotting/frame.cpp" PROPERTIES
    COMPILE_OPTIONS "--embed-dir=${PROJECT_SOURCE_DIR}/resources/plot_frames")
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
katana_add_simd_sources(katana_cad AVX2 simd/scene_avx2.cpp)
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
