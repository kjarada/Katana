#pragma once

// The named colour ramps: the ONE table (docs/surface_display.md, "The
// ramps"). A surface's display style names one of them (cad/terrain/
// surface_style.hpp), the 3D scene's elevation and slope colours are two of
// them, and RASTER SHADE and RASTER SLOPE paint with the same five colours
// (interop/geo/colour_ramps.hpp), so `ramp=terrain` is one map on a surface
// style, on a shaded raster and in a legend.
//
// It is plain data: three bytes a colour and no colour type of the drawing's.
// That is what lets it sit in the lowest layer that both `cad` and `interop`
// may see (tools/check_layering.cmake: `cad` may not see `interop`, and
// `interop` may not see `render`), so neither has a table of its own.
//
// Every ramp is five colours evenly spaced over 0 to 1 and blended linearly
// between them. Five, because that is the form the 3D scene's vector kernels
// read (src/katana_cad/simd/scene_kernels.hpp: four segments a channel), so a
// surface coloured smoothly by ANY ramp takes the kernel path. How a colour
// is blended at a value stays with the caller: the scene's lookup is
// held to the bit by its kernels (cad::surfaceRampColour), and RASTER SHADE
// hands the stops to GDAL's colour-map, which blends them itself.

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <string_view>

namespace katana::terrain {

struct RampColour {
    std::uint8_t r = 0;
    std::uint8_t g = 0;
    std::uint8_t b = 0;

    friend bool operator==(const RampColour&, const RampColour&) = default;
};

inline constexpr std::size_t kColourRampStops = 5;
using ColourRampStops = std::array<RampColour, kColourRampStops>;

struct NamedColourRamp {
    std::string_view name;
    ColourRampStops stops;
    // Where the five colours come from: a published scheme, or the program's
    // own choice and why. A ramp is data with a source, not a taste.
    std::string_view source;
};

// Every ramp, the default first ("terrain"): terrain, earth, diverging (cut
// blue to fill red), slope (green to red), grey, spectral. The order is the
// order the verbs and the style form list them in.
[[nodiscard]] std::span<const NamedColourRamp> colourRamps();

// The names of colourRamps(), in its order.
[[nodiscard]] std::span<const std::string_view> colourRampNames();

// The ramp of that name, in any case; nullptr for none. The one lookup: a
// surface style's ramp and RASTER SHADE's ramp= both find a ramp through it.
[[nodiscard]] const NamedColourRamp* findColourRamp(std::string_view name);

} // namespace katana::terrain
