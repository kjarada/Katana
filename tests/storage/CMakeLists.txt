# Survey job persistence (test_survey_job*.cpp) is globbed so its tests can be
# added without editing this list.
file(GLOB _storage_survey_job_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/test_survey_job*.cpp")

katana_add_test_suite(storage
    LIBS katana_storage
    SOURCES test_storage.cpp ${_storage_survey_job_tests}
)
