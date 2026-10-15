# The drawing's surfaces through katana_cli (docs/terrain.md, "Surfaces in the
# drawing"): kept in the project, styled, shown and hidden, and undone like any
# other edit. Included from src/katana_app/CMakeLists.txt when the interop
# module is built (SURFACE FROM reads the sample DEM through GDAL).
set(_surface_document_samples "${PROJECT_SOURCE_DIR}/samples/gis")
set(_surface_document_project "${CMAKE_CURRENT_BINARY_DIR}/cli-surfaces.katana")

# A surface made from the sample terrain (10 800 cells, every one a vertex:
# src/katana_app/geo/cli/surface.cmake works the numbers), styled, copied into
# a style of the project's own and hidden, then saved - and opened again in a
# second process, where both surfaces are listed in their styles and the
# copy still hidden, and the project's style has the interval it was given.
add_test(NAME cli.surfaces_cleanup
    COMMAND ${CMAKE_COMMAND} -E rm -rf "${_surface_document_project}")
add_test(NAME cli.a_surface_made_and_styled_is_saved_with_the_project
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_document_samples}/terrain.asc\" NAME ground"
            -c "SURFACE STYLE ground Contours"
            -c "SURFACESTYLE NEW Fine FROM Contours interval=0.5"
            -c "SURFACE COPY ground fine"
            -c "SURFACE STYLE fine Fine"
            -c "SURFACE HIDE fine"
            -c "SAVE \"${_surface_document_project}\"")
add_test(NAME cli.a_saved_surface_is_listed_again_with_its_style_when_the_project_opens
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "OPEN \"${_surface_document_project}\""
            -c "SURFACE LIST"
            -c "SURFACESTYLE INFO Fine")
set_tests_properties(cli.surfaces_cleanup PROPERTIES FIXTURES_SETUP cli_surfaces_clean)
set_tests_properties(cli.a_surface_made_and_styled_is_saved_with_the_project PROPERTIES
    FIXTURES_REQUIRED cli_surfaces_clean FIXTURES_SETUP cli_surfaces_saved
    FAIL_REGULAR_EXPRESSION "error")
set_tests_properties(cli.a_saved_surface_is_listed_again_with_its_style_when_the_project_opens PROPERTIES
    FIXTURES_REQUIRED cli_surfaces_saved
    PASS_REGULAR_EXPRESSION "surface name=ground triangles=[0-9]+ points=10800 [^\r\n]* style=Contours visible=yes[\r\n]+surface name=fine triangles=[0-9]+ points=10800 [^\r\n]* style=Fine visible=no.*listed surfaces=2 .*style name=Fine default=no surfaces=1 .*options [^\r\n]*interval=0\\.5 "
    FAIL_REGULAR_EXPRESSION "error")

# SURFACE FROM is one undo step: UNDO and the drawing has none.
add_test(NAME cli.surface_from_then_undo_leaves_no_surface
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_document_samples}/terrain.asc\" NAME ground"
            -c "UNDO"
            -c "SURFACE LIST")
set_tests_properties(cli.surface_from_then_undo_leaves_no_surface PROPERTIES
    PASS_REGULAR_EXPRESSION "1 undone.*listed surfaces=0 rasters=0"
    FAIL_REGULAR_EXPRESSION "error")

# The default style is not deleted: refused, and the session exits with a
# failure.
add_test(NAME cli.the_default_surface_style_is_not_deleted
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "SURFACESTYLE DELETE Standard")
set_tests_properties(cli.the_default_surface_style_is_not_deleted PROPERTIES WILL_FAIL TRUE)

# EXPORT to IFC writes each of the drawing's surfaces once: two surfaces, two
# terrain elements. Two tasks each wrote the export's loop over the drawing's
# surfaces and the merge kept both, so every surface went out twice and the
# reply said surfaces=4 here.
set(_surfaces_once_ifc "${CMAKE_CURRENT_BINARY_DIR}/cli-surfaces-once.ifc")
add_test(NAME cli.ifc_export_of_surfaces_cleanup
    COMMAND ${CMAKE_COMMAND} -E rm -f "${_surfaces_once_ifc}")
add_test(NAME cli.ifc_export_writes_each_surface_of_the_drawing_once
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "SURFACE FROM FILE \"${_surface_document_samples}/terrain.asc\" NAME ground"
            -c "SURFACE COPY ground copy"
            -c "EXPORT \"${_surfaces_once_ifc}\"")
set_tests_properties(cli.ifc_export_of_surfaces_cleanup PROPERTIES
    FIXTURES_SETUP cli_surfaces_once_clean)
set_tests_properties(cli.ifc_export_writes_each_surface_of_the_drawing_once PROPERTIES
    FIXTURES_REQUIRED cli_surfaces_once_clean
    PASS_REGULAR_EXPRESSION "counts [^\r\n]* surfaces=2[\r\n].*class name=IfcGeographicElement count=2[\r\n]"
    FAIL_REGULAR_EXPRESSION "error")
