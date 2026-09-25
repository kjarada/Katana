#pragma once

// The Vulkan renderer's shaders, compiled to SPIR-V at build time (docs/gpu.md,
// "Shaders"). The GLSL is shaders/*.vert|geom|frag; qsb bakes each stage to a
// .qsb in the build tree (gpu/CMakeLists.txt) and baked_shaders.cpp embeds the
// bytes with #embed, so the program carries its shaders and compiles nothing
// on the user's machine. Built on Linux only (KATANA_GPU_VULKAN); Windows
// draws with the runtime HLSL (shader_compiler.hpp).

#include "shader_library.hpp"

namespace katana::qt::gpu {

// Every program of both expansions, from the embedded .qsb blobs.
[[nodiscard]] const ShaderLibrary& bakedShaders();

} // namespace katana::qt::gpu
