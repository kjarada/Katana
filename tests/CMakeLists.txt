# One test executable per module keeps link dependencies honest: the math tests
# cannot accidentally rely on SQLite, the geometry tests cannot rely on Qt.
#
#   katana_add_test_suite(<module> LIBS <targets...> SOURCES <files...>)

function(katana_add_test_suite module)
    cmake_parse_arguments(ARG "" "" "LIBS;SOURCES" ${ARGN})
    set(_target katana_${module}_tests)
    add_executable(${_target} ${ARG_SOURCES})
    target_link_libraries(${_target} PRIVATE ${ARG_LIBS} GTest::gtest_main)
    target_include_directories(${_target} PRIVATE "${CMAKE_CURRENT_FUNCTION_LIST_DIR}")
    katana_target_defaults(${_target})
    # Beside the application, not among it: bin/ holds what ships.
    set_target_properties(${_target} PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${KATANA_TEST_OUTPUT_DIRECTORY}")
    katana_register_tests(${_target})
endfunction()

add_test(NAME layering
    COMMAND ${CMAKE_COMMAND} -DKATANA_ROOT=${PROJECT_SOURCE_DIR}
            -P ${PROJECT_SOURCE_DIR}/tools/check_layering.cmake)

# tests/<name>/ holds the suite of module katana_<name>; it is configured only
# when that module is.
file(GLOB _suites LIST_DIRECTORIES true "${CMAKE_CURRENT_SOURCE_DIR}/*")
foreach(_suite IN LISTS _suites)
    get_filename_component(_name "${_suite}" NAME)
    if(EXISTS "${_suite}/CMakeLists.txt" AND TARGET katana_${_name})
        add_subdirectory("${_suite}")
    endif()
endforeach()

# A plot produced headlessly, so the PDF writing inside the Qt widget is
# exercised by ctest and not only by hand (PLAN.MD Phase 22). The script
# copies the sample project first: a test must never touch repository data.
if(TARGET katana)
    add_test(NAME qt_plot_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/plot_headless.pdf"
            -P "${PROJECT_SOURCE_DIR}/tools/check_plot.cmake")
    set_tests_properties(qt_plot_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The whole main window, built and painted headlessly: theme, toolbars, every
    # icon, the viewport with the sample in it. See tools/check_screenshot.cmake.
    add_test(NAME qt_screenshot_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/screenshot_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_screenshot_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# A layer switched off through its box in the layer panel, headlessly. This
# crashed the application when the panel was rebuilt inside its own itemChanged
# signal; the check is in MainWindow::toggleLayerThroughPanel. BUILDING is a
# layer of the sample with entities on it.
if(TARGET katana)
    add_test(NAME qt_toggle_layer_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/toggle_layer_headless.png"
            "-DTOGGLE_LAYER=BUILDING"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_toggle_layer_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The same click after a 12d archive import that brought a surface, a
    # point cloud and a split layout: the plan viewport is replaced, and its
    # document listener must go with it. This crashed 5 runs in 6 before
    # Document::ListenerHandle; a run that survives by luck still passes,
    # which is why the unit tests in test_cad.cpp carry the proof.
    add_test(NAME qt_import_12da_then_toggle_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/all_geometries.12daz"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_toggle_headless.png"
            "-DTOGGLE_LAYER=Cadastre"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_import_12da_then_toggle_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # Point symbols painted from their styles (PLAN.MD 20.2 slice 2), into a
    # directory that is NOT a project: "Open Failed" was a modal box, which in
    # a headless run is a hang the TIMEOUT below catches.
    add_test(NAME qt_import_symbols_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/symbols.12da"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_symbols_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_import_symbols_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The styles and linetypes manager (PLAN.MD 20.2 slice 3), opened over a
    # 12d import so that its tables hold real linestyles and symbols. What it
    # protects is that the dialog builds, lays out and PAINTS - its tables,
    # its form, its two previews - which is otherwise only ever checked by a
    # person opening the menu.
    add_test(NAME qt_style_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/symbols.12da"
            "-DSTYLE_MANAGER=ON"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/style_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_style_manager_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The attribute manager (PLAN.MD 20.2 slice 5) over a 12d import, on the
    # first entity - a permanent mark whose attributes came from 12d as two
    # groups. It builds the tree from the flattened keys and paints it.
    # No project, so entity 1 IS the first imported string rather than the
    # first entity of the sample, which has no 12d attributes at all.
    add_test(NAME qt_attribute_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/all_geometries.12da"
            "-DATTRIBUTES=1"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/attribute_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_attribute_manager_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()
