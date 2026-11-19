# SURVEY EXPORT through katana_cli (docs/survey.md, "SURVEY EXPORT"): the
# drawing's survey points written to a delimited point file by a line, as
# Survey > Export Survey Points writes them. Included from
# tests/cli/CMakeLists.txt.
#
# By hand: FORWARD from 334000,6249999 (Z 20.125) due north 1 m, level, is
# point 101 at 334000,6250000, Z 20.125; from 334010.5,6250009.25 (no height)
# it is point 102 at 334010.5,6250010.25 with no height. The file is then read
# by GDAL's CSV driver - another reader - told which columns are the
# coordinates, and the two points come back where they were.
set(_survey_io_out "${CMAKE_CURRENT_BINARY_DIR}/cli survey io")

add_test(NAME cli.survey_io_folder
    COMMAND ${CMAKE_COMMAND} -E make_directory "${_survey_io_out}")
set_tests_properties(cli.survey_io_folder PROPERTIES FIXTURES_SETUP cli_survey_io)

add_test(NAME cli.survey_export_writes_the_drawings_survey_points_and_another_reader_reads_them
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            ${KATANA_PROGRAM_katana_cli}
            -c "FORWARD 334000,6249999,20.125 0 1 0 101"
            -c "FORWARD 334010.5,6250009.25 0 1 - 102"
            -c "SURVEY EXPORT \"${_survey_io_out}/points.csv\""
            -c "NEW"
            -c "IMPORT \"${_survey_io_out}/points.csv\" oo=X_POSSIBLE_NAMES=Easting oo=Y_POSSIBLE_NAMES=Northing"
            -c "LIST")
set_tests_properties(cli.survey_export_writes_the_drawings_survey_points_and_another_reader_reads_them PROPERTIES
    FIXTURES_REQUIRED cli_survey_io
    PASS_REGULAR_EXPRESSION "exported file=\"[^\"]*/points\\.csv\" kind=survey-points points=2 layout=\"P,E,N,Z,C,D;delimiter=comma;header=1;quote=double\" decimals=3 unit=metres without_elevation=1 not_points=0 .*imported file=\"[^\"]*/points\\.csv\" kind=vector entities=2 .*Point  layer=points  at 334000,6250000[\r\n]+[^\r\n]*Point  layer=points  at 334010\\.5,6250010\\.25"
    FAIL_REGULAR_EXPRESSION "error")

# A scope that takes nothing is said, and no file is written: the next line
# refuses the file that is not there.
add_test(NAME cli.survey_export_of_a_scope_that_takes_nothing_says_so_and_writes_nothing
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            ${KATANA_PROGRAM_katana_cli}
            -c "FORWARD 0,-1,5 0 1 0 1"
            -c "SURVEY EXPORT \"${_survey_io_out}/nothing.csv\" AREA 100,100,200,200"
            -c "INFO \"${_survey_io_out}/nothing.csv\"")
set_tests_properties(cli.survey_export_of_a_scope_that_takes_nothing_says_so_and_writes_nothing PROPERTIES
    FIXTURES_REQUIRED cli_survey_io
    PASS_REGULAR_EXPRESSION "export file=\"[^\"]*/nothing\\.csv\" kind=survey-points ran=no reason=\"the scope took nothing\"[\r\n]+scope scope=area area=100,100,200,200 matched=0[\r\n]+error: NotFound")

# A drawing with no survey points is refused, the line failing the run.
add_test(NAME cli.survey_export_of_a_drawing_with_no_survey_points_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            ${KATANA_PROGRAM_katana_cli}
            -c "POINT 1,2"
            -c "SURVEY EXPORT \"${_survey_io_out}/none.csv\"")
set_tests_properties(cli.survey_export_of_a_drawing_with_no_survey_points_is_refused PROPERTIES
    FIXTURES_REQUIRED cli_survey_io
    PASS_REGULAR_EXPRESSION "error: InvalidState: the drawing has no survey points")
