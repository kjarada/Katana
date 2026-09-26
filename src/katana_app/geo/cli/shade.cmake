# The RASTER SHADE verb through katana_cli (docs/terrain.md, "Shading").
# Included from src/katana_app/CMakeLists.txt when the interop module is built.
set(_shade_samples "${PROJECT_SOURCE_DIR}/samples/gis")

# samples/gis/terrain.asc: 120 x 90 cells, heights 24.892 to 38.819 (its
# least and greatest values, read from the text grid). Relief over hillshade
# spreads the terrain ramp over that range, its five stops at the quarters,
# and keeps the picture as a derived reference raster of the DEM's size,
# which SURFACE LIST then lists.
add_test(NAME cli.raster_shade_of_the_sample_terrain_adds_a_reference
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "RASTER SHADE FILE \"${_shade_samples}/terrain.asc\" style=relief+hillshade"
            -c "SURFACE LIST")
set_tests_properties(cli.raster_shade_of_the_sample_terrain_adds_a_reference PROPERTIES
    PASS_REGULAR_EXPRESSION "ramp name=terrain min=24\\.892 max=38\\.819 from=data.*output arg=output kind=raster target=reference id=1 name=terrain-relief-hillshade raster=120x90.*legend value=24\\.892 r=38 g=115 b=0.*legend value=38\\.819 r=255 g=255 b=255.*raster id=1 name=terrain-relief-hillshade kind=derived width=120 height=90"
    FAIL_REGULAR_EXPRESSION "error")
