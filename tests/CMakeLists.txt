# One test executable per module keeps link dependencies honest: the math tests
# cannot accidentally rely on SQLite, the geometry tests cannot rely on Qt.
#
#   katana_add_test_suite(<module> LIBS <targets...> SOURCES <files...>)

function(katana_add_test_suite module)
    cmake_parse_arguments(ARG "" "" "LIBS;SOURCES" ${ARGN})
    set(_target katana_${module}_tests)
    add_executable(${_target} ${ARG_SOURCES})
    target_link_libraries(${_target} PRIVATE ${ARG_LIBS} GTest::gtest_main)
    target_include_directories(${_target} PRIVATE "${CMAKE_CURRENT_FUNCTION_LIST_DIR}")
    katana_target_defaults(${_target})
    # Beside the application, not among it: bin/ holds what ships.
    set_target_properties(${_target} PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${KATANA_TEST_OUTPUT_DIRECTORY}")
    katana_register_tests(${_target})
endfunction()

add_test(NAME layering
    COMMAND ${CMAKE_COMMAND} -DKATANA_ROOT=${PROJECT_SOURCE_DIR}
            -P ${PROJECT_SOURCE_DIR}/tools/check_layering.cmake)

# tests/<name>/ holds the suite of module katana_<name>; it is configured only
# when that module is.
file(GLOB _suites LIST_DIRECTORIES true "${CMAKE_CURRENT_SOURCE_DIR}/*")
foreach(_suite IN LISTS _suites)
    get_filename_component(_name "${_suite}" NAME)
    if(EXISTS "${_suite}/CMakeLists.txt" AND TARGET katana_${_name})
        add_subdirectory("${_suite}")
    endif()
endforeach()

# A plot produced headlessly, so the PDF writing inside the Qt widget is
# exercised by ctest and not only by hand (PLAN.MD Phase 22). The script
# copies the sample project first: a test must never touch repository data.
if(TARGET katana)
    add_test(NAME qt_plot_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/plot_headless.pdf"
            -P "${PROJECT_SOURCE_DIR}/tools/check_plot.cmake")
    set_tests_properties(qt_plot_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The whole main window, built and painted headlessly: theme, toolbars, every
    # icon, the viewport with the sample in it. See tools/check_screenshot.cmake.
    add_test(NAME qt_screenshot_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/screenshot_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_screenshot_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# The whole main window with a 12d customisation loaded into it: the two
# libraries and the mapfile read, resolved against each other, and reported.
# This is the path the File > Load 12d Customisation menu item takes, so what
# ctest exercises is what a person gets (PLAN.MD 20.3). It runs without the
# files when they are not in the checkout - they carry their author's licence
# notice - and then proves only that the switch does no harm.
if(TARGET katana)
    add_test(NAME qt_customisation_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_headless.png"
            "-DCUSTOMISE_DIR=${PROJECT_SOURCE_DIR}/docs/12d Refrence Files"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_customisation_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# A layer switched off through its box in the layer panel, headlessly. This
# crashed the application when the panel was rebuilt inside its own itemChanged
# signal; the check is in MainWindow::toggleLayerThroughPanel. BUILDING is a
# layer of the sample with entities on it.
if(TARGET katana)
    add_test(NAME qt_toggle_layer_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/toggle_layer_headless.png"
            "-DTOGGLE_LAYER=BUILDING"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_toggle_layer_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The same click after a 12d archive import that brought a surface, a
    # point cloud and a split layout: the plan viewport is replaced, and its
    # document listener must go with it. This crashed 5 runs in 6 before
    # Document::ListenerHandle; a run that survives by luck still passes,
    # which is why the unit tests in test_cad.cpp carry the proof.
    add_test(NAME qt_import_12da_then_toggle_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/all_geometries.12daz"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_toggle_headless.png"
            "-DTOGGLE_LAYER=Cadastre"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_import_12da_then_toggle_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # Point symbols painted from their styles (PLAN.MD 20.2 slice 2), into a
    # directory that is NOT a project: "Open Failed" was a modal box, which in
    # a headless run is a hang the TIMEOUT below catches.
    add_test(NAME qt_import_symbols_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/symbols.12da"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_symbols_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_import_symbols_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The styles and linetypes manager (PLAN.MD 20.2 slice 3), opened over a
    # 12d import so that its tables hold real linestyles and symbols. What it
    # protects is that the dialog builds, lays out and PAINTS - its tables,
    # its form, its two previews - which is otherwise only ever checked by a
    # person opening the menu.
    add_test(NAME qt_style_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/symbols.12da"
            "-DSTYLE_MANAGER=ON"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/style_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_style_manager_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The attribute manager (PLAN.MD 20.2 slice 5) over a 12d import, on the
    # first entity - a permanent mark whose attributes came from 12d as two
    # groups. It builds the tree from the flattened keys and paints it.
    # No project, so entity 1 IS the first imported string rather than the
    # first entity of the sample, which has no 12d attributes at all.
    add_test(NAME qt_attribute_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/all_geometries.12da"
            "-DATTRIBUTES=1"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/attribute_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_attribute_manager_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # ---- the GIS menu: GDAL and PDAL through the QActions a person clicks ----
    #
    # Surface From Raster on the sample DEM. The expected range is the
    # minimum and maximum of the values in samples/gis/terrain.asc, read
    # straight from the text grid (10 800 values, no no-data), not from
    # Katana: 24.892 to 38.819. Every pixel is a TIN vertex at this size (under
    # the 400 000-point cap), and linear interpolation cannot leave the range
    # of its vertices, so the surface's range is exactly the grid's. The old
    # command rebuilt heights from 8-bit display greys between two numbers it
    # asked for - a question that hangs a headless run (audit QT-23).
    set(_gis "${PROJECT_SOURCE_DIR}/samples/gis")
    set(_qt_env "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen")
    add_test(NAME qt_surface_from_raster_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${_gis}/terrain.asc"
            "-DACTIONS=surfaceFromRaster"
            "-DEXPECT=elevation 24\\.892 to 38\\.819"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/surface_from_raster_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Surface From Point Cloud on the sample scan, which is classified: `pdal
    # info --stats --enumerate Classification` lists classes 2, 3, 5 and 6,
    # and filters.range on Classification[2:2] keeps 29 512 of its 40 000
    # points. The surface must be built from those ground returns and say so
    # (audit QT-10) - any grouping separator, since the log is localised.
    add_test(NAME qt_surface_from_cloud_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${_gis}/survey_scan.las"
            "-DACTIONS=surfaceFromPointCloud"
            "-DEXPECT=Using the 29[^0-9]?512 ground points \\(ASPRS class 2\\); 10[^0-9]?488 other"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/surface_from_cloud_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # File > New takes the imported raster with the drawing (audit QT-17):
    # afterwards there is no raster for Surface From Raster to use. Before
    # the fix the orthophoto of the discarded drawing stayed behind the new
    # one, and this built a surface from it.
    add_test(NAME qt_new_drops_reference_data_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${_gis}/terrain.asc"
            "-DACTIONS=fileNew,surfaceFromRaster"
            "-DEXPECT=No raster is loaded"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/new_drops_reference_data_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # GIS > Dataset Information and the import dialog, described by GDAL and
    # PDAL from the file and painted.
    add_test(NAME qt_dataset_info_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDATASET_INFO=${_gis}/terrain.asc"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/dataset_info_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_import_options_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT_OPTIONS=${_gis}/survey_scan.las"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_options_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_surface_from_raster_headless qt_surface_from_cloud_headless
                         qt_new_drops_reference_data_headless
                         qt_dataset_info_headless qt_import_options_headless
        PROPERTIES ENVIRONMENT_MODIFICATION "${_qt_env}" TIMEOUT 180)

    # The layer manager (PLAN.MD 20.2 slice 6) over the sample project, whose
    # layers have colours, linetypes and entities to count.
    add_test(NAME qt_layer_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DLAYER_MANAGER=ON"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/layer_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_layer_manager_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # ---- the Survey menu (PLAN.MD 45 slice 9), through the actions a person clicks --
    #
    # Each dialog is opened by its action, filled and pressed by the object
    # names of survey/survey_dialogs.hpp, and grabbed; what it reported is read
    # from the log. The numbers are worked by hand, not taken from a run:
    #
    # Inverse (1000,5000) -> (1003,5004): a 3-4-5 triangle, 5.000 at azimuth
    # atan2(3, 4) = 36.8698976 degrees = 36 deg 52' 11.63" (0.8698976 * 60 =
    # 52.19386', 0.19386 * 60 = 11.63").
    add_test(NAME qt_survey_inverse_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyInverse"
            "-DFILL=from=1000,5000|to=1003,5004"
            "-DPRESS=compute"
            "-DEXPECT=Horizontal distance 5\\.000.*Azimuth 36[^0-9]+52'11\\.63"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_inverse_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Area of Selection over samples/gis/parcels.geojson, everything selected.
    # Its outlines by the shoelace formula, from the file's coordinates:
    #   42A (185,0) (300,0) (310,70) (200,95):
    #     0 + 21000 + (29450 - 14000) + (0 - 17575) = 18875 -> 9437.5 m^2
    #   42B (200,95) (310,70) (320,150) (215,165):
    #     (14000 - 29450) + (46500 - 22400) + (52800 - 32250) + (20425 - 33000)
    #     = 16625 -> 8312.5 m^2
    #   the spoil heaps, 30 x 30 and 30 x 25 rectangles: 900 and 750 m^2
    # 19 400 m^2 in all = 1.9400 ha (1 ha = 10 000 m^2). The two roads and
    # the three control points have no area and are skipped, and said to be.
    add_test(NAME qt_survey_area_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/samples/gis/parcels.geojson"
            "-DSELECT_ALL=ON"
            "-DACTIONS=surveyArea"
            "-DEXPECT=Total  area 19400\\.000 m[^ ]* \\(1\\.9400 ha\\).*Skipped 3: a point has no area"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_area_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Forward along the same 3-4-5 direction, 5 from (1000,5000,20) with a
    # height difference of -1: the far corner (1003,5004) at 20 - 1 = 19,
    # added as a point named P7 on the current layer.
    add_test(NAME qt_survey_forward_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyForward"
            "-DFILL=from=1000,5000,20|direction=N 36d52m11.63s E|distance=5|heightDifference=-1|name=P7"
            "-DPRESS=addPoint"
            "-DEXPECT=New point P7  E 1003\\.000 N 5004\\.000 Z 19\\.000.*Created on layer"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_forward_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A right angle is 100 gon: 400 gon to the circle, by definition.
    add_test(NAME qt_survey_angle_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyAngleCalculator"
            "-DFILL=angle=90|unit=Degrees"
            "-DPRESS=convert"
            "-DEXPECT=100\\.00000000 gon"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_angle_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A 200 x 100 rectangle walked clockwise from A(1000,5000): north 100,
    # east 200, south 100, west 200, turning 270 degrees (clockwise, back to
    # forward) at every corner - with BC booked 200.100. The angles close;
    # the figure overshoots 0.100 east: precision 600.1 / 0.1 = 1 : 6001.
    # Bowditch moves C by -0.100 * (100 + 200.1) / 600.1 = -0.050 in E:
    # 1200.100 - 0.050 = 1200.050. Added: B, C and D (A is the control it
    # hangs on) and the traverse line - 4 entities.
    add_test(NAME qt_survey_traverse_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyTraverse"
            "-DFILL=name=Block|start=1000,5000|startMark=1000,5100|legs=A 270 100\\nB 270 200.1\\nC 270 100\\nD 270 200"
            "-DPRESS=addToDrawing"
            "-DEXPECT=precision 1 : 6001.*C +1200\\.100 +5100\\.000 +1200\\.050 +5100\\.000.*Added to layer 0: 4 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_traverse_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A level book reduced by hand (height of collimation): BM1 100.000 +
    # BS 1.500 = 101.500; CP1 101.500 - 0.500 = 101.000, + 2.250 = 103.250;
    # BM2 103.250 - 3.260 = 99.990 against its known 100.000: -0.010. The
    # check: sum BS 3.750 - sum FS 3.760 = -0.010. Allowable 12 mm x
    # sqrt(0.250 km) = 6 mm, which the 10 mm misclosure exceeds.
    add_test(NAME qt_survey_level_book_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyLevelBook"
            "-DFILL=startLevel=100|closingLevel=100|book=BM1 1.500 - -\\nA - 2.000 -\\nCP1 2.250 - 0.500 100\\nB - 1.250 -\\nBM2 - - 3.260 150|adjustment=By distance levelled"
            "-DPRESS=reduce"
            "-DEXPECT=sum BS 3\\.750 - sum FS 3\\.760 = -0\\.010.*Misclosure -0\\.0100 \\(computed - known\\); allowable 0\\.0060"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_level_book_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # UTM by definition (NGA.SIG.0012): zone 30's central meridian is 3 deg
    # W, the false easting 500 000 m, the false northing 0 in the north, the
    # scale on the central meridian 0.9996 - so (0, -3) is E 500 000, N 0.
    add_test(NAME qt_survey_converter_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyCoordinateConverter"
            "-DFILL=source=4326|target=32630|points=origin 0 -3"
            "-DPRESS=convert"
            "-DEXPECT=origin +0\\.000000000 -3\\.000000000 +-> +500000\\.000 0\\.000 +target k 0\\.99960000"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_converter_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_survey_inverse_headless qt_survey_area_headless
                         qt_survey_forward_headless qt_survey_angle_headless
                         qt_survey_traverse_headless qt_survey_level_book_headless
                         qt_survey_converter_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()
