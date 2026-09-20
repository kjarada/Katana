find_package(nlohmann_json CONFIG REQUIRED)

add_library(katana_entity STATIC
    entity_geometry.cpp
    entity_database.cpp
    tables.cpp
    layer_path.cpp
    serialization.cpp
)
katana_target_defaults(katana_entity)
target_link_libraries(katana_entity
    PUBLIC katana_geometry
    PRIVATE nlohmann_json::nlohmann_json
)
