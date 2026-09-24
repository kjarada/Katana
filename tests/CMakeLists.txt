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

# The view widgets on their own, headlessly (tests/qt_widgets/). Not found by
# the loop above: the application has no katana_qt library, so the suite
# compiles the widgets' sources itself and is configured whenever the
# application is.
if(TARGET katana)
    add_subdirectory(qt_widgets)
endif()

# Every headless check in this file runs the application offscreen, with the
# runtime it was built against first on PATH and a TIMEOUT, whatever shell
# ctest is started from. In a Git Bash whose PATH puts another MinGW's bin
# first, the application loads the wrong C++ runtime and dies before main
# (0xc0000139, entry point not found); without the platform it opens real
# windows, or fails where there is no display; without the TIMEOUT a modal
# box nobody closes holds the run for ctest's default 1,500 seconds. A check
# added without its set_tests_properties passes only where the caller's own
# environment happens to be right, so this test names it. The scan is
# deferred to the end of the file, after the last set_tests_properties.
function(katana_name_headless_checks_without_their_environment)
    get_property(_tests DIRECTORY PROPERTY TESTS)
    set(_bare "")
    foreach(_test IN LISTS _tests)
        if(NOT _test MATCHES "_headless$")
            continue()
        endif()
        get_test_property(${_test} ENVIRONMENT_MODIFICATION _environment)
        get_test_property(${_test} TIMEOUT _timeout)
        if(NOT _environment MATCHES "(^|;)PATH=path_list_prepend:"
           OR NOT _environment MATCHES "(^|;)QT_QPA_PLATFORM=set:offscreen(;|$)"
           OR NOT _timeout)
            list(APPEND _bare ${_test})
        endif()
    endforeach()
    if(_bare)
        string(JOIN ", " _named ${_bare})
    else()
        set(_named "none")
    endif()
    add_test(NAME qt_every_headless_check_runs_offscreen_with_the_runtime_first_on_its_path
        COMMAND ${CMAKE_COMMAND} -E echo
            "Headless checks without the offscreen platform, the runtime on PATH or a timeout: ${_named}.")
    set_tests_properties(qt_every_headless_check_runs_offscreen_with_the_runtime_first_on_its_path
        PROPERTIES PASS_REGULAR_EXPRESSION "or a timeout: none[.]")
endfunction()
if(TARGET katana)
    cmake_language(DEFER CALL katana_name_headless_checks_without_their_environment)
endif()

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

# The whole main window with a customisation loaded into it: the two
# libraries and the survey code file read, resolved against each other, and
# reported. This is the path the Format > Load Customisation menu item takes, so what
# ctest exercises is what a person gets (PLAN.MD 20.3). It runs without the
# files when they are not in the checkout - they carry their author's licence
# notice - and then proves only that the switch does no harm.
if(TARGET katana)
    add_test(NAME qt_customisation_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_headless.png"
            "-DCUSTOMISE_DIR=${KATANA_CUSTOMISATION_DIR}"
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

    # The styles and linetypes manager (PLAN.MD 20.2 slice 3), opened over an
    # archive import so that its tables hold real linestyles and symbols. What it
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
    # The import wizard, the Point Manager, the export and the Point Report
    # (PLAN.MD 45 slices 10, 11, 13), driven page by page (DRIVE, see
    # tools/check_screenshot.cmake) on tests/surveyio/data. The fixture
    # survey_points_pnezd.csv has a header naming its columns and 12 points,
    # counted by hand: 1 to 12, point 4 without an elevation, point 9's
    # description "Hydrant, fire" quoted. Its detection is never Identified
    # (the delimited probe is weak by design), so every run chooses the format.
    set(_survey_data "${PROJECT_SOURCE_DIR}/tests/surveyio/data")
    set(_survey_format "format=Delimited text points (CSV, TXT)")
    # Import all 12, then the Point Manager lists them - 12 rows - and the
    # filter "CP" keeps the two points whose code is CP (1 and 11; no id or
    # description holds "cp").
    add_test(NAME qt_survey_import_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|!next|unit=metres|!next|!next|!import|#surveyPointManager|filter=CP"
            "-DEXPECT=Records read: 12.*Points to create: 12.*Imported 12 point\\(s\\) from survey_points_pnezd\\.csv.*SurveyPointsDock: 12 points, 2 shown"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_import_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A headerless file does not say which column is the northing: Next on the
    # layout page is refused until the order is confirmed, and then its 3
    # points import.
    add_test(NAME qt_survey_import_uncertain_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_headerless.txt|!next|${_survey_format}|!next|columns=P,N,E,Z|delimiter=whitespace|headerLines=0|!next|confirmOrder=on|!next|unit=metres|!next|!next|!import"
            "-DEXPECT=could not tell the order of the columns.*Imported 3 point\\(s\\) from survey_points_headerless\\.txt"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_import_uncertain_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The order corrected through the role boxes, one box at a time, as a
    # person does it. survey_points_headerless_penz.txt is
    # survey_points_headerless.txt with its 2nd and 3rd columns swapped by
    # hand, so it is P,E,N,Z; with no header the proposal cannot tell and puts
    # P,N,E,Z first. Box 2 goes to "ignored" (no northing column) and then to
    # easting (two easting columns) - both layouts surveyio refuses, which once
    # removed every role box, so the fill of role3 found no box - and box 3 to
    # northing. The points exported as P,N,E,Z with no header are then the
    # original file, text for text: the eastings are the 330 000s and the
    # northings the 6 250 000s.
    add_test(NAME qt_survey_import_role_boxes_swap_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_headerless_penz.txt|!next|${_survey_format}|!next|role2=ignored|role2=easting|role3=northing|confirmOrder=on|!next|unit=metres|!next|!next|!import|@surveyExport|file=${CMAKE_CURRENT_BINARY_DIR}/survey_role_swap.txt|columns=P,N,E,Z|delimiter=whitespace|header=off|decimals=3|!export"
            "-DCOMPARE=${_survey_data}/survey_points_headerless.txt|${CMAKE_CURRENT_BINARY_DIR}/survey_role_swap.txt"
            "-DEXPECT=Layout: P,E,N,Z.delimiter=whitespace.header=0.*Imported 3 point\\(s\\) from survey_points_headerless_penz\\.txt.*Exported 3 point\\(s\\) to survey_role_swap\\.txt"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_import_role_boxes_swap_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Round trip through files: export(import(fixture)) with the fixture's own
    # layout and 3 decimals is the fixture, text for text; that export read
    # back in (replacing the points of the same ids) and written again is the
    # fixture again.
    add_test(NAME qt_survey_export_round_trip_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|!next|unit=metres|!next|!next|!import|@surveyExport|file=${CMAKE_CURRENT_BINARY_DIR}/survey_round_trip_1.csv|columns=P,N,E,Z,C,D|decimals=3|!export|@surveyImport|file=${CMAKE_CURRENT_BINARY_DIR}/survey_round_trip_1.csv|!next|${_survey_format}|!next|!next|unit=metres|!next|existing=replace the drawing's point|!next|!import|@surveyExport|file=${CMAKE_CURRENT_BINARY_DIR}/survey_round_trip_2.csv|!export"
            "-DCOMPARE=${_survey_data}/survey_points_pnezd.csv|${CMAKE_CURRENT_BINARY_DIR}/survey_round_trip_1.csv|${CMAKE_CURRENT_BINARY_DIR}/survey_round_trip_2.csv"
            "-DEXPECT=Exported 12 point\\(s\\) to survey_round_trip_1\\.csv.*12 point\\(s\\) in the drawing are replaced.*Exported 12 point\\(s\\) to survey_round_trip_2\\.csv"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_export_round_trip_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The stated transformation: UTM zone 30N (EPSG:32630) to 30S
    # (EPSG:32730) is the same projection with a false northing of 10 000 000 m
    # (NGA.SIG.0012), so the export is the fixture with 10 000 000 added to
    # every northing by hand (survey_points_pnezd_utm30s.csv) and nothing
    # else changed - not the eastings, not the heights.
    add_test(NAME qt_survey_import_transform_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|!next|unit=metres|declared=32630|target=32730|!next|!next|!import|@surveyExport|file=${CMAKE_CURRENT_BINARY_DIR}/survey_transform.csv|columns=P,N,E,Z,C,D|decimals=3|!export"
            "-DCOMPARE=${_survey_data}/survey_points_pnezd_utm30s.csv|${CMAKE_CURRENT_BINARY_DIR}/survey_transform.csv"
            "-DEXPECT=Transformation: from EPSG:32630 to EPSG:32730.*Imported 12 point"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_import_transform_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The Point Report after the import lists all 12 (nothing is selected).
    # A layout template is saved, chosen and deleted in the same run, so the
    # settings are left as they were found.
    add_test(NAME qt_survey_point_report_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|templateName=ctest survey layout|!saveTemplate|template=ctest survey layout|!deleteTemplate|!next|unit=metres|!next|!next|!import|@surveyPointReport"
            "-DEXPECT=Saved the survey layout template \"ctest survey layout\": P,N,E,Z,C,D.delimiter=comma.header=1.quote=double.*Deleted the survey layout template.*Point Report: 12 point\\(s\\)"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_point_report_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A template saved or deleted in the import wizard reaches an Export
    # dialog that is already open. The Export dialog is opened FIRST, so its
    # list is filled before the save (a list filled only when the dialog was
    # built never offered it). The template is then chosen in it; after the
    # wizard deletes it, choosing it again must be refused - the run ends on
    # that refusal (REFUSED, see tools/check_screenshot.cmake), with the
    # settings as it found them. The Delete before the Save clears a
    # template an earlier, killed run may have left; when there is none it
    # only logs that there is none.
    add_test(NAME qt_survey_templates_reach_export_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyExport|@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|templateName=ctest shared layout|!deleteTemplate|!saveTemplate|@surveyExport|template=ctest shared layout|@surveyImport|!deleteTemplate|@surveyExport|template=ctest shared layout"
            "-DREFUSED=Saved the survey layout template \"ctest shared layout\".*Deleted the survey layout template \"ctest shared layout\".*--fill: template has no choice 'ctest shared layout'"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_templates_reach_export_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # ---- the window's wiring of the managers' foundations ----------------------
    #
    # Format > Load Customisation merges (audit QT-21), through the window's
    # CUSTOMISE line, which runs the menu item's applyCustomisation: the
    # hand-written fixture survey code file REPLACES the map, then the symbol fixture
    # alone is MERGED in - and the map must still be there. By hand, from the
    # fixtures' head comments: test_survey.mapfile has 11 rules, and
    # test_symbols.4d 4 definitions, all named TEST ..., which no other
    # library defines, so all 4 are added.
    set(_fixture "${PROJECT_SOURCE_DIR}/tests/archive12d/data/customisation")
    add_test(NAME qt_customisation_merges_and_keeps_the_map_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE REPLACE ${_fixture}/test_survey.mapfile|>CUSTOMISE ${_fixture}/test_symbols.4d"
            "-DEXPECT=test_symbols\\.4d: style library, 4 added, 0 replaced[^\r\n]*[\r\n]+Customisation now: [0-9,]+ definitions \\([0-9,]+ symbols\\) and 11 survey code rules"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_merge_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # A file named twice in one CUSTOMISE is read once, as katana_cli reads it
    # (cad::distinctCustomisationFiles): the second naming is the same file
    # by another spelling. By hand: the mapfile REPLACES the map, read once,
    # so the map holds its 11 rules (the fixture's head comment); read twice
    # it held 22.
    add_test(NAME qt_a_file_named_twice_in_one_load_is_read_once_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE REPLACE ${_fixture}/test_survey.mapfile ${_fixture}/../customisation/test_survey.mapfile"
            "-DEXPECT=test_survey\\.mapfile is named twice in this load; it is read once\\..*Customisation now: [0-9,]+ definitions \\([0-9,]+ symbols\\) and 11 survey code rules"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_named_twice_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # A typed SAVE that cannot save - an untitled drawing, no directory given
    # - leaves a drawing nobody touched unmodified, so a NEW after it asks
    # nothing and goes ahead. Writing the record before the failed save
    # marked it modified.
    add_test(NAME qt_a_save_that_cannot_go_ahead_leaves_the_drawing_unmodified_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>SAVE|>NEW"
            "-DEXPECT=the drawing has not been saved to a project yet.*new drawing"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/failed_save_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A headless run has nobody to answer "save the changes first?": a NEW
    # after an edit is refused and said, and the edit stays - LIST still
    # counts the one line - rather than waiting on a box no one can see.
    add_test(NAME qt_a_headless_new_after_an_edit_is_refused_rather_than_asked_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LINE 0,0 10,0|>NEW|>LIST"
            "-DEXPECT=Unsaved Changes: the drawing has unsaved changes.*1 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/headless_new_refused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # The project records its customisation (ProjectMetadata::customisation):
    # the site plan saved with the symbol fixture loaded, then opened by a
    # second run that never loads it, which must say which file is missing -
    # a warning in the log, the project opened all the same.
    set(_recorded "${CMAKE_CURRENT_BINARY_DIR}/customisation_recorded.katana")
    set(_resaved "${CMAKE_CURRENT_BINARY_DIR}/customisation_resaved.katana")
    add_test(NAME qt_customisation_recorded_cleanup
        COMMAND ${CMAKE_COMMAND} -E rm -rf "${_recorded}" "${_resaved}")
    add_test(NAME qt_a_project_records_its_customisation_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE ${_fixture}/test_symbols.4d|>LINE 0,0 10,0|>SAVE ${_recorded}"
            "-DEXPECT=saved to "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_record_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_opening_names_the_customisation_not_loaded_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>OPEN ${_recorded}"
            "-DEXPECT=opened .*Warning: this project was drawn with customisation files that are not loaded: \"test_symbols\\.4d\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_missing_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Saved where the file is missing, the project still records it: the
    # open warns, the save (to a copy, so the test above reads its own
    # project) writes the record, and opening the copy warns again. By hand:
    # this run never loads test_symbols.4d, so nothing it does can judge it.
    add_test(NAME qt_saving_where_a_file_is_missing_keeps_it_recorded_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>OPEN ${_recorded}|>SAVE ${_resaved}|>NEW|>OPEN ${_resaved}"
            "-DEXPECT=not loaded: \"test_symbols\\.4d\".*saved to .*new drawing.*opened .*not loaded: \"test_symbols\\.4d\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_resaved_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_customisation_recorded_cleanup PROPERTIES
        FIXTURES_SETUP qt_recorded_clean)
    set_tests_properties(qt_a_project_records_its_customisation_headless PROPERTIES
        FIXTURES_REQUIRED qt_recorded_clean FIXTURES_SETUP qt_recorded)
    set_tests_properties(qt_opening_names_the_customisation_not_loaded_headless
                         qt_saving_where_a_file_is_missing_keeps_it_recorded_headless
        PROPERTIES FIXTURES_REQUIRED qt_recorded)

    # The current style (decision D9) and the Properties panel's Style row,
    # driven through their widgets. By hand: Kerb made current, a line is
    # drawn in it (entity 1); ByLayer made current, two lines are drawn
    # ByLayer (2 and 3); everything selected, the row given Kerb and applied:
    # only 2 and 3 change, as ONE command - Kerb holds 3 entities, one UNDO
    # takes it back to 1, and REDO to 3 again.
    add_test(NAME qt_current_style_and_the_style_row_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>STYLE NEW Kerb|%PropertiesToolBar|CurrentStyleCombo=Kerb|>LINE 0,9 10,9|>STYLE USAGE Kerb|CurrentStyleCombo=ByLayer|>LINE 0,0 10,0|>LINE 0,5 10,5|>SELECT ALL|%PropertiesDock|PropertyStyle=Kerb|!PropertyStyleApply|>STYLE USAGE Kerb|>UNDO|>STYLE USAGE Kerb|>REDO|>STYLE USAGE Kerb"
            "-DEXPECT=New work is drawn in Kerb.*style Kerb: used by 1 entity, e.g. id=1.*New work is drawn in ByLayer.*Style Kerb given to 2 entities.*style Kerb: used by 3 entities.*style Kerb: used by 1 entity.*style Kerb: used by 3 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/style_row_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_customisation_merges_and_keeps_the_map_headless
                         qt_customisation_recorded_cleanup
                         qt_a_project_records_its_customisation_headless
                         qt_opening_names_the_customisation_not_loaded_headless
                         qt_current_style_and_the_style_row_headless
                         qt_a_file_named_twice_in_one_load_is_read_once_headless
                         qt_a_save_that_cannot_go_ahead_leaves_the_drawing_unmodified_headless
                         qt_a_headless_new_after_an_edit_is_refused_rather_than_asked_headless
                         qt_saving_where_a_file_is_missing_keeps_it_recorded_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # ---- the menus: Format's managers, Purge Unused, the tools by alias -------
    #
    # Each manager opened THROUGH ITS FORMAT MENU ACTION (@ is --dialog), over
    # the hand-written fixture customisation merged onto whatever is built
    # in; the driver says what opened, and a manager opened modally would say
    # "(modal)" after its title, where each EXPECT wants the line to end. What each
    # manager then shows is read back with ? (--report).
    #
    # Styles: two styles made on the command line, so the table lists them.
    add_test(NAME qt_format_menu_opens_the_style_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=>STYLE NEW Kerb|>STYLE NEW Water|@formatStyles|?styleTable"
            "-DEXPECT=--dialog formatStyles opened styleManagerDialog \"Styles and Linetypes\"[\r\n]+styleTable: Kerb \\|[^;]*; Water \\|"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_style_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The same manager's Linetypes page (managerTabs, filled as a click on
    # its tab does) narrowed to the fixture's group Test/Lines. By hand from
    # test_linestyles.4d's head comment: that group holds TEST Dashed Kerb, a
    # paper linestyle, and TEST Gate, a two-point one, and nothing else. The
    # Kerb chosen, the details pane explains its kind in the general words.
    add_test(NAME qt_the_style_manager_lists_a_library_group_s_linestyles_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=@formatStyles|managerTabs=Linetypes|linetypeGroup=Test/Lines|linetypeTable=TEST Dashed Kerb|?linetypeTable|?libraryDetails"
            "-DEXPECT=linetypeTable: TEST Dashed Kerb \\| Library \\| Test/Lines \\| paper \\|[^;\r\n]*; TEST Gate \\| Library \\| Test/Lines \\| two-point \\|[^;\r\n]*[\r\n]+libraryDetails: TEST Dashed Kerb Group: Test/Lines File: test_linestyles\\.4d Kind: Paper linestyle: millimetres on the plot, the same at every scale[^\r\n]*Library linestyles belong to the session's customisation and are read-only here\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_style_manager_linetypes_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Symbols, searched for TEST. By hand from the fixture's head comments:
    # its four definitions are all symbols by decision D3 - TEST Tree too,
    # though it is not `mode vertex`, because rule TR* draws it as one - and
    # PX* asks for TEST Missing Symbol, which nothing defines and the library
    # lists as missing. Sorted by name, case folded, the five come together.
    add_test(NAME qt_format_menu_opens_the_symbol_library_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=@formatSymbols|filterText=TEST|?symbolGrid"
            "-DEXPECT=--dialog formatSymbols opened symbolLibraryDialog \"Symbol Library\"[\r\n]+symbolGrid: [^\r\n]*TEST Missing Symbol ; TEST Survey Mark ; TEST Tree ; TEST U Turn ; TEST Valve"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_symbol_library_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The code manager explaining a fixture code. By hand from
    # test_survey.mapfile: WM01 is met by the prefix key WM* (rule #0 of the
    # fixture, map_data), which puts it on the layer TEST SERVICES (the
    # rule's model field).
    add_test(NAME qt_format_menu_opens_the_code_manager_and_explains_a_code_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=@formatSurveyCodes|testCode=WM01|?explainHeading|?explainFields"
            "-DEXPECT=--dialog formatSurveyCodes opened surveyCodeManagerDialog \"Survey Code Manager\"[\r\n]+explainHeading: WM01: matched - met by the prefix key WM\\*[^\r\n]*[\r\n]+explainFields: model \\| TEST SERVICES \\| #[0-9]+ WM\\* \\(map_data\\)"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_code_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # What Format says in the real window, in general words: every item of
    # the menu with its status tip (?formatMenu) - the window's own Load and
    # Replace Loaded Customisation among the managers' - the log of a
    # customisation loaded and of each manager opened, and none of it names
    # another program's formats (FORBID, in either case). The widget tests
    # walk the managers' own texts (qt_widgets.GeneralTexts.*).
    add_test(NAME qt_the_format_menu_and_the_customisation_log_name_no_other_program_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=?formatMenu|@formatStyles|@formatSymbols|@formatSurveyCodes"
            "-DEXPECT=test_survey\\.mapfile: survey code file, [0-9,]+ added[^\r\n]*[\r\n].*formatMenu: Format: Layers\\.\\.\\. \\[[^\r\n]*Styles and Linetypes\\.\\.\\. \\[[^\r\n]*library linestyles[^\r\n]*Load Customisation\\.\\.\\. \\[Load linestyle and symbol libraries \\(\\.4d\\) and survey code files \\(\\.mapfile\\)[^\r\n]*Replace Loaded Customisation\\.\\.\\. \\[[^\r\n]*[\r\n].*opened surveyCodeManagerDialog"
            "-DFORBID=12[dD]"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_texts_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # FORBID holds the application to what it SAYS, not to where the run
    # lives. Here the output - and so the empty project the log says it
    # could not open - is in a folder named as a worktree for the archive
    # work might be: the log prints that path (EXPECT), and the check passes.
    add_test(NAME qt_a_forbidden_word_in_the_run_s_own_paths_is_not_held_against_it_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=%CommandLineDock"
            "-DEXPECT=Open Failed[^\r\n]*worktree_12da_export"
            "-DFORBID=12[dD]"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/worktree_12da_export/forbid_paths_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Only the path is set aside: the rest of the line that holds it is still
    # read. Forbidding the words that line opens with fails the run, and the
    # failure names them (PASS_REGULAR_EXPRESSION, set below).
    add_test(NAME qt_a_forbidden_word_beside_a_path_in_the_log_still_fails_the_run_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=%CommandLineDock"
            "-DFORBID=Open Failed"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/worktree_12da_export/forbid_beside_a_path_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_a_forbidden_word_beside_a_path_in_the_log_still_fails_the_run_headless
        PROPERTIES PASS_REGULAR_EXPRESSION
            "the run reported /Open Failed/ \\(\"Open Failed\"\\), its own paths aside")
    # The symbol library putting a fixture symbol on imported points: the 12
    # points of survey_points_pnezd.csv (counted by hand, see the survey
    # import test) imported, all selected, TEST Survey Mark chosen in the
    # grid and Assign pressed - one new style of that name holds all 12.
    add_test(NAME qt_the_symbol_library_puts_a_fixture_symbol_on_imported_points_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|!next|unit=metres|!next|!next|!import|>SELECT ALL|@formatSymbols|symbolGrid=TEST Survey Mark|?detailName|!assignToPoints|>STYLE USAGE TEST Survey Mark"
            "-DEXPECT=Imported 12 point\\(s\\).*detailName: TEST Survey Mark[\r\n]+12 points now in style \"TEST Survey Mark\" \\(new\\), drawing \"TEST Survey Mark\"\\..*style TEST Survey Mark: used by 12 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/symbol_assign_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Format > Purge Unused (* is --trigger, the menu item in its turn). By
    # hand: Spare is worn by nothing; Kerb is current and worn by the line;
    # a new drawing's continuous and none are protected. So one style goes,
    # and one UNDO brings it back. The grab is the command line, showing it.
    add_test(NAME qt_purge_unused_deletes_what_nothing_uses_as_one_undo_step_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=>STYLE NEW Spare|>STYLE NEW Kerb|%PropertiesToolBar|CurrentStyleCombo=Kerb|>LINE 0,0 10,0|*formatPurge|>STYLE USAGE Spare|>UNDO|>STYLE USAGE Spare|%CommandLineDock"
            "-DEXPECT=Purge Unused: purged 1 style, 0 linetypes and 0 hatch patterns \\(one Undo restores them\\)\\. Styles: Spare\\.[\r\n]+! [^\r\n]*Spare[^\r\n]*[\r\n]+1 undone[\r\n]+style Spare: unused"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/purge_unused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Format > Layers opens the Layers dialog BESIDE the drawing, not modal
    # (the opening line would end "(modal)"), and it follows the drawing: a
    # layer made on the command line while it is open appears in its table
    # after the one reload the DocumentWatcher makes. By hand: a new drawing
    # has the one layer "0"; LAYER NEW survey adds "survey", after it by name.
    add_test(NAME qt_format_layers_opens_beside_the_drawing_and_follows_it_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatLayers|?layerTable|>LAYER NEW survey|?layerTable"
            "-DEXPECT=--dialog formatLayers opened formatLayersDialog \"Layers\"[\r\n]+layerTable: 0 \\|[^;\r\n]*[\r\n].*layerTable: 0 \\|[^;\r\n]* ; survey \\|"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_layers_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A tool started by its alias on the command line and answered there: by
    # hand, LINE, 0,0 and 10,0, then Enter - one segment, so ONE line, which
    # the tool reports ("1 line") before starting again for the next chain;
    # Enter at its first prompt ends it, so LIST reaches the interpreter and
    # finds the one entity. ">" alone is Enter on an empty command line. The
    # menus and toolbars show it: Line's action checked while it runs, and
    # Select again once it has ended.
    add_test(NAME qt_a_tool_started_by_its_alias_draws_from_typed_points_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LINE|?draw.line|?toolSelect|>0,0|>10,0|>|>|?draw.line|?toolSelect|>LIST"
            "-DEXPECT=draw.line: Line, checked[\r\n]+toolSelect: Select, unchecked[\r\n]+1 line[\r\n]+draw.line: Line, unchecked[\r\n]+toolSelect: Select, checked[\r\n]+1 entities[\r\n]+1  Line  layer=0  0,0 -> 10,0  length=10"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/tool_by_alias_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A tool that cannot start - the only view shows a section, so there is
    # no plan view to draw in - is left unchecked, with Select checked: the
    # click checks a tool's action before the start is refused, and a
    # refusal changes no tool, so nothing else would put the marks right.
    add_test(NAME qt_a_tool_refused_for_want_of_a_plan_view_is_left_unchecked_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=*viewShowsSection|*draw.line|?draw.line|?toolSelect"
            "-DEXPECT=there is no plan view to draw in[^\r\n]*[\r\n]+draw.line: Line, unchecked[\r\n]+toolSelect: Select, checked"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/refused_tool_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Quitting while the Survey Code Manager holds unapplied rule edits: they
    # live only in its buffer, so a headless QUIT, with nobody to ask, is
    # refused and said - as a quit with unsaved drawing changes is - rather
    # than the edits going with the window. By hand: the built-in map's 1,624
    # rules and a duplicate of rule 0 make 1625 in the buffer.
    add_test(NAME qt_quitting_with_unapplied_code_manager_edits_is_refused_and_said_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatSurveyCodes|!ruleDuplicate|?dirtyIndicator|>QUIT"
            "-DEXPECT=dirtyIndicator: Unapplied edits:[^\r\n]* 1625 rules[.][\r\n]+! Unapplied Edits: the Survey Code Manager has rule edits that are not on the drawing"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/quit_with_code_edits_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The tools a CAD user reaches for daily, in the menus the catalogue
    # makes (src/katana_qt/tools/tool_menus.cpp): a Tools menu made beside
    # the window's tool menus for the Inquiry and Select groups, Divide and
    # Measure in Draw, Lengthen, Reverse and Match Properties in Modify. Each
    # menu is opened for its screenshot and read back item by item.
    add_test(NAME qt_the_tools_menu_holds_the_inquiry_and_select_tools_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=%toolsMenu|?toolsMenu"
            "-DEXPECT=toolsMenu: Tools: Distance .*; Area .*; ID Point .*; Angle .*; List .*; Select Similar .*; Quick Select "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/tools_menu_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_the_draw_menu_offers_divide_and_measure_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=%drawMenu|?drawMenu"
            "-DEXPECT=drawMenu: Draw: Point .*; Arc ; Divide .*; Measure "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/draw_menu_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_the_modify_menu_offers_lengthen_reverse_and_match_properties_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=%modifyMenu|?modifyMenu"
            "-DEXPECT=modifyMenu: Modify: Move .*; Explode .*; Lengthen .*; Reverse .*; Match Properties "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/modify_menu_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # An inquiry tool driven from the command line: DIST, then two typed
    # points, and the inverse of the 3-4-5 leg in the log - 5.000 by hand.
    # ZOOM typed between them is taken by the tool host as the view's
    # command (ToolHost::onTransparent), never handed to the tool as a
    # malformed point, and the tool carries on at the same step. No view
    # runs it yet, so the host refuses it aloud; the EXPECT holds that
    # refusal before the inverse, because a ZOOM wrongly sent to the tool
    # is refused by the tool instead ("'ZOOM' is not an input this tool
    # takes now") and the inverse still follows, which the result alone
    # cannot tell apart. When the view sets onTransparent the refusal
    # becomes the prompt shown again, and this EXPECT with it.
    add_test(NAME qt_dist_typed_at_the_command_line_reports_the_inverse_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>DIST|>0,0|>ZOOM|>3,4"
            "-DEXPECT=! ZOOM cannot run inside Distance; press Esc to end the tool first[.].*Horizontal distance 5[.]000"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/dist_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Quick Select is a form asked at the command line, one question at a
    # time with a default for Enter, so a headless run fills it as a person
    # does and no dialog waits on anyone: kind Line, Enter for any layer and
    # for no condition. One line drawn, one selected.
    add_test(NAME qt_quick_select_is_answered_at_the_command_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LINE 0,0 10,0|>CIRCLE 5,5 2|>SELECT NONE|>QSELECT|>Line|>|>"
            "-DEXPECT=1 entity selected: Lines on any layer, in the drawing"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/quick_select_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Every key sequence the window answers to reaches one thing: Qt
    # disables an ambiguous shortcut for BOTH actions, so a clash is two
    # keys that silently do nothing; and within each menu every underlined
    # letter is one item's (--check-shortcuts).
    add_test(NAME qt_every_shortcut_and_menu_letter_reaches_one_thing_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCHECK_SHORTCUTS=ON"
            "-DEXPECT=shortcuts: [0-9]+ key sequences, each reaching one thing"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/shortcuts_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_format_menu_opens_the_style_manager_headless
                         qt_the_style_manager_lists_a_library_group_s_linestyles_headless
                         qt_format_menu_opens_the_symbol_library_headless
                         qt_format_menu_opens_the_code_manager_and_explains_a_code_headless
                         qt_the_format_menu_and_the_customisation_log_name_no_other_program_headless
                         qt_a_forbidden_word_in_the_run_s_own_paths_is_not_held_against_it_headless
                         qt_a_forbidden_word_beside_a_path_in_the_log_still_fails_the_run_headless
                         qt_the_symbol_library_puts_a_fixture_symbol_on_imported_points_headless
                         qt_purge_unused_deletes_what_nothing_uses_as_one_undo_step_headless
                         qt_format_layers_opens_beside_the_drawing_and_follows_it_headless
                         qt_a_tool_started_by_its_alias_draws_from_typed_points_headless
                         qt_a_tool_refused_for_want_of_a_plan_view_is_left_unchecked_headless
                         qt_quitting_with_unapplied_code_manager_edits_is_refused_and_said_headless
                         qt_the_tools_menu_holds_the_inquiry_and_select_tools_headless
                         qt_the_draw_menu_offers_divide_and_measure_headless
                         qt_the_modify_menu_offers_lengthen_reverse_and_match_properties_headless
                         qt_dist_typed_at_the_command_line_reports_the_inverse_headless
                         qt_quick_select_is_answered_at_the_command_line_headless
                         qt_every_shortcut_and_menu_letter_reaches_one_thing_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    set_tests_properties(qt_survey_inverse_headless qt_survey_area_headless
                         qt_survey_forward_headless qt_survey_angle_headless
                         qt_survey_traverse_headless qt_survey_level_book_headless
                         qt_survey_converter_headless qt_survey_import_headless
                         qt_survey_import_uncertain_headless
                         qt_survey_import_role_boxes_swap_headless
                         qt_survey_export_round_trip_headless
                         qt_survey_import_transform_headless
                         qt_survey_point_report_headless
                         qt_survey_templates_reach_export_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()
