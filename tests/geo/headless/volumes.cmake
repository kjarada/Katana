# Terrain > Volumes in the real window (docs/volumes.md, "In the window"),
# driven headlessly through tools/check_screenshot.cmake. Here, with the
# geoprocessing checks, because the two surfaces it measures are made by
# SURFACE FROM, the geoprocessing executor's verb. Included from
# tests/CMakeLists.txt when the window is built.
#
# Two closed 3D strings round the square [0, 10]^2, at 10 on layer base and
# at 11 on layer top, each made a surface of two triangles: a 1 m slab. The
# dialog, as a person fills it, measures it inside the square [2, 6]^2 typed
# corner by corner - 4 x 4 x 1 = 16 m3 of fill, none of cut - and then makes
# the cut/fill map, the top less the base, as a surface of the drawing, and the
# envelope, the higher of the two, as another.
add_test(NAME qt_volumes_dialog_measures_inside_typed_corners_and_makes_the_cut_fill_map_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>LAYER NEW base|>LAYER SET base|>PLINE3D 0,0,10 10,0,10 10,10,10 0,10,10 CLOSE|>LAYER NEW top|>LAYER SET top|>PLINE3D 0,0,11 10,0,11 10,10,11 0,10,11 CLOSE|>SURFACE FROM LAYERS base ONLY NAME base|>SURFACE FROM LAYERS top ONLY NAME top|@terrainVolumes|volumesBase=base|volumesTop=top|!volumesBoundaryCorners|volumesCorners=2,2 6,2 6,6 2,6|?volumesCommand|!volumesRun|?volumesStatus|!volumesCutFillMap|?volumesStatus|!volumesEnvelope|?volumesReply|?volumesStatus"
        "-DEXPECT=--dialog terrainVolumes opened volumesDialog \"Volumes\".*volumesCommand: VOLUME SURFACES base top INSIDE 2,2 6,2 6,6 2,6.*volumesStatus: Cut 0\\.000 m3, fill 16\\.000 m3, net 16\\.000 m3.*volumesStatus: Made the cut/fill map.*volumesReply: envelope first=base second=top side=max.*volumesStatus: Made the envelope"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/volumes_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
set_tests_properties(
    qt_volumes_dialog_measures_inside_typed_corners_and_makes_the_cut_fill_map_headless
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
    TIMEOUT 180)
