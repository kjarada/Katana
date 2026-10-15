// The one table of colour ramps (include/katana/terrain/colour_ramps.hpp,
// docs/surface_display.md "The ramps"): what is in it, where each ramp's
// colours come from, and how a ramp is found. The surface styles
// (cad/terrain/surface_ramps.cpp) and RASTER SHADE / RASTER SLOPE
// (interop/geo/colour_ramps.cpp) are tested to read this table, not to hold
// colours of their own, in their own suites.

#include <gtest/gtest.h>

#include <cstddef>
#include <cstdint>
#include <set>
#include <string>
#include <string_view>
#include <vector>

#include "katana/terrain/colour_ramps.hpp"

namespace {

using katana::terrain::colourRampNames;
using katana::terrain::colourRamps;
using katana::terrain::findColourRamp;
using katana::terrain::kColourRampStops;
using katana::terrain::NamedColourRamp;
using katana::terrain::RampColour;

// A colour from its web hex, as the schemes publish it.
constexpr RampColour hex(unsigned value)
{
    return RampColour{static_cast<std::uint8_t>((value >> 16) & 0xFFu),
                      static_cast<std::uint8_t>((value >> 8) & 0xFFu),
                      static_cast<std::uint8_t>(value & 0xFFu)};
}

} // namespace

TEST(ColourRamps, TheTableHasSixRampsOfFiveStopsEachNamedOnceTheDefaultFirst)
{
    const std::vector<std::string_view> expected{"terrain", "earth", "diverging",
                                                 "slope",   "grey",  "spectral"};
    const auto ramps = colourRamps();
    const auto names = colourRampNames();
    ASSERT_EQ(ramps.size(), expected.size());
    ASSERT_EQ(names.size(), expected.size());
    std::set<std::string_view> seen;
    for (std::size_t i = 0; i < expected.size(); ++i) {
        EXPECT_EQ(ramps[i].name, expected[i]);
        EXPECT_EQ(names[i], ramps[i].name) << i;
        EXPECT_TRUE(seen.insert(ramps[i].name).second) << ramps[i].name;
        // A ramp is data with a source: no ramp is without its reason.
        EXPECT_FALSE(ramps[i].source.empty()) << ramps[i].name;
        EXPECT_EQ(ramps[i].stops.size(), kColourRampStops);
    }
    EXPECT_EQ(kColourRampStops, 5u);
}

TEST(ColourRamps, ARampIsFoundInAnyCaseAndAnUnknownOrEmptyNameIsNone)
{
    const auto ramps = colourRamps();
    EXPECT_EQ(findColourRamp("terrain"), &ramps[0]);
    EXPECT_EQ(findColourRamp("TERRAIN"), &ramps[0]);
    EXPECT_EQ(findColourRamp("Spectral"), &ramps[5]);
    EXPECT_EQ(findColourRamp("gReY"), &ramps[4]);
    EXPECT_EQ(findColourRamp("rainbow"), nullptr);
    EXPECT_EQ(findColourRamp(""), nullptr);
    // The whole name, not a prefix or a padded one.
    EXPECT_EQ(findColourRamp("terra"), nullptr);
    EXPECT_EQ(findColourRamp(" terrain"), nullptr);
    EXPECT_EQ(findColourRamp("terrain "), nullptr);
}

TEST(ColourRamps, TheBrewerRampsAreTheirPublishedFiveClassSchemes)
{
    // Cynthia Brewer's ColorBrewer (colorbrewer2.org), five classes. YlOrBr
    // is published light to dark: #ffffd4 #fed98e #fe9929 #d95f0e #993404. RdBu
    // is published red to blue (#ca0020 #f4a582 #f7f7f7 #92c5de #0571b0) and
    // Spectral red to blue (#d7191c #fdae61 #ffffbf #abdda4 #2b83ba); a cut and
    // fill map has cut blue and low, so both are used reversed.
    const NamedColourRamp* earth = findColourRamp("earth");
    ASSERT_NE(earth, nullptr);
    const katana::terrain::ColourRampStops yellowOrangeBrown{hex(0xffffd4), hex(0xfed98e), hex(0xfe9929),
                                                  hex(0xd95f0e), hex(0x993404)};
    EXPECT_EQ(earth->stops, yellowOrangeBrown);

    const NamedColourRamp* diverging = findColourRamp("diverging");
    ASSERT_NE(diverging, nullptr);
    const katana::terrain::ColourRampStops rdBuReversed{hex(0x0571b0), hex(0x92c5de), hex(0xf7f7f7),
                                                        hex(0xf4a582), hex(0xca0020)};
    EXPECT_EQ(diverging->stops, rdBuReversed);

    const NamedColourRamp* spectral = findColourRamp("spectral");
    ASSERT_NE(spectral, nullptr);
    const katana::terrain::ColourRampStops spectralReversed{hex(0x2b83ba), hex(0xabdda4), hex(0xffffbf),
                                                            hex(0xfdae61), hex(0xd7191c)};
    EXPECT_EQ(spectral->stops, spectralReversed);
}

TEST(ColourRamps, TheProgramsOwnRampsAreWhatTheirSourcesSay)
{
    // terrain: the 3D view's elevation ramp since 2026-09-24, deep blue to
    // snow, five stops (docs/render.md, "Surfaces").
    const NamedColourRamp* terrain = findColourRamp("terrain");
    ASSERT_NE(terrain, nullptr);
    const katana::terrain::ColourRampStops elevation{RampColour{40, 70, 140}, RampColour{60, 160, 90},
                                                     RampColour{235, 220, 130}, RampColour{200, 130, 70},
                                                     RampColour{245, 245, 245}};
    EXPECT_EQ(terrain->stops, elevation);

    // slope: green flat, yellow at half, red at the end of the range, the
    // blue held at 38; the quarter stops are the old line's value there,
    // rounded: red 2t is 0.5 at t = 1/4, which is 127.5, rounded half up to
    // 128, and green 2 - 2t at t = 3/4 likewise.
    const NamedColourRamp* slope = findColourRamp("slope");
    ASSERT_NE(slope, nullptr);
    const katana::terrain::ColourRampStops line{RampColour{0, 255, 38}, RampColour{128, 255, 38},
                                                RampColour{255, 255, 38}, RampColour{255, 128, 38},
                                                RampColour{255, 0, 38}};
    EXPECT_EQ(slope->stops, line);

    // grey: dark to light in equal steps of 48, so the order of the values
    // survives a monochrome plot: 32 + 48 k for k = 0 to 4 is 32, 80, 128,
    // 176, 224, and the three channels are equal.
    const NamedColourRamp* grey = findColourRamp("grey");
    ASSERT_NE(grey, nullptr);
    for (std::size_t k = 0; k < grey->stops.size(); ++k) {
        const auto level = static_cast<std::uint8_t>(32 + 48 * k);
        EXPECT_EQ(grey->stops[k], (RampColour{level, level, level})) << k;
    }
}
