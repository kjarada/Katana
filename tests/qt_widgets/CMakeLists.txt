# The three view widgets (src/katana_qt/{viewport,render_view,section_view}
# _widget), driven headlessly on Qt's offscreen platform: what they frame, what
# they draw, what their hooks report and when their focus makes them the
# active view; and the workspace that docks them, with the chrome it shares
# with the main window's panels. The application is one executable with no library of its own,
# so the widgets' sources are compiled in here as they are into katana.
#
# Registered with the prefix "qt_widgets." so that `ctest -R qt_` runs these
# with the application's headless checks.
# Found in src/katana_qt's own directory scope, which this one does not see.
find_package(Qt6 REQUIRED COMPONENTS Widgets PrintSupport)

set(_qt_src "${PROJECT_SOURCE_DIR}/src/katana_qt")
# The customisation managers' shared pieces (the style painter the plan view
# now draws library linestyles and symbols with, the thumbnails, the pickers) and
# their dialogs, with the tests written for them: globbed, as they are in the
# application, so their authors add files without editing this list.
file(GLOB _qt_customisation_sources CONFIGURE_DEPENDS "${_qt_src}/customisation/*.cpp")
file(GLOB _qt_customisation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/customisation/*.cpp")
# The interactive tools hosted in the plan view (src/katana_qt/tools/: the host,
# the menus built from the catalogue, the tools' icons) and their tests: globbed
# for the same reason.
file(GLOB _qt_tool_sources CONFIGURE_DEPENDS "${_qt_src}/tools/*.cpp")
file(GLOB _qt_tool_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.cpp")
# The survey import wizard, the reduction options and report, Survey Jobs
# and the Subsurface Utilities dialog and workbench (src/katana_qt/survey/),
# with their tests: the pages are driven by their object names as a person
# clicks them, on the fixtures of tests/surveyio/data and
# tests/qt_widgets/survey/data.
file(GLOB _qt_survey_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/survey/*.cpp")
set(_qt_survey_sources
    ${_qt_src}/survey/survey_import_wizard.cpp
    ${_qt_src}/survey/survey_templates.cpp
    ${_qt_src}/survey/reduction_options_widget.cpp
    ${_qt_src}/survey/reduction_report_view.cpp
    ${_qt_src}/survey/survey_task.cpp
    ${_qt_src}/survey/survey_job_support.cpp
    ${_qt_src}/survey/survey_jobs_dialog.cpp
    ${_qt_src}/survey/utility_dialog.cpp
    ${_qt_src}/survey/utility_workbench.cpp)
# The sheets' dialogs, panels and output (src/katana_qt/plotting/) and their
# tests: globbed, as they are in the application.
file(GLOB _qt_plotting_sources CONFIGURE_DEPENDS "${_qt_src}/plotting/*.cpp")
file(GLOB _qt_plotting_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/plotting/*.cpp")
# The annotation front end (src/katana_qt/annotation/) and its tests: globbed,
# as it is in the application.
file(GLOB _qt_annotation_sources CONFIGURE_DEPENDS "${_qt_src}/annotation/*.cpp")
file(GLOB _qt_annotation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/annotation/*.cpp")
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
    # The background job runner (test_jobs).
    test_jobs.cpp
    ${_qt_src}/jobs.cpp
    # GIS > Online Data's dialog (test_gis_online).
    test_gis_online.cpp
    ${_qt_src}/gis_online_dialog.cpp
    ${_qt_src}/viewport_widget.cpp
    ${_qt_src}/plan_painter.cpp
    ${_qt_src}/sheet_painter.cpp
    ${_qt_src}/sheet_editor.cpp
    ${_qt_src}/render_view_widget.cpp
    ${_qt_src}/section_view_widget.cpp
    ${_qt_src}/theme.cpp
    # The workspace and the dock chrome its views wear (test_view_chrome).
    ${_qt_src}/view_workspace.cpp
    ${_qt_src}/view_layers_popup.cpp
    ${_qt_src}/dock_chrome.cpp
    ${_qt_src}/icons.cpp
    # The styles and linetypes manager and the layer manager, built on the
    # customisation widgets (tests/qt_widgets/customisation/test_style_manager*).
    ${_qt_src}/style_manager.cpp
    ${_qt_src}/layer_manager.cpp
    # File > Project Coordinate System (test_project_crs_dialog).
    ${_qt_src}/project_crs_dialog.cpp
    test_project_crs_dialog.cpp
    # The plan view's shortcut menu and Edit > Select by ID
    # (test_plan_context_menu).
    ${_qt_src}/plan_context_menu.cpp
    ${_qt_src}/select_by_id_dialog.cpp
    test_plan_context_menu.cpp
    ${_qt_customisation_sources}
    ${_qt_customisation_tests}
    ${_qt_tool_sources}
    ${_qt_tool_tests}
    ${_qt_survey_sources}
    ${_qt_survey_tests}
    ${_qt_plotting_sources}
    ${_qt_plotting_tests}
    ${_qt_annotation_sources}
    ${_qt_annotation_tests}
)
target_include_directories(katana_qt_widget_tests PRIVATE "${_qt_src}"
                                                          "${CMAKE_CURRENT_SOURCE_DIR}")
# katana_archive12d: the style painter names library pens through its colour table.
# katana_surveyio and katana_survey: the survey pages read and reduce field files.
target_link_libraries(katana_qt_widget_tests PRIVATE
    katana_cad katana_interop katana_render katana_archive12d katana_survey katana_surveyio
    Qt6::Widgets Qt6::PrintSupport GTest::gtest)
target_compile_definitions(katana_qt_widget_tests PRIVATE
    KATANA_SURVEYIO_DATA="${PROJECT_SOURCE_DIR}/tests/surveyio/data"
    KATANA_SURVEY_UI_DATA="${CMAKE_CURRENT_SOURCE_DIR}/survey/data")
katana_target_defaults(katana_qt_widget_tests)
# The 3D view hosts the GPU renderer where it is built, so the widget tests
# compile what the application does; offscreen, the rules choose software.
if(TARGET katana_gpu)
    target_link_libraries(katana_qt_widget_tests PRIVATE katana_gpu)
endif()
set_target_properties(katana_qt_widget_tests PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${KATANA_TEST_OUTPUT_DIRECTORY}")

# katana_register_tests, with the prefix and the offscreen platform. main.cpp
# forces the platform as well; the environment says so to anyone reading the
# test's properties.
if(WIN32)
    set_property(TARGET katana_qt_widget_tests PROPERTY TEST_LAUNCHER
        ${CMAKE_COMMAND} -E env --modify "PATH=path_list_prepend:${KATANA_RUNTIME_BIN}")
endif()
gtest_discover_tests(katana_qt_widget_tests
    TEST_PREFIX "qt_widgets."
    DISCOVERY_MODE PRE_TEST
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
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
                           OnTheDesktopAFailedGpuViewLeavesTheSessionToTheSoftwareRasteriser)
        add_test(NAME qt_widgets_gpu.RenderViewGpu.${_case}
            COMMAND "${KATANA_XVFB_RUN}" -a -s "-screen 0 1280x1024x24"
                    $<TARGET_FILE:katana_qt_widget_tests> --gtest_filter=RenderViewGpu.${_case})
        set_tests_properties(qt_widgets_gpu.RenderViewGpu.${_case} PROPERTIES
            ENVIRONMENT_MODIFICATION
                "QT_QPA_PLATFORM=set:offscreen;KATANA_WIDGET_TEST_PLATFORM=set:xcb;KATANA_RENDERER=set:gpu"
            TIMEOUT 120)
    endforeach()
endif()
