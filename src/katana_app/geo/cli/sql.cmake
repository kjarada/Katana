# GIS SQL through katana_cli (docs/geoprocessing.md, "V5"): the session's own
# executor, end to end. Included from src/katana_app/CMakeLists.txt when the
# interop module is built.

# Two 50 x 40 lots: 2 of them, 2000 + 2000 = 4000 m2, by hand; a query
# changes nothing, so LIST still counts the two.
add_test(NAME cli.gis_sql_counts_and_sums_the_area_of_two_lots
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "RECT 0,0 50,40" -c "RECT 50,0 100,40"
            -c "GIS SQL \"SELECT COUNT(*) AS lots, SUM(ST_Area(geometry)) AS area FROM polygons\" DRAWING"
            -c "LIST")
set_tests_properties(cli.gis_sql_counts_and_sums_the_area_of_two_lots PROPERTIES
    PASS_REGULAR_EXPRESSION "column name=lots key=lots type=integer.*column name=area key=area type=real.*row lots=2 area=4000.*sql dialect=sqlite tables=polygons rows=1.*2 entities"
    FAIL_REGULAR_EXPRESSION "error")

# A query reads: DELETE is refused, and the session exits with a failure.
add_test(NAME cli.gis_sql_refuses_a_statement_that_is_not_a_select
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "RECT 0,0 50,40"
            -c "GIS SQL \"DELETE FROM polygons\" DRAWING")
set_tests_properties(cli.gis_sql_refuses_a_statement_that_is_not_a_select PROPERTIES WILL_FAIL TRUE)
