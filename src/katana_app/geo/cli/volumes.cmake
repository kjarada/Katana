# The volume verbs through katana_cli (docs/volumes.md): VOLUME and SURFACE
# DIFFERENCE are the interpreter's, and the surfaces they measure are made by
# SURFACE FROM, which is the geoprocessing executor's - so these run only
# where the interop module is built, and are included from
# src/katana_app/CMakeLists.txt with the other geo/cli files.
#
# Every test draws the same two surfaces: a closed 3D string round the square
# [0, 10]^2 at 10 on layer base, and one at 11 on layer top, each triangulated
# into a surface of two triangles. Between them is a 1 m slab of 100 m^3.
set(_volume_surfaces
    -c "LAYER NEW base" -c "LAYER SET base"
    -c "PLINE3D 0,0,10 10,0,10 10,10,10 0,10,10 CLOSE"
    -c "LAYER NEW top" -c "LAYER SET top"
    -c "PLINE3D 0,0,11 10,0,11 10,10,11 0,10,11 CLOSE"
    -c "SURFACE FROM LAYERS base ONLY NAME base"
    -c "SURFACE FROM LAYERS top ONLY NAME top")

# The whole slab, and inside the square [2, 6]^2 typed corner by corner:
# 4 x 4 x 1 = 16.
add_test(NAME cli.volume_between_two_surfaces_is_the_slab_whole_and_inside_a_square
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_volume_surfaces}
            -c "VOLUME SURFACES base top"
            -c "VOLUME SURFACES base top INSIDE 2,2 6,2 6,6 2,6")
set_tests_properties(cli.volume_between_two_surfaces_is_the_slab_whole_and_inside_a_square
    PROPERTIES
    PASS_REGULAR_EXPRESSION "volume method=exact base=base top=top cut=0\\.000 fill=100\\.000 net=100\\.000 area=100\\.000.*polygon\\.area=16\\.000 cut=0\\.000 fill=16\\.000"
    FAIL_REGULAR_EXPRESSION "error")

# Against the level 10.5 the base is all below it: 0.5 x 100 = 50 of fill.
add_test(NAME cli.volume_to_a_level_reads_what_lies_below_it_as_fill
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_volume_surfaces}
            -c "VOLUME DATUM base 10.5")
set_tests_properties(cli.volume_to_a_level_reads_what_lies_below_it_as_fill PROPERTIES
    PASS_REGULAR_EXPRESSION "volume method=exact surface=base level=10\\.5 cut=0\\.000 fill=50\\.000 net=50\\.000 area=100\\.000 above=0\\.000 below=50\\.000"
    FAIL_REGULAR_EXPRESSION "error")

# Sections every 5 m along y = 5, 5 m each side: three sections of 10 m^2,
# and 10 x 10 = 100 by end areas.
add_test(NAME cli.volume_sections_along_an_alignment_give_the_end_area_total
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_volume_surfaces}
            -c "ALIGN NEW cl 0,5 10,5"
            -c "VOLUME SECTIONS base top ALIGNMENT cl interval=5 left=5 right=5")
set_tests_properties(cli.volume_sections_along_an_alignment_give_the_end_area_total PROPERTIES
    PASS_REGULAR_EXPRESSION "section station=5\\.000 cut\\.area=0\\.000 fill\\.area=10\\.000 width=10\\.000.*volume method=endarea base=base top=top alignment=cl .*sections=3 .*cut=0\\.000 fill=100\\.000"
    FAIL_REGULAR_EXPRESSION "error")

# The cut/fill map: the top less the base, 1 everywhere, kept as a surface
# the session then lists beside the two.
add_test(NAME cli.surface_difference_keeps_the_cut_fill_map_as_a_surface
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_volume_surfaces}
            -c "SURFACE DIFFERENCE base top"
            -c "SURFACE LIST")
set_tests_properties(cli.surface_difference_keeps_the_cut_fill_map_as_a_surface PROPERTIES
    PASS_REGULAR_EXPRESSION "surface name=top-base triangles=[0-9]+ points=[0-9]+ bounds=0\\.000,0\\.000,10\\.000,10\\.000 zmin=1\\.000 zmax=1\\.000 .*style=\"Cut Fill\".*listed surfaces=3"
    FAIL_REGULAR_EXPRESSION "error")

# A surface no one made is refused, and the session fails.
add_test(NAME cli.volume_of_a_surface_that_is_not_there_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "VOLUME SURFACES ground design")
set_tests_properties(cli.volume_of_a_surface_that_is_not_there_is_refused PROPERTIES
    WILL_FAIL TRUE)
