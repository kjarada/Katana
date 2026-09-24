// Point symbols (PLAN.MD 20.2, slice 2): one definition of each shape, in
// model units, for every renderer.
#include <gtest/gtest.h>

#include <cmath>
#include <numbers>

#include "katana/cad/symbols.hpp"
#include "katana/entity/tables.hpp"

using katana::cad::symbolStrokes;
using katana::geometry::Point2;

namespace {

constexpr double kPi = std::numbers::pi;

double radius(const Point2& vertex, const Point2& centre)
{
    return std::hypot(vertex.x - centre.x, vertex.y - centre.y);
}

} // namespace

TEST(Symbols, EveryNamedSymbolHasItsOwnShapeAndAnyOtherNameFallsBackToOne)
{
    for (const std::string_view name : katana::entity::symbolNames()) {
        const auto strokes = symbolStrokes(name, Point2(0.0, 0.0), 1.0);
        EXPECT_FALSE(strokes.empty()) << name;
        for (const auto& stroke : strokes) {
            EXPECT_GE(stroke.vertices.size(), 2u) << name;
            // Every shape fits the unit square: a symbol that reaches
            // outside its own size would overlap the text beside it.
            for (const auto& vertex : stroke.vertices) {
                EXPECT_LE(std::fabs(vertex.x), 1.0 + 1e-12) << name;
                EXPECT_LE(std::fabs(vertex.y), 1.0 + 1e-12) << name;
            }
        }
    }
    // Only "no symbol" draws nothing. A name that is not one of the sixteen
    // is a real library linestyle name - "SEWR Manhole Cover" - and falls back to
    // the shape its words suggest, because the alternative is a point that
    // draws nothing at all when no library defines its name.
    EXPECT_TRUE(symbolStrokes(katana::entity::kNoSymbol, Point2(0.0, 0.0), 1.0).empty());
    EXPECT_FALSE(symbolStrokes("SEWR Manhole Cover", Point2(0.0, 0.0), 1.0).empty());
    EXPECT_EQ(symbolStrokes("SEWR Manhole Cover", Point2(0.0, 0.0), 1.0),
              symbolStrokes("manhole", Point2(0.0, 0.0), 1.0));
    // A name suggesting nothing still draws: a circle is the last resort.
    EXPECT_EQ(symbolStrokes("blob", Point2(0.0, 0.0), 1.0),
              symbolStrokes("circle", Point2(0.0, 0.0), 1.0));
}

TEST(Symbols, ACircleIsChordsOnTheCircleOfTheGivenSizeAboutTheCentre)
{
    const Point2 centre(100.0, -50.0);
    const auto strokes = symbolStrokes("circle", centre, 2.5);
    ASSERT_EQ(strokes.size(), 1u);
    EXPECT_TRUE(strokes[0].closed);
    EXPECT_EQ(strokes[0].vertices.size(), 24u);
    for (const auto& vertex : strokes[0].vertices) {
        EXPECT_NEAR(radius(vertex, centre), 2.5, 1e-12);
    }
    // 24 chords of a circle: consecutive vertices 15 degrees apart.
    const Point2& a = strokes[0].vertices[0];
    const Point2& b = strokes[0].vertices[1];
    EXPECT_NEAR(std::atan2(b.y - centre.y, b.x - centre.x) - std::atan2(a.y - centre.y, a.x - centre.x),
                kPi / 12.0, 1e-12);
}

TEST(Symbols, ACrossIsTheTwoDiagonalsOfTheSquare)
{
    const auto strokes = symbolStrokes("cross", Point2(10.0, 20.0), 3.0);
    ASSERT_EQ(strokes.size(), 2u);
    EXPECT_FALSE(strokes[0].closed);
    EXPECT_EQ(strokes[0].vertices.front(), Point2(7.0, 17.0));
    EXPECT_EQ(strokes[0].vertices.back(), Point2(13.0, 23.0));
    EXPECT_EQ(strokes[1].vertices.front(), Point2(7.0, 23.0));
    EXPECT_EQ(strokes[1].vertices.back(), Point2(13.0, 17.0));
}

TEST(Symbols, RotationTurnsTheShapeCounterClockwiseAboutTheCentre)
{
    // A quarter turn takes the arrow's tip from east to north.
    const Point2 centre(5.0, 5.0);
    const auto east = symbolStrokes("arrow", centre, 2.0);
    const auto north = symbolStrokes("arrow", centre, 2.0, kPi / 2.0);
    ASSERT_EQ(east.size(), north.size());
    EXPECT_NEAR(east[0].vertices.back().x, 7.0, 1e-12);
    EXPECT_NEAR(east[0].vertices.back().y, 5.0, 1e-12);
    EXPECT_NEAR(north[0].vertices.back().x, 5.0, 1e-12);
    EXPECT_NEAR(north[0].vertices.back().y, 7.0, 1e-12);
    // Rotation moves nothing closer to or further from the centre.
    for (std::size_t s = 0; s < east.size(); ++s) {
        ASSERT_EQ(east[s].vertices.size(), north[s].vertices.size());
        for (std::size_t i = 0; i < east[s].vertices.size(); ++i) {
            EXPECT_NEAR(radius(east[s].vertices[i], centre), radius(north[s].vertices[i], centre),
                        1e-12);
        }
    }
}

TEST(Symbols, SizeZeroDrawsNothingSoTheCallerMustChooseAMarkSize)
{
    // Documented in symbols.hpp: 0 means "the viewport's own mark", which is
    // the caller's to size in pixels. Everything collapses onto the centre.
    for (const auto& stroke : symbolStrokes("target", Point2(1.0, 2.0), 0.0)) {
        for (const auto& vertex : stroke.vertices) {
            EXPECT_EQ(vertex, Point2(1.0, 2.0));
        }
    }
}
