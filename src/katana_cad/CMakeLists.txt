add_library(katana_cad STATIC
    document.cpp
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
    plot.cpp
    dimension_draw.cpp
    scene.cpp
    viewport_layout.cpp
    layer_overrides.cpp
    view_set.cpp
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
katana_target_defaults(katana_cad)
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
