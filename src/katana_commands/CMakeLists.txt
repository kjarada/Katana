add_library(katana_commands STATIC
    change_set.cpp
    command_stack.cpp
    entity_commands.cpp
    layer_commands.cpp
)
katana_target_defaults(katana_commands)
target_link_libraries(katana_commands PUBLIC katana_entity)
