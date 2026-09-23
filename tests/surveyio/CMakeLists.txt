# Globbed, for the same reason src/katana_surveyio is: a format parser and its
# tests are written as one piece, by one author, at the same time as four other
# authors are writing theirs. An explicit list here would make "add a parser's
# tests" an edit to this one shared file by all of them at once. The objection
# to globbing - that a file joins the build unnoticed - does not apply to a test
# suite, where a file that is not run is a file whose absence ctest reports as a
# smaller count.
file(GLOB _surveyio_test_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/*.cpp")

katana_add_test_suite(surveyio
    LIBS katana_surveyio
    SOURCES
        ${_surveyio_test_sources}
)
