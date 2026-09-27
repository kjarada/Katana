# EXPORT on the shared scope and filter, with its options, through katana_cli
# (docs/interop.md, "Export options"). Included from
# src/katana_app/CMakeLists.txt when the interop module is built.
#
# The drawing, by hand: two 10 x 10 squares on layer lots and one line on
# layer roads - three entities, two of them closed polylines.
set(_export_options_out "${CMAKE_CURRENT_BINARY_DIR}/cli export options")
file(MAKE_DIRECTORY "${_export_options_out}")

# The scope takes the two squares; the reply says so after the exported record.
add_test(NAME cli.export_of_a_layer_writes_only_what_the_scope_takes
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "LAYER NEW lots" -c "LAYER SET lots" -c "RECT 0,0 10,10" -c "RECT 20,0 30,10"
            -c "LAYER NEW roads" -c "LAYER SET roads" -c "LINE 0,20 30,20"
            -c "EXPORT \"${_export_options_out}/lots.gpkg\" LAYERS lots layername=parcels")
set_tests_properties(cli.export_of_a_layer_writes_only_what_the_scope_takes PROPERTIES
    PASS_REGULAR_EXPRESSION "exported file=\"[^\"]*/lots\\.gpkg\" kind=vector driver=GPKG features=2 skipped=0 layers=parcels [^\r\n]*[\r\n]+scope scope=layers layers=lots [^\r\n]*matched=2"
    FAIL_REGULAR_EXPRESSION "error")

# PREVIEW says what would be written and writes nothing: WHERE TYPE=line takes
# the one line.
add_test(NAME cli.export_preview_says_what_the_scope_takes_and_writes_nothing
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "RECT 0,0 10,10" -c "LINE 0,20 30,20"
            -c "EXPORT \"${_export_options_out}/never.gpkg\" DRAWING WHERE TYPE=line PREVIEW")
set_tests_properties(cli.export_preview_says_what_the_scope_takes_and_writes_nothing PROPERTIES
    PASS_REGULAR_EXPRESSION "export file=\"[^\"]*/never\\.gpkg\" kind=vector driver=GPKG preview=yes entities=1[\r\n]+scope scope=drawing where=\"?TYPE=line\"? matched=1"
    FAIL_REGULAR_EXPRESSION "error|exported ")

# A creation option is checked against the driver's own list before anything
# is written: GDAL would only warn and carry on without it.
add_test(NAME cli.export_with_an_unknown_layer_option_is_refused_by_name
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "RECT 0,0 10,10"
            -c "EXPORT \"${_export_options_out}/refused.gpkg\" lco=NO_SUCH_OPTION=1")
set_tests_properties(cli.export_with_an_unknown_layer_option_is_refused_by_name PROPERTIES
    PASS_REGULAR_EXPRESSION "error: InvalidArgument: GPKG has no layer creation option NO_SUCH_OPTION")

# split=layer then append: one file layer per drawing layer, and a second
# EXPORT adds its layer to the GeoPackage; INFO reads the three back.
add_test(NAME cli.export_split_and_append_build_one_geopackage
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli>
            -c "LAYER NEW lots" -c "LAYER SET lots" -c "RECT 0,0 10,10"
            -c "LAYER NEW roads" -c "LAYER SET roads" -c "LINE 0,20 30,20"
            -c "EXPORT \"${_export_options_out}/site.gpkg\" split=layer"
            -c "LAYER NEW pegs" -c "LAYER SET pegs" -c "POINT 5,30"
            -c "EXPORT \"${_export_options_out}/site.gpkg\" LAYERS pegs layername=pegs append"
            -c "INFO \"${_export_options_out}/site.gpkg\"")
set_tests_properties(cli.export_split_and_append_build_one_geopackage PROPERTIES
    PASS_REGULAR_EXPRESSION "layers=lots,roads.*layers=pegs [^\r\n]*append=yes.*layer name=(lots|roads|pegs) .*layer name=(lots|roads|pegs) .*layer name=(lots|roads|pegs) "
    FAIL_REGULAR_EXPRESSION "error")

# No scope word is the whole drawing; WHERE alone is the one scope parser's
# rule, MODIFY's - the selection, filtered - and DRAWING WHERE filters the
# drawing. Two lines, one of them selected: WHERE TYPE=line takes one, DRAWING
# WHERE TYPE=line two, and the scope record says which it took.
add_test(NAME cli.export_where_alone_filters_the_selection_and_drawing_where_the_drawing
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_cli> -c "LINE 0,0 10,0" -c "LINE 0,5 10,5" -c "SELECT NONE"
            -c "SELECT 1"
            -c "EXPORT \"${_export_options_out}/where.geojson\" WHERE TYPE=line PREVIEW"
            -c "EXPORT \"${_export_options_out}/where.geojson\" DRAWING WHERE TYPE=line PREVIEW")
set_tests_properties(cli.export_where_alone_filters_the_selection_and_drawing_where_the_drawing PROPERTIES
    PASS_REGULAR_EXPRESSION "preview=yes entities=1[\r\n]+scope scope=selection where=\"TYPE=line\" matched=1.*preview=yes entities=2[\r\n]+scope scope=drawing where=\"TYPE=line\" matched=2"
    FAIL_REGULAR_EXPRESSION "error")
