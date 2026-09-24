// Which renderer a 3D view uses (gpu/renderer_choice.hpp): every rule, in
// order, as a pure function - no GPU, window or environment variable.

#include <gtest/gtest.h>

#include "gpu/renderer_choice.hpp"

using katana::qt::gpu::chooseRenderer;
using katana::qt::gpu::RendererEnvironment;
using katana::qt::gpu::RendererKind;

namespace {

// Everything in favour of the GPU; each test takes one thing away.
RendererEnvironment desktop()
{
    RendererEnvironment environment;
    environment.gpuBuilt = true;
    environment.platformName = "windows";
    return environment;
}

} // namespace

TEST(RendererChoice, TheWindowsDesktopWithNothingAgainstItGetsTheGpu)
{
    EXPECT_EQ(chooseRenderer(desktop()).kind, RendererKind::Gpu);
    RendererEnvironment asked = desktop();
    asked.rendererOverride = "gpu";
    EXPECT_EQ(chooseRenderer(asked).kind, RendererKind::Gpu);
}

TEST(RendererChoice, HeadlessPlatformsGetTheSoftwareRasteriser)
{
    for (const char* platform : {"offscreen", "minimal", "", "xcb"}) {
        RendererEnvironment environment = desktop();
        environment.platformName = platform;
        const auto decision = chooseRenderer(environment);
        EXPECT_EQ(decision.kind, RendererKind::Software) << platform;
        EXPECT_NE(decision.reason.find("platform"), std::string::npos) << decision.reason;
    }
}

TEST(RendererChoice, TheEnvironmentVariableForcesSoftwareInAnyCase)
{
    RendererEnvironment environment = desktop();
    environment.rendererOverride = "Software";
    const auto decision = chooseRenderer(environment);
    EXPECT_EQ(decision.kind, RendererKind::Software);
    EXPECT_NE(decision.reason.find("KATANA_RENDERER"), std::string::npos);
}

TEST(RendererChoice, AskingForTheGpuCannotOverrideAPlatformThatCannotShowIt)
{
    RendererEnvironment environment = desktop();
    environment.platformName = "offscreen";
    environment.rendererOverride = "gpu";
    EXPECT_EQ(chooseRenderer(environment).kind, RendererKind::Software);
}

TEST(RendererChoice, TheUsersSettingAndAnEarlierFailureEachChooseSoftware)
{
    RendererEnvironment preferred = desktop();
    preferred.userPrefersSoftware = true;
    EXPECT_EQ(chooseRenderer(preferred).kind, RendererKind::Software);

    RendererEnvironment failed = desktop();
    failed.gpuFailedThisSession = true;
    const auto decision = chooseRenderer(failed);
    EXPECT_EQ(decision.kind, RendererKind::Software);
    EXPECT_NE(decision.reason.find("failed"), std::string::npos);
}

TEST(RendererChoice, ACopyBuiltWithoutTheGpuRendererAlwaysUsesSoftware)
{
    RendererEnvironment environment = desktop();
    environment.gpuBuilt = false;
    environment.rendererOverride = "gpu";
    EXPECT_EQ(chooseRenderer(environment).kind, RendererKind::Software);
}

TEST(RendererChoice, AnUnknownOverrideIsIgnoredAndSaidSo)
{
    RendererEnvironment environment = desktop();
    environment.rendererOverride = "vulcan";
    const auto decision = chooseRenderer(environment);
    EXPECT_EQ(decision.kind, RendererKind::Gpu);
    EXPECT_NE(decision.reason.find("vulcan"), std::string::npos) << decision.reason;
}
