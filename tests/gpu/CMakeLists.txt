# The GPU renderer (src/katana_qt/gpu, docs/gpu.md), tested headlessly.
#
# QRhiWidget cannot render under Qt's offscreen platform, but raw QRhi on
# Direct3D 11 can render into a texture there - on the machine's GPU and on
# WARP, Windows' software device - so these tests draw small DrawLists that
# way and compare the pixels with the software rasteriser's, within a
# tolerance: a GPU frame is never bit-identical to the CPU's, nor from one
# driver to the next. A test that needs a device SKIPS when there is none
# (a machine without Direct3D 11, a CI runner without a GPU) rather than
# failing; the rest - the precision arithmetic, the packing, the renderer
# choice - need no device and always run.
#
# Registered with the prefix "gpu." (`ctest -R ^gpu\.`).
add_executable(katana_gpu_tests
    main.cpp
    gpu_test_support.cpp
    gpu_test_support.hpp
    test_scene_origin.cpp
    test_gpu_scene.cpp
    test_renderer_choice.cpp
    test_gpu_render.cpp
    test_gpu_layers.cpp
    test_gpu_scene_view.cpp
)
# tests/ too, as katana_add_test_suite gives every suite: simd_levels.hpp.
target_include_directories(katana_gpu_tests PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}"
                                                    "${CMAKE_CURRENT_SOURCE_DIR}/..")
target_link_libraries(katana_gpu_tests PRIVATE katana_gpu katana_render GTest::gtest)
katana_target_defaults(katana_gpu_tests)
set_target_properties(katana_gpu_tests PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${KATANA_TEST_OUTPUT_DIRECTORY}")
if(WIN32)
    set_property(TARGET katana_gpu_tests PROPERTY TEST_LAUNCHER
        ${CMAKE_COMMAND} -E env --modify "PATH=path_list_prepend:${KATANA_RUNTIME_BIN}")
endif()
# The platform the suite runs on (tests/gpu/main.cpp reads it). Windows:
# offscreen, where raw QRhi on Direct3D 11 still renders into textures.
# Linux: Qt's offscreen platform cannot make a Vulkan instance, so the suite
# runs on xcb under Xvfb - which also lets the "OnTheDesktop" cases draw a
# real QRhiWidget, on lavapipe when there is no GPU. Without xvfb-run the
# suite runs offscreen and every case that needs a device skips, saying so.
# macOS: offscreen too - Metal, like Direct3D 11, renders into textures
# without a window.
set(_katana_gpu_platform "offscreen")
if(NOT WIN32 AND NOT APPLE)
    find_program(KATANA_XVFB_RUN xvfb-run
        DOC "Runs the Vulkan GPU tests under a virtual X server (Debian/Ubuntu: xvfb)")
    if(KATANA_XVFB_RUN)
        set_property(TARGET katana_gpu_tests PROPERTY TEST_LAUNCHER
            "${KATANA_XVFB_RUN}" -a -s "-screen 0 1280x1024x24")
        set(_katana_gpu_platform "xcb")
    else()
        message(STATUS "xvfb-run not found: the Vulkan GPU tests will skip their device cases")
    endif()
endif()
gtest_discover_tests(katana_gpu_tests
    TEST_PREFIX "gpu."
    DISCOVERY_MODE PRE_TEST
    DISCOVERY_TIMEOUT 60
    PROPERTIES ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_GPU_TEST_PLATFORM=set:${_katana_gpu_platform}"
)
if(NOT _katana_gpu_platform STREQUAL "offscreen")
    # What the widget tells its host under the offscreen platform, where every
    # headless Katana run happens: the one case the xcb run skips.
    add_test(NAME gpu_offscreen.GpuSceneView.UnderTheOffscreenPlatformReportsThatItCannotRenderSoTheHostFallsBack
        COMMAND katana_gpu_tests
            --gtest_filter=GpuSceneView.UnderTheOffscreenPlatformReportsThatItCannotRenderSoTheHostFallsBack)
    set_tests_properties(
        gpu_offscreen.GpuSceneView.UnderTheOffscreenPlatformReportsThatItCannotRenderSoTheHostFallsBack
        PROPERTIES ENVIRONMENT_MODIFICATION
            "QT_QPA_PLATFORM=set:offscreen;KATANA_GPU_TEST_PLATFORM=set:offscreen")
endif()
