#include "renderer_choice.hpp"

#include <algorithm>
#include <cctype>

#include <QGuiApplication>
#include <QtGlobal>

namespace katana::qt::gpu {

namespace {

[[nodiscard]] std::string lowered(std::string text)
{
    std::transform(text.begin(), text.end(), text.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return text;
}

} // namespace

const char* toString(RendererKind kind)
{
    switch (kind) {
    case RendererKind::Gpu:
        return "GPU";
    case RendererKind::Software:
        return "software";
    }
    return "unknown";
}

const char* toString(GpuBackend backend)
{
    switch (backend) {
    case GpuBackend::Direct3D11:
        return "Direct3D 11";
    case GpuBackend::Vulkan:
        return "Vulkan";
    case GpuBackend::Metal:
        return "Metal";
    }
    return "unknown";
}

RendererDecision chooseRenderer(const RendererEnvironment& environment)
{
    const std::string requested = lowered(environment.rendererOverride);
    const bool requestKnown = requested.empty() || requested == "gpu" || requested == "software";
    // Appended to whatever reason wins, so a mistyped value is never silent.
    const std::string ignored =
        requestKnown ? std::string()
                      : " (KATANA_RENDERER=" + environment.rendererOverride +
                            " is not 'gpu' or 'software' and was ignored)";

    if (!environment.gpuBuilt) {
        return {RendererKind::Software, "the GPU renderer is not built into this copy" + ignored};
    }
    if (requested == "software") {
        return {RendererKind::Software, "KATANA_RENDERER=software asks for the software renderer"};
    }
    if (environment.userPrefersSoftware) {
        return {RendererKind::Software, "the software renderer is chosen in the settings" + ignored};
    }
    if (environment.gpuFailedThisSession) {
        return {RendererKind::Software,
                "the GPU renderer failed earlier in this session" + ignored};
    }
    const std::string platform = lowered(environment.platformName);
    bool shown = false;
    switch (environment.backend) {
    case GpuBackend::Direct3D11:
        shown = platform == "windows";
        break;
    case GpuBackend::Vulkan:
        shown = platform == "xcb" || platform == "wayland";
        break;
    case GpuBackend::Metal:
        shown = platform == "cocoa";
        break;
    }
    const std::string api = toString(environment.backend);
    if (!shown) {
        const std::string name = platform.empty() ? std::string("none") : platform;
        return {RendererKind::Software,
                "the '" + name + "' platform cannot show the " + api + " view" + ignored};
    }
    RendererDecision decision{RendererKind::Gpu, api + " on the " + platform + " platform" + ignored};
    decision.softwareDeviceAllowed = requested == "gpu";
    return decision;
}

RendererEnvironment currentRendererEnvironment(bool userPrefersSoftware, bool gpuFailedThisSession)
{
    RendererEnvironment environment;
    environment.gpuBuilt = true; // this file is only compiled into the GPU module
#if defined(KATANA_GPU_VULKAN)
    environment.backend = GpuBackend::Vulkan;
#elif defined(KATANA_GPU_METAL)
    environment.backend = GpuBackend::Metal;
#else
    environment.backend = GpuBackend::Direct3D11;
#endif
    if (qGuiApp != nullptr) {
        environment.platformName = QGuiApplication::platformName().toStdString();
    }
    environment.rendererOverride = qEnvironmentVariable("KATANA_RENDERER").trimmed().toStdString();
    environment.userPrefersSoftware = userPrefersSoftware;
    environment.gpuFailedThisSession = gpuFailedThisSession;
    return environment;
}

} // namespace katana::qt::gpu
