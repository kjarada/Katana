# GIS CHECK, GIS REPAIR and GIS COVERAGE through katana_cli
# (docs/geoprocessing.md, "V4"): the session's own executor, end to end.
# Included from src/katana_app/CMakeLists.txt when the interop module is built.

# (30,0) (40,10) (40,0) (30,10): its edges cross at (35,5), by hand.
add_test(NAME cli.gis_check_finds_the_bow_ties_crossing_at_35_5
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "PLINE 30,0 40,10 40,0 30,10 CLOSE"
            -c "GIS CHECK DRAWING")
set_tests_properties(cli.gis_check_finds_the_bow_ties_crossing_at_35_5 PROPERTIES
    PASS_REGULAR_EXPRESSION "problem kind=self-intersection entity=1 at=35,5 reason=Self-intersection.*check features=1 problems=1 entities=1"
    FAIL_REGULAR_EXPRESSION "error")

# Repaired, the bow-tie is its two triangles of base 10 and height 5: 25 m2
# each, the first keeping entity 1's id.
add_test(NAME cli.gis_repair_makes_the_bow_tie_two_triangles_of_25_square_metres
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "PLINE 30,0 40,10 40,0 30,10 CLOSE"
            -c "GIS REPAIR DRAWING" -c "LIST")
set_tests_properties(cli.gis_repair_makes_the_bow_tie_two_triangles_of_25_square_metres PROPERTIES
    PASS_REGULAR_EXPRESSION "split entity=1 parts=2 kept=1 created=2.*output arg=output kind=vector target=in-place created=1 updated=1.*1  Polyline  layer=0  vertices=3  closed  length=[0-9.]+  area=25.*2  Polyline  layer=0  vertices=3  closed  length=[0-9.]+  area=25"
    FAIL_REGULAR_EXPRESSION "error")

# A clean moves boundaries: without REPLACE it is refused, and the session
# exits with a failure.
add_test(NAME cli.gis_coverage_clean_without_replace_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "RECT 0,0 50,40" -c "GIS COVERAGE CLEAN DRAWING gap=0.05")
set_tests_properties(cli.gis_coverage_clean_without_replace_is_refused PROPERTIES WILL_FAIL TRUE)
