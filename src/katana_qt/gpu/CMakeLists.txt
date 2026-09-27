# The GPU renderer for the 3D view (docs/gpu.md).
#
# A library of its own, not sources compiled into `katana`, so that the tests
# (tests/gpu) and the GPU benchmark (benchmarks/gpu) link exactly what the
# application will, and so that `katana_gpu` existing is the one thing a host
# asks to know whether the GPU path was built (it also defines KATANA_HAS_GPU
# for whoever links it).
#
# It draws through QRhi, Qt's rendering hardware interface. QRhi's headers are
# semi-public: they live under QtGui/<version>/QtGui/rhi and reach the include
# path only through Qt6::GuiPrivate, with a narrower compatibility promise than
# the rest of Qt. That is why everything QRhi-shaped is kept inside this
# directory behind katana's own types (render::DrawList in, render::Camera in).
#
# SHADERS depend on the platform (docs/gpu.md, "Shaders"):
#
#   Windows  HLSL source compiled at run time by the d3dcompiler_47.dll every
#            Windows 10 and 11 install carries, drawn on Direct3D 11. No
#            build-time tool.
#   Linux    Vulkan-style GLSL (shaders/), compiled to SPIR-V at BUILD time by
#            Qt's qsb (Qt6ShaderTools, in the Linux toolchain) and embedded in
#            the library with #embed, drawn on Vulkan.
#   macOS    The same GLSL and the same baking, with qsb also translating each
#            stage to Metal Shading Language (--msl), drawn on Metal. Metal has
#            no geometry stage, so the two geometry shaders stay SPIR-V only
#            and the renderer draws lines and points with its instanced
#            expansion there (gpu_renderer.cpp asks QRhi::GeometryShader).
#
# The renderer does not know which: it asks a ShaderLibrary for a program
# (shader_library.hpp, defaultShaders()). The two sets must draw the same
# thing, and a change to one belongs in the other; the tests hold each to the
# software rasteriser on its own platform.

if(WIN32 OR APPLE OR CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(_katana_gpu_default ON)
else()
    set(_katana_gpu_default OFF)
endif()
option(KATANA_GPU "Build the GPU renderer for the 3D view (Direct3D 11 on Windows, Vulkan on Linux, Metal on macOS)"
       ${_katana_gpu_default})

if(NOT KATANA_GPU)
    message(STATUS "KATANA_GPU is OFF: the 3D view uses the software rasteriser only")
    return()
endif()

if(NOT WIN32 AND NOT APPLE AND NOT CMAKE_SYSTEM_NAME STREQUAL "Linux")
    message(WARNING
        "KATANA_GPU is ON but this is not Windows, Linux or macOS, and the GPU renderer has "
        "shaders for Direct3D 11, Vulkan and Metal only (docs/gpu.md, 'Shaders'); it is not "
        "built and the 3D view uses the software rasteriser.")
    return()
endif()

# Qt warns at configure time that GuiPrivate ties the build to this exact Qt
# version. That is known and accepted - it is why QRhi stays behind this
# directory's own interface (docs/gpu.md, "Risks") - so the warning is off.
set(QT_NO_PRIVATE_MODULE_WARNING ON)
find_package(Qt6 QUIET COMPONENTS Gui GuiPrivate Widgets)
if(NOT TARGET Qt6::GuiPrivate)
    message(WARNING
        "KATANA_GPU is ON but Qt6::GuiPrivate was not found, and QRhi's headers (rhi/qrhi.h) "
        "are only on its include path. The GPU renderer is not built and the 3D view uses the "
        "software rasteriser. Install Qt's private development files (in MSYS2 they ship with "
        "qt6-base) or configure with -DKATANA_GPU=OFF to silence this.")
    return()
endif()
if(NOT WIN32)
    # qsb, the shader baker, is a host tool of Qt6ShaderTools.
    find_package(Qt6 QUIET COMPONENTS ShaderToolsTools)
    if(NOT TARGET Qt6::qsb)
        message(WARNING
            "KATANA_GPU is ON but Qt's shader baker (Qt6::qsb, from Qt6ShaderTools) was not "
            "found, and the Vulkan renderer's shaders are compiled by it at build time. The GPU "
            "renderer is not built and the 3D view uses the software rasteriser. "
            "tools/setup_linux_toolchain.py installs it with Qt, or configure with "
            "-DKATANA_GPU=OFF to silence this.")
        return()
    endif()
endif()

set(_katana_gpu_sources
    scene_origin.cpp
    scene_origin.hpp
    gpu_scene.cpp
    gpu_scene.hpp
    shader_library.cpp
    shader_library.hpp
    gpu_renderer.cpp
    gpu_renderer.hpp
    offscreen_gpu.cpp
    offscreen_gpu.hpp
    renderer_choice.cpp
    renderer_choice.hpp
    gpu_scene_view.cpp
    gpu_scene_view.hpp
)
if(WIN32)
    list(APPEND _katana_gpu_sources shader_compiler.cpp shader_compiler.hpp)
else()
    list(APPEND _katana_gpu_sources baked_shaders.cpp baked_shaders.hpp)
endif()
add_library(katana_gpu STATIC ${_katana_gpu_sources})
katana_target_defaults(katana_gpu)
# Consumers include "gpu/gpu_renderer.hpp" and so on, from src/katana_qt.
target_include_directories(katana_gpu PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/..")
# GuiPrivate is PUBLIC because the tests drive raw QRhi themselves (tests/gpu).
target_link_libraries(katana_gpu PUBLIC katana_render Qt6::Gui Qt6::GuiPrivate Qt6::Widgets)
target_compile_definitions(katana_gpu PUBLIC KATANA_HAS_GPU=1)
# The vertex packing's AVX2 and NEON kernels (gpu_scene.cpp, docs/gpu.md "Packing"): the
# same on either backend, so before the Windows return below.
katana_add_simd_sources(katana_gpu AVX2 simd/pack_avx2.cpp NEON simd/pack_neon.cpp)

if(WIN32)
    # shader_compiler.cpp calls the HLSL compiler itself (d3dcompiler_47.dll,
    # part of Windows, which QRhi would otherwise load to compile the same
    # source).
    target_link_libraries(katana_gpu PRIVATE d3dcompiler_47)
    target_compile_definitions(katana_gpu PUBLIC KATANA_GPU_D3D11=1)
    message(STATUS "KATANA_GPU is ON: building the Direct3D 11 renderer (runtime HLSL shaders)")
    return()
endif()

# Every stage baked to a .qsb holding SPIR-V for Vulkan 1.0, in the build
# tree, then embedded by baked_shaders.cpp. Each stage names the two shared
# pieces it may include, so editing either rebakes what uses it.
set(_katana_shader_dir "${CMAKE_CURRENT_SOURCE_DIR}/shaders")
set(_katana_baked_dir "${CMAKE_CURRENT_BINARY_DIR}/baked_shaders")
set(_katana_stages
    triangles.vert triangles.frag
    line_ends.vert lines.geom point_centres.vert cloud_centres.vert sprites.geom
    lines_instanced.vert points_instanced.vert cloud_instanced.vert
    lines.frag points.frag cloud.frag)
set(_katana_baked "")
foreach(_stage IN LISTS _katana_stages)
    set(_out "${_katana_baked_dir}/${_stage}.qsb")
    # Metal Shading Language 1.2 beside the SPIR-V on macOS; never for a
    # geometry stage, which Metal does not have and qsb cannot translate.
    set(_katana_msl "")
    if(APPLE AND NOT _stage MATCHES "\\.geom$")
        set(_katana_msl --msl 12)
    endif()
    add_custom_command(
        OUTPUT "${_out}"
        COMMAND "${CMAKE_COMMAND}" -E make_directory "${_katana_baked_dir}"
        COMMAND "${CMAKE_COMMAND}" -E env LC_ALL=C.UTF-8
                $<TARGET_FILE:Qt6::qsb> --silent ${_katana_msl} -o "${_out}"
                "${_katana_shader_dir}/${_stage}"
        DEPENDS "${_katana_shader_dir}/${_stage}" "${_katana_shader_dir}/frame.glsl"
                "${_katana_shader_dir}/quad.glsl" Qt6::qsb
        COMMENT "Baking the ${_stage} shader"
        VERBATIM)
    list(APPEND _katana_baked "${_out}")
endforeach()
set_source_files_properties(baked_shaders.cpp PROPERTIES
    OBJECT_DEPENDS "${_katana_baked}"
    COMPILE_OPTIONS "--embed-dir=${_katana_baked_dir}")
if(APPLE)
    target_compile_definitions(katana_gpu PUBLIC KATANA_GPU_METAL=1)
    message(STATUS "KATANA_GPU is ON: building the Metal renderer (MSL baked by qsb)")
else()
    target_compile_definitions(katana_gpu PUBLIC KATANA_GPU_VULKAN=1)
    message(STATUS "KATANA_GPU is ON: building the Vulkan renderer (SPIR-V baked by qsb)")
endif()
