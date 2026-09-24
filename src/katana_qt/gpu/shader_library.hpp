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
// catch a shader error at build time instead of at the first frame. One
// caveat: qsb cannot translate a GEOMETRY shader into HLSL or MSL, so the
// geometry stage of Expansion::GeometryShader would still be this file's
// hand-written HLSL (qsb takes it with --replace), and Metal, which has no
// geometry stage, would draw with Expansion::Instanced.

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

// How a line or a point becomes the quad that is drawn (docs/gpu.md,
// "Lines and points"). Both give the same pixels; they differ in cost.
enum class Expansion {
    // A geometry shader turns each line (two vertices of a line list) or each
    // point into a four-vertex strip. The vertex shader transforms every end
    // once. The default: on the owner's scene, where every TIN edge is a line,
    // it is several times cheaper (docs/gpu.md, "Measurements").
    GeometryShader,
    // Each line or point is an instance of six vertices, widened by the vertex
    // shader. No geometry stage, so it runs where there is none - Metal has
    // none, which matters once precompiled shaders bring other backends - but
    // six-vertex instances keep the GPU's vertex stage badly occupied, and
    // every one of the six transforms both ends of its line again.
    Instanced,
};

inline constexpr std::array<Expansion, 2> kAllExpansions{Expansion::GeometryShader,
                                                         Expansion::Instanced};

[[nodiscard]] const char* toString(Expansion expansion);

struct ShaderStages {
    QShader vertex;
    // Invalid (QShader{}) when the program has no geometry stage: the
    // triangles, and everything under Expansion::Instanced.
    QShader geometry;
    QShader fragment;
};

class ShaderLibrary {
  public:
    virtual ~ShaderLibrary() = default;
    // The stages of `program` drawn the `expansion` way, or why there are
    // none. The triangles have one form, returned for either expansion.
    [[nodiscard]] virtual katana::core::Result<ShaderStages> program(Program program,
                                                                     Expansion expansion) const = 0;
    // For logs and the status line: which kind of library this is.
    [[nodiscard]] virtual std::string describe() const = 0;
};

// HLSL source (shader model 5.0) compiled at run time. Direct3D 11 only.
[[nodiscard]] const ShaderLibrary& runtimeHlslShaders();

// The HLSL text itself, for a test that wants to see what is compiled; an
// empty string for a stage the program does not have.
[[nodiscard]] const char* hlslSource(Program program, Expansion expansion, QShader::Stage stage);

// Shaders serialized by qsb (QShader::serialize), one blob per stage of each
// program and expansion. Not used by the build today; it is the seam
// precompiled shaders plug into. A blob that does not deserialize is reported
// when the program is asked for, not ignored.
class SerializedShaderLibrary final : public ShaderLibrary {
  public:
    struct Blobs {
        QByteArray vertex;
        QByteArray geometry; // empty: no geometry stage
        QByteArray fragment;
    };
    using BlobTable = std::array<std::array<Blobs, kAllExpansions.size()>, kAllPrograms.size()>;

    explicit SerializedShaderLibrary(BlobTable blobs);

    // Every program of `library` serialized, as a build step running qsb would
    // produce them. Lets a test prove the seam with the shaders there are.
    [[nodiscard]] static katana::core::Result<BlobTable> serialize(const ShaderLibrary& library);

    [[nodiscard]] katana::core::Result<ShaderStages> program(Program program,
                                                             Expansion expansion) const override;
    [[nodiscard]] std::string describe() const override { return "precompiled .qsb shaders"; }

  private:
    BlobTable blobs_;
};

} // namespace katana::qt::gpu
