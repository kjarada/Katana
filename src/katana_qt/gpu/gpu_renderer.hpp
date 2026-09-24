#pragma once

// Draws a render::DrawList through a render::Camera with QRhi (docs/gpu.md).
//
// The GPU twin of render::Rasterizer, behind the same seam: it sees a draw
// list and a camera and nothing of the document, so everything the scene
// builder decides (what is drawn, in what colour) reaches the screen the same
// way down both paths. What it does differently is how it draws:
//
//   * positions relative to a scene origin kept in double (scene_origin.hpp);
//   * reversed-Z into a 32-bit float depth buffer, with an infinite far plane
//     for perspective;
//   * 4x multisampling (the render target's choice; see OffscreenGpu and
//     GpuSceneView);
//   * lines and points widened into screen-space quads on the GPU - by a
//     geometry shader, or by instancing where there is none (Expansion) -
//     and antialiased analytically in the fragment shader;
//   * optionally, lighting per pixel from face normals (LightingMode);
//   * vertical exaggeration as a uniform, so changing it rebuilds nothing.
//
// UPLOADS happen only when the scene changes. setDrawList packs the list
// (gpu_scene.hpp) and marks it dirty; the next render() uploads it once. Every
// other frame writes one 160-byte uniform block - the camera - and draws.
//
// The renderer owns no render target. It is handed a QRhi, a render pass
// descriptor to build pipelines against, and each frame a command buffer and
// a target: GpuSceneView gives it the widget's, OffscreenGpu a texture's.

#include <cstddef>
#include <cstdint>
#include <memory>

#include "gpu_scene.hpp"
#include "katana/core/error.hpp"
#include "katana/render/camera.hpp"
#include "katana/render/draw_list.hpp"
#include "shader_compiler.hpp"
#include "shader_library.hpp"

class QRhi;
class QRhiBuffer;
class QRhiCommandBuffer;
class QRhiGraphicsPipeline;
class QRhiRenderPassDescriptor;
class QRhiRenderTarget;
class QRhiShaderResourceBindings;

namespace katana::qt::gpu {

enum class LightingMode {
    // Use the draw list's colours as they are. The scene builder bakes flat
    // lighting into them (cad::SceneOptions::lightDirection), so this is the
    // mode that matches the software path pixel for pixel.
    Baked,
    // Light each pixel from its face normal and FrameSettings::lightDirection.
    // For a draw list built with the scene builder's own lighting switched off
    // (a zero cad::SceneOptions::lightDirection); lighting baked colours again
    // would darken them twice.
    PerPixel,
};

struct FrameSettings {
    katana::render::Rgba background = katana::render::rgba(28, 30, 36);
    LightingMode lighting = LightingMode::Baked;
    // World direction TOWARDS the light; normalised here. The scene builder's
    // default, so switching modes keeps the sun where it was.
    katana::math::Vec3 lightDirection{0.35, -0.5, 0.79};
    double ambient = 0.35;
    // z' = datum + (z - datum) * factor, applied in the vertex shader. A
    // draw list for this renderer is built with exaggeration 1.
    double verticalExaggeration = 1.0;
    double exaggerationDatum = 0.0;
    // Device pixels per logical pixel. Line widths and point sizes are in
    // logical pixels (as DrawLine::width is) and are scaled by this, so a
    // 1 px line is one LOGICAL pixel wide on a 125% display, not a thinner one.
    double pixelRatio = 1.0;
    // Screen diameter of a cloud point, in logical pixels.
    double cloudPointSize = 2.0;
    // Perspective depth to infinity (scene_origin.hpp). Off only to compare
    // with the software path's clipping at the camera's far plane.
    bool infiniteFar = true;
};

struct GpuFrameStats {
    std::size_t triangles = 0;
    std::size_t lines = 0;
    std::size_t points = 0;
    std::size_t cloudPoints = 0;
    // True when this frame uploaded the scene (it had changed).
    bool uploaded = false;
    std::size_t uploadedBytes = 0;
};

class GpuRenderer {
  public:
    GpuRenderer();
    ~GpuRenderer();
    GpuRenderer(const GpuRenderer&) = delete;
    GpuRenderer& operator=(const GpuRenderer&) = delete;

    // Creates the pipelines for render targets compatible with `pass` at
    // `sampleCount` samples per pixel, widening lines and points the
    // `preferred` way, or by instancing when the device has no geometry
    // stage. Fails with RenderingFailure when a shader does not compile or a
    // pipeline cannot be made: the caller falls back to the software path
    // rather than drawing nothing.
    [[nodiscard]] katana::core::Status
    initialise(QRhi* rhi, QRhiRenderPassDescriptor* pass, int sampleCount,
               const ShaderLibrary& shaders = compiledHlslShaders(),
               Expansion preferred = Expansion::GeometryShader);
    // Drops every GPU resource (the QRhi is going away, or the target format
    // changed). The packed scene is kept and re-uploaded after initialise().
    void releaseResources();
    [[nodiscard]] bool initialised() const;
    // How lines and points are widened since initialise().
    [[nodiscard]] Expansion expansion() const;

    // Packs `list` for the GPU; it is uploaded by the next render().
    void setDrawList(const katana::render::DrawList& list);
    // Takes an already packed scene (a test that fixes the origin).
    void setScene(GpuSceneData scene);
    [[nodiscard]] const GpuSceneData& scene() const;

    // Point clouds, drawn after the scene as round sprites.
    void setPointCloud(PointCloudData cloud);
    void clearPointCloud();

    // Records one frame into `commands`: the upload if the scene changed, the
    // uniforms, and a render pass on `target` that clears it and draws. The
    // camera's viewport must match the target's size in pixels; a camera with
    // no viewport size fails with InvalidArgument (drawing it would show a
    // wrong framing, as Rasterizer::render argues).
    [[nodiscard]] katana::core::Result<GpuFrameStats>
    render(QRhiCommandBuffer* commands, QRhiRenderTarget* target,
           const katana::render::Camera& camera, const FrameSettings& settings = {});

    // How many times the scene has been uploaded: the proof, for a test, that
    // a frame which changed only the camera uploaded nothing.
    [[nodiscard]] std::size_t uploadCount() const;

  private:
    struct Resources;
    std::unique_ptr<Resources> resources_;
};

} // namespace katana::qt::gpu
