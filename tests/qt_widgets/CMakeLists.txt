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
find_package(Qt6 REQUIRED COMPONENTS Widgets)

set(_qt_src "${PROJECT_SOURCE_DIR}/src/katana_qt")
# The customisation managers' shared pieces (the style painter the plan view
# now draws 12d linestyles and symbols with, the thumbnails, the pickers) and
# their dialogs, with the tests written for them: globbed, as they are in the
# application, so their authors add files without editing this list.
file(GLOB _qt_customisation_sources CONFIGURE_DEPENDS "${_qt_src}/customisation/*.cpp")
file(GLOB _qt_customisation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/customisation/*.cpp")
add_executable(katana_qt_widget_tests
    main.cpp
    test_view_focus.cpp
    test_render_view.cpp
    test_section_view.cpp
    test_plan_view.cpp
    test_view_chrome.cpp
    ${_qt_src}/viewport_widget.cpp
    ${_qt_src}/render_view_widget.cpp
    ${_qt_src}/section_view_widget.cpp
    ${_qt_src}/theme.cpp
    # The workspace and the dock chrome its views wear (test_view_chrome).
    ${_qt_src}/view_workspace.cpp
    ${_qt_src}/view_layers_popup.cpp
    ${_qt_src}/dock_chrome.cpp
    ${_qt_src}/icons.cpp
    ${_qt_customisation_sources}
    ${_qt_customisation_tests}
)
target_include_directories(katana_qt_widget_tests PRIVATE "${_qt_src}"
                                                          "${CMAKE_CURRENT_SOURCE_DIR}")
# katana_archive12d: the style painter names 12d pens through its colour table.
target_link_libraries(katana_qt_widget_tests PRIVATE
    katana_cad katana_interop katana_render katana_archive12d Qt6::Widgets GTest::gtest)
katana_target_defaults(katana_qt_widget_tests)
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
