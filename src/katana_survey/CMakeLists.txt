# Eigen is an implementation detail of the adjustment code: it is linked PRIVATE
# and never appears in a public header (tools/check_layering.cmake enforces it).
find_package(Eigen3 REQUIRED NO_MODULE)

add_library(katana_survey STATIC
    angles.cpp
    data_model.cpp
    network.cpp
    cogo.cpp
    traverse.cpp
    leveling.cpp
    statistics.cpp
    error_propagation.cpp
    least_squares.cpp
    network_adjustment.cpp
    report_summary.cpp
)
# The reduction (reduction*.cpp) is globbed so that the files implementing it -
# corrections, pairing, the adjustment driver, the report renderers - can be
# added and the placeholder reduction_stub.cpp removed without editing this
# list. CONFIGURE_DEPENDS re-globs on the next build.
file(GLOB _survey_reduction_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/reduction*.cpp")
target_sources(katana_survey PRIVATE ${_survey_reduction_sources})
# The subsurface utility tools (subsurface*.cpp, include/katana/survey/subsurface,
# docs/subsurface_utilities.md) are globbed the same way.
file(GLOB _survey_subsurface_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/subsurface*.cpp")
target_sources(katana_survey PRIVATE ${_survey_subsurface_sources})
katana_target_defaults(katana_survey)
target_link_libraries(katana_survey PUBLIC katana_math PRIVATE Eigen3::Eigen)
