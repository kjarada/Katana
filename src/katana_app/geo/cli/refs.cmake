# The reference layers managed and kept, through katana_cli (docs/interop.md,
# "Reference layers"). Included from src/katana_app/CMakeLists.txt when the
# interop module is built.
#
# By hand: terrain.asc is 120 x 90 cells; survey_scan.las 40 000 points
# (`pdal info`).
set(_refs_samples "${PROJECT_SOURCE_DIR}/samples/gis")
set(_refs_project "${CMAKE_CURRENT_BINARY_DIR}/cli refs.katana")

add_test(NAME cli.refs_cleanup COMMAND ${CMAKE_COMMAND} -E rm -rf "${_refs_project}")
set_tests_properties(cli.refs_cleanup PROPERTIES FIXTURES_SETUP cli_refs_clean)

# Saved with a raster at half opacity and a hidden cloud; opened by a second
# process, whose OPEN reads both again from their files, as they were shown.
add_test(NAME cli.refs_a_project_is_saved_with_its_reference_layers
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "RECT 0,0 10,5"
            -c "IMPORT \"${_refs_samples}/terrain.asc\""
            -c "IMPORT \"${_refs_samples}/survey_scan.las\""
            -c "REFS OPACITY terrain 0.5" -c "REFS HIDE 2" -c "SAVE \"${_refs_project}\"")
add_test(NAME cli.refs_an_opened_project_reads_its_reference_layers_again
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "OPEN \"${_refs_project}\"" -c "REFS")
set_tests_properties(cli.refs_a_project_is_saved_with_its_reference_layers PROPERTIES
    FIXTURES_REQUIRED cli_refs_clean FIXTURES_SETUP cli_refs_saved
    FAIL_REGULAR_EXPRESSION "error")
set_tests_properties(cli.refs_an_opened_project_reads_its_reference_layers_again PROPERTIES
    FIXTURES_REQUIRED cli_refs_saved
    PASS_REGULAR_EXPRESSION "restored layers=2 missing=0.*reference id=1 kind=raster name=terrain width=120 height=90 [^\r\n]*visible=yes opacity=0\\.5 [^\r\n]*[\r\n]+reference id=2 kind=pointcloud name=survey_scan points=40000 [^\r\n]*visible=no [^\r\n]*[\r\n]+references rasters=1 clouds=1 missing=0"
    FAIL_REGULAR_EXPRESSION "error")

# Overviews are written beside the raster's file, so they are built only when
# the line says CONFIRM; without it the line is refused and says what it
# would have written.
add_test(NAME cli.refs_overviews_are_refused_without_confirm
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "IMPORT \"${_refs_samples}/terrain.asc\""
            -c "REFS OVERVIEWS 1")
set_tests_properties(cli.refs_overviews_are_refused_without_confirm PROPERTIES
    PASS_REGULAR_EXPRESSION "error: Unsupported: REFS OVERVIEWS writes [^\r\n]*/terrain\\.asc\\.ovr beside the raster's file; add CONFIRM")
