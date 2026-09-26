#pragma once

// The colour ramps terrain shading draws with (docs/terrain.md, "Shading"):
// the built-in ones, a person's own GDAL colour-map file, and a ramp spread
// over a range of values as the colour-map text GDAL's `raster color-map`
// reads - so the colours a legend lists are, value for value, the ones the
// picture was painted with.
//
//   terrain     green lowland through tan and brown to white peaks: relief
//   diverging   blue, white in the middle, red: a difference, cut and fill
//               (on design minus existing, cut is blue and fill red)
//   slope       green, pale yellow, red: gentle to steep
//   grey        black to white: a band drawn as it is ("plain")

#include <cstdint>
#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::interop::geo {

struct RampStop {
    double position = 0.0; // 0 at the range's start, 1 at its end
    std::uint8_t r = 0, g = 0, b = 0;
};

struct ColourRamp {
    std::string name;
    std::vector<RampStop> stops; // positions ascending, the first 0 and the last 1
};

// The built-in ramp of that name (any case); nullptr for none.
[[nodiscard]] const ColourRamp* builtInRamp(std::string_view name);
// Their names, in the order above.
[[nodiscard]] std::vector<std::string> builtInRampNames();

// One colour at one value: what a legend lists.
struct LegendEntry {
    double value = 0.0;
    std::uint8_t r = 0, g = 0, b = 0;
};

// The ramp's stops at their values over [low, high].
[[nodiscard]] std::vector<LegendEntry> spread(const ColourRamp& ramp, double low, double high);

// The entries as GDAL's colour-map text, a line per entry - "value r g b
// 255", each value written exactly, so a cell holding the range's end takes
// the ramp's end colour exactly - then, when `noData`, "nv 0 0 0 0": no-data
// transparent. GDAL interpolates between the lines.
[[nodiscard]] std::string colourMapText(const std::vector<LegendEntry>& entries, bool noData);

// A person's colour-map file as legend entries: the lines GDAL's colour-map
// reads, "value r g b [a]" (blanks, tabs or commas between), a value given
// as a percentage placed on [low, high] as GDAL places it; "nv" lines and
// named colours are skipped (a legend lists values). NotFound for a file
// that is not there; InvalidArgument for one with no value line.
[[nodiscard]] katana::core::Result<std::vector<LegendEntry>>
readColourMap(const std::filesystem::path& file, double low, double high);

} // namespace katana::interop::geo
