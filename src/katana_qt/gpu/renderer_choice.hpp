#pragma once

// Which renderer a 3D view should use: the GPU one or the software rasteriser
// (docs/gpu.md, "Fallback").
//
// The software rasteriser is not going away. It is the only renderer that
// works under Qt's offscreen platform, where every test and every headless
// screenshot runs (QRhiWidget reports renderFailed there for every graphics
// API; GpuSceneView's offscreen test shows it); it is the bit-exact reference the GPU is tested
// against; and it is what a machine whose GPU or driver misbehaves falls back
// to. So a view that can host both asks this function, once when it is built
// and again whenever something below changes, and follows the answer.
//
// THE RULES, first match wins:
//
//   1. The GPU renderer was not built (KATANA_GPU off)       -> software
//   2. KATANA_RENDERER=software in the environment          -> software
//   3. The user's setting asks for software                  -> software
//   4. The GPU view already failed in this session          -> software
//      (QRhiWidget::renderFailed, or GpuRenderer::initialise failing): a
//      renderer that failed once is not retried behind the user's back.
//   5. The platform cannot show the build's GPU view         -> software
//      (offscreen and minimal - headless runs - and, for the Direct3D 11
//      build, anything but the windows platform; for the Vulkan build,
//      anything but xcb and wayland)
//   6. Otherwise                                             -> GPU
//      (KATANA_RENDERER=gpu cannot make rules 1-5 go away, and the GPU is
//      already the default. What it does add is permission to draw on a
//      software device - WARP, lavapipe - which the view otherwise refuses:
//      RendererDecision::softwareDeviceAllowed.)
//
// Any other value of KATANA_RENDERER is ignored and reported in the reason,
// so a typo is visible in the log rather than silently meaning "gpu".
//
// The decision is a PURE function of RendererEnvironment, so every rule is
// unit-tested without a GPU, a window or an environment variable;
// currentRendererEnvironment() is the one place that reads the real ones.

#include <string>

namespace katana::qt::gpu {

enum class RendererKind { Gpu, Software };

// The graphics API the GPU renderer was built for (gpu/CMakeLists.txt): it
// decides which platforms can show it.
enum class GpuBackend { Direct3D11, Vulkan };

[[nodiscard]] const char* toString(GpuBackend backend);

[[nodiscard]] const char* toString(RendererKind kind);

struct RendererEnvironment {
    // False when the GPU renderer is not compiled in.
    bool gpuBuilt = true;
    GpuBackend backend = GpuBackend::Direct3D11;
    // QGuiApplication::platformName(): "windows", "offscreen", "minimal", ...
    std::string platformName;
    // The value of KATANA_RENDERER, empty when unset.
    std::string rendererOverride;
    // The user's own choice (a preference), true to force the software path.
    bool userPrefersSoftware = false;
    // A GPU view has failed to render in this session.
    bool gpuFailedThisSession = false;
};

struct RendererDecision {
    RendererKind kind = RendererKind::Software;
    // One sentence for the log and the status line: why this renderer.
    std::string reason;
    // The GPU view may draw on a software device (WARP, lavapipe) - only when
    // KATANA_RENDERER=gpu asks for the GPU path whatever it runs on. Without
    // that such a device is refused and the rasteriser draws instead: it was
    // written to do the CPU's work well, and a software GPU device was not.
    bool softwareDeviceAllowed = false;
};

[[nodiscard]] RendererDecision chooseRenderer(const RendererEnvironment& environment);

// The environment as it is now: the backend this copy was built for, the
// platform from the running QGuiApplication (empty if there is none),
// KATANA_RENDERER from the process environment. The two flags are the
// caller's to know.
[[nodiscard]] RendererEnvironment currentRendererEnvironment(bool userPrefersSoftware,
                                                             bool gpuFailedThisSession);

} // namespace katana::qt::gpu
