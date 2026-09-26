// GIS HULL and GIS CLIP (src/katana_app/geo/hull_clip_verbs.cpp,
// docs/geoprocessing.md "V3"): through the one executor, every expected
// value worked by hand beside it.

#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "contract_support.hpp"
#include "katana/gis/processing.hpp"
#include "vector_fixture.hpp"

namespace {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

const std::string kData = KATANA_GEO_TEST_DATA;

class GisHull : public katana::geo_test::VectorFixture {};
class GisClip : public katana::geo_test::VectorFixture {};

double lengthOf(const Entity& entity)
{
    if (const auto* segment = std::get_if<katana::geometry::Segment2>(&entity.geometry)) {
        return segment->length();
    }
    if (const auto* line = std::get_if<Polyline2>(&entity.geometry)) {
        return line->length();
    }
    return 0.0;
}

TEST_F(GisHull, ConvexHullOfTheCornersAndCentreOfASquareIsTheSquare)
{
    for (const Point2 at : {Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10), Point2(5, 5)}) {
        add(katana::entity::PointGeometry{at}, "survey");
    }
    const std::string reply = ok("GIS HULL LAYERS survey");
    const auto hulls = on("gis/hull");
    ASSERT_EQ(hulls.size(), 1u);
    const Polyline2 ring = std::get<Polyline2>(hulls.front().geometry);
    EXPECT_EQ(ring.vertices.size(), 4u); // the centre is inside, not a corner
    EXPECT_NEAR(ring.area(), 100.0, 1e-9);
    const auto hull = record(reply, "hull");
    ASSERT_TRUE(hull);
    EXPECT_EQ(hull->get("kind"), "convex");
    EXPECT_EQ(hull->get("points"), "5");
    EXPECT_EQ(hull->get("area"), "100.000");
}

TEST_F(GisHull, ConcaveHullOfAnLShapedPointSetIsSmallerThanItsConvexHull)
{
    // The points of a 1 m grid over an L: the arm y = 0..2 for x = 0..10,
    // and the arm x = 0..2 for y = 3..10 - 57 points in an L of 10 x 2 +
    // 2 x 8 = 36 m2. Its convex hull cuts the corner from (10,2) to (2,10):
    // 100 - 8 x 8 / 2 = 68 m2. Tight (ratio 0.1), the hull follows the L
    // but for the half square it cuts across the inner corner, (2,3) to
    // (3,2): 36 + 0.5 = 36.5 m2.
    for (int x = 0; x <= 10; ++x) {
        for (int y = 0; y <= 2; ++y) {
            add(katana::entity::PointGeometry{{double(x), double(y)}}, "survey");
        }
    }
    for (int x = 0; x <= 2; ++x) {
        for (int y = 3; y <= 10; ++y) {
            add(katana::entity::PointGeometry{{double(x), double(y)}}, "survey");
        }
    }
    const std::string convex = ok("GIS HULL LAYERS survey convex TO LAYER convex");
    const std::string concave = ok("GIS HULL LAYERS survey concave=0.1 TO LAYER concave");
    EXPECT_EQ(record(convex, "hull")->get("points"), "57");
    EXPECT_NEAR(areaOn("convex"), 68.0, 1e-9);
    EXPECT_NEAR(areaOn("concave"), 36.5, 1e-9);
    EXPECT_EQ(record(concave, "hull")->get("kind"), "concave");
    EXPECT_EQ(record(concave, "hull")->get("ratio"), "0.1");
}

TEST_F(GisHull, FewerThanThreePointsBoundNoAreaAndSaySo)
{
    add(katana::entity::PointGeometry{{0, 0}}, "survey");
    add(katana::entity::PointGeometry{{10, 0}}, "survey");
    const std::string reply = ok("GIS HULL LAYERS survey");
    EXPECT_TRUE(on("gis/hull").empty());
    EXPECT_NE(reply.find("they bound no area"), std::string::npos) << reply;
}

TEST_F(GisClip, ClipOfALineByABoxHasHandComputedLength)
{
    // A 100 m line along y = 0 through a box from x = 10 to 40: 30 m of it.
    line(0, 0, 100, 0, "pipes");
    rect(10, -5, 40, 5, "site");
    const std::string reply = ok("GIS CLIP LAYERS pipes BY LAYERS site");
    const auto pieces = on("gis/clip");
    ASSERT_EQ(pieces.size(), 1u);
    EXPECT_NEAR(lengthOf(pieces.front()), 30.0, 1e-9);
    EXPECT_EQ(record(reply, "clip")->get("length"), "30.000");
    EXPECT_EQ(on("pipes").size(), 1u); // drawn beside, the pipe stays whole
}

TEST_F(GisClip, ClipInPlaceKeepsTheId)
{
    const EntityId pipe = line(0, 0, 100, 0, "pipes");
    rect(10, -5, 40, 5, "site");
    const std::string reply = ok("GIS CLIP LAYERS pipes BY LAYERS site REPLACE");
    const Entity* kept = document.model().entities.find(pipe);
    ASSERT_NE(kept, nullptr);
    EXPECT_NEAR(lengthOf(*kept), 30.0, 1e-9);
    const auto output = record(reply, "output");
    EXPECT_EQ(output->get("target"), "in-place");
    EXPECT_EQ(output->get("updated"), "1");
    EXPECT_TRUE(on("gis/clip").empty());
}

TEST_F(GisClip, APartSplitIntoTwoKeepsTheIdOnTheLargerAndReportsTheOther)
{
    // Boxes over x = 10..40 (30 m) and 60..95 (35 m): the pipe keeps its id
    // on the 35 m piece, and the 30 m one is made beside it.
    const EntityId pipe = line(0, 0, 100, 0, "pipes");
    rect(10, -5, 40, 5, "site");
    rect(60, -5, 95, 5, "site");
    const std::string reply = ok("GIS CLIP LAYERS pipes BY LAYERS site REPLACE");
    const auto pipes = on("pipes");
    ASSERT_EQ(pipes.size(), 2u);
    EXPECT_EQ(pipes[0].id, pipe);
    EXPECT_NEAR(lengthOf(pipes[0]), 35.0, 1e-9);
    EXPECT_NEAR(lengthOf(pipes[1]), 30.0, 1e-9);
    EXPECT_EQ(katana::entity::toString(pipes[1].properties.at("gis.source")), std::to_string(pipe));
    const auto split = record(reply, "split");
    ASSERT_TRUE(split) << reply;
    EXPECT_EQ(split->get("entity"), std::to_string(pipe));
    EXPECT_EQ(split->get("parts"), "2");
    EXPECT_EQ(split->get("created"), std::to_string(pipes[1].id));
}

TEST_F(GisClip, InPlaceWhatIsOutsideGoesAndWhatIsInsideStaysAsItWasInOneStep)
{
    const EntityId inside = line(15, 0, 35, 0, "pipes");
    const EntityId outside = line(0, 50, 100, 50, "pipes");
    rect(10, -5, 40, 5, "site");
    const Entity before = *document.model().entities.find(inside);
    const std::string reply = ok("GIS CLIP LAYERS pipes BY LAYERS site REPLACE");
    EXPECT_EQ(*document.model().entities.find(inside), before);
    EXPECT_FALSE(document.model().entities.contains(outside));
    const auto clip = record(reply, "clip");
    EXPECT_EQ(clip->get("whole"), "1");
    EXPECT_EQ(clip->get("outside"), "1");
    EXPECT_EQ(record(reply, "output")->get("deleted"), "1");
    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_TRUE(document.model().entities.contains(outside));
}

TEST_F(GisClip, AnAreaWhollyOutsideGoesWithItsHolesInOneStep)
{
    // A lot with a courtyard cut out of it (an area and its hole are
    // entities of their own, joined by the gis.ring tag), far from the site:
    // clipped in place, the lot is outside, and so is its hole - left
    // behind, the hole would become an area of its own the next time the
    // layer is read.
    const EntityId lot = rect(100, 100, 200, 200, "lots");
    const EntityId hole = rect(140, 140, 160, 160, "lots",
                               katana::entity::PropertyMap{{"gis.ring", std::string("hole")}});
    const EntityId kept = rect(10, 10, 20, 20, "lots");
    rect(0, 0, 50, 50, "site");
    const std::string reply = ok("GIS CLIP LAYERS lots BY LAYERS site REPLACE");
    EXPECT_FALSE(document.model().entities.contains(lot));
    EXPECT_FALSE(document.model().entities.contains(hole));
    EXPECT_TRUE(document.model().entities.contains(kept));
    EXPECT_EQ(record(reply, "clip")->get("outside"), "1") << reply;
    EXPECT_EQ(record(reply, "output")->get("deleted"), "2") << reply;
    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_TRUE(document.model().entities.contains(lot));
    EXPECT_TRUE(document.model().entities.contains(hole));
}

TEST_F(GisClip, AnInPlaceClipRefusesADrawingChangedWhileItRan)
{
    // Prepared, then the pipe moves before the apply: cutting it now would
    // write over the move, so the apply refuses and changes nothing.
    const EntityId pipe = line(0, 0, 100, 0, "pipes");
    rect(10, -5, 40, 5, "site");
    auto prepared =
        katana::app::geo::prepare(context, "GIS CLIP LAYERS pipes BY LAYERS site REPLACE");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    auto apply = prepared->work({}, {});
    ASSERT_TRUE(apply.ok()) << apply.error().describe();
    ASSERT_TRUE(
        document.execute(katana::commands::moveEntities({pipe}, katana::math::Vec2(1.0, 0.0)))
            .ok());
    const Entity moved = *document.model().entities.find(pipe);
    auto applied = (*apply)(context);
    ASSERT_FALSE(applied.ok());
    EXPECT_EQ(applied.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(*document.model().entities.find(pipe), moved);
}

TEST_F(GisClip, AHoleMovedWhileItRanIsNotDeletedWithItsArea)
{
    // The hole is deleted with its area, so an edit to the hole while the
    // job ran is a changed drawing too.
    rect(100, 100, 200, 200, "lots");
    const EntityId hole = rect(140, 140, 160, 160, "lots",
                               katana::entity::PropertyMap{{"gis.ring", std::string("hole")}});
    rect(0, 0, 50, 50, "site");
    auto prepared =
        katana::app::geo::prepare(context, "GIS CLIP LAYERS lots BY LAYERS site REPLACE");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    auto apply = prepared->work({}, {});
    ASSERT_TRUE(apply.ok()) << apply.error().describe();
    ASSERT_TRUE(
        document.execute(katana::commands::moveEntities({hole}, katana::math::Vec2(1.0, 0.0)))
            .ok());
    const std::size_t before = entityCount();
    auto applied = (*apply)(context);
    ASSERT_FALSE(applied.ok());
    EXPECT_EQ(applied.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(entityCount(), before);
}

TEST_F(GisClip, AFileBoundaryWorksLikeADrawnOne)
{
    // The corridor of lots.geojson runs from x = -10 to 110: a line from
    // -20 to 120 along its middle keeps 120 m.
    line(-20, 20, 120, 20, "pipes");
    const std::string reply = ok("GIS CLIP LAYERS pipes BY FILE \"" + kData +
                                 "/lots.geojson\" where=\"kind='corridor'\"");
    ASSERT_EQ(on("gis/clip").size(), 1u) << reply;
    EXPECT_NEAR(lengthOf(on("gis/clip").front()), 120.0, 1e-9);
}

TEST_F(GisClip, WhatItCannotDoIsRefusedByName)
{
    line(0, 0, 100, 0, "pipes");
    rect(10, -5, 40, 5, "site");
    const auto refused = [&](const std::string& line) {
        auto reply = run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument)
            << line << ": " << reply.error().describe();
    };
    refused("GIS CLIP LAYERS pipes");
    refused("GIS CLIP LAYERS pipes BY");
    refused("GIS CLIP LAYERS pipes BY LAYERS site REPLACE TO LAYER cut");
    refused("GIS CLIP LAYERS pipes BY LAYERS site where=\"a=1\"");
    refused("GIS HULL DRAWING concave=2");
    refused("GIS HULL DRAWING holes");
    refused("GIS HULL DRAWING convex concave=0.5");
}

TEST(GisHullClipContract, TheArgumentsTheVerbsBindAreGdals)
{
    using katana::geo_test::expectArgument;
    expectArgument({"vector", "concave-hull"}, "ratio", gp::ArgType::Real, true);
    expectArgument({"vector", "concave-hull"}, "allow-holes", gp::ArgType::Boolean, false);
    expectArgument({"vector", "clip"}, "like", gp::ArgType::Dataset, false);
    expectArgument({"vector", "clip"}, "like-layer", gp::ArgType::String, false);
    expectArgument({"vector", "clip"}, "like-where", gp::ArgType::String, false);
}

} // namespace
