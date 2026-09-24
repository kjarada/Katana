# The plan view's paint and plot benchmarks (bench_plan_paint.cpp) on a
# generated survey drawing (survey_drawing.cpp). A target of its own, because
# it links Qt and the plan view's sources and katana_benchmarks links neither;
# the sources are compiled in as tests/qt_widgets compiles them - the same
# list, because the customisation widgets reach the workspace and managers -
# the application having no library of its own. Run it like the other one, or
# side by side with another build's copy through
# tools/compare_benchmarks.py --alternate.
find_package(Qt6 REQUIRED COMPONENTS Widgets)

set(_qt_src "${PROJECT_SOURCE_DIR}/src/katana_qt")
file(GLOB _qt_customisation_sources CONFIGURE_DEPENDS "${_qt_src}/customisation/*.cpp")
file(GLOB _qt_tool_sources CONFIGURE_DEPENDS "${_qt_src}/tools/*.cpp")
add_executable(katana_qt_benchmarks
    main.cpp
    bench_plan_paint.cpp
    bench_plan_cloud.cpp
    survey_drawing.cpp
    survey_drawing.hpp
    ${_qt_src}/viewport_widget.cpp
    ${_qt_src}/render_view_widget.cpp
    ${_qt_src}/section_view_widget.cpp
    ${_qt_src}/theme.cpp
    ${_qt_src}/view_workspace.cpp
    ${_qt_src}/view_layers_popup.cpp
    ${_qt_src}/dock_chrome.cpp
    ${_qt_src}/icons.cpp
    ${_qt_src}/style_manager.cpp
    ${_qt_src}/layer_manager.cpp
    ${_qt_src}/plan_painter.cpp
    ${_qt_customisation_sources}
    ${_qt_tool_sources}
)
target_include_directories(katana_qt_benchmarks PRIVATE "${_qt_src}" "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(katana_qt_benchmarks PRIVATE
    benchmark::benchmark katana_cad katana_interop katana_render katana_archive12d Qt6::Widgets)
katana_target_defaults(katana_qt_benchmarks)
set_target_properties(katana_qt_benchmarks PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${KATANA_BENCHMARK_OUTPUT_DIRECTORY}")
