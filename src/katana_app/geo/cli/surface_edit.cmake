# The surface edits through katana_cli (docs/surface_editing.md): SURFACE EDIT,
# MERGE, PASTE, CLIP and OFFSET are the interpreter's verbs, on the drawing's
# surfaces, one undo step each. Included from src/katana_app/CMakeLists.txt
# when the interop module is built (geo/cli/*.cmake).
set(_surface_edit_samples "${PROJECT_SOURCE_DIR}/samples/gis")

# samples/gis/terrain.asc as a surface: 10 800 vertices from 24.892 to 38.819
# (cli.surface_from_the_sample_terrain_then_list works the figures). RAISE 1
# moves every vertex by exactly 1: 25.892 to 39.819.
add_test(NAME cli.surface_edit_raise_moves_every_vertex_by_the_amount
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "SURFACE EDIT ground RAISE 1"
            -c "SURFACE INFO ground")
set_tests_properties(cli.surface_edit_raise_moves_every_vertex_by_the_amount PROPERTIES
    PASS_REGULAR_EXPRESSION "surface name=ground operation=raise .*moved=10800 held=0 crossings=0 largest_change=1 changed=yes.*surface name=ground triangles=[0-9]+ points=10800 bounds=-4\\.250,-4\\.250,174\\.250,129\\.250 zmin=25\\.892 zmax=39\\.819"
    FAIL_REGULAR_EXPRESSION "error")

# One undo step: UNDO puts the levels back.
add_test(NAME cli.surface_edit_is_one_undo_step
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "SURFACE EDIT ground RAISE 1"
            -c "UNDO"
            -c "SURFACE INFO ground")
set_tests_properties(cli.surface_edit_is_one_undo_step PROPERTIES
    PASS_REGULAR_EXPRESSION "points=10800 bounds=-4\\.250,-4\\.250,174\\.250,129\\.250 zmin=24\\.892 zmax=38\\.819"
    FAIL_REGULAR_EXPRESSION "error")

# A boundary inside the sample's extent clips it exactly to the square typed:
# its bounds are the square's.
add_test(NAME cli.surface_edit_boundary_clips_to_the_typed_square
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "SURFACE EDIT ground BOUNDARY 10,10 60,10 60,40 10,40"
            -c "SURFACE INFO ground")
set_tests_properties(cli.surface_edit_boundary_clips_to_the_typed_square PROPERTIES
    PASS_REGULAR_EXPRESSION "surface name=ground operation=boundary .*changed=yes.*surface name=ground triangles=[0-9]+ points=[0-9]+ bounds=10\\.000,10\\.000,60\\.000,40\\.000"
    FAIL_REGULAR_EXPRESSION "error")

# OFFSET makes a second surface 2 m down; the first is as it was.
add_test(NAME cli.surface_offset_makes_a_copy_by_name
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_edit_samples}/terrain.asc\" NAME ground"
            -c "SURFACE OFFSET ground -2 NAME subgrade"
            -c "SURFACE INFO subgrade"
            -c "SURFACE INFO ground")
set_tests_properties(cli.surface_offset_makes_a_copy_by_name PROPERTIES
    PASS_REGULAR_EXPRESSION "surface name=subgrade .*zmin=22\\.892 zmax=36\\.819.*surface name=ground .*zmin=24\\.892 zmax=38\\.819"
    FAIL_REGULAR_EXPRESSION "error")

# HELP SURFACE EDIT names every verb of the family.
add_test(NAME cli.surface_edit_help_names_the_family
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "HELP SURFACE EDIT")
set_tests_properties(cli.surface_edit_help_names_the_family PROPERTIES
    PASS_REGULAR_EXPRESSION "SURFACE EDIT <name> FLIP x,y.*SURFACE PASTE <top> INTO <base>")

# A surface nothing has is refused, and the session exits with a failure.
add_test(NAME cli.surface_edit_of_no_such_surface_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "SURFACE EDIT nothing THIN 0.1")
set_tests_properties(cli.surface_edit_of_no_such_surface_is_refused PROPERTIES WILL_FAIL TRUE)
