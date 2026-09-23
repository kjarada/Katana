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
katana_target_defaults(katana_cad)
# katana_geodesy for the survey tools' coordinate converter (survey_tools.cpp):
# the one survey tool whose answer is PROJ's, reached through the transformer
# that refuses a ballpark datum shift. tools/check_layering.cmake already lets
# cad see geodesy; PROJ itself stays behind katana_geodesy's src/ (Rule 4).
target_link_libraries(katana_cad PUBLIC katana_commands katana_storage katana_terrain katana_render katana_survey
                      katana_geodesy)
