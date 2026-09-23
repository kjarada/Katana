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
katana_target_defaults(katana_cad)
target_link_libraries(katana_cad PUBLIC katana_commands katana_storage katana_terrain katana_render katana_survey)
