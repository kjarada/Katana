#include "offscreen_gpu.hpp"

#include <algorithm>
#include <cstring>

#include <QGuiApplication>
#include <rhi/qrhi.h>
#if defined(KATANA_GPU_VULKAN)
#include <QVulkanInstance>
#endif

namespace katana::qt::gpu {

using katana::core::ErrorCode;
using katana::core::makeError;

const char* toString(GpuDevice device)
{
    switch (device) {
    case GpuDevice::Hardware:
        return "hardware";
    case GpuDevice::Software:
        return "software";
    }
    return "unknown";
}

struct OffscreenGpu::Parts {
#if defined(KATANA_GPU_VULKAN)
    // Before the QRhi, so it is destroyed after it: the device is made from it.
    std::unique_ptr<QVulkanInstance> vulkan;
#endif
    // Declared before the resources so it is destroyed after them: every
    // resource below belongs to it.
    std::unique_ptr<QRhi> rhi;
    int width = 0;
    int height = 0;
    int sampleCount = 1;
    std::unique_ptr<QRhiTexture> resolved;     // what is read back
    std::unique_ptr<QRhiRenderBuffer> msaa;    // what is drawn into, when multisampled
    std::unique_ptr<QRhiTexture> depth;        // D32F, reversed Z
    std::unique_ptr<QRhiTextureRenderTarget> target;
    std::unique_ptr<QRhiRenderPassDescriptor> pass;
    // Declared after the resources it draws with, so it is released before them.
    GpuRenderer renderer;
    double lastGpuMs = 0.0;

    ~Parts()
    {
        // The renderer's buffers and pipelines belong to `rhi` too; release
        // them explicitly while it still exists.
        renderer.releaseResources();
    }
};

OffscreenGpu::OffscreenGpu() : parts_(std::make_unique<Parts>()) {}

OffscreenGpu::~OffscreenGpu() = default;

katana::core::Result<std::unique_ptr<OffscreenGpu>>
OffscreenGpu::create(int width, int height, const OffscreenOptions& options)
{
    if (width <= 0 || height <= 0 || width > 16384 || height > 16384) {
        return makeError(ErrorCode::InvalidArgument, "offscreen target size out of range");
    }
    std::unique_ptr<OffscreenGpu> gpu(new OffscreenGpu());
    Parts& p = *gpu->parts_;
    p.width = width;
    p.height = height;

    QRhi::Flags flags;
    if (options.device == GpuDevice::Software) {
        flags |= QRhi::PreferSoftwareRenderer;
    }
    if (options.timestamps) {
        flags |= QRhi::EnableTimestamps;
    }
#if defined(KATANA_GPU_D3D11)
    const char* const api = "Direct3D 11";
    QRhiD3D11InitParams params;
    p.rhi.reset(QRhi::create(QRhi::D3D11, &params, flags));
#elif defined(KATANA_GPU_VULKAN)
    const char* const api = "Vulkan";
    // A platform without Vulkan - Qt's offscreen one, where ordinary Katana
    // tests run - cannot make an instance; that is no device, not a failure.
    p.vulkan = std::make_unique<QVulkanInstance>();
    p.vulkan->setExtensions(QRhiVulkanInitParams::preferredInstanceExtensions());
    if (!p.vulkan->create()) {
        return makeError(ErrorCode::Unsupported,
                         "no Vulkan instance on the '" +
                             QGuiApplication::platformName().toStdString() +
                             "' platform (the GPU tests run on xcb, under Xvfb when there is "
                             "no display)");
    }
    QRhiVulkanInitParams params;
    params.inst = p.vulkan.get();
    p.rhi.reset(QRhi::create(QRhi::Vulkan, &params, flags));
#endif
    if (!p.rhi) {
        return makeError(ErrorCode::Unsupported,
                         std::string("no ") + api + " device (" + toString(options.device) + ")");
    }
    const bool cpuDevice = p.rhi->driverInfo().deviceType == QRhiDriverInfo::CpuDevice;
    if (options.device == GpuDevice::Hardware && cpuDevice) {
        // Asked for the GPU and given a software device: say so rather than
        // measure the wrong thing.
        return makeError(ErrorCode::Unsupported,
                         std::string("only a software ") + api + " device is available");
    }
    if (options.device == GpuDevice::Software && !cpuDevice) {
        // PreferSoftwareRenderer is a preference: without lavapipe installed
        // Vulkan hands over the GPU, and the software run would repeat the
        // hardware one under the other name.
        return makeError(ErrorCode::Unsupported,
                         std::string("no software ") + api + " device (" +
                             p.rhi->driverInfo().deviceName.toStdString() + " is hardware)");
    }

    const QList<int> counts = p.rhi->supportedSampleCounts();
    p.sampleCount = counts.contains(options.sampleCount) ? std::max(options.sampleCount, 1) : 1;
    if (!p.rhi->isTextureFormatSupported(QRhiTexture::D32F)) {
        return makeError(ErrorCode::Unsupported, "the device has no 32-bit float depth format");
    }

    const QSize size(width, height);
    p.resolved.reset(p.rhi->newTexture(QRhiTexture::RGBA8, size, 1,
                                       QRhiTexture::RenderTarget |
                                           QRhiTexture::UsedAsTransferSource));
    if (!p.resolved->create()) {
        return makeError(ErrorCode::RenderingFailure, "could not create the colour texture");
    }
    QRhiColorAttachment color;
    if (p.sampleCount > 1) {
        p.msaa.reset(p.rhi->newRenderBuffer(QRhiRenderBuffer::Color, size, p.sampleCount, {},
                                            QRhiTexture::RGBA8));
        if (!p.msaa->create()) {
            return makeError(ErrorCode::RenderingFailure,
                             "could not create the multisampled colour buffer");
        }
        color = QRhiColorAttachment(p.msaa.get());
        color.setResolveTexture(p.resolved.get());
    } else {
        color = QRhiColorAttachment(p.resolved.get());
    }
    p.depth.reset(p.rhi->newTexture(QRhiTexture::D32F, size, p.sampleCount,
                                    QRhiTexture::RenderTarget));
    if (!p.depth->create()) {
        return makeError(ErrorCode::RenderingFailure, "could not create the depth texture");
    }
    QRhiTextureRenderTargetDescription description(color);
    description.setDepthTexture(p.depth.get());
    p.target.reset(p.rhi->newTextureRenderTarget(description));
    p.pass.reset(p.target->newCompatibleRenderPassDescriptor());
    p.target->setRenderPassDescriptor(p.pass.get());
    if (!p.target->create()) {
        return makeError(ErrorCode::RenderingFailure, "could not create the render target");
    }
    if (auto status = p.renderer.initialise(p.rhi.get(), p.pass.get(), p.sampleCount,
                                             options.shaders != nullptr
                                                 ? *options.shaders
                                                 : defaultShaders(),
                                             options.expansion);
        !status) {
        return status.error();
    }
    return gpu;
}

GpuRenderer& OffscreenGpu::renderer() { return parts_->renderer; }

QRhi* OffscreenGpu::rhi() { return parts_->rhi.get(); }

int OffscreenGpu::width() const { return parts_->width; }

int OffscreenGpu::height() const { return parts_->height; }

int OffscreenGpu::sampleCount() const { return parts_->sampleCount; }

std::string OffscreenGpu::deviceName() const
{
    return parts_->rhi->driverInfo().deviceName.toStdString();
}

double OffscreenGpu::lastGpuMilliseconds() const { return parts_->lastGpuMs; }

katana::core::Result<GpuFrameStats> OffscreenGpu::renderFrame(const katana::render::Camera& camera,
                                                              const FrameSettings& settings,
                                                              std::vector<katana::render::Rgba>* pixels)
{
    Parts& p = *parts_;
    QRhiCommandBuffer* commands = nullptr;
    if (p.rhi->beginOffscreenFrame(&commands) != QRhi::FrameOpSuccess) {
        return makeError(ErrorCode::RenderingFailure, "the GPU could not begin a frame");
    }
    auto stats = p.renderer.render(commands, p.target.get(), camera, settings);
    QRhiReadbackResult readback;
    if (stats && pixels != nullptr) {
        QRhiResourceUpdateBatch* batch = p.rhi->nextResourceUpdateBatch();
        batch->readBackTexture(QRhiReadbackDescription(p.resolved.get()), &readback);
        commands->resourceUpdate(batch);
    }
    // Synchronous: returns once the GPU has finished, so the read-back is filled.
    if (p.rhi->endOffscreenFrame() != QRhi::FrameOpSuccess) {
        return makeError(ErrorCode::RenderingFailure, "the GPU could not finish the frame");
    }
    if (!stats) {
        return stats;
    }
    p.lastGpuMs = commands->lastCompletedGpuTime() * 1000.0;

    if (pixels != nullptr) {
        const std::size_t count =
            static_cast<std::size_t>(p.width) * static_cast<std::size_t>(p.height);
        if (static_cast<std::size_t>(readback.data.size()) < count * 4) {
            return makeError(ErrorCode::RenderingFailure, "the read-back came back short");
        }
        pixels->resize(count);
        const auto* bytes = reinterpret_cast<const unsigned char*>(readback.data.constData());
        // RGBA8 in memory order R, G, B, A -> 0xAARRGGBB.
        for (std::size_t i = 0; i < count; ++i) {
            const unsigned char* px = bytes + i * 4;
            (*pixels)[i] = katana::render::rgba(px[0], px[1], px[2], px[3]);
        }
    }
    return stats;
}

} // namespace katana::qt::gpu
