find_package(nlohmann_json CONFIG REQUIRED)

add_library(katana_entity STATIC
    entity_geometry.cpp
    entity_database.cpp
    tables.cpp
    layer_path.cpp
    geometry_blob.cpp
    display.cpp
    table_usage.cpp
    dimension_text.cpp
    serialization.cpp
    style_library.cpp
    survey_map.cpp
    colour_names.cpp
    linework_codes.cpp
    customisation.cpp
    validation.cpp
    text_block.cpp
    annotation.cpp
    anchor.cpp
    label_text.cpp
    label_values.cpp
    leader_values.cpp
    curve_pieces.cpp
)
katana_target_defaults(katana_entity)
target_link_libraries(katana_entity
    PUBLIC katana_geometry
    PRIVATE nlohmann_json::nlohmann_json
)
