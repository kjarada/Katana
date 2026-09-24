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
    if (platform != "windows") {
        const std::string shown = platform.empty() ? std::string("none") : platform;
        return {RendererKind::Software,
                "the '" + shown + "' platform cannot show the Direct3D 11 view" + ignored};
    }
    return {RendererKind::Gpu, "Direct3D 11 on the windows platform" + ignored};
}

RendererEnvironment currentRendererEnvironment(bool userPrefersSoftware, bool gpuFailedThisSession)
{
    RendererEnvironment environment;
    environment.gpuBuilt = true; // this file is only compiled into the GPU module
    if (qGuiApp != nullptr) {
        environment.platformName = QGuiApplication::platformName().toStdString();
    }
    environment.rendererOverride = qEnvironmentVariable("KATANA_RENDERER").trimmed().toStdString();
    environment.userPrefersSoftware = userPrefersSoftware;
    environment.gpuFailedThisSession = gpuFailedThisSession;
    return environment;
}

} // namespace katana::qt::gpu
