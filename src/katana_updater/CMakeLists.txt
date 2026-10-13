# katana_updater: the fourth program, the helper that swaps the install's files
# while Katana is not running, and puts them back if anything goes wrong
# (docs/update.md; the algorithm is include/katana/update/apply.hpp).
#
# It links the update library and katana_core and NOTHING else - no GDAL, no Qt,
# no katana_app - and is STATIC, so that it imports only the operating system's
# DLLs. That matters because it never runs from bin/: the engine copies it to
# <state>/run-<tx>/ and runs the copy, so at commit nothing under the install
# root is loaded and no loaded file is renamed. A helper that loaded
# libstdc++-6.dll from a bin/ it was about to replace would lock exactly what it
# had to swap. tests/update checks the imports (katana_updater_imports_only_os_dlls).
#
# The files process_scan_{win,posix}.cpp and registry_{win,none}.cpp are in this
# directory because the helper's task owns them, but src/katana_update compiles
# them (they are the engine's API too); only main.cpp is the program.
add_executable(katana_updater main.cpp)
katana_target_defaults(katana_updater)
target_link_libraries(katana_updater PRIVATE katana_update katana_core)

# MinGW (GCC, and clang on ARM64): -static takes libstdc++, libgcc and libwinpthread
# in, leaving the C runtime, which is the operating system's (the api-ms-win-crt
# set). Elsewhere the toolchain's libraries are used as they are.
set(KATANA_UPDATER_LINK_OPTIONS "")
if(WIN32 AND NOT MSVC)
    set(KATANA_UPDATER_LINK_OPTIONS -static)
endif()
target_link_options(katana_updater PRIVATE ${KATANA_UPDATER_LINK_OPTIONS})

# For katana_updater_test (tests/update/CMakeLists.txt): the same source, built again
# with a test key. It is the SAME main.cpp and the same link options, so what the
# end-to-end tests run is what ships, less the keys.
set_property(GLOBAL PROPERTY KATANA_UPDATER_MAIN "${CMAKE_CURRENT_SOURCE_DIR}/main.cpp")
set_property(GLOBAL PROPERTY KATANA_UPDATER_LINK_OPTIONS "${KATANA_UPDATER_LINK_OPTIONS}")
