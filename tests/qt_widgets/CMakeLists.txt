# The three view widgets (src/katana_qt/{viewport,render_view,section_view}
# _widget), driven headlessly on Qt's offscreen platform: what they frame, what
# they draw, what their hooks report and when their focus makes them the
# active view; and the workspace that docks them, with the chrome it shares
# with the main window's panels. The widgets' objects are the application's
# own: katana_qt_ui (src/katana_qt/CMakeLists.txt), compiled once and linked
# into katana and here alike.
#
# Registered with the prefix "qt_widgets." so that `ctest -R qt_` runs these
# with the application's headless checks.
# Found in src/katana_qt's own directory scope, which this one does not see.
find_package(Qt6 REQUIRED COMPONENTS Widgets PrintSupport)

# The tests of the customisation managers, the tools, the survey pages, the
# sheets, the annotation front end, the geoprocessing workbench and the
# drawing front end: globbed, as their sources are in the application, so
# their authors add files without editing this list. The survey pages are
# driven by their object names as a person clicks them, on the fixtures of
# tests/surveyio/data and tests/qt_widgets/survey/data.
file(GLOB _qt_customisation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/customisation/*.cpp")
file(GLOB _qt_tool_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.cpp")
file(GLOB _qt_survey_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/survey/*.cpp")
file(GLOB _qt_plotting_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/plotting/*.cpp")
file(GLOB _qt_annotation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/annotation/*.cpp")
file(GLOB _qt_geo_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/geo/*.cpp")
file(GLOB _qt_drawing_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/drawing/*.cpp")
add_executable(katana_qt_widget_tests
    main.cpp
    test_view_focus.cpp
    test_render_view.cpp
    test_render_view_gpu.cpp
    test_section_view.cpp
    test_plan_view.cpp
    test_plan_painter.cpp
    test_sheet_painter.cpp
    test_sheet_editor.cpp
    test_plan_cloud.cpp
    test_view_chrome.cpp
    # Linked plan views, their Link button and the VIEWS and ZOOM lines the
    # workspace answers (test_view_links).
    test_view_links.cpp
    # The selection in every view: ghosts where a view hides the layer, and
    # each view's switch for them (test_view_selection).
    test_view_selection.cpp
    # The background job runner (test_jobs).
    test_jobs.cpp
    # GIS > Online Data's dialog (test_gis_online).
    test_gis_online.cpp
    # File > Project Coordinate System (test_project_crs_dialog).
    test_project_crs_dialog.cpp
    # File > Run Script and the SCRIPT verb's reading and running (test_script_runner).
    test_script_runner.cpp
    # Help > Command Reference and Keyboard Shortcuts (test_command_reference).
    test_command_reference.cpp
    # Where an import lands: the Placement group, File > Import's step and
    # GIS > Import Vector Data's dialog (test_import_placement).
    test_import_placement.cpp
    # The plan view's shortcut menu and Edit > Select by ID
    # (test_plan_context_menu).
    test_plan_context_menu.cpp
    # The theme's style, stylesheet and fonts, and the icons (test_theme).
    test_theme.cpp
    # The Properties panel's tree (test_property_panel).
    test_property_panel.cpp
    # Terrain > Alignment Manager (test_alignment_manager).
    test_alignment_manager.cpp
    # File > Import IFC and Export IFC (test_ifc_dialogs).
    test_ifc_dialogs.cpp
    ${_qt_customisation_tests}
    ${_qt_tool_tests}
    ${_qt_survey_tests}
    ${_qt_plotting_tests}
    ${_qt_annotation_tests}
    ${_qt_geo_tests}
    ${_qt_drawing_tests}
)
target_include_directories(katana_qt_widget_tests PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")
# katana_qt_ui brings the window's objects and, PUBLIC, everything they link
# and the src/katana_qt include path.
target_link_libraries(katana_qt_widget_tests PRIVATE katana_qt_ui GTest::gtest)
target_compile_definitions(katana_qt_widget_tests PRIVATE
    KATANA_SURVEYIO_DATA="${PROJECT_SOURCE_DIR}/tests/surveyio/data"
    KATANA_SURVEY_UI_DATA="${CMAKE_CURRENT_SOURCE_DIR}/survey/data"
    KATANA_QT_WIDGET_DATA="${CMAKE_CURRENT_SOURCE_DIR}/data"
    KATANA_SAMPLES="${PROJECT_SOURCE_DIR}/samples")
katana_target_defaults(katana_qt_widget_tests)
# The 3D view hosts the GPU renderer where it is built (katana_qt_ui links
# katana_gpu), so the widget tests run what the application does; offscreen,
# the rules choose software.
if(TARGET katana_gpu)
    # The GPU suite's support, for its whyNoVulkanDriver: the OnTheDesktop
    # cases of test_render_view_gpu.cpp skip on it as the gpu suite's do.
    target_sources(katana_qt_widget_tests PRIVATE
        "${PROJECT_SOURCE_DIR}/tests/gpu/gpu_test_support.cpp")
    target_include_directories(katana_qt_widget_tests PRIVATE "${PROJECT_SOURCE_DIR}/tests/gpu")
endif()
set_target_properties(katana_qt_widget_tests PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${KATANA_TEST_OUTPUT_DIRECTORY}")

# katana_register_tests, with the prefix and the offscreen platform. main.cpp
# forces the platform as well; the environment says so to anyone reading the
# test's properties.
# The same minute's discovery timeout as every other suite
# (cmake/KatanaTargetDefaults.cmake says why).
katana_discover_tests(katana_qt_widget_tests
    PREFIX "qt_widgets."
    ENVIRONMENT "QT_QPA_PLATFORM=set:offscreen"
)
# The 3D view drawn on the GPU (test_render_view_gpu.cpp) needs a platform a
# QRhiWidget can be shown on: on Linux, xcb under Xvfb, with KATANA_RENDERER=gpu
# so that lavapipe may draw it where there is no GPU. Each case in a process
# of its own - a GPU failure is remembered for the session. Offscreen, above,
# the same cases skip.
if(NOT WIN32)
    find_program(KATANA_XVFB_RUN xvfb-run
        DOC "Runs the Vulkan GPU tests under a virtual X server (Debian/Ubuntu: xvfb)")
endif()
if(TARGET katana_gpu AND NOT WIN32 AND KATANA_XVFB_RUN)
    foreach(_case IN ITEMS OnTheDesktopTheHostsLayersAreDrawnByItsGpuChild
                           OnTheDesktopAnEditReachesTheGpuView
                           OnTheDesktopTheWheelOverTheGpuViewZoomsTowardsWhatIsUnderTheCursor
                           OnTheDesktopADeepZoomFarFromTheScenesCentrePacksTheLayersAgainstThePivot
                           OnTheDesktopAFailedGpuViewLeavesTheSessionToTheSoftwareRasteriser)
        add_test(NAME qt_widgets_gpu.RenderViewGpu.${_case}
            COMMAND "${KATANA_XVFB_RUN}" -a -s "-screen 0 1280x1024x24"
                    $<TARGET_FILE:katana_qt_widget_tests> --gtest_filter=RenderViewGpu.${_case})
        # A case that skips - no Vulkan driver on the machine - exits 0, which
        # a plain add_test would count as passed; gtest_discover_tests says
        # so for the other cases, this says so for these.
        set_tests_properties(qt_widgets_gpu.RenderViewGpu.${_case} PROPERTIES
            ENVIRONMENT_MODIFICATION
                "QT_QPA_PLATFORM=set:offscreen;KATANA_WIDGET_TEST_PLATFORM=set:xcb;KATANA_RENDERER=set:gpu"
            SKIP_REGULAR_EXPRESSION "\\[  SKIPPED \\]"
            TIMEOUT 120)
    endforeach()
endif()
