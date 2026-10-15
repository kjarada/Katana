// The surface styles' ramp table: every colour ramp a style may name, which
// the plan view, the 3D scene (its elevation and slope colours, scene.cpp) and
// the legends all colour through (docs/surface_display.md, "The ramps").
// RASTER SHADE and RASTER SLOPE keep a table of their own
// (interop/geo/colour_ramps.cpp) whose terrain, diverging, slope and grey are
// other colours; the doc says why the two are not one.

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <span>
#include <string_view>

#include "katana/cad/terrain/surface_colouring.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

namespace {

using katana::entity::Color;

constexpr Color rgb(std::uint8_t r, std::uint8_t g, std::uint8_t b) { return Color{r, g, b, 255}; }

struct Ramp {
    std::string_view name;
    SurfaceRampStops stops;
};

// Five stops each, evenly spaced (surface_colouring.hpp says why five).
constexpr std::array<Ramp, 6> kRamps{{
    // The 3D view's one elevation ramp since 2026-09-24 (docs/render.md,
    // "Surfaces"): deep blue, green, sand, brown, snow - a hypsometric tint
    // whose neighbouring bands stay distinguishable. FIRST, so it is the
    // default and what an unknown name falls back to, and unchanged to the
    // bit, so the scene's colours and its kernels' are what they were.
    {"terrain",
     {rgb(40, 70, 140), rgb(60, 160, 90), rgb(235, 220, 130), rgb(200, 130, 70),
      rgb(245, 245, 245)}},
    // ColorBrewer's YlOrBr, five classes (Cynthia Brewer, colorbrewer2.org):
    // pale yellow low to dark brown high, sequential and legible in grey.
    {"earth",
     {rgb(255, 255, 212), rgb(254, 217, 142), rgb(254, 153, 41), rgb(217, 95, 14),
      rgb(153, 52, 4)}},
    // ColorBrewer's RdBu, five classes, reversed so that low (cut, a
    // difference below zero) is blue and high (fill) red, white between: a
    // cut/fill map's convention.
    {"diverging",
     {rgb(5, 113, 176), rgb(146, 197, 222), rgb(247, 247, 247), rgb(244, 165, 130),
      rgb(202, 0, 32)}},
    // The 3D view's slope colours since 2026-09-24: green flat, yellow at
    // half, red at the end of the range - in the scene 1 : 1, where a
    // batter stops being trafficable. Its quarter stops are that line's
    // value there rounded (128); between stops it may now differ from the
    // old line by one level of 255, which no scene test held.
    {"slope",
     {rgb(0, 255, 38), rgb(128, 255, 38), rgb(255, 255, 38), rgb(255, 128, 38), rgb(255, 0, 38)}},
    // Dark to light in equal steps of 48: the order of the values survives
    // a monochrome plot.
    {"grey",
     {rgb(32, 32, 32), rgb(80, 80, 80), rgb(128, 128, 128), rgb(176, 176, 176),
      rgb(224, 224, 224)}},
    // ColorBrewer's Spectral, five classes, reversed: blue low to red high,
    // the most hues of any ramp here for telling many heights apart.
    {"spectral",
     {rgb(43, 131, 186), rgb(171, 221, 164), rgb(255, 255, 191), rgb(253, 174, 97),
      rgb(215, 25, 28)}},
}};

constexpr std::array<std::string_view, kRamps.size()> kRampNames = [] {
    std::array<std::string_view, kRamps.size()> names{};
    for (std::size_t i = 0; i < kRamps.size(); ++i) {
        names[i] = kRamps[i].name;
    }
    return names;
}();

const Ramp* findRamp(std::string_view name)
{
    const auto found = std::ranges::find_if(kRamps, [name](const Ramp& ramp) {
        return katana::core::equalsIgnoringCase(ramp.name, name);
    });
    return found == kRamps.end() ? nullptr : &*found;
}

} // namespace

std::span<const std::string_view> surfaceRampNames() { return kRampNames; }

const SurfaceRampStops& surfaceRampStops(std::string_view ramp)
{
    const Ramp* found = findRamp(ramp);
    return found != nullptr ? found->stops : kRamps.front().stops;
}

bool isSurfaceRamp(std::string_view ramp) { return findRamp(ramp) != nullptr; }

Color surfaceRampColour(const SurfaceRampStops& stops, double t)
{
    // The scene's elevationRampColor before the table, operation for
    // operation: the kernels (simd/scene_kernels.hpp) are held to it to the
    // bit, so it is not to be rewritten as an "equivalent" expression.
    constexpr int kStops = static_cast<int>(kSurfaceRampStops);
    const double clamped = std::clamp(std::isfinite(t) ? t : 0.0, 0.0, 1.0) * (kStops - 1);
    const int lower = std::min(static_cast<int>(clamped), kStops - 2);
    const double f = clamped - static_cast<double>(lower);
    const auto blend = [f](std::uint8_t a, std::uint8_t b) {
        return static_cast<std::uint8_t>(static_cast<double>(a) +
                                         (static_cast<double>(b) - static_cast<double>(a)) * f +
                                         0.5);
    };
    const Color& a = stops[static_cast<std::size_t>(lower)];
    const Color& b = stops[static_cast<std::size_t>(lower + 1)];
    return Color{blend(a.r, b.r), blend(a.g, b.g), blend(a.b, b.b), 255};
}

Color surfaceRampColour(std::string_view ramp, double t)
{
    return surfaceRampColour(surfaceRampStops(ramp), t);
}

} // namespace katana::cad
