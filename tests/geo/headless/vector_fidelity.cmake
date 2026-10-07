# Vector fidelity in the real window (docs/interop.md, "Fidelity"), driven
# headlessly through tools/check_screenshot.cmake. Included from
# tests/CMakeLists.txt when the window is built.
#
# The window's EXPORT - typed, or the Export Drawing dialog's - passes the
# project's coordinate system as the session's does: a KML of a drawing in
# MGA zone 56 is written in longitude and latitude and read back there,
# beside the point it came from.
# PROJ's own cs2cs puts 330000,6250000 in EPSG:28356 at
# 151.161906846,-33.876653623.
set(_fidelity_headless "${CMAKE_CURRENT_BINARY_DIR}/fidelity_headless")
file(MAKE_DIRECTORY "${_fidelity_headless}")
add_test(NAME qt_export_of_a_projected_drawing_to_kml_is_in_longitude_and_latitude_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>CRS SET EPSG:28356|>POINT 330000,6250000|>EXPORT \"${_fidelity_headless}/peg.kml\"|>IMPORT \"${_fidelity_headless}/peg.kml\"|>LIST"
        "-DEXPECT=converted from GDA94 / MGA zone 56 \\(EPSG:28356\\) to longitude and latitude.*Point  layer=0  at 330000,6250000.*Point  layer=0  at 151\\.161906846,-33\\.876653623"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/fidelity_kml_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(qt_export_of_a_projected_drawing_to_kml_is_in_longitude_and_latitude_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
