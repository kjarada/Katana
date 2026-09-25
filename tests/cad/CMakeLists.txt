# The interactive tools' tests (tests/cad/tools/) are globbed, as their
# sources are (src/katana_cad/CMakeLists.txt): each tool family's tests are
# written by that family's author, at the same time as the others.
file(GLOB _cad_tool_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.cpp")
# The customisation managers' tests, globbed as their sources are.
file(GLOB _cad_customisation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/customisation/*.cpp")
# The sheet model's tests (docs/plotting.md), globbed as its sources are.
file(GLOB _cad_plotting_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/plotting/*.cpp")
file(GLOB _cad_survey_job_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/test_survey_job*.cpp")
# The annotation system's tests (docs/annotation.md), globbed as its sources are.
file(GLOB _cad_annotation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/annotation/*.cpp")
# The AS 5488 utility verbs and their drawing (docs/subsurface_utilities.md).
file(GLOB _cad_utility_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/utilities/*.cpp")

katana_add_test_suite(cad
    LIBS katana_cad
    SOURCES test_cad.cpp test_indexed_queries.cpp test_dashing.cpp test_dimension_draw.cpp test_section.cpp test_viewport_layout.cpp test_scene.cpp test_scene_simd.cpp test_hatching.cpp test_corridor.cpp test_grading.cpp test_parcel.cpp test_plot.cpp test_symbols.cpp test_style_drawing.cpp test_survey_coding.cpp test_survey_import.cpp test_layer_inheritance.cpp test_layer_overrides.cpp test_view_set.cpp test_survey_tools.cpp test_document_index.cpp test_document_changes.cpp test_project_crs.cpp
        ${_cad_tool_tests}
        ${_cad_customisation_tests}
        ${_cad_plotting_tests}
        ${_cad_survey_job_tests}
        ${_cad_annotation_tests}
        ${_cad_utility_tests}
        test_alignment_report.cpp
)
# The sample investigation the utility tests draw and report, the one the
# cli.utility_* tests and docs/subsurface_utilities.md work by hand.
target_compile_definitions(katana_cad_tests PRIVATE
    KATANA_UTILITY_SAMPLES="${PROJECT_SOURCE_DIR}/samples/utilities")
# The Modify > Edit family's tests reach its internal header
# (tools/modify_edit_support.hpp: EditSession, drawable) the way the family's
# sources do, rather than by a path climbing out of tests/.
target_include_directories(katana_cad_tests PRIVATE "${PROJECT_SOURCE_DIR}/src/katana_cad")
