# The reduction's tests (test_reduction*.cpp) are globbed so that the files
# testing it can be added without editing this list.
file(GLOB _survey_reduction_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/test_reduction*.cpp")
# And the subsurface utility tools' (test_subsurface*.cpp) likewise.
file(GLOB _survey_subsurface_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/test_subsurface*.cpp")

katana_add_test_suite(survey
    LIBS katana_survey
    SOURCES
        ${_survey_reduction_tests}
        ${_survey_subsurface_tests}
        test_raw_observations.cpp
        test_angles.cpp
        test_cogo.cpp
        test_data_model.cpp
        test_error_propagation.cpp
        test_least_squares.cpp
        test_leveling.cpp
        test_network_adjustment.cpp
        test_statistics.cpp
        test_traverse.cpp
)
