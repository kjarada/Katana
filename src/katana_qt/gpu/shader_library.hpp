#pragma once

// Where the GPU renderer's shaders come from (docs/gpu.md, "Shaders").
//
// TODAY: HLSL source. QRhi accepts a QShader holding HLSL text and its
// Direct3D 11 backend compiles it with d3dcompiler_47.dll - part of every
// Windows 10 and 11 install - when the pipeline is created. No build-time
// tool and no package: the price is Direct3D 11 only, and a compile on the
// first frame (measured 28-121 ms, map_accel).
//
// LATER: precompiled shaders. Qt's `qsb` tool (package qt6-shadertools, not
// installed; the owner has not approved adding it) turns one Vulkan-style GLSL
// source into a .qsb holding SPIR-V, GLSL, HLSL/DXBC and MSL at build time.
// Those bytes would be embedded like the customisation (tools/
// embed_customisation.py: no rcc, no moc) and handed to
// SerializedShaderLibrary below, and the renderer would not change - it only
// ever asks a ShaderLibrary for a program. That would add the Vulkan, Metal
// and OpenGL backends (Linux and macOS), skip the first-frame compile, and
// catch a shader error at build time instead of at the first frame.

#include <array>
#include <string>

#include <rhi/qshader.h>

#include "katana/core/error.hpp"

namespace katana::qt::gpu {

// The four things the renderer draws, one pipeline each.
enum class Program {
    Triangles,  // filled, optionally lit per pixel from their face normal
    Lines,      // screen-space quads with analytic antialiasing
    Points,     // the draw list's square markers, as sprites
    CloudPoints // round sprites of a fixed screen size, for point clouds
};

inline constexpr std::array<Program, 4> kAllPrograms{Program::Triangles, Program::Lines,
                                                     Program::Points, Program::CloudPoints};

[[nodiscard]] const char* toString(Program program);

struct ShaderPair {
    QShader vertex;
    QShader fragment;
};

class ShaderLibrary {
  public:
    virtual ~ShaderLibrary() = default;
    // The vertex and fragment shader of `program`, or why there are none.
    [[nodiscard]] virtual katana::core::Result<ShaderPair> program(Program program) const = 0;
    // For logs and the status line: which kind of library this is.
    [[nodiscard]] virtual std::string describe() const = 0;
};

// HLSL source (shader model 5.0) compiled at run time. Direct3D 11 only.
[[nodiscard]] const ShaderLibrary& runtimeHlslShaders();

// The HLSL text itself, for a test that wants to see what is compiled.
[[nodiscard]] const char* hlslSource(Program program, QShader::Stage stage);

// Shaders serialized by qsb (QShader::serialize), one vertex and one fragment
// blob per program. Not used by the build today; it is the seam precompiled
// shaders plug into. A blob that does not deserialize is reported when the
// program is asked for, not ignored.
class SerializedShaderLibrary final : public ShaderLibrary {
  public:
    struct Blobs {
        QByteArray vertex;
        QByteArray fragment;
    };
    explicit SerializedShaderLibrary(std::array<Blobs, kAllPrograms.size()> blobs);

    [[nodiscard]] katana::core::Result<ShaderPair> program(Program program) const override;
    [[nodiscard]] std::string describe() const override { return "precompiled .qsb shaders"; }

  private:
    std::array<Blobs, kAllPrograms.size()> blobs_;
};

} // namespace katana::qt::gpu
