# Headless command-line front end. Proves the engine is complete without a GUI.
add_executable(katana_cli main.cpp)
katana_target_defaults(katana_cli)
target_link_libraries(katana_cli PRIVATE katana_cad katana_archive12d)

# Interoperability is optional: with -DKATANA_BUILD_IO=OFF the CLI still builds
# and simply has no IMPORT/EXPORT verbs, which is what the sanitizer job needs.
if(TARGET katana_interop)
    target_link_libraries(katana_cli PRIVATE katana_interop)
    target_compile_definitions(katana_cli PRIVATE KATANA_WITH_INTEROP=1)
endif()

if(KATANA_BUILD_TESTS)
    # End-to-end: draw, save, reopen in a second process, and query.
    set(_project "${CMAKE_CURRENT_BINARY_DIR}/cli-smoke.katana")
    add_test(NAME cli.save
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli>
                -c "LAYER NEW Survey #FF0000" -c "LAYER SET Survey"
                -c "RECT 0,0 30,20" -c "CIRCLE 15,10 5" -c "SAVE \"${_project}\"")
    add_test(NAME cli.reopen
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "OPEN \"${_project}\"" -c "LIST")
    add_test(NAME cli.rejects_bad_input
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "CIRCLE 0,0 -1")
    add_test(NAME cli.cleanup COMMAND ${CMAKE_COMMAND} -E rm -rf "${_project}")

    set_tests_properties(cli.cleanup PROPERTIES FIXTURES_SETUP cli_project)
    set_tests_properties(cli.save PROPERTIES FIXTURES_REQUIRED cli_project FIXTURES_SETUP cli_saved)
    set_tests_properties(cli.reopen PROPERTIES
        FIXTURES_REQUIRED cli_saved
        PASS_REGULAR_EXPRESSION "2 entities.*Polyline.*layer=Survey.*area=600")
    set_tests_properties(cli.rejects_bad_input PROPERTIES WILL_FAIL TRUE)
endif()

# INFO, COPC and IMPORT ... LOCAL, on the sample GIS data. Expected values are
# the files' own: terrain.asc's header says 120 x 90 cells of 1.5 from a
# lower-left corner at (-5, -5), so it spans to (175, 130); `pdal info`
# counts 40 000 points in survey_scan.las.
if(KATANA_BUILD_TESTS AND TARGET katana_interop)
    set(_gis "${PROJECT_SOURCE_DIR}/samples/gis")
    set(_copc "${CMAKE_CURRENT_BINARY_DIR}/cli-scan.copc.laz")
    add_test(NAME cli.info_raster
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "INFO ${_gis}/terrain.asc")
    add_test(NAME cli.info_point_cloud
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "INFO ${_gis}/survey_scan.las")
    # Converted, then described: the description must now say COPC, with
    # every point kept.
    add_test(NAME cli.copc_cleanup COMMAND ${CMAKE_COMMAND} -E rm -f "${_copc}")
    add_test(NAME cli.copc_then_info
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "COPC ${_gis}/survey_scan.las ${_copc}"
                -c "INFO ${_copc}")
    # Audit QT-13/14: the raster's LOCAL is refused by name, where it used to
    # be left on the path and reported as a missing file "terrain.asc LOCAL".
    add_test(NAME cli.local_is_refused_for_reference_data
        COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
                $<TARGET_FILE:katana_cli> -c "IMPORT ${_gis}/terrain.asc LOCAL")
    set_tests_properties(cli.info_raster PROPERTIES PASS_REGULAR_EXPRESSION
        "Raster: 120 x 90 pixels, 1 band.*Pixel size: 1\\.5 x 1\\.5.*Bounds: \\(-5\\.000, -5\\.000\\) to \\(175\\.000, 130\\.000\\)")
    set_tests_properties(cli.info_point_cloud PROPERTIES PASS_REGULAR_EXPRESSION
        "Point cloud: 40,000 points.*COPC: no")
    set_tests_properties(cli.copc_cleanup PROPERTIES FIXTURES_SETUP cli_copc)
    set_tests_properties(cli.copc_then_info PROPERTIES FIXTURES_REQUIRED cli_copc
        PASS_REGULAR_EXPRESSION "converted survey_scan\\.las.*Point cloud: 40,000 points.*COPC: yes")
    set_tests_properties(cli.local_is_refused_for_reference_data PROPERTIES
        PASS_REGULAR_EXPRESSION "LOCAL is not supported for rasters and point clouds")
endif()
