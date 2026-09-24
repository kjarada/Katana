#include "gpu_test_support.hpp"

#include <algorithm>
#include <cstddef>
#include <cstdio>
#include <cstdlib>

#include <QDir>
#include <QImage>
#include <QString>

#include "katana/core/task_pool.hpp"
#include "katana/render/rasterizer.hpp"

namespace katana::qt::gpu::testing {

std::string describe(const Comparison& c);

using katana::render::Rgba;

std::unique_ptr<OffscreenGpu> makeGpu(int width, int height, GpuDevice device, std::string& whyNot,
                                      const OffscreenOptions& base)
{
    OffscreenOptions options = base;
    options.device = device;
    auto created = OffscreenGpu::create(width, height, options);
    if (!created) {
        whyNot = created.error().describe();
        return nullptr;
    }
    return std::move(created.value());
}

Image cpuRender(const katana::render::DrawList& list, const katana::render::Camera& camera,
                Rgba background)
{
    auto target = katana::render::Framebuffer::create(camera.viewportWidth(),
                                                      camera.viewportHeight());
    if (!target) {
        return {};
    }
    katana::core::TaskPool pool(0);
    katana::render::RenderOptions options;
    options.background = background;
    options.pool = &pool;
    katana::render::Rasterizer rasterizer;
    if (!rasterizer.render(list, camera, *target, options)) {
        return {};
    }
    return target->color();
}

int brightness(Rgba color)
{
    return katana::render::redOf(color) + katana::render::greenOf(color) +
           katana::render::blueOf(color);
}

Comparison compare(const Image& a, const Image& b, int width, int height, Rgba background,
                   int threshold)
{
    Comparison result;
    const std::size_t count = static_cast<std::size_t>(width) * static_cast<std::size_t>(height);
    if (a.size() != count || b.size() != count) {
        result.coverageMismatches = count; // nothing to compare is as bad as it gets
        return result;
    }
    const int ground = brightness(background);
    const auto coveredIn = [&](const Image& image, int x, int y) {
        return brightness(image[static_cast<std::size_t>(y) * static_cast<std::size_t>(width) +
                                static_cast<std::size_t>(x)]) -
                   ground >=
               threshold;
    };
    const auto channelDifference = [](Rgba p, Rgba q) {
        const int dr = std::abs(katana::render::redOf(p) - katana::render::redOf(q));
        const int dg = std::abs(katana::render::greenOf(p) - katana::render::greenOf(q));
        const int db = std::abs(katana::render::blueOf(p) - katana::render::blueOf(q));
        return std::max({dr, dg, db});
    };
    for (int y = 0; y < height; ++y) {
        for (int x = 0; x < width; ++x) {
            const std::size_t i =
                static_cast<std::size_t>(y) * static_cast<std::size_t>(width) +
                static_cast<std::size_t>(x);
            const bool inA = coveredIn(a, x, y);
            const bool inB = coveredIn(b, x, y);
            result.coveredA += inA ? 1 : 0;
            result.coveredB += inB ? 1 : 0;
            if (inA != inB) {
                ++result.coverageMismatches;
            }
            const int difference = channelDifference(a[i], b[i]);
            if (difference > 2) {
                ++result.differingPixels;
            }
            bool interior = x > 0 && y > 0 && x + 1 < width && y + 1 < height;
            for (int dy = -1; interior && dy <= 1; ++dy) {
                for (int dx = -1; interior && dx <= 1; ++dx) {
                    const std::size_t j = i + static_cast<std::size_t>(
                                                  static_cast<std::ptrdiff_t>(dy) * width + dx);
                    interior = coveredIn(a, x + dx, y + dy) && coveredIn(b, x + dx, y + dy) &&
                               channelDifference(a[i], a[j]) <= 16;
                }
            }
            if (interior) {
                ++result.interiorPixels;
                result.maxInteriorDifference = std::max(result.maxInteriorDifference, difference);
            }
        }
    }
    if (!qEnvironmentVariable("KATANA_GPU_TEST_IMAGES").isEmpty()) {
        std::printf("  compared: %s\n", describe(result).c_str());
    }
    return result;
}

std::string describe(const Comparison& c)
{
    return "covered " + std::to_string(c.coveredA) + " / " + std::to_string(c.coveredB) +
           ", coverage mismatches " + std::to_string(c.coverageMismatches) + ", interior " +
           std::to_string(c.interiorPixels) + " (max difference " +
           std::to_string(c.maxInteriorDifference) + "), differing " +
           std::to_string(c.differingPixels);
}

void saveForLooking(const std::string& name, const Image& image, int width, int height)
{
    const QString directory = qEnvironmentVariable("KATANA_GPU_TEST_IMAGES");
    if (directory.isEmpty() ||
        image.size() != static_cast<std::size_t>(width) * static_cast<std::size_t>(height)) {
        return;
    }
    QDir().mkpath(directory);
    std::printf("  saved %s.png\n", name.c_str());
    const QImage view(reinterpret_cast<const uchar*>(image.data()), width, height,
                      width * static_cast<int>(sizeof(Rgba)), QImage::Format_ARGB32);
    view.copy().save(QDir(directory).filePath(QString::fromStdString(name) + ".png"));
}

} // namespace katana::qt::gpu::testing
