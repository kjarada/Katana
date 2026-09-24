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
# SHADERS are HLSL source compiled at run time by the d3dcompiler_47.dll every
# Windows 10 and 11 install carries, which is why the default is ON on Windows
# only: no other backend can consume them. Precompiled .qsb shaders (which would
# add Vulkan, Metal and OpenGL and remove the first-frame compile) need the
# qt6-shadertools package, which is not part of the toolchain today; see
# shader_library.hpp for the seam that takes them.

if(WIN32)
    set(_katana_gpu_default ON)
else()
    set(_katana_gpu_default OFF)
endif()
option(KATANA_GPU "Build the Direct3D 11 renderer for the 3D view (Windows)" ${_katana_gpu_default})

if(NOT KATANA_GPU)
    message(STATUS "KATANA_GPU is OFF: the 3D view uses the software rasteriser only")
    return()
endif()

if(NOT WIN32)
    message(WARNING
        "KATANA_GPU is ON but this is not Windows. The GPU renderer's shaders are HLSL for "
        "Direct3D 11 only until precompiled .qsb shaders are added (docs/gpu.md, 'Shaders'); "
        "the GPU renderer is not built and the 3D view uses the software rasteriser.")
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

add_library(katana_gpu STATIC
    scene_origin.cpp
    scene_origin.hpp
    gpu_scene.cpp
    gpu_scene.hpp
    shader_library.cpp
    shader_library.hpp
    shader_compiler.cpp
    shader_compiler.hpp
    gpu_renderer.cpp
    gpu_renderer.hpp
    offscreen_gpu.cpp
    offscreen_gpu.hpp
    renderer_choice.cpp
    renderer_choice.hpp
    gpu_scene_view.cpp
    gpu_scene_view.hpp
)
katana_target_defaults(katana_gpu)
# Consumers include "gpu/gpu_renderer.hpp" and so on, from src/katana_qt.
target_include_directories(katana_gpu PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/..")
# GuiPrivate is PUBLIC because the tests drive raw QRhi themselves (tests/gpu).
target_link_libraries(katana_gpu PUBLIC katana_render Qt6::Gui Qt6::GuiPrivate Qt6::Widgets)
# shader_compiler.cpp calls the HLSL compiler itself (d3dcompiler_47.dll, part
# of Windows, which QRhi would otherwise load to compile the same source).
target_link_libraries(katana_gpu PRIVATE d3dcompiler_47)
target_compile_definitions(katana_gpu PUBLIC KATANA_HAS_GPU=1)
message(STATUS "KATANA_GPU is ON: building the Direct3D 11 renderer (runtime HLSL shaders)")
