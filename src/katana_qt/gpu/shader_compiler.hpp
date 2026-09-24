#pragma once

// The runtime HLSL compiled to Direct3D bytecode by Katana itself, once per
// process (docs/gpu.md, "Shaders").
//
// Handing QRhi HLSL SOURCE (runtimeHlslShaders) makes every QRhi compile it
// again: the first 3D view, a second one, a 3D dock floated into a window of
// its own (which starts QRhi afresh), each test's target. The eleven stages
// the default expansion draws with take 110-175 ms to compile on the
// development laptop (BM_GpuShaderCompile), more than the device, target and
// pipelines together take without them (BM_GpuStartUp/1). So
// this library calls d3dcompiler_47 - the same compiler QRhi would load, part
// of every Windows 10 and 11 install - itself, keeps each stage's bytecode
// for the life of the process, and gives QRhi the bytecode: only the first
// pipeline to need a stage pays for it, and a host can pay that on a worker
// thread at start-up (precompileHlslShaders) so no view ever does. A compile
// error also comes back as a Status carrying the compiler's own message,
// where QRhi only writes it to the Qt log.
//
// This is not the precompiled .qsb route (shader_library.hpp): the compile
// still happens on the user's machine, once per run. It needs no package and
// no build step, and it is what a build step would feed: the same bytes.

#include <QByteArray>

#include "katana/core/error.hpp"
#include "shader_library.hpp"

namespace katana::qt::gpu {

// The default library: HLSL compiled once per process to bytecode.
[[nodiscard]] const ShaderLibrary& compiledHlslShaders();

// Compiles every stage `expansion` needs now, so the first view does not wait
// for it. Safe to call from any thread, and more than once (the second call
// finds everything compiled).
[[nodiscard]] katana::core::Status precompileHlslShaders(
    Expansion expansion = Expansion::GeometryShader);

// One stage's HLSL (entry point `main`, shader model 5.0) to bytecode, with no
// caching: RenderingFailure with the compiler's message when it does not
// compile. The benchmark times it; the library above caches it.
[[nodiscard]] katana::core::Result<QByteArray> compileHlsl(const char* source,
                                                           QShader::Stage stage);

} // namespace katana::qt::gpu
