// The surface styles' view of the ramp table: every colour ramp a style may
// name, which the plan view, the 3D scene (its elevation and slope colours,
// scene.cpp) and the legends all colour through (docs/surface_display.md, "The
// ramps"). The colours are not here: the table is katana/terrain/
// colour_ramps.hpp's, the same one RASTER SHADE and RASTER SLOPE paint with.
// This file turns its rows into the drawing's colours (full alpha) and keeps
// the lookup arithmetic the scene's kernels are held to.

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <span>
#include <string_view>
#include <vector>

#include "katana/cad/terrain/surface_colouring.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/terrain/colour_ramps.hpp"

namespace katana::cad {

static_assert(katana::terrain::kColourRampStops == kSurfaceRampStops);

namespace {

using katana::entity::Color;

// The table's ramps as the drawing's colours, in the table's order, built once
// and never changed: the scene keeps a pointer to a ramp's stops
// (`&surfaceRampStops(name)`), which a vector that grew would move.
const std::vector<SurfaceRampStops>& drawingRamps()
{
    static const std::vector<SurfaceRampStops> ramps = [] {
        std::vector<SurfaceRampStops> built;
        for (const katana::terrain::NamedColourRamp& ramp : katana::terrain::colourRamps()) {
            SurfaceRampStops stops{};
            for (std::size_t i = 0; i < stops.size(); ++i) {
                stops[i] = Color{ramp.stops[i].r, ramp.stops[i].g, ramp.stops[i].b, 255};
            }
            built.push_back(stops);
        }
        return built;
    }();
    return ramps;
}

} // namespace

std::span<const std::string_view> surfaceRampNames() { return katana::terrain::colourRampNames(); }

const SurfaceRampStops& surfaceRampStops(std::string_view ramp)
{
    const katana::terrain::NamedColourRamp* found = katana::terrain::findColourRamp(ramp);
    // An unknown name is the first ramp's, as the header says.
    const std::size_t index =
        found == nullptr ? 0 : static_cast<std::size_t>(found - katana::terrain::colourRamps().data());
    return drawingRamps()[index];
}

bool isSurfaceRamp(std::string_view ramp) { return katana::terrain::findColourRamp(ramp) != nullptr; }

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
