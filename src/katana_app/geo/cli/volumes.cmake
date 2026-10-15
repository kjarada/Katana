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

# A north-going alignment up the middle of the same slab, x = 5: its first and
# last sections lie ON the slab's south and north edges. The tilt of cos(pi/2)
# in a section's ends once left half of the first out of the sum, so the slab
# measured short; whole, every section is 10 m wide and 10 m^2 and the slab is
# 10 x 10 x 1 = 100.
add_test(NAME cli.volume_sections_northwards_over_the_slab_are_whole_at_its_edges
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_volume_surfaces}
            -c "ALIGN NEW north 5,0 5,10"
            -c "VOLUME SECTIONS base top ALIGNMENT north interval=5")
set_tests_properties(cli.volume_sections_northwards_over_the_slab_are_whole_at_its_edges PROPERTIES
    PASS_REGULAR_EXPRESSION "section station=0\.000 key=yes cut\.area=0\.000 fill\.area=10\.000 width=10\.000.*volume method=endarea base=base top=top alignment=north .*sections=3 sections\.empty=0 cut=0\.000 fill=100\.000"
    FAIL_REGULAR_EXPRESSION "error")

# Sections round a curve: a 1 m slab over [0, 200]^2 (40 000 m^3), the alignment
# 20,20 - 180,20 - 180,180 with a curve of radius 40 at the corner, 240 + 20 pi
# = 302.832 m long. With the reach across all the common ground the sections
# cross one another and the old sum was 62 101 m^3; that is refused, naming the
# radius; 15 m either side is a band 30 wide, 30 x 302.832 = 9084.956.
set(_volume_curve
    -c "LAYER NEW base" -c "LAYER SET base"
    -c "PLINE3D 0,0,10 200,0,10 200,200,10 0,200,10 CLOSE"
    -c "LAYER NEW top" -c "LAYER SET top"
    -c "PLINE3D 0,0,11 200,0,11 200,200,11 0,200,11 CLOSE"
    -c "SURFACE FROM LAYERS base ONLY NAME base"
    -c "SURFACE FROM LAYERS top ONLY NAME top"
    -c "ALIGN NEW cl 20,20 180,20 180,180"
    -c "ALIGN SET cl 1 40")
add_test(NAME cli.volume_sections_round_a_curve_give_the_band_when_the_reach_is_short_of_its_radius
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_volume_curve}
            -c "VOLUME SECTIONS base top ALIGNMENT cl interval=5 left=15 right=15")
set_tests_properties(cli.volume_sections_round_a_curve_give_the_band_when_the_reach_is_short_of_its_radius
    PROPERTIES
    PASS_REGULAR_EXPRESSION "volume method=endarea base=base top=top alignment=cl .*left=15\.000 right=15\.000 .*cut=0\.000 fill=9084\.956"
    FAIL_REGULAR_EXPRESSION "error")

# The default reach is refused on the same curve (the exit code is not read
# when a pass pattern is set, so the pattern is the refusal itself).
add_test(NAME cli.volume_sections_round_a_curve_with_the_default_reach_are_refused_not_counted_twice
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_volume_curve}
            -c "VOLUME SECTIONS base top ALIGNMENT cl interval=10")
set_tests_properties(cli.volume_sections_round_a_curve_with_the_default_reach_are_refused_not_counted_twice
    PROPERTIES
    PASS_REGULAR_EXPRESSION "error: InvalidArgument: the sections at stations [0-9.]+ and [0-9.]+ cross each other.*at most 40\.000 m")


# Two lots sharing the edge x = 5.5, which is a line of 1 m cell centres: the
# lots [2, 5.5] x [2, 8] and [5.5, 8] x [2, 8] hold 4 x 6 and 3 x 6 centres
# (24 + 18 = 42 when each is measured alone and summed, the 6 on the edge
# twice), and their union [2, 8]^2 holds 6 x 6 = 36 cells of a 1 m slab: 36 m^3.
add_test(NAME cli.volume_grid_over_lots_sharing_an_edge_counts_the_cells_on_it_once
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_volume_surfaces}
            -c "LAYER NEW lots" -c "LAYER SET lots"
            -c "PLINE 2,2 5.5,2 5.5,8 2,8 CLOSE"
            -c "PLINE 5.5,2 8,2 8,8 5.5,8 CLOSE"
            -c "VOLUME GRID base top cell=1 INSIDE FROM LAYERS lots")
set_tests_properties(cli.volume_grid_over_lots_sharing_an_edge_counts_the_cells_on_it_once
    PROPERTIES
    PASS_REGULAR_EXPRESSION "volume method=grid base=base top=top cell=1 polygons=2 polygon\\.area=36\\.000 cut=0\\.000 fill=36\\.000 net=36\\.000 area=36\\.000 cells=36"
    FAIL_REGULAR_EXPRESSION "error")

# The base slab is flat at 10: against the level 10, inside a skewed
# quadrilateral of 89.6635 m^2 (the shoelace sum), nothing is above or below it,
# and no area is either. The overlay reads the surface and the polygon's flat
# level by weights that sum to 1 to a few ulps, so the differences at the
# corners of its pieces were +/- 1e-15, and 44.629 m^2 of the polygon were
# reported as cut area and 23.317 as fill area.
add_test(NAME cli.volume_datum_of_a_flat_surface_at_its_own_level_has_no_cut_area_or_fill_area
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> ${_volume_surfaces}
            -c "VOLUME DATUM base 10 INSIDE 0.123,0.456 9.876,0.321 9.5,9.9 0.2,9.7")
set_tests_properties(cli.volume_datum_of_a_flat_surface_at_its_own_level_has_no_cut_area_or_fill_area
    PROPERTIES
    PASS_REGULAR_EXPRESSION "volume method=exact surface=base level=10 polygons=1 polygon\\.area=89\\.664 cut=0\\.000 fill=0\\.000 net=0\\.000 area=89\\.664 above=0\\.000 below=0\\.000 cut\\.area=0\\.000 fill\\.area=0\\.000"
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
