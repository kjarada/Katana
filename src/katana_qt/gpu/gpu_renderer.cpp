#include "gpu_renderer.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <string>

#include <rhi/qrhi.h>

#include "scene_origin.hpp"

namespace katana::qt::gpu {

using katana::core::ErrorCode;
using katana::core::makeError;

namespace {

// The constant buffer every program reads. Mirrors KATANA_GPU_FRAME_CBUFFER
// in shader_library.cpp field for field; float4s only, so HLSL's packing
// rules put each exactly where C++ does.
struct FrameUniforms {
    std::array<float, 16> mvp{};
    std::array<float, 4> viewport{};
    std::array<float, 4> light{};
    std::array<float, 4> eyeRel{};
    std::array<float, 4> forwardMode{};
    std::array<float, 4> params{};
};
static_assert(sizeof(FrameUniforms) == 144);

// FILL OFFSET. Filled triangles are pushed AWAY from the eye by a polygon
// offset, so that a line lying in a surface - a TIN edge, a string draped on
// it - draws over it rather than z-fighting into dashes. Reversed Z puts far
// at 0, so "away" is negative.
//
// The slope term does the work: a line quad is flat in depth across its
// width while the surface under it slopes, and at a grazing angle the surface
// changes depth by metres per pixel. The quad reaches half a pixel of
// antialiasing fringe past its width's half (shader_library.cpp), which for a
// 1 px line is 1 px, so pushing the surface back by two pixels of its own
// depth slope leaves the line a pixel to spare. The constant term covers a surface seen face on, where the
// slope is zero: for a float depth buffer one unit is 2^(e - 23) of the
// value's own exponent, so 64 units is a relative 4e-6 to 8e-6 of the depth -
// millimetres at a kilometre.
//
// This replaces the software path's constant NDC bias, which was sized for a
// standard-Z buffer and pulls lines metres forward, through buildings.
constexpr int kFillConstantBias = -64;
constexpr float kFillSlopeBias = -2.0f;

constexpr std::size_t kProgramCount = kAllPrograms.size();

[[nodiscard]] std::size_t indexOf(Program program) { return static_cast<std::size_t>(program); }

// The scene's buffers hold the same bytes whichever way they are drawn
// (gpu_scene.hpp); only how the GPU steps through them differs. Through a
// geometry shader a line is two 20-byte vertices of a line list and a point
// one vertex of a point list; instanced, a line is one 40-byte instance and a
// point one 24-byte instance of a six-vertex quad.
[[nodiscard]] QRhiVertexInputLayout layoutFor(Program program, Expansion expansion)
{
    QRhiVertexInputLayout layout;
    using Attribute = QRhiVertexInputAttribute;
    using Binding = QRhiVertexInputBinding;
    const bool instanced = expansion == Expansion::Instanced;
    const auto binding = [instanced](std::size_t stride) {
        return instanced ? Binding(static_cast<quint32>(stride), Binding::PerInstance)
                         : Binding(static_cast<quint32>(stride));
    };
    switch (program) {
    case Program::Triangles:
        layout.setBindings({Binding(sizeof(GpuVertex))});
        layout.setAttributes({Attribute(0, 0, Attribute::Float3, 0),
                              Attribute(0, 1, Attribute::UNormByte4, 12)});
        break;
    case Program::Lines:
        if (instanced) {
            layout.setBindings({binding(sizeof(GpuLine))});
            layout.setAttributes({Attribute(0, 0, Attribute::Float3, 0),
                                  Attribute(0, 1, Attribute::UNormByte4, 12),
                                  Attribute(0, 2, Attribute::Float, 16),
                                  Attribute(0, 3, Attribute::Float3, 20),
                                  Attribute(0, 4, Attribute::UNormByte4, 32),
                                  Attribute(0, 5, Attribute::Float, 36)});
        } else {
            layout.setBindings({binding(sizeof(GpuLineEnd))});
            layout.setAttributes({Attribute(0, 0, Attribute::Float3, 0),
                                  Attribute(0, 1, Attribute::UNormByte4, 12),
                                  Attribute(0, 2, Attribute::Float, 16)});
        }
        break;
    case Program::Points:
        layout.setBindings({binding(sizeof(GpuPoint))});
        layout.setAttributes({Attribute(0, 0, Attribute::Float3, 0),
                              Attribute(0, 1, Attribute::UNormByte4, 12),
                              Attribute(0, 2, Attribute::Float, 16)});
        break;
    case Program::CloudPoints:
        layout.setBindings({binding(sizeof(GpuCloudPoint))});
        layout.setAttributes({Attribute(0, 0, Attribute::Float3, 0),
                              Attribute(0, 1, Attribute::UNormByte4, 12)});
        break;
    }
    return layout;
}

[[nodiscard]] QRhiGraphicsPipeline::Topology topologyFor(Program program, Expansion expansion)
{
    if (program == Program::Triangles || expansion == Expansion::Instanced) {
        return QRhiGraphicsPipeline::Triangles;
    }
    return program == Program::Lines ? QRhiGraphicsPipeline::Lines : QRhiGraphicsPipeline::Points;
}

[[nodiscard]] std::array<float, 4> float4(double x, double y, double z, double w)
{
    return {static_cast<float>(x), static_cast<float>(y), static_cast<float>(z),
            static_cast<float>(w)};
}

} // namespace

struct GpuRenderer::Resources {
    QRhi* rhi = nullptr;
    int sampleCount = 1;
    Expansion expansion = Expansion::GeometryShader;
    // Two uniform blocks: the scene and a point cloud are packed against
    // different origins, so each needs its own matrix.
    std::unique_ptr<QRhiBuffer> sceneUniforms;
    std::unique_ptr<QRhiBuffer> cloudUniforms;
    std::unique_ptr<QRhiShaderResourceBindings> sceneBindings;
    std::unique_ptr<QRhiShaderResourceBindings> cloudBindings;
    std::array<std::unique_ptr<QRhiGraphicsPipeline>, kProgramCount> pipelines;

    std::unique_ptr<QRhiBuffer> vertices;
    std::unique_ptr<QRhiBuffer> indices;
    std::unique_ptr<QRhiBuffer> lines;
    std::unique_ptr<QRhiBuffer> points;
    std::unique_ptr<QRhiBuffer> cloud;

    GpuSceneData scene;
    bool sceneDirty = true;
    PointCloudData cloudData;
    bool cloudDirty = false;
    std::size_t uploads = 0;

    void releaseGpu()
    {
        for (auto& pipeline : pipelines) {
            pipeline.reset();
        }
        sceneBindings.reset();
        cloudBindings.reset();
        sceneUniforms.reset();
        cloudUniforms.reset();
        vertices.reset();
        indices.reset();
        lines.reset();
        points.reset();
        cloud.reset();
        rhi = nullptr;
        // Whatever was uploaded went with the buffers.
        sceneDirty = true;
        cloudDirty = !cloudData.points.empty();
    }
};

GpuRenderer::GpuRenderer() : resources_(std::make_unique<Resources>()) {}

GpuRenderer::~GpuRenderer() = default;

bool GpuRenderer::initialised() const { return resources_->rhi != nullptr; }

Expansion GpuRenderer::expansion() const { return resources_->expansion; }

std::size_t GpuRenderer::uploadCount() const { return resources_->uploads; }

const GpuSceneData& GpuRenderer::scene() const { return resources_->scene; }

void GpuRenderer::releaseResources() { resources_->releaseGpu(); }

katana::core::Status GpuRenderer::initialise(QRhi* rhi, QRhiRenderPassDescriptor* pass,
                                             int sampleCount, const ShaderLibrary& shaders,
                                             Expansion preferred)
{
    Resources& r = *resources_;
    r.releaseGpu();
    if (rhi == nullptr || pass == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "no QRhi or render pass to draw with");
    }
    r.sampleCount = std::max(sampleCount, 1);
    r.expansion = preferred == Expansion::GeometryShader &&
                          rhi->isFeatureSupported(QRhi::GeometryShader)
                      ? Expansion::GeometryShader
                      : Expansion::Instanced;

    // The geometry shaders read the frame's constants too (the viewport, the
    // pixel ratio), so the block is bound to that stage when there is one.
    auto stages = QRhiShaderResourceBinding::VertexStage | QRhiShaderResourceBinding::FragmentStage;
    if (r.expansion == Expansion::GeometryShader) {
        stages |= QRhiShaderResourceBinding::GeometryStage;
    }
    r.sceneUniforms.reset(
        rhi->newBuffer(QRhiBuffer::Dynamic, QRhiBuffer::UniformBuffer, sizeof(FrameUniforms)));
    r.cloudUniforms.reset(
        rhi->newBuffer(QRhiBuffer::Dynamic, QRhiBuffer::UniformBuffer, sizeof(FrameUniforms)));
    if (!r.sceneUniforms->create() || !r.cloudUniforms->create()) {
        r.releaseGpu();
        return makeError(ErrorCode::RenderingFailure, "could not create the uniform buffers");
    }
    r.sceneBindings.reset(rhi->newShaderResourceBindings());
    r.sceneBindings->setBindings(
        {QRhiShaderResourceBinding::uniformBuffer(0, stages, r.sceneUniforms.get())});
    r.cloudBindings.reset(rhi->newShaderResourceBindings());
    r.cloudBindings->setBindings(
        {QRhiShaderResourceBinding::uniformBuffer(0, stages, r.cloudUniforms.get())});
    if (!r.sceneBindings->create() || !r.cloudBindings->create()) {
        r.releaseGpu();
        return makeError(ErrorCode::RenderingFailure, "could not create the shader bindings");
    }

    for (const Program program : kAllPrograms) {
        auto shaderStages = shaders.program(program, r.expansion);
        if (!shaderStages) {
            r.releaseGpu();
            return shaderStages.error();
        }
        std::unique_ptr<QRhiGraphicsPipeline> pipeline(rhi->newGraphicsPipeline());
        if (shaderStages->geometry.isValid()) {
            pipeline->setShaderStages({{QRhiShaderStage::Vertex, shaderStages->vertex},
                                       {QRhiShaderStage::Geometry, shaderStages->geometry},
                                       {QRhiShaderStage::Fragment, shaderStages->fragment}});
        } else {
            pipeline->setShaderStages({{QRhiShaderStage::Vertex, shaderStages->vertex},
                                       {QRhiShaderStage::Fragment, shaderStages->fragment}});
        }
        pipeline->setTopology(topologyFor(program, r.expansion));
        pipeline->setVertexInputLayout(layoutFor(program, r.expansion));
        pipeline->setSampleCount(r.sampleCount);
        pipeline->setDepthTest(true);
        pipeline->setDepthWrite(true);
        // Reversed Z: nearer is GREATER. Strictly greater, so of two
        // primitives at exactly the same depth the first drawn wins, as the
        // software path's strictly-less test does.
        pipeline->setDepthOp(QRhiGraphicsPipeline::Greater);
        if (program == Program::Triangles) {
            pipeline->setDepthBias(kFillConstantBias);
            pipeline->setSlopeScaledDepthBias(kFillSlopeBias);
        } else {
            // Antialiased coverage goes out as alpha.
            QRhiGraphicsPipeline::TargetBlend blend;
            blend.enable = true;
            blend.srcColor = QRhiGraphicsPipeline::SrcAlpha;
            blend.dstColor = QRhiGraphicsPipeline::OneMinusSrcAlpha;
            blend.srcAlpha = QRhiGraphicsPipeline::One;
            blend.dstAlpha = QRhiGraphicsPipeline::OneMinusSrcAlpha;
            pipeline->setTargetBlends({blend});
        }
        pipeline->setShaderResourceBindings(program == Program::CloudPoints ? r.cloudBindings.get()
                                                                           : r.sceneBindings.get());
        pipeline->setRenderPassDescriptor(pass);
        if (!pipeline->create()) {
            r.releaseGpu();
            return makeError(ErrorCode::RenderingFailure,
                             std::string("the ") + toString(program) + " (" +
                                 toString(r.expansion) + ") pipeline could not be created (" +
                                 shaders.describe() +
                                 "; the shader compiler's message is in the Qt log)");
        }
        r.pipelines[indexOf(program)] = std::move(pipeline);
    }
    r.rhi = rhi;
    r.sceneDirty = true;
    r.cloudDirty = !r.cloudData.points.empty();
    return {};
}

void GpuRenderer::setDrawList(const katana::render::DrawList& list)
{
    packDrawList(list, resources_->scene);
    resources_->sceneDirty = true;
}

void GpuRenderer::setScene(GpuSceneData scene)
{
    resources_->scene = std::move(scene);
    resources_->sceneDirty = true;
}

void GpuRenderer::setPointCloud(PointCloudData cloud)
{
    resources_->cloudData = std::move(cloud);
    resources_->cloudDirty = true;
}

void GpuRenderer::clearPointCloud()
{
    resources_->cloudData.clear();
    resources_->cloudDirty = true;
}

namespace {

// Uploads `bytes` into `buffer`, replacing it by a larger one when it has
// outgrown it. Static, not Immutable: an edited scene of the same size or
// smaller reuses the allocation. Returns false when the GPU refused it.
[[nodiscard]] bool upload(QRhi& rhi, QRhiResourceUpdateBatch& batch,
                          std::unique_ptr<QRhiBuffer>& buffer, QRhiBuffer::UsageFlags usage,
                          const void* data, std::size_t bytes)
{
    if (bytes == 0) {
        return true; // nothing to draw; the draw call is skipped too
    }
    if (!buffer || buffer->size() < bytes) {
        buffer.reset(rhi.newBuffer(QRhiBuffer::Static, usage, static_cast<quint32>(bytes)));
        if (!buffer->create()) {
            buffer.reset();
            return false;
        }
    }
    batch.uploadStaticBuffer(buffer.get(), 0, static_cast<quint32>(bytes), data);
    return true;
}

[[nodiscard]] FrameUniforms uniformsFor(const katana::render::Camera& camera,
                                        const katana::math::Vec3& origin,
                                        const FrameSettings& settings, const QRhi& rhi, int width,
                                        int height)
{
    ProjectionOptions projection;
    projection.infiniteFar = settings.infiniteFar;
    projection.clip.yUpInNdc = rhi.isYUpInNDC();
    projection.clip.depthZeroToOne = rhi.isClipDepthZeroToOne();

    FrameUniforms u;
    u.mvp = toFloatColumnMajor(relativeViewProjection(camera, origin, projection));
    u.viewport = float4(width, height, 1.0 / width, 1.0 / height);

    katana::math::Vec3 light = settings.lightDirection;
    const double lightLength = light.length();
    const bool lit = settings.lighting == LightingMode::PerPixel && std::isfinite(lightLength) &&
                     lightLength > 1.0e-9;
    if (lit) {
        light = light / lightLength;
    }
    const double ambient = std::isfinite(settings.ambient) ? std::clamp(settings.ambient, 0.0, 1.0)
                                                           : 0.35;
    u.light = float4(light.x, light.y, light.z, ambient);

    const katana::math::Vec3 eye = camera.eye() - origin; // in double, then rounded
    const bool perspective = camera.projection() == katana::render::Projection::Perspective;
    u.eyeRel = float4(eye.x, eye.y, eye.z, perspective ? 1.0 : 0.0);
    const katana::math::Vec3 forward = camera.forward();
    u.forwardMode = float4(forward.x, forward.y, forward.z, lit ? 1.0 : 0.0);

    const double factor = std::isfinite(settings.verticalExaggeration) &&
                                  settings.verticalExaggeration > 0.0
                              ? settings.verticalExaggeration
                              : 1.0;
    const double datum =
        std::isfinite(settings.exaggerationDatum) ? settings.exaggerationDatum : 0.0;
    const double ratio =
        std::isfinite(settings.pixelRatio) && settings.pixelRatio > 0.0 ? settings.pixelRatio : 1.0;
    const double cloudSize = std::isfinite(settings.cloudPointSize) && settings.cloudPointSize > 0.0
                                 ? settings.cloudPointSize
                                 : 2.0;
    u.params = float4(factor, datum - origin.z, ratio, cloudSize);
    return u;
}

} // namespace

katana::core::Result<GpuFrameStats> GpuRenderer::render(QRhiCommandBuffer* commands,
                                                        QRhiRenderTarget* target,
                                                        const katana::render::Camera& camera,
                                                        const FrameSettings& settings)
{
    Resources& r = *resources_;
    if (r.rhi == nullptr) {
        return makeError(ErrorCode::InvalidState, "the GPU renderer is not initialised");
    }
    if (commands == nullptr || target == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "no command buffer or render target");
    }
    const QSize size = target->pixelSize();
    if (size.width() <= 0 || size.height() <= 0) {
        return makeError(ErrorCode::InvalidArgument, "render target is empty");
    }
    if (camera.viewportWidth() != size.width() || camera.viewportHeight() != size.height()) {
        return makeError(ErrorCode::InvalidArgument,
                         "camera viewport " + std::to_string(camera.viewportWidth()) + "x" +
                             std::to_string(camera.viewportHeight()) +
                             " does not match the render target " +
                             std::to_string(size.width()) + "x" + std::to_string(size.height()));
    }

    GpuFrameStats stats;
    QRhiResourceUpdateBatch* batch = r.rhi->nextResourceUpdateBatch();
    if (r.sceneDirty) {
        const GpuSceneData& s = r.scene;
        const bool ok =
            upload(*r.rhi, *batch, r.vertices, QRhiBuffer::VertexBuffer, s.vertices.data(),
                   s.vertices.size() * sizeof(GpuVertex)) &&
            upload(*r.rhi, *batch, r.indices, QRhiBuffer::IndexBuffer, s.triangleIndices.data(),
                   s.triangleIndices.size() * sizeof(std::uint32_t)) &&
            upload(*r.rhi, *batch, r.lines, QRhiBuffer::VertexBuffer, s.lines.data(),
                   s.lines.size() * sizeof(GpuLine)) &&
            upload(*r.rhi, *batch, r.points, QRhiBuffer::VertexBuffer, s.points.data(),
                   s.points.size() * sizeof(GpuPoint));
        if (!ok) {
            batch->release();
            return makeError(ErrorCode::RenderingFailure,
                             "the GPU refused a scene buffer of " +
                                 std::to_string(s.byteSize()) + " bytes");
        }
        r.sceneDirty = false;
        ++r.uploads;
        stats.uploaded = true;
        stats.uploadedBytes += s.byteSize();
    }
    if (r.cloudDirty) {
        const std::size_t bytes = r.cloudData.points.size() * sizeof(GpuCloudPoint);
        if (!upload(*r.rhi, *batch, r.cloud, QRhiBuffer::VertexBuffer, r.cloudData.points.data(),
                    bytes)) {
            batch->release();
            return makeError(ErrorCode::RenderingFailure,
                             "the GPU refused a point cloud of " + std::to_string(bytes) +
                                 " bytes");
        }
        r.cloudDirty = false;
        stats.uploadedBytes += bytes;
    }

    const FrameUniforms sceneUniforms =
        uniformsFor(camera, r.scene.origin, settings, *r.rhi, size.width(), size.height());
    batch->updateDynamicBuffer(r.sceneUniforms.get(), 0, sizeof(FrameUniforms), &sceneUniforms);
    const bool drawCloud = !r.cloudData.points.empty() && r.cloud;
    if (drawCloud) {
        const FrameUniforms cloudUniforms =
            uniformsFor(camera, r.cloudData.origin, settings, *r.rhi, size.width(), size.height());
        batch->updateDynamicBuffer(r.cloudUniforms.get(), 0, sizeof(FrameUniforms),
                                   &cloudUniforms);
    }

    const katana::render::Rgba bg = settings.background;
    const QColor clear(katana::render::redOf(bg), katana::render::greenOf(bg),
                       katana::render::blueOf(bg), katana::render::alphaOf(bg));
    // Depth clears to 0: the FAR end of a reversed-Z buffer.
    commands->beginPass(target, clear, {0.0f, 0}, batch);
    commands->setViewport(QRhiViewport(0.0f, 0.0f, static_cast<float>(size.width()),
                                       static_cast<float>(size.height())));

    const GpuSceneData& s = r.scene;
    if (!s.triangleIndices.empty() && r.vertices && r.indices) {
        commands->setGraphicsPipeline(r.pipelines[indexOf(Program::Triangles)].get());
        commands->setShaderResources();
        const QRhiCommandBuffer::VertexInput input(r.vertices.get(), 0);
        commands->setVertexInput(0, 1, &input, r.indices.get(), 0, QRhiCommandBuffer::IndexUInt32);
        commands->drawIndexed(static_cast<quint32>(s.triangleIndices.size()));
        stats.triangles = s.triangleCount();
    }
    // Lines and points after the fills they lie on, as the software path
    // orders them, and blended over them.
    // Through the geometry shader every line is two vertices and every point
    // one; instanced, each is one instance of a six-vertex quad.
    const bool instanced = r.expansion == Expansion::Instanced;
    const auto drawQuads = [&](Program program, QRhiBuffer* buffer, std::size_t count,
                               quint32 verticesEach) {
        commands->setGraphicsPipeline(r.pipelines[indexOf(program)].get());
        commands->setShaderResources();
        const QRhiCommandBuffer::VertexInput input(buffer, 0);
        commands->setVertexInput(0, 1, &input);
        if (instanced) {
            commands->draw(6, static_cast<quint32>(count));
        } else {
            commands->draw(static_cast<quint32>(count) * verticesEach);
        }
    };
    if (!s.lines.empty() && r.lines) {
        drawQuads(Program::Lines, r.lines.get(), s.lines.size(), 2);
        stats.lines = s.lines.size();
    }
    if (!s.points.empty() && r.points) {
        drawQuads(Program::Points, r.points.get(), s.points.size(), 1);
        stats.points = s.points.size();
    }
    if (drawCloud) {
        drawQuads(Program::CloudPoints, r.cloud.get(), r.cloudData.points.size(), 1);
        stats.cloudPoints = r.cloudData.points.size();
    }
    commands->endPass();
    return stats;
}

} // namespace katana::qt::gpu
