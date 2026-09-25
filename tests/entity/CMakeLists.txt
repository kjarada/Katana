# Tests written alongside the customisation managers, globbed so that several
# authors can add a file at once without editing this list.
file(GLOB _entity_customisation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/customisation/*.cpp")
# The annotation model's tests (docs/annotation.md), globbed for the same reason.
file(GLOB _entity_annotation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/annotation/*.cpp")

katana_add_test_suite(entity
    LIBS katana_entity
    SOURCES test_entity.cpp test_layer_tree.cpp test_geometry_blob.cpp test_display.cpp test_dimension_text.cpp test_named_table.cpp
        ${_entity_customisation_tests}
        ${_entity_annotation_tests}
)
