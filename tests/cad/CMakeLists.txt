# The interactive tools' tests (tests/cad/tools/) are globbed, as their
# sources are (src/katana_cad/CMakeLists.txt): each tool family's tests are
# written by that family's author, at the same time as the others.
file(GLOB _cad_tool_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.cpp")

katana_add_test_suite(cad
    LIBS katana_cad
    SOURCES test_cad.cpp test_indexed_queries.cpp test_dashing.cpp test_dimension_draw.cpp test_section.cpp test_viewport_layout.cpp test_scene.cpp test_hatching.cpp test_corridor.cpp test_grading.cpp test_parcel.cpp test_plot.cpp test_symbols.cpp test_style_drawing.cpp test_survey_coding.cpp test_survey_import.cpp test_layer_inheritance.cpp test_layer_overrides.cpp test_view_set.cpp test_survey_tools.cpp
        ${_cad_tool_tests}
)
