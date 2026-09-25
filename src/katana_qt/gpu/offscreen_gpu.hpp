#pragma once

// The GPU renderer drawing into a texture instead of a widget (docs/gpu.md,
// "Testing").
//
// QRhiWidget cannot render under Qt's offscreen platform - every Katana test
// and headless screenshot runs there - but QRhi itself can render into a
// texture with no window: on Windows through Direct3D 11, on the real GPU or
// on WARP, Windows' software device; on Linux through Vulkan, on the real GPU
// or on Mesa's lavapipe (tests/gpu runs on both). Vulkan does need a platform
// that can make a Vulkan instance, which Qt's offscreen one cannot, so the
// Linux tests run on xcb under Xvfb (tests/gpu/CMakeLists.txt). This class is
// a QRhi of its own, a multisampled colour buffer resolved into a texture, a
// 32-bit float depth texture, and a synchronous frame with an optional
// read-back and the GPU's own timestamp for the frame. The tests compare its
// pixels with the software rasteriser's; the benchmark times it.

#include <cstddef>
#include <memory>
#include <string>
#include <vector>

#include "gpu_renderer.hpp"
#include "katana/core/error.hpp"
#include "katana/render/framebuffer.hpp"

class QRhi;

namespace katana::qt::gpu {

enum class GpuDevice {
    Hardware, // the machine's GPU
    // The platform's software device: WARP on Windows, which every install
    // has; lavapipe on Linux, Mesa's Vulkan on the CPU (mesa-vulkan-drivers).
    // Slow, but a GPU-less machine can still run the tests on it.
    Software,
};

[[nodiscard]] const char* toString(GpuDevice device);

struct OffscreenOptions {
    GpuDevice device = GpuDevice::Hardware;
    // 4 unless the device cannot; 1 turns multisampling off.
    int sampleCount = 4;
    // Ask the GPU to time each frame (QRhi::EnableTimestamps).
    bool timestamps = false;
    // How lines and points are widened; the renderer falls back to Instanced
    // on a device without a geometry stage (GpuRenderer::initialise).
    Expansion expansion = Expansion::GeometryShader;
    // Where the shaders come from; null for defaultShaders(). Must
    // outlive create().
    const ShaderLibrary* shaders = nullptr;
};

class OffscreenGpu {
  public:
    // Fails with Unsupported when there is no device of the kind asked for -
    // the tests SKIP on that, rather than fail - and with
    // RenderingFailure when the device exists but a target or pipeline
    // cannot be made.
    [[nodiscard]] static katana::core::Result<std::unique_ptr<OffscreenGpu>>
    create(int width, int height, const OffscreenOptions& options = {});

    ~OffscreenGpu();
    OffscreenGpu(const OffscreenGpu&) = delete;
    OffscreenGpu& operator=(const OffscreenGpu&) = delete;

    [[nodiscard]] GpuRenderer& renderer();
    [[nodiscard]] QRhi* rhi();
    [[nodiscard]] int width() const;
    [[nodiscard]] int height() const;
    [[nodiscard]] int sampleCount() const;
    // The adapter's name, for test and benchmark output.
    [[nodiscard]] std::string deviceName() const;

    // Draws one frame through `camera` and waits for the GPU to finish it.
    // With `pixels`, the resolved colour is read back into it as 0xAARRGGBB,
    // row-major, top row first - render::Framebuffer's layout - so the two
    // paths' images compare directly.
    [[nodiscard]] katana::core::Result<GpuFrameStats>
    renderFrame(const katana::render::Camera& camera, const FrameSettings& settings = {},
                std::vector<katana::render::Rgba>* pixels = nullptr);

    // What the GPU itself measured for the most recent frame whose timing has
    // come back, in milliseconds; 0 before any has, or without timestamps.
    [[nodiscard]] double lastGpuMilliseconds() const;

  private:
    OffscreenGpu();
    struct Parts;
    std::unique_ptr<Parts> parts_;
};

} // namespace katana::qt::gpu
