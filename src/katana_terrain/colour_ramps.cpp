// The named colour ramps (katana/terrain/colour_ramps.hpp): the one table.

#include "katana/terrain/colour_ramps.hpp"

#include <algorithm>
#include <array>
#include <string_view>

#include "katana/core/text.hpp"

namespace katana::terrain {

namespace {

constexpr RampColour rgb(std::uint8_t r, std::uint8_t g, std::uint8_t b) { return RampColour{r, g, b}; }

constexpr std::array<NamedColourRamp, 6> kRamps{{
    // FIRST, so it is the default and what an unknown name falls back to.
    // Unchanged to the bit, so the scene's colours and its kernels' are what
    // they were (cad::elevationRampColor is held to it).
    {"terrain",
     {rgb(40, 70, 140), rgb(60, 160, 90), rgb(235, 220, 130), rgb(200, 130, 70),
      rgb(245, 245, 245)},
     "the 3D view's one elevation ramp since 2026-09-24 (docs/render.md, \"Surfaces\"): deep "
     "blue, green, sand, brown, snow - a hypsometric tint whose neighbouring bands stay "
     "distinguishable"},
    {"earth",
     {rgb(255, 255, 212), rgb(254, 217, 142), rgb(254, 153, 41), rgb(217, 95, 14),
      rgb(153, 52, 4)},
     "ColorBrewer YlOrBr, five classes (Cynthia Brewer, colorbrewer2.org): pale yellow low to "
     "dark brown high, sequential and legible in grey"},
    {"diverging",
     {rgb(5, 113, 176), rgb(146, 197, 222), rgb(247, 247, 247), rgb(244, 165, 130),
      rgb(202, 0, 32)},
     "ColorBrewer RdBu, five classes, reversed so that low (cut, a difference below zero) is "
     "blue and high (fill) red, white between: a cut/fill map's convention"},
    {"slope",
     {rgb(0, 255, 38), rgb(128, 255, 38), rgb(255, 255, 38), rgb(255, 128, 38), rgb(255, 0, 38)},
     "the 3D view's slope colours since 2026-09-24: green flat, yellow at half, red at the end "
     "of the range; the quarter stops are that line's value there, rounded"},
    {"grey",
     {rgb(32, 32, 32), rgb(80, 80, 80), rgb(128, 128, 128), rgb(176, 176, 176),
      rgb(224, 224, 224)},
     "chosen here: dark to light in equal steps of 48, so the order of the values survives a "
     "monochrome plot"},
    {"spectral",
     {rgb(43, 131, 186), rgb(171, 221, 164), rgb(255, 255, 191), rgb(253, 174, 97),
      rgb(215, 25, 28)},
     "ColorBrewer Spectral, five classes, reversed: blue low to red high, the most hues of any "
     "ramp here for telling many heights apart"},
}};

constexpr std::array<std::string_view, kRamps.size()> kNames = [] {
    std::array<std::string_view, kRamps.size()> names{};
    for (std::size_t i = 0; i < kRamps.size(); ++i) {
        names[i] = kRamps[i].name;
    }
    return names;
}();

} // namespace

std::span<const NamedColourRamp> colourRamps() { return kRamps; }

std::span<const std::string_view> colourRampNames() { return kNames; }

const NamedColourRamp* findColourRamp(std::string_view name)
{
    const auto found = std::ranges::find_if(kRamps, [name](const NamedColourRamp& ramp) {
        return katana::core::equalsIgnoringCase(ramp.name, name);
    });
    return found == kRamps.end() ? nullptr : &*found;
}

} // namespace katana::terrain
