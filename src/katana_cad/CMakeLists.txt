add_library(katana_cad STATIC
    document.cpp
    selection.cpp
    view_transform.cpp
    snapping.cpp
    command_interpreter.cpp
    section.cpp
    scene.cpp
    viewport_layout.cpp
)
katana_target_defaults(katana_cad)
target_link_libraries(katana_cad PUBLIC katana_commands katana_storage katana_terrain katana_render)
