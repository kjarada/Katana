// The ramp table and the colouring rule a surface's style gives its faces
// (cad/terrain/surface_colouring.hpp, docs/surface_display.md). Every
// expected colour is a published ramp's stop, the old scene constant it
// replaces, or a blend worked out by hand below it.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <numbers>
#include <string_view>
#include <vector>

#include "katana/cad/scene.hpp"
#include "katana/cad/terrain/surface_colouring.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/terrain/colour_ramps.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace {

using katana::cad::BandPiece;
using katana::cad::ColourStop;
using katana::cad::SurfaceColouring;
using katana::cad::SurfaceDisplayStyle;
using katana::cad::SurfaceFaces;
using katana::entity::Color;
using katana::geometry::Point3;

constexpr Color rgb(std::uint8_t r, std::uint8_t g, std::uint8_t b) { return Color{r, g, b, 255}; }

SurfaceColouring bandedRamp(double low, double high, int bands)
{
    SurfaceColouring colouring;
    colouring.faces = SurfaceFaces::Elevation;
    colouring.ramp = katana::cad::surfaceRampStops("terrain");
    colouring.banded = true;
    colouring.bands = bands;
    colouring.low = low;
    colouring.high = high;
    return colouring;
}

double pieceArea(const BandPiece& piece)
{
    // The shoelace formula in plan.
    double twice = 0.0;
    for (std::size_t i = 0; i < piece.count; ++i) {
        const Point3& a = piece.corners[i];
        const Point3& b = piece.corners[(i + 1) % piece.count];
        twice += a.x * b.y - b.x * a.y;
    }
    return 0.5 * std::abs(twice);
}

} // namespace

TEST(SurfaceRamps, TheTerrainRampIsTheScenesOldElevationRampToTheBit)
{
    // The scene's palette before the table: kRampR {40, 60, 235, 200, 245},
    // kRampG {70, 160, 220, 130, 245}, kRampB {140, 90, 130, 70, 245}.
    EXPECT_EQ(katana::cad::surfaceRampColour("terrain", 0.0), rgb(40, 70, 140));
    EXPECT_EQ(katana::cad::surfaceRampColour("terrain", 0.25), rgb(60, 160, 90));
    EXPECT_EQ(katana::cad::surfaceRampColour("terrain", 0.5), rgb(235, 220, 130));
    EXPECT_EQ(katana::cad::surfaceRampColour("terrain", 0.75), rgb(200, 130, 70));
    EXPECT_EQ(katana::cad::surfaceRampColour("terrain", 1.0), rgb(245, 245, 245));
    // Between stops: at 0.125, half way from (40, 70, 140) to (60, 160, 90),
    // each channel rounded half up: 50, 115, 115.
    EXPECT_EQ(katana::cad::surfaceRampColour("terrain", 0.125), rgb(50, 115, 115));
    // Clamped, and not finite is the start.
    EXPECT_EQ(katana::cad::surfaceRampColour("terrain", -3.0), rgb(40, 70, 140));
    EXPECT_EQ(katana::cad::surfaceRampColour("terrain", 7.0), rgb(245, 245, 245));
    EXPECT_EQ(katana::cad::surfaceRampColour("terrain", std::nan("")), rgb(40, 70, 140));
    // And the scene's own function is the table's, everywhere.
    for (int i = 0; i <= 1000; ++i) {
        const double t = i / 1000.0;
        const Color colour = katana::cad::surfaceRampColour("terrain", t);
        EXPECT_EQ(katana::cad::elevationRampColor(t),
                  katana::render::rgba(colour.r, colour.g, colour.b))
            << t;
    }
}

TEST(SurfaceRamps, TheTableNamesSixRampsAndAnUnknownNameIsTheFirst)
{
    const std::vector<std::string_view> expected{"terrain", "earth", "diverging",
                                                 "slope",   "grey",  "spectral"};
    const auto names = katana::cad::surfaceRampNames();
    ASSERT_EQ(names.size(), expected.size());
    for (std::size_t i = 0; i < expected.size(); ++i) {
        EXPECT_EQ(names[i], expected[i]);
    }
    EXPECT_TRUE(katana::cad::isSurfaceRamp("SPECTRAL"));
    EXPECT_FALSE(katana::cad::isSurfaceRamp("rainbow"));
    EXPECT_EQ(katana::cad::surfaceRampColour("rainbow", 0.0), rgb(40, 70, 140));
}

TEST(SurfaceRamps, TheDivergingRampIsColorBrewersRdBuReversedBlueLowWhiteMiddleRedHigh)
{
    // RdBu, five classes: #ca0020 #f4a582 #f7f7f7 #92c5de #0571b0, reversed.
    EXPECT_EQ(katana::cad::surfaceRampColour("diverging", 0.0), rgb(0x05, 0x71, 0xb0));
    EXPECT_EQ(katana::cad::surfaceRampColour("diverging", 0.5), rgb(0xf7, 0xf7, 0xf7));
    EXPECT_EQ(katana::cad::surfaceRampColour("diverging", 1.0), rgb(0xca, 0x00, 0x20));
}

TEST(SurfaceRamps, TheSlopeRampKeepsTheScenesOldSlopeColoursAtItsStops)
{
    // The old line: red = 2t, green = 2 - 2t, blue = 0.15, each clamped to
    // 0..1, times 255 and rounded half up - so blue is 38 (38.75 truncated
    // after the half is added to 38.25), and at t = 0.25 red is 128.
    EXPECT_EQ(katana::cad::surfaceRampColour("slope", 0.0), rgb(0, 255, 38));
    EXPECT_EQ(katana::cad::surfaceRampColour("slope", 0.25), rgb(128, 255, 38));
    EXPECT_EQ(katana::cad::surfaceRampColour("slope", 0.5), rgb(255, 255, 38));
    EXPECT_EQ(katana::cad::surfaceRampColour("slope", 0.75), rgb(255, 128, 38));
    EXPECT_EQ(katana::cad::surfaceRampColour("slope", 1.0), rgb(255, 0, 38));
}

TEST(SurfaceRamps, EveryStyleRampIsTheOneTablesRampAtFullAlphaAndItsNamesAreTheTables)
{
    // The styles hold no colours of their own: each ramp is the terrain
    // layer's table row (katana/terrain/colour_ramps.hpp), the same one RASTER
    // SHADE paints with, with alpha 255 - and the names a style may use are
    // its names, in its order.
    const auto table = katana::terrain::colourRamps();
    const auto names = katana::cad::surfaceRampNames();
    ASSERT_EQ(names.size(), table.size());
    for (std::size_t r = 0; r < table.size(); ++r) {
        EXPECT_EQ(names[r], table[r].name);
        const katana::cad::SurfaceRampStops& stops = katana::cad::surfaceRampStops(table[r].name);
        for (std::size_t i = 0; i < stops.size(); ++i) {
            EXPECT_EQ(stops[i], rgb(table[r].stops[i].r, table[r].stops[i].g, table[r].stops[i].b))
                << table[r].name << " " << i;
        }
    }
}

TEST(SurfaceRamps, ARampsStopsAreOneObjectWhateverTheCaseOfTheNameAndAnUnknownNameIsTheFirstsObject)
{
    // The scene keeps a pointer to a ramp's stops (`&surfaceRampStops(name)`),
    // so a lookup must hand back the one object, not a copy that dies.
    EXPECT_EQ(&katana::cad::surfaceRampStops("terrain"), &katana::cad::surfaceRampStops("TERRAIN"));
    EXPECT_NE(&katana::cad::surfaceRampStops("terrain"), &katana::cad::surfaceRampStops("grey"));
    EXPECT_EQ(&katana::cad::surfaceRampStops("rainbow"), &katana::cad::surfaceRampStops("terrain"));
    EXPECT_EQ(&katana::cad::surfaceRampStops(""), &katana::cad::surfaceRampStops("terrain"));
}

TEST(SurfaceAspect, EachSectorIsCentredOnItsCompassPoint)
{
    constexpr double kDegree = std::numbers::pi / 180.0;
    EXPECT_EQ(katana::cad::aspectSector(0.0), 0);
    // Either side of each edge by a tenth of a degree: exactly on an edge
    // is a matter of the last bit of pi.
    EXPECT_EQ(katana::cad::aspectSector(22.4 * kDegree), 0);
    EXPECT_EQ(katana::cad::aspectSector(22.6 * kDegree), 1);
    EXPECT_EQ(katana::cad::aspectSector(45.0 * kDegree), 1);
    EXPECT_EQ(katana::cad::aspectSector(90.0 * kDegree), 2);
    EXPECT_EQ(katana::cad::aspectSector(180.0 * kDegree), 4);
    EXPECT_EQ(katana::cad::aspectSector(270.0 * kDegree), 6);
    EXPECT_EQ(katana::cad::aspectSector(337.4 * kDegree), 7);
    EXPECT_EQ(katana::cad::aspectSector(337.6 * kDegree), 0);
    EXPECT_EQ(katana::cad::aspectSector(354.0 * kDegree), 0);
    EXPECT_EQ(katana::cad::aspectSector(-90.0 * kDegree), 6);
    EXPECT_EQ(katana::cad::aspectSector(std::nan("")), 0);
    EXPECT_EQ(katana::cad::aspectSectorName(0), "N");
    EXPECT_EQ(katana::cad::aspectSectorName(6), "W");
    EXPECT_EQ(katana::cad::aspectSectorName(9), "NE"); // modulo eight
}

TEST(SurfaceColouring, ABandedRampHasEqualBandsAndItsEndBandsAreTheRampsEnds)
{
    const SurfaceColouring colouring = bandedRamp(0.0, 10.0, 5);
    ASSERT_EQ(colouring.bandCount(), 5u);
    EXPECT_EQ(colouring.bandOf(0.0), 0u);
    EXPECT_EQ(colouring.bandOf(1.99), 0u);
    EXPECT_EQ(colouring.bandOf(2.0), 1u);
    EXPECT_EQ(colouring.bandOf(9.99), 4u);
    EXPECT_EQ(colouring.bandOf(10.0), 4u);
    EXPECT_EQ(colouring.bandOf(-5.0), 0u); // below the range: the first band
    EXPECT_EQ(colouring.bandOf(15.0), 4u); // above it: the last
    // Band k is the ramp at k / 4.
    EXPECT_EQ(colouring.bandColour(0), rgb(40, 70, 140));
    EXPECT_EQ(colouring.bandColour(2), rgb(235, 220, 130));
    EXPECT_EQ(colouring.bandColour(4), rgb(245, 245, 245));
    EXPECT_EQ(colouring.colourAt(5.0), rgb(235, 220, 130));
    EXPECT_EQ(colouring.bandLow(1), 2.0);
    EXPECT_EQ(colouring.bandHigh(4), 10.0);
}

TEST(SurfaceColouring, StopsOverrideTheRampAndABandRunsFromItsStopToTheNext)
{
    SurfaceColouring colouring = bandedRamp(0.0, 100.0, 10);
    colouring.stops = {{0.0, rgb(255, 0, 0)}, {2.0, rgb(0, 255, 0)}, {5.0, rgb(0, 0, 255)}};
    ASSERT_EQ(colouring.bandCount(), 3u);
    EXPECT_EQ(colouring.colourAt(1.0), rgb(255, 0, 0));
    EXPECT_EQ(colouring.colourAt(2.0), rgb(0, 255, 0));
    EXPECT_EQ(colouring.colourAt(4.99), rgb(0, 255, 0));
    EXPECT_EQ(colouring.colourAt(100.0), rgb(0, 0, 255));
    EXPECT_EQ(colouring.colourAt(-1.0), rgb(255, 0, 0));
    EXPECT_FALSE(colouring.bandLow(0).has_value());
    EXPECT_EQ(colouring.bandHigh(0), 2.0);
    EXPECT_EQ(colouring.bandLow(2), 5.0);
    EXPECT_FALSE(colouring.bandHigh(2).has_value());
}

TEST(SurfaceColouring, SmoothStopsBlendBetweenTheTwoStopsEitherSide)
{
    SurfaceColouring colouring;
    colouring.faces = SurfaceFaces::Elevation;
    colouring.stops = {{0.0, rgb(0, 0, 0)}, {10.0, rgb(200, 100, 50)}};
    // Half way: 100, 50, 25 (100.5, 50.5 and 25.5 truncated after adding
    // the half to 100, 50, 25).
    EXPECT_EQ(colouring.colourAt(5.0), rgb(100, 50, 25));
    EXPECT_EQ(colouring.colourAt(-1.0), rgb(0, 0, 0));
    EXPECT_EQ(colouring.colourAt(20.0), rgb(200, 100, 50));
}

TEST(SurfaceColouring, ARangeNarrowerThanTheToleranceIsTheRampsMiddleAndNotFiniteIsFlat)
{
    SurfaceColouring colouring;
    colouring.faces = SurfaceFaces::Elevation;
    colouring.low = 5.0;
    colouring.high = 5.0;
    colouring.flat = rgb(1, 2, 3);
    EXPECT_EQ(colouring.colourAt(5.0), rgb(235, 220, 130));
    EXPECT_EQ(colouring.colourAt(std::nan("")), rgb(1, 2, 3));
    EXPECT_EQ(colouring.colourAt(std::numeric_limits<double>::infinity()), rgb(1, 2, 3));
}

TEST(SurfaceColouring, TheStylesRangeReplacesTheSurfacesOwnEndByEnd)
{
    SurfaceDisplayStyle style;
    style.rangeMin = 10.0;
    const SurfaceColouring lowGiven = katana::cad::surfaceColouring(style, SurfaceFaces::Elevation, 0.0, 20.0);
    EXPECT_EQ(lowGiven.low, 10.0);
    EXPECT_EQ(lowGiven.high, 20.0);
    style.rangeMax = 12.0;
    const SurfaceColouring both = katana::cad::surfaceColouring(style, SurfaceFaces::Elevation, 0.0, 20.0);
    EXPECT_EQ(both.low, 10.0);
    EXPECT_EQ(both.high, 12.0);
    // Back to front: the point at its low end.
    style.rangeMin = 30.0;
    const SurfaceColouring backwards = katana::cad::surfaceColouring(style, SurfaceFaces::Elevation, 0.0, 20.0);
    EXPECT_EQ(backwards.low, 30.0);
    EXPECT_EQ(backwards.high, 30.0);
}

TEST(SurfaceColouring, SlopeIsInPercentAndItsDefaultRangeEndsAtTheSteepestFace)
{
    // The plane z = x / 2 rises 1 in 2: 50 percent on every face.
    auto tin = katana::terrain::TinSurface::create(
        {{0.0, 0.0, 0.0}, {10.0, 0.0, 5.0}, {10.0, 10.0, 5.0}, {0.0, 10.0, 0.0}}, {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(tin.ok());
    EXPECT_NEAR(katana::cad::steepestSlopePercent(*tin), 50.0, 1e-9);
    SurfaceDisplayStyle style;
    const SurfaceColouring colouring = katana::cad::surfaceColouring(style, SurfaceFaces::Slope, *tin);
    EXPECT_EQ(colouring.low, 0.0);
    EXPECT_NEAR(colouring.high, 50.0, 1e-9);
    EXPECT_EQ(katana::cad::steepestSlopePercent(katana::terrain::TinSurface{}), 0.0);
}

TEST(SurfaceBandPieces, ATriangleAcrossFiveBandsIsCutIntoFivePiecesThatCoverItExactly)
{
    // z runs 0 to 10 across the triangle; bands of 2 from 0 to 10.
    const SurfaceColouring colouring = bandedRamp(0.0, 10.0, 5);
    const Point3 a(0.0, 0.0, 0.0);
    const Point3 b(10.0, 0.0, 10.0);
    const Point3 c(0.0, 10.0, 5.0);
    std::vector<BandPiece> pieces;
    katana::cad::bandPieces(colouring, a, b, c, pieces);
    ASSERT_EQ(pieces.size(), 5u);
    double area = 0.0;
    for (std::size_t k = 0; k < pieces.size(); ++k) {
        EXPECT_EQ(pieces[k].band, k);
        EXPECT_GE(pieces[k].count, 3u);
        area += pieceArea(pieces[k]);
        for (std::size_t i = 0; i < pieces[k].count; ++i) {
            const double z = pieces[k].corners[i].z;
            EXPECT_GE(z, 2.0 * static_cast<double>(k) - 1e-12);
            EXPECT_LE(z, 2.0 * static_cast<double>(k + 1) + 1e-12);
        }
    }
    // The triangle's own area: half of 10 x 10.
    EXPECT_NEAR(area, 50.0, 1e-9);
}

TEST(SurfaceBandPieces, ATriangleInOneBandIsOnePieceAndAnUnbandedColouringNeverCuts)
{
    const SurfaceColouring colouring = bandedRamp(0.0, 10.0, 5);
    std::vector<BandPiece> pieces;
    katana::cad::bandPieces(colouring, {0, 0, 2.5}, {1, 0, 3.0}, {0, 1, 3.5}, pieces);
    ASSERT_EQ(pieces.size(), 1u);
    EXPECT_EQ(pieces.front().band, 1u);
    EXPECT_EQ(pieces.front().count, 3u);
    SurfaceColouring smooth = colouring;
    smooth.banded = false;
    pieces.clear();
    katana::cad::bandPieces(smooth, {0, 0, 0}, {10, 0, 10}, {0, 10, 5}, pieces);
    EXPECT_EQ(pieces.size(), 1u);
}

TEST(SurfaceLegend, BandsHaveAnEntryEachLabelledByTheirEdges)
{
    const auto legend = katana::cad::surfaceLegend(bandedRamp(25.0, 35.0, 5));
    EXPECT_EQ(legend.title, "Elevation");
    EXPECT_FALSE(legend.gradient);
    ASSERT_EQ(legend.entries.size(), 5u);
    EXPECT_EQ(legend.entries[0].label, "25 - 27");
    EXPECT_EQ(legend.entries[4].label, "33 - 35");
    EXPECT_EQ(legend.entries[0].colour, rgb(40, 70, 140));
}

TEST(SurfaceLegend, BandedStopsOpenTheFirstBandBelowAndTheLastAbove)
{
    SurfaceColouring colouring = bandedRamp(0.0, 100.0, 10);
    colouring.faces = SurfaceFaces::Slope;
    for (const double value : {0.0, 2.0, 5.0, 10.0, 20.0, 33.0}) {
        colouring.stops.push_back({value, rgb(static_cast<std::uint8_t>(value), 0, 0)});
    }
    const auto legend = katana::cad::surfaceLegend(colouring);
    EXPECT_EQ(legend.title, "Slope %");
    ASSERT_EQ(legend.entries.size(), 6u);
    EXPECT_EQ(legend.entries[0].label, "< 2");
    EXPECT_EQ(legend.entries[1].label, "2 - 5");
    EXPECT_EQ(legend.entries[5].label, "33 +");
}

TEST(SurfaceLegend, AspectIsTheEightSectorsAndLevelAndNoFacesHasNone)
{
    SurfaceColouring colouring;
    colouring.faces = SurfaceFaces::Aspect;
    const auto legend = katana::cad::surfaceLegend(colouring);
    ASSERT_EQ(legend.entries.size(), 9u);
    EXPECT_EQ(legend.entries[0].label, "N");
    EXPECT_EQ(legend.entries[7].label, "NW");
    EXPECT_EQ(legend.entries[8].label, "level");
    colouring.faces = SurfaceFaces::None;
    EXPECT_TRUE(katana::cad::surfaceLegend(colouring).entries.empty());
}

TEST(SurfaceLegend, ASmoothRampIsAGradientLabelledAtItsEnds)
{
    SurfaceColouring colouring = bandedRamp(24.892, 38.819, 10);
    colouring.banded = false;
    const auto legend = katana::cad::surfaceLegend(colouring);
    EXPECT_TRUE(legend.gradient);
    ASSERT_EQ(legend.entries.size(), 9u);
    EXPECT_EQ(legend.entries.front().label, "24.892");
    EXPECT_EQ(legend.entries.back().label, "38.819");
    EXPECT_TRUE(legend.entries[4].label.empty());
    EXPECT_EQ(legend.entries.front().colour, rgb(40, 70, 140));
    EXPECT_EQ(legend.entries.back().colour, rgb(245, 245, 245));
}

TEST(SurfaceLegend, DecimalsAreTheFewestThatWriteEveryValue)
{
    const double whole[] = {25.0, 26.0};
    const double half[] = {0.5, 3.0};
    const double fine[] = {24.892, 26.284699999999997};
    const double third[] = {1.0 / 3.0};
    EXPECT_EQ(katana::cad::legendDecimals(whole), 0);
    EXPECT_EQ(katana::cad::legendDecimals(half), 1);
    EXPECT_EQ(katana::cad::legendDecimals(fine), 3);
    EXPECT_EQ(katana::cad::legendDecimals(third), 3); // at most three
    EXPECT_EQ(katana::cad::formatLegendValue(26.284699999999997, 3), "26.285");
    EXPECT_EQ(katana::cad::formatLegendValue(-0.0, 1), "0.0");
}

TEST(SurfaceColouring, ACollapsedRangeIsAPointAndAValueKeepsTheSideOfItItIsOn)
{
    // low = high = 40: below the point is the first band (the ramp's start),
    // above it the last (its end), and at it the middle - band 5 / 2 = 2,
    // the ramp at 1/2. One middle for every value, whichever side it was on,
    // painted a terrain wholly below min=40 in the middle band.
    SurfaceColouring colouring = bandedRamp(40.0, 40.0, 5);
    ASSERT_TRUE(colouring.collapsed());
    EXPECT_EQ(colouring.bandOf(30.0), 0u);
    EXPECT_EQ(colouring.bandOf(50.0), 4u);
    EXPECT_EQ(colouring.bandOf(40.0), 2u);
    // Within the geometric tolerance (1e-7) is at it; past it is a side.
    EXPECT_EQ(colouring.bandOf(40.0 + 1.0e-9), 2u);
    EXPECT_EQ(colouring.bandOf(40.0 + 1.0e-6), 4u);
    EXPECT_EQ(colouring.bandOf(40.0 - 1.0e-6), 0u);
    EXPECT_EQ(colouring.colourAt(30.0), rgb(40, 70, 140));
    EXPECT_EQ(colouring.colourAt(50.0), rgb(245, 245, 245));
    EXPECT_EQ(colouring.colourAt(40.0), rgb(235, 220, 130));
    // The same for a smooth ramp.
    colouring.banded = false;
    EXPECT_EQ(colouring.colourAt(30.0), rgb(40, 70, 140));
    EXPECT_EQ(colouring.colourAt(50.0), rgb(245, 245, 245));
    EXPECT_EQ(colouring.colourAt(40.0), rgb(235, 220, 130));
    // Not finite is still flat; a range that is not collapsed is not one.
    EXPECT_EQ(colouring.colourAt(std::nan("")), colouring.flat);
    EXPECT_FALSE(bandedRamp(0.0, 10.0, 5).collapsed());
}

TEST(SurfaceColouring, ALevelFaceUnderSlopeIsTheRampsStartForSlopeIsNeverBelowItsPoint)
{
    // Slope's range is 0 to the steepest face: for a level surface the point
    // 0. A face with no steepness is the slope ramp's start, green (0, 255,
    // 38) = what the 3D view drew it before styles - not the middle, yellow
    // (255, 255, 38), which reads as half way to the steep end.
    auto level = katana::terrain::TinSurface::create(
        {{0.0, 0.0, 5.0}, {10.0, 0.0, 5.0}, {10.0, 10.0, 5.0}, {0.0, 10.0, 5.0}}, {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(level.ok());
    ASSERT_EQ(katana::cad::steepestSlopePercent(*level), 0.0);
    SurfaceDisplayStyle style;
    style.ramp = "slope";
    SurfaceColouring smooth = katana::cad::surfaceColouring(style, SurfaceFaces::Slope, *level);
    ASSERT_TRUE(smooth.collapsed());
    EXPECT_EQ(smooth.colourAt(0.0), rgb(0, 255, 38));
    // A face steeper than the point (a surface edited since, or a range
    // given) is the end.
    EXPECT_EQ(smooth.colourAt(10.0), rgb(255, 0, 38));
    style.banded = true;
    style.bands = 4;
    const SurfaceColouring banded = katana::cad::surfaceColouring(style, SurfaceFaces::Slope, *level);
    EXPECT_EQ(banded.bandOf(0.0), 0u);
    EXPECT_EQ(banded.colourAt(0.0), rgb(0, 255, 38));
    EXPECT_EQ(banded.bandOf(10.0), 3u);
}

TEST(SurfaceLegend, ACollapsedRangeSaysBelowAtAndAboveItsPointNotNBandsOfZeroWidth)
{
    // Five swatches labelled "40 - 40" in five colours, none saying which one
    // is drawn, was the legend of min=40 over a terrain wholly below it.
    const auto banded = katana::cad::surfaceLegend(bandedRamp(40.0, 40.0, 5));
    EXPECT_FALSE(banded.gradient);
    ASSERT_EQ(banded.entries.size(), 3u);
    EXPECT_EQ(banded.entries[0].label, "< 40");
    EXPECT_EQ(banded.entries[1].label, "40");
    EXPECT_EQ(banded.entries[2].label, "> 40");
    EXPECT_EQ(banded.entries[0].colour, rgb(40, 70, 140));    // the first band
    EXPECT_EQ(banded.entries[1].colour, rgb(235, 220, 130));  // the middle band
    EXPECT_EQ(banded.entries[2].colour, rgb(245, 245, 245));  // the last
    EXPECT_FALSE(banded.entries[0].from.has_value());
    EXPECT_EQ(banded.entries[0].to, 40.0);
    EXPECT_EQ(banded.entries[1].from, 40.0);
    EXPECT_EQ(banded.entries[1].to, 40.0);
    EXPECT_EQ(banded.entries[2].from, 40.0);
    EXPECT_FALSE(banded.entries[2].to.has_value());
    // A smooth ramp says the same three, not nine of one colour.
    SurfaceColouring smooth = bandedRamp(40.0, 40.0, 5);
    smooth.banded = false;
    const auto gradient = katana::cad::surfaceLegend(smooth);
    EXPECT_FALSE(gradient.gradient);
    ASSERT_EQ(gradient.entries.size(), 3u);
    EXPECT_EQ(gradient.entries[0].colour, rgb(40, 70, 140));
    EXPECT_EQ(gradient.entries[2].colour, rgb(245, 245, 245));
    // Slope is never below its point 0: a level pad has the point and above.
    SurfaceColouring slope = bandedRamp(0.0, 0.0, 4);
    slope.faces = SurfaceFaces::Slope;
    slope.ramp = katana::cad::surfaceRampStops("slope");
    const auto level = katana::cad::surfaceLegend(slope);
    ASSERT_EQ(level.entries.size(), 2u);
    EXPECT_EQ(level.title, "Slope %");
    EXPECT_EQ(level.entries[0].label, "0");
    EXPECT_EQ(level.entries[0].colour, rgb(0, 255, 38));
    EXPECT_EQ(level.entries[1].label, "> 0");
    EXPECT_EQ(level.entries[1].colour, rgb(255, 0, 38));
}

TEST(SurfaceLegend, ABandedRampTheDataRunsPastTakesThoseValuesInItsEndBandsAndSaysSo)
{
    // min=30 max=35 over a terrain of 24.892 to 38.819, five bands: the first
    // band is drawn on everything below 31 and the last on everything above
    // 34, so "30 - 31" and "34 - 35" misread the colours that cover 24.9 to
    // 30 and 35 to 38.8.
    SurfaceDisplayStyle style;
    style.banded = true;
    style.bands = 5;
    style.rangeMin = 30.0;
    style.rangeMax = 35.0;
    const SurfaceColouring open =
        katana::cad::surfaceColouring(style, SurfaceFaces::Elevation, 24.892, 38.819);
    EXPECT_TRUE(open.openBelow);
    EXPECT_TRUE(open.openAbove);
    EXPECT_FALSE(open.bandLow(0).has_value());
    EXPECT_FALSE(open.bandHigh(4).has_value());
    EXPECT_EQ(open.bandHigh(0), 31.0);
    EXPECT_EQ(open.bandLow(4), 34.0);
    const auto legend = katana::cad::surfaceLegend(open);
    ASSERT_EQ(legend.entries.size(), 5u);
    EXPECT_EQ(legend.entries[0].label, "< 31");
    EXPECT_EQ(legend.entries[1].label, "31 - 32");
    EXPECT_EQ(legend.entries[3].label, "33 - 34");
    EXPECT_EQ(legend.entries[4].label, "34 +");
    EXPECT_FALSE(legend.entries[0].from.has_value());
    EXPECT_FALSE(legend.entries[4].to.has_value());
    // The picture is unchanged by what the legend says: bandOf still puts
    // 25 in the first band and 38 in the last.
    EXPECT_EQ(open.bandOf(25.0), 0u);
    EXPECT_EQ(open.bandOf(38.0), 4u);
    // A range the data lies inside leaves both ends closed, as before: the
    // sample terrain's own bands read "20 - 26" ... "44 - 50".
    style.rangeMin = 20.0;
    style.rangeMax = 50.0;
    const SurfaceColouring closed =
        katana::cad::surfaceColouring(style, SurfaceFaces::Elevation, 24.892, 38.819);
    EXPECT_FALSE(closed.openBelow);
    EXPECT_FALSE(closed.openAbove);
    const auto wide = katana::cad::surfaceLegend(closed);
    ASSERT_EQ(wide.entries.size(), 5u);
    EXPECT_EQ(wide.entries[0].label, "20 - 26");
    EXPECT_EQ(wide.entries[4].label, "44 - 50");
    // An end the style leaves to the surface is closed whatever the other does.
    style.rangeMin = std::nullopt;
    style.rangeMax = 35.0;
    const SurfaceColouring half =
        katana::cad::surfaceColouring(style, SurfaceFaces::Elevation, 24.892, 38.819);
    EXPECT_FALSE(half.openBelow);
    EXPECT_TRUE(half.openAbove);
}

TEST(SurfaceLegend, OneBandedStopIsEveryValueNotAnythingFromItUp)
{
    // bandOf puts every value in the only band, so "30 +" claimed what the
    // colour does not mean: ground below 30 is drawn in it as well.
    SurfaceColouring colouring = bandedRamp(0.0, 100.0, 10);
    colouring.stops = {{30.0, rgb(255, 0, 0)}};
    EXPECT_EQ(colouring.bandCount(), 1u);
    EXPECT_FALSE(colouring.bandLow(0).has_value());
    EXPECT_FALSE(colouring.bandHigh(0).has_value());
    EXPECT_EQ(colouring.colourAt(10.0), rgb(255, 0, 0));
    const auto legend = katana::cad::surfaceLegend(colouring);
    ASSERT_EQ(legend.entries.size(), 1u);
    EXPECT_EQ(legend.entries[0].label, "all");
    EXPECT_FALSE(legend.entries[0].from.has_value());
    EXPECT_FALSE(legend.entries[0].to.has_value());
    EXPECT_EQ(legend.entries[0].colour, rgb(255, 0, 0));
}

TEST(SurfaceBandPieces, ATriangleAcrossTheMaximumNumberOfBandsIsAtMostThatManyPieces)
{
    // The 3D view's geometry is the pieces: a triangle crossing every one of
    // the style's bands is that many pieces and no more, so the maximum the
    // style check allows (100) bounds a surface's pieces at 100 a triangle,
    // however fine the bands. z runs 0 to 10 across the triangle (0, 0, 0)
    // (10, 0, 10) (0, 10, 5), 0.1 a band: all 100 bands are crossed and the
    // pieces, being a partition of the triangle, add up to its area, 50.
    const SurfaceColouring colouring = bandedRamp(0.0, 10.0, katana::cad::kMaximumSurfaceBands);
    std::vector<BandPiece> pieces;
    katana::cad::bandPieces(colouring, {0.0, 0.0, 0.0}, {10.0, 0.0, 10.0}, {0.0, 10.0, 5.0}, pieces);
    EXPECT_EQ(pieces.size(), 100u);
    double area = 0.0;
    for (const BandPiece& piece : pieces) {
        EXPECT_GE(piece.count, 3u);
        EXPECT_LE(piece.count, 5u);
        area += pieceArea(piece);
    }
    EXPECT_NEAR(area, 50.0, 1e-9);
}
