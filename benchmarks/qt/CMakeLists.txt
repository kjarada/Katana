# The plan view's paint and plot benchmarks (bench_plan_paint.cpp) on a
# generated survey drawing (survey_drawing.cpp). A target of its own, because
# it links Qt and the plan view and katana_benchmarks links neither. The plan
# view's objects are the application's own, katana_qt_ui
# (src/katana_qt/CMakeLists.txt): compiled once, linked here as into katana
# and the widget tests. Run it like the other one, or side by side with
# another build's copy through tools/compare_benchmarks.py --alternate.
find_package(Qt6 REQUIRED COMPONENTS Widgets)

add_executable(katana_qt_benchmarks
    main.cpp
    bench_plan_paint.cpp
    bench_plan_cloud.cpp
    survey_drawing.cpp
    survey_drawing.hpp
)
target_include_directories(katana_qt_benchmarks PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(katana_qt_benchmarks PRIVATE benchmark::benchmark katana_qt_ui)
katana_target_defaults(katana_qt_benchmarks)
set_target_properties(katana_qt_benchmarks PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${KATANA_BENCHMARK_OUTPUT_DIRECTORY}")
