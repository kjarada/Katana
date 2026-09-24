#pragma once

// What the GPU tests share: making a device or saying why there is none,
// rendering the same scene on the software rasteriser, and comparing the two
// images the way a GPU frame can be compared - by coverage and by colour,
// within a tolerance, never bit for bit.

#include <cstddef>
#include <memory>
#include <string>
#include <vector>

#include "gpu/offscreen_gpu.hpp"
#include "katana/render/camera.hpp"
#include "katana/render/draw_list.hpp"

namespace katana::qt::gpu::testing {

using Image = std::vector<katana::render::Rgba>;

// An offscreen Direct3D 11 target, or null with `whyNot` filled in. Tests
// GTEST_SKIP on null: no device is not a failure of the renderer.
[[nodiscard]] std::unique_ptr<OffscreenGpu> makeGpu(int width, int height, GpuDevice device,
                                                    std::string& whyNot,
                                                    const OffscreenOptions& base = {});

// The same list through the software rasteriser, single-threaded, at the
// camera's viewport size.
[[nodiscard]] Image cpuRender(const katana::render::DrawList& list,
                              const katana::render::Camera& camera,
                              katana::render::Rgba background);

// r + g + b.
[[nodiscard]] int brightness(katana::render::Rgba color);

struct Comparison {
    std::size_t coveredA = 0;
    std::size_t coveredB = 0;
    // Pixels covered in one image and not the other.
    std::size_t coverageMismatches = 0;
    // Pixels whose 3x3 neighbourhood is covered in BOTH images and holds no
    // colour edge in the first (no neighbour more than 16 levels from the
    // centre in any channel): away from every outline and from every border
    // between two primitives, where multisampling blends and the two fill
    // rules round differently.
    std::size_t interiorPixels = 0;
    // The largest difference in any one channel over the interior pixels.
    int maxInteriorDifference = 0;
    // Pixels (anywhere) differing by more than 2 in some channel.
    std::size_t differingPixels = 0;
};

// A pixel is "covered" when it is at least `threshold` brighter than the
// background (brightness above).
[[nodiscard]] Comparison compare(const Image& a, const Image& b, int width, int height,
                                 katana::render::Rgba background, int threshold);

// With KATANA_GPU_TEST_IMAGES set to a directory, saves `image` there as
// <name>.png so a person can LOOK at what the test compared. A no-op otherwise.
void saveForLooking(const std::string& name, const Image& image, int width, int height);

// The comparison in one line, for failure messages.
[[nodiscard]] std::string describe(const Comparison& comparison);

} // namespace katana::qt::gpu::testing
