// GIS BUFFER and GIS DISSOLVE (src/katana_app/geo/buffer_verbs.cpp,
// docs/geoprocessing.md "V1"): run through the one executor as the window and
// the session run them, every expected value worked by hand beside it.

#include <algorithm>
#include <cmath>
#include <numbers>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "contract_support.hpp"
#include "katana/gis/processing.hpp"
#include "session.hpp"
#include "vector_fixture.hpp"

namespace {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::PropertyMap;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

class GisBuffer : public katana::geo_test::VectorFixture {};
class GisDissolve : public katana::geo_test::VectorFixture {};

// The one closed polyline on `layer`.
Polyline2 onlyRing(const std::vector<Entity>& entities)
{
    EXPECT_EQ(entities.size(), 1u);
    if (entities.empty()) {
        return {};
    }
    const auto* ring = std::get_if<Polyline2>(&entities.front().geometry);
    EXPECT_NE(ring, nullptr);
    return ring == nullptr ? Polyline2{} : *ring;
}

std::string text(const Entity& entity, const std::string& key)
{
    const auto found = entity.properties.find(key);
    return found == entity.properties.end() ? std::string("<none>")
                                            : katana::entity::toString(found->second);
}

TEST_F(GisBuffer, AHundredMetreLineBufferedOneMetreFlatIs200SquareMetres)
{
    // By hand: 100 m long, 1 m either side, no caps: a 100 x 2 rectangle.
    const auto source = line(0, 0, 100, 0);
    const std::string reply = ok("GIS BUFFER DRAWING distance=1 caps=flat");
    const Polyline2 ring = onlyRing(on("gis/buffer"));
    EXPECT_TRUE(ring.closed);
    EXPECT_NEAR(ring.area(), 200.0, 1e-9);
    const auto scope = record(reply, "scope");
    ASSERT_TRUE(scope);
    EXPECT_EQ(scope->get("matched"), "1");
    EXPECT_EQ(scope->get("lines"), "1");
    const auto output = record(reply, "output");
    ASSERT_TRUE(output);
    EXPECT_EQ(output->get("layer"), "gis/buffer");
    EXPECT_EQ(output->get("created"), "1");
    const auto buffer = record(reply, "buffer");
    ASSERT_TRUE(buffer);
    EXPECT_EQ(buffer->get("area"), "200.000");
    EXPECT_EQ(buffer->get("caps"), "flat");
    // The source is named, and the algorithm; a measure kept on an entity
    // would go stale, so none is written.
    const Entity result = on("gis/buffer").front();
    EXPECT_EQ(text(result, "gis.op"), "vector buffer");
    EXPECT_EQ(text(result, "gis.source"), std::to_string(source));
    EXPECT_EQ(text(result, "area"), "<none>");
}

TEST_F(GisBuffer, AMinusThreeMetreMitreSetbackOfATwentyByFortyLotIs14By34)
{
    // By hand: 3 m in from every side of a 20 x 40 lot leaves 14 x 34 =
    // 476 m2, from (3,3) to (17,37); mitred corners stay square.
    rect(0, 0, 20, 40, "lots");
    ok("GIS BUFFER LAYERS lots distance=-3 joins=mitre TO LAYER setbacks");
    const Polyline2 ring = onlyRing(on("setbacks"));
    EXPECT_NEAR(ring.area(), 476.0, 1e-9);
    const katana::geometry::Box2 box = ring.boundingBox();
    EXPECT_NEAR(box.min.x, 3.0, 1e-9);
    EXPECT_NEAR(box.min.y, 3.0, 1e-9);
    EXPECT_NEAR(box.max.x, 17.0, 1e-9);
    EXPECT_NEAR(box.max.y, 37.0, 1e-9);
    EXPECT_EQ(ring.withoutDuplicateVertices().vertices.size(), 4u);
}

TEST_F(GisBuffer, LeftOfAWestToEastLineIsNorth)
{
    // Walking west to east, the left hand points north: y from 0 to 2, and
    // right is south. One-sided, the ends are flat: 100 x 2 = 200 m2 each.
    line(0, 0, 100, 0);
    ok("GIS BUFFER DRAWING distance=2 side=left TO LAYER left");
    ok("GIS BUFFER LAYERS 0 distance=2 side=right TO LAYER right");
    const Polyline2 left = onlyRing(on("left"));
    const Polyline2 right = onlyRing(on("right"));
    EXPECT_NEAR(left.boundingBox().min.y, 0.0, 1e-9);
    EXPECT_NEAR(left.boundingBox().max.y, 2.0, 1e-9);
    EXPECT_NEAR(right.boundingBox().min.y, -2.0, 1e-9);
    EXPECT_NEAR(right.boundingBox().max.y, 0.0, 1e-9);
    EXPECT_NEAR(left.area(), 200.0, 1e-9);
    EXPECT_NEAR(right.area(), 200.0, 1e-9);
}

TEST_F(GisBuffer, APerEntityDistanceFromAPropertyIsUsed)
{
    // Clearances of 1 m (a real) and 3 m (a whole number) on two 100 m
    // lines, flat caps: 200 and 600 m2. A line with no clearance is skipped
    // and counted, not buffered by some default.
    const auto one = line(0, 0, 100, 0, "0", PropertyMap{{"clearance", 1.0}});
    const auto three = line(0, 50, 100, 50, "0", PropertyMap{{"clearance", std::int64_t{3}}});
    line(0, 100, 100, 100);
    const std::string reply = ok("GIS BUFFER DRAWING distance=prop:clearance caps=flat");
    const auto results = on("gis/buffer");
    ASSERT_EQ(results.size(), 2u);
    for (const Entity& result : results) {
        const double area = std::get<Polyline2>(result.geometry).area();
        if (text(result, "gis.source") == std::to_string(one)) {
            EXPECT_NEAR(area, 200.0, 1e-9);
            EXPECT_EQ(text(result, "clearance"), "1");
        } else {
            EXPECT_EQ(text(result, "gis.source"), std::to_string(three));
            EXPECT_NEAR(area, 600.0, 1e-9);
        }
    }
    const auto buffer = record(reply, "buffer");
    ASSERT_TRUE(buffer);
    EXPECT_EQ(buffer->get("distance"), "prop:clearance");
    EXPECT_EQ(buffer->get("groups"), "2");
    EXPECT_EQ(buffer->get("skipped.no_distance"), "1");
    EXPECT_EQ(buffer->get("area"), "800.000");
}

TEST_F(GisBuffer, ARoundPointBufferIsAnExactCircle)
{
    // A point buffered 5 m with round caps is the circle of radius 5 about
    // it, pi x 25 = 78.540 m2 - drawn as a circle, not as chords.
    const auto source = add(katana::entity::PointGeometry{{10, 20}}, "trees",
                            PropertyMap{{"species", std::string("fig")}});
    const std::string reply = ok("GIS BUFFER LAYERS trees distance=5 TO LAYER tpz");
    const auto results = on("tpz");
    ASSERT_EQ(results.size(), 1u);
    const auto* circle = std::get_if<katana::geometry::Circle2>(&results.front().geometry);
    ASSERT_NE(circle, nullptr);
    EXPECT_EQ(circle->center, Point2(10, 20));
    EXPECT_EQ(circle->radius, 5.0);
    EXPECT_EQ(text(results.front(), "species"), "fig");
    EXPECT_EQ(text(results.front(), "gis.source"), std::to_string(source));
    const auto buffer = record(reply, "buffer");
    ASSERT_TRUE(buffer);
    EXPECT_EQ(buffer->get("circles"), "1");
    EXPECT_EQ(buffer->get("area"), "78.540");
    EXPECT_NEAR(areaOn("tpz"), 25.0 * std::numbers::pi, 1e-12);
}

TEST_F(GisBuffer, ALineBufferedInwardsIsNothingAndSaysSo)
{
    // GEOS buffers a line inwards to an empty polygon: nothing is drawn, and
    // the reply counts it rather than inventing a shape.
    line(0, 0, 100, 0);
    const std::string reply = ok("GIS BUFFER DRAWING distance=-1");
    EXPECT_TRUE(on("gis/buffer").empty());
    const auto buffer = record(reply, "buffer");
    ASSERT_TRUE(buffer);
    EXPECT_EQ(buffer->get("empty"), "1");
    EXPECT_EQ(buffer->get("features"), "0");
}

TEST_F(GisBuffer, DissolvedBuffersOfCrossingLinesAreOneArea)
{
    // Two 100 m lines crossing at right angles, 1 m either side with flat
    // caps: two 200 m2 strips sharing a 2 x 2 square, 400 - 4 = 396 m2 as
    // one area.
    line(0, 50, 100, 50);
    line(50, 0, 50, 100);
    const std::string reply = ok("GIS BUFFER DRAWING distance=1 caps=flat DISSOLVE");
    EXPECT_EQ(on("gis/buffer").size(), 1u);
    EXPECT_NEAR(areaOn("gis/buffer"), 396.0, 1e-9);
    EXPECT_EQ(record(reply, "buffer")->get("dissolve"), "all");
}

TEST_F(GisBuffer, OptionsAfterAWhereFilterAreTheVerbsOwn)
{
    // The filter ends where its conditions do: distance= and caps= are the
    // buffer's, and only the line matches TYPE=line.
    line(0, 0, 100, 0);
    rect(0, 10, 10, 20);
    const std::string reply = ok("GIS BUFFER DRAWING WHERE TYPE=line distance=1 caps=flat");
    EXPECT_EQ(record(reply, "scope")->get("matched"), "1");
    EXPECT_NEAR(areaOn("gis/buffer"), 200.0, 1e-9);
}

TEST_F(GisBuffer, PreviewChangesNothing)
{
    line(0, 0, 100, 0);
    const std::size_t before = entityCount();
    const std::string reply = ok("GIS BUFFER DRAWING distance=1 PREVIEW");
    EXPECT_EQ(entityCount(), before);
    EXPECT_TRUE(reply.starts_with("gis op=buffer preview=yes")) << reply;
    EXPECT_NE(reply.find("preview valid=yes changed=no"), std::string::npos);
    EXPECT_EQ(record(reply, "scope")->get("matched"), "1");
}

TEST_F(GisBuffer, AScopeThatTakesNothingIsReportedNotAnError)
{
    line(0, 0, 100, 0);
    const std::string reply = ok("GIS BUFFER AREA 1000,1000,1010,1010 distance=1");
    EXPECT_TRUE(reply.starts_with("gis op=buffer seconds=0.000 cancelled=no ran=no")) << reply;
    EXPECT_EQ(record(reply, "scope")->get("matched"), "0");
    EXPECT_TRUE(on("gis/buffer").empty());
}

TEST_F(GisBuffer, WhatItCannotDoIsRefusedByName)
{
    line(0, 0, 100, 0);
    const auto refused = [&](const std::string& words, ErrorCode code) {
        auto reply = run("GIS BUFFER DRAWING " + words);
        ASSERT_FALSE(reply.ok()) << words;
        EXPECT_EQ(reply.error().code, code) << words << ": " << reply.error().describe();
    };
    refused("caps=flat", ErrorCode::InvalidArgument);          // no distance
    refused("distance=0", ErrorCode::InvalidArgument);         // nothing to buffer by
    refused("distance=wide", ErrorCode::InvalidArgument);      // not a number
    refused("distance=1 side=up", ErrorCode::InvalidArgument); // not a side
    refused("distance=1 distance=2", ErrorCode::InvalidArgument);
    refused("distance=prop:clearance", ErrorCode::InvalidArgument); // no such property
    refused("distance=1 dissolve=owner", ErrorCode::InvalidArgument);
    refused("distance=1 TO REFERENCE shade", ErrorCode::Unsupported);
    refused("distance=1 banana", ErrorCode::InvalidArgument);
    EXPECT_TRUE(on("gis/buffer").empty());
}

TEST_F(GisDissolve, DissolveByOwnerMergesAdjacentLots)
{
    // Two 50 x 40 lots of Smith's sharing an edge: one 100 x 40 area of
    // 4000 m2; Jones's lot on its own stays 2000 m2.
    rect(0, 0, 50, 40, "lots", PropertyMap{{"owner", std::string("Smith")}});
    rect(50, 0, 100, 40, "lots", PropertyMap{{"owner", std::string("Smith")}});
    rect(0, 100, 50, 140, "lots", PropertyMap{{"owner", std::string("Jones")}});
    const std::string reply = ok("GIS DISSOLVE LAYERS lots by=owner");
    const auto results = on("gis/dissolve");
    ASSERT_EQ(results.size(), 2u);
    for (const Entity& result : results) {
        const double area = std::get<Polyline2>(result.geometry).area();
        EXPECT_NEAR(area, text(result, "owner") == "Smith" ? 4000.0 : 2000.0, 1e-9)
            << text(result, "owner");
    }
    const auto dissolve = record(reply, "dissolve");
    ASSERT_TRUE(dissolve);
    EXPECT_EQ(dissolve->get("groups"), "2");
    EXPECT_EQ(dissolve->get("areas"), "3");
    EXPECT_EQ(dissolve->get("area"), "6000.000");
    EXPECT_EQ(on("lots").size(), 3u); // without REPLACE the lots stay
}

TEST_F(GisDissolve, KeepIdenticalKeepsOnlySharedValues)
{
    // Both lots are zoned R1 and differ in name: keep=identical carries the
    // zone to the merged area and leaves the names behind.
    rect(0, 0, 50, 40, "lots",
         PropertyMap{{"owner", std::string("Smith")}, {"zone", std::string("R1")},
                     {"name", std::string("A")}});
    rect(50, 0, 100, 40, "lots",
         PropertyMap{{"owner", std::string("Smith")}, {"zone", std::string("R1")},
                     {"name", std::string("B")}});
    ok("GIS DISSOLVE LAYERS lots by=owner keep=identical TO LAYER kept");
    ok("GIS DISSOLVE LAYERS lots by=owner TO LAYER plain");
    const auto kept = on("kept");
    ASSERT_EQ(kept.size(), 1u);
    EXPECT_EQ(text(kept.front(), "owner"), "Smith");
    EXPECT_EQ(text(kept.front(), "zone"), "R1");
    EXPECT_EQ(text(kept.front(), "name"), "<none>");
    const auto plain = on("plain");
    ASSERT_EQ(plain.size(), 1u);
    EXPECT_EQ(text(plain.front(), "owner"), "Smith");
    EXPECT_EQ(text(plain.front(), "zone"), "<none>");
}

TEST_F(GisDissolve, ReplaceDeletesTheSourcesInTheSameUndoStep)
{
    rect(0, 0, 50, 40, "lots");
    rect(50, 0, 100, 40, "lots");
    const std::string reply = ok("GIS DISSOLVE LAYERS lots REPLACE");
    EXPECT_TRUE(on("lots").empty());
    EXPECT_NEAR(areaOn("gis/dissolve"), 4000.0, 1e-9);
    EXPECT_EQ(record(reply, "output")->get("deleted"), "2");
    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_EQ(on("lots").size(), 2u);
    EXPECT_TRUE(on("gis/dissolve").empty());
}

TEST_F(GisDissolve, PreviewChangesNothingAndLinesAreNotMerged)
{
    rect(0, 0, 50, 40, "lots");
    line(0, 50, 100, 50, "lots");
    const std::size_t before = entityCount();
    const std::string reply = ok("GIS DISSOLVE LAYERS lots REPLACE PREVIEW");
    EXPECT_EQ(entityCount(), before);
    EXPECT_TRUE(reply.starts_with("gis op=dissolve preview=yes")) << reply;
    EXPECT_EQ(record(reply, "dissolve")->get("areas"), "1");
    EXPECT_NE(reply.find("points and lines in the scope are not areas"), std::string::npos);
}

TEST_F(GisDissolve, AnInPlaceReplaceRefusesADrawingChangedWhileItRan)
{
    // Prepared, then a lot moves before the apply: deleting it now would
    // lose the move, so the apply refuses and changes nothing.
    rect(0, 0, 50, 40, "lots");
    const auto moved = rect(50, 0, 100, 40, "lots");
    auto prepared = katana::app::geo::prepare(context, "GIS DISSOLVE LAYERS lots REPLACE");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    auto apply = prepared->work({}, {});
    ASSERT_TRUE(apply.ok()) << apply.error().describe();
    ASSERT_TRUE(document
                    .execute(katana::commands::moveEntities({moved}, katana::math::Vec2(1.0, 0.0)))
                    .ok());
    const std::size_t before = entityCount();
    auto applied = (*apply)(context);
    ASSERT_FALSE(applied.ok());
    EXPECT_EQ(applied.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(entityCount(), before);
}

TEST(GeoLineWord, ABlankAnEmptyWordAndAKeywordAreQuotedAndAQuoteIsRefused)
{
    using katana::app::geo::lineWord;
    EXPECT_EQ(lineWord("gis/easement"), "gis/easement");
    EXPECT_EQ(lineWord("set backs"), "\"set backs\"");
    EXPECT_EQ(lineWord(""), "\"\"");
    for (const char* keyword : {"preview", "PREVIEW", "Confirm", "to", "FROM", "overwrite",
                                "replace", "name", "minus", "where", "reference"}) {
        EXPECT_EQ(lineWord(keyword), "\"" + std::string(keyword) + "\"") << keyword;
    }
    EXPECT_FALSE(lineWord("a\"b").has_value());
    EXPECT_FALSE(lineWord("a\nb").has_value());
}

TEST_F(GisBuffer, AnOutputNamedLikeAKeywordGoesWhereItSaysOnceQuoted)
{
    // TO LAYER preview was refused ("needs the layer's path"), and a dialog
    // or an agent that wrote a name as it was typed wrote exactly that.
    // Quoted by the one rule, the name is a name.
    line(0, 0, 100, 0, "pipes");
    auto bare = run("GIS BUFFER LAYERS pipes distance=1 TO LAYER preview");
    EXPECT_FALSE(bare.ok());
    for (const char* name : {"preview", "CONFIRM", "overwrite", "from"}) {
        const std::string reply =
            ok("GIS BUFFER LAYERS pipes distance=1 TO LAYER " + *katana::app::geo::lineWord(name));
        EXPECT_EQ(on(name).size(), 1u) << reply;
    }
}

TEST(GisBufferContract, TheArgumentsTheVerbsBindAreGdals)
{
    using katana::geo_test::expectArgument;
    expectArgument({"vector", "buffer"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"vector", "buffer"}, "distance", gp::ArgType::Real, true);
    expectArgument({"vector", "buffer"}, "endcap-style", gp::ArgType::String, false);
    expectArgument({"vector", "buffer"}, "join-style", gp::ArgType::String, false);
    expectArgument({"vector", "buffer"}, "side", gp::ArgType::String, false);
    expectArgument({"vector", "buffer"}, "quadrant-segments", gp::ArgType::Integer, false);
    expectArgument({"vector", "combine"}, "group-by", gp::ArgType::StringList, false);
    expectArgument({"vector", "combine"}, "add-extra-fields", gp::ArgType::String, false);
    expectArgument({"vector", "dissolve"}, "input", gp::ArgType::DatasetList, true);
    // The words the verbs pass are among GDAL's choices.
    const auto buffer = gp::describe({"vector", "buffer"});
    ASSERT_TRUE(buffer.ok());
    for (const gp::ArgSpec& arg : buffer->args) {
        if (arg.name == "join-style") {
            EXPECT_NE(std::ranges::find(arg.choices, std::string("mitre")), arg.choices.end());
        }
        if (arg.name == "side") {
            EXPECT_NE(std::ranges::find(arg.choices, std::string("left")), arg.choices.end());
        }
    }
}

TEST(GisBufferSession, TheVerbsRunThroughTheSessionAndHelpNamesThem)
{
    // katana_cli's and katana_mcp's path: the session asks the executor.
    katana::app::Session session(nullptr);
    EXPECT_TRUE(session.run("LINE 0,0 100,0"));
    EXPECT_TRUE(session.run("GIS BUFFER DRAWING distance=1 caps=flat"));
    EXPECT_FALSE(session.run("GIS BUFFER DRAWING"));
    const std::string help = katana::app::Session::helpText();
    EXPECT_NE(help.find("GIS BUFFER"), std::string::npos);
    EXPECT_NE(help.find("GIS DISSOLVE"), std::string::npos);
}

} // namespace
