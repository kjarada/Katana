# surveyio: manufacturer survey field data -> katana::survey values.
#
# Two targets for one module, and the reason is the registry.
#
# Every .cpp in this directory is compiled in, by glob rather than by a list.
# That is deliberate and it is half the point of the module's design: the parsers
# are written in parallel, and a list would turn "add a parser" into an edit to
# this one shared file by every author at once. CONFIGURE_DEPENDS makes Ninja
# re-run the glob when a file appears, so a new parser needs no reconfigure by
# hand. The usual objection to globbing - that a file can join the build without
# anyone noticing - is answered by the registry: a format that is not registered
# is not offered, and tests/surveyio asserts on what is registered.
file(GLOB _surveyio_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/*.cpp")

# A parser registers its format from its own translation unit (the contract is in
# include/katana/surveyio/format.hpp), so its object file defines nothing that
# anything else references. A linker pulls a member out of a static archive ONLY
# to satisfy an undefined symbol, so such a parser is silently dropped: the format
# is absent from the registry, a file it could have read is reported as
# unrecognised, and nothing anywhere says why. That is exactly the silent failure
# PLAN.MD section 36 forbids, so the archive is forced in whole.
#
# MEASURED on this toolchain (GCC 16.2, MSYS2 UCRT64, CMake + Ninja), with a
# module whose only contribution is one self-registering object:
#
#   plain static archive                       registered = 0   (dropped)
#   OBJECT library, consumer links it directly registered = 1
#   OBJECT library, through a STATIC middle    registered = 0   (dropped again:
#       CMake hands the objects to the direct consumer, and when that consumer is
#       an archive the member is unreferenced inside it and the trap returns -
#       which is precisely how app and qt will consume this module)
#   this design (WHOLE_ARCHIVE), both cases    registered = 1
#
# So an OBJECT library is not enough. The code lives in katana_surveyio_archive
# and the name everyone links, katana_surveyio, is an INTERFACE target that wraps
# it in WHOLE_ARCHIVE. Wrapping it HERE rather than asking each consumer to write
# $<LINK_LIBRARY:WHOLE_ARCHIVE,...> is the point: a correctness rule that has to
# be remembered in app, in qt and in every future consumer is a rule that will be
# forgotten in one of them, and the symptom would be a missing file format rather
# than a link error.
add_library(katana_surveyio_archive STATIC ${_surveyio_sources})
katana_target_defaults(katana_surveyio_archive)
target_link_libraries(katana_surveyio_archive PUBLIC katana_survey)

add_library(katana_surveyio INTERFACE)
katana_target_defaults(katana_surveyio)
target_link_libraries(katana_surveyio INTERFACE
    "$<LINK_LIBRARY:WHOLE_ARCHIVE,katana_surveyio_archive>" katana_survey)
