// GIS CHECK, GIS REPAIR and GIS COVERAGE (src/katana_app/geo/check_verbs.cpp,
// docs/geoprocessing.md "V4"): through the one executor, every expected value
// worked by hand beside it.

#include <algorithm>
#include <cmath>
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
using katana::entity::EntityId;
using katana::entity::PropertyMap;
using katana::geometry::Polyline2;

class GisCheck : public katana::geo_test::VectorFixture {
  protected:
    // (30,0) (40,10) (40,0) (30,10): the edges (30,0)-(40,10) and
    // (40,0)-(30,10) cross where x - 30 = y and 40 - x = y, at (35, 5).
    EntityId bowTie(PropertyMap properties = {})
    {
        return area({{30, 0}, {40, 10}, {40, 0}, {30, 10}}, "lots", std::move(properties));
    }

    std::vector<katana::app::geo::Record> problems(const std::string& reply) const
    {
        std::vector<katana::app::geo::Record> found;
        for (katana::app::geo::Record& one : katana::app::geo::parseRecords(reply)) {
            if (one.kind == "problem") {
                found.push_back(std::move(one));
            }
        }
        return found;
    }

    // Four lots about an enclosed gap 0.02 m wide: L (0..50) and R
    // (50.02..100) side by side, T above and B below them across the full
    // width, each with vertices where the others' corners meet it, so every
    // edge but the gap's matches.
    void gapFixture()
    {
        rect(0, 0, 50, 40, "lots", PropertyMap{{"name", std::string("L")}});
        rect(50.02, 0, 100, 40, "lots", PropertyMap{{"name", std::string("R")}});
        area({{0, 40}, {50, 40}, {50.02, 40}, {100, 40}, {100, 80}, {0, 80}}, "lots",
             PropertyMap{{"name", std::string("T")}});
        area({{0, -40}, {100, -40}, {100, 0}, {50.02, 0}, {50, 0}, {0, 0}}, "lots",
             PropertyMap{{"name", std::string("B")}});
    }
};

std::string text(const Entity& entity, const std::string& key)
{
    const auto found = entity.properties.find(key);
    return found == entity.properties.end() ? std::string("<none>")
                                            : katana::entity::toString(found->second);
}

TEST_F(GisCheck, ABowTieSelfIntersectsAt35Comma5)
{
    const EntityId tie = bowTie();
    rect(0, 0, 10, 10, "lots");
    const std::string reply = ok("GIS CHECK DRAWING");
    const auto found = problems(reply);
    ASSERT_EQ(found.size(), 1u) << reply;
    EXPECT_EQ(found.front().get("kind"), "self-intersection");
    EXPECT_EQ(found.front().get("entity"), std::to_string(tie));
    EXPECT_EQ(found.front().get("at"), "35,5");
    EXPECT_EQ(found.front().get("reason"), "Self-intersection");
    const auto check = record(reply, "check");
    ASSERT_TRUE(check);
    EXPECT_EQ(check->get("features"), "2");
    EXPECT_EQ(check->get("problems"), "1");
    // A check changes nothing without markers.
    EXPECT_EQ(entityCount(), 2u);
}

TEST_F(GisCheck, RepairOfTheBowTieGivesTwoTrianglesOf25SquareMetresEach)
{
    // Split at (35,5): two triangles of base 10 and height 5, 25 m2 each.
    const EntityId tie = bowTie();
    const std::string reply = ok("GIS REPAIR DRAWING");
    const auto lots = on("lots");
    ASSERT_EQ(lots.size(), 2u);
    for (const Entity& lot : lots) {
        EXPECT_NEAR(std::get<Polyline2>(lot.geometry).area(), 25.0, 1e-9);
    }
    const auto split = record(reply, "split");
    ASSERT_TRUE(split) << reply;
    EXPECT_EQ(split->get("entity"), std::to_string(tie));
    EXPECT_EQ(split->get("parts"), "2");
    EXPECT_EQ(split->get("created"), std::to_string(lots.back().id));
    const auto output = record(reply, "output");
    ASSERT_TRUE(output);
    EXPECT_EQ(output->get("target"), "in-place");
    EXPECT_EQ(output->get("updated"), "1");
    EXPECT_EQ(output->get("created"), "1");
    // Repaired, the check finds nothing.
    EXPECT_TRUE(problems(ok("GIS CHECK DRAWING")).empty());
}

TEST_F(GisCheck, RepairKeepsTheId)
{
    // The id - and so a label or an association on it - survives; the other
    // triangle is a copy of the lot, properties and all, naming its source.
    const EntityId tie = bowTie(PropertyMap{{"owner", std::string("Smith")}});
    const EntityId valid = rect(0, 0, 10, 10, "lots");
    const Entity squareBefore = *document.model().entities.find(valid);
    const std::string reply = ok("GIS REPAIR LAYERS lots");
    const Entity* kept = document.model().entities.find(tie);
    ASSERT_NE(kept, nullptr);
    EXPECT_NEAR(std::get<Polyline2>(kept->geometry).area(), 25.0, 1e-9);
    EXPECT_EQ(text(*kept, "owner"), "Smith");
    const Entity made = on("lots").back();
    EXPECT_NE(made.id, tie);
    EXPECT_EQ(text(made, "owner"), "Smith");
    EXPECT_EQ(text(made, "gis.source"), std::to_string(tie));
    // A valid area is not touched, not even rewritten.
    EXPECT_EQ(*document.model().entities.find(valid), squareBefore);
    EXPECT_EQ(record(reply, "repair")->get("valid"), "1");
    EXPECT_EQ(record(reply, "repair")->get("repaired"), "1");
    // One step: UNDO brings the bow-tie back whole.
    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_EQ(on("lots").size(), 2u);
    EXPECT_EQ(std::get<Polyline2>(document.model().entities.find(tie)->geometry).vertices.size(), 4u);
}

TEST_F(GisCheck, RepairRefusesADrawingChangedWhileItRan)
{
    const EntityId tie = bowTie();
    auto prepared = katana::app::geo::prepare(context, "GIS REPAIR DRAWING");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    auto apply = prepared->work({}, {});
    ASSERT_TRUE(apply.ok()) << apply.error().describe();
    ASSERT_TRUE(
        document.execute(katana::commands::moveEntities({tie}, katana::math::Vec2(1.0, 0.0))).ok());
    const Entity moved = *document.model().entities.find(tie);
    auto applied = (*apply)(context);
    ASSERT_FALSE(applied.ok());
    EXPECT_EQ(applied.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(*document.model().entities.find(tie), moved);
}

TEST_F(GisCheck, PreviewChangesNothing)
{
    bowTie();
    const std::size_t before = entityCount();
    const std::string reply = ok("GIS REPAIR DRAWING PREVIEW");
    EXPECT_TRUE(reply.starts_with("gis op=repair preview=yes")) << reply;
    EXPECT_EQ(entityCount(), before);
    EXPECT_NE(ok("GIS CHECK DRAWING markers=checks PREVIEW").find("preview valid=yes changed=no"),
              std::string::npos);
    EXPECT_TRUE(on("checks").empty());
}

TEST_F(GisCheck, MarkersAreReplacedOnRerun)
{
    bowTie();
    ok("GIS CHECK LAYERS lots markers=checks");
    ASSERT_EQ(on("checks").size(), 1u);
    const Entity marker = on("checks").front();
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(marker.geometry).position,
              katana::geometry::Point2(35, 5));
    EXPECT_EQ(text(marker, "gis.marker"), "check");
    EXPECT_EQ(text(marker, "gis.problem"), "self-intersection");
    // Again: the last check's marker gives way to this one's.
    const std::string again = ok("GIS CHECK LAYERS lots markers=checks");
    EXPECT_EQ(on("checks").size(), 1u);
    const auto output = record(again, "output");
    ASSERT_TRUE(output);
    EXPECT_EQ(output->get("arg"), "markers");
    EXPECT_EQ(output->get("created"), "1");
    EXPECT_EQ(output->get("deleted"), "1");
    // Repaired and checked again: no problem, and no marker left behind.
    ok("GIS REPAIR LAYERS lots");
    ok("GIS CHECK LAYERS lots markers=checks");
    EXPECT_TRUE(on("checks").empty());
}

TEST_F(GisCheck, OverlappingLotsReportTheirOverlapEdges)
{
    // A at 0..50 and B at 49..100 overlap by a metre: each has an edge
    // inside the other - A's at x = 50, B's at x = 49.
    const EntityId a = rect(0, 0, 50, 40, "lots");
    const EntityId b = rect(49, 0, 100, 40, "lots");
    const std::string reply = ok("GIS COVERAGE CHECK LAYERS lots");
    const auto found = problems(reply);
    ASSERT_EQ(found.size(), 2u) << reply;
    std::vector<std::string> entities;
    for (const auto& problem : found) {
        EXPECT_EQ(problem.get("kind"), "overlap") << reply;
        entities.push_back(problem.get("entity").value_or(""));
    }
    EXPECT_NE(std::ranges::find(entities, std::to_string(a)), entities.end());
    EXPECT_NE(std::ranges::find(entities, std::to_string(b)), entities.end());
    EXPECT_EQ(record(reply, "coverage")->get("overlaps"), "2");
}

TEST_F(GisCheck, AnEnclosedGapNarrowerThanGapIsReported)
{
    gapFixture();
    // Without gap= a gap is not looked for, and every edge matches.
    EXPECT_TRUE(problems(ok("GIS COVERAGE CHECK LAYERS lots")).empty());
    // The 0.02 m gap is narrower than 0.05: L's edge at x = 50 and R's at
    // x = 50.02 border it.
    const std::string reply = ok("GIS COVERAGE CHECK LAYERS lots gap=0.05");
    const auto found = problems(reply);
    ASSERT_EQ(found.size(), 2u) << reply;
    for (const auto& problem : found) {
        EXPECT_EQ(problem.get("kind"), "gap") << reply;
        EXPECT_EQ(problem.get("length"), "40.000");
    }
    const auto coverage = record(reply, "coverage");
    EXPECT_EQ(coverage->get("gaps"), "2");
    EXPECT_EQ(coverage->get("gap"), "0.05");
}

TEST_F(GisCheck, WhereAProblemIsIsSaidToTheMillimetre)
{
    // Four rectangles about a 0.02 m gap (the reviewer's case): the gap along
    // R's west edge, x = 50.02 from y = 0 to 40, is found at its middle,
    // (50.02, 20). GEOS works it out as 20.000000000000007, which the reply
    // printed; a coordinate is said to the millimetre, as length= is.
    rect(0, 0, 50, 40, "lots");
    const EntityId right = rect(50.02, 0, 100, 40, "lots");
    rect(0, 40, 100, 80, "lots");
    rect(0, -40, 100, 0, "lots");
    const std::string reply = ok("GIS COVERAGE CHECK LAYERS lots gap=0.05");
    bool found = false;
    for (const auto& problem : problems(reply)) {
        const std::string at = problem.get("at").value_or("");
        EXPECT_EQ(at.find("0000000"), std::string::npos) << reply;
        if (problem.get("entity") == std::to_string(right)) {
            EXPECT_EQ(at, "50.02,20") << reply;
            found = true;
        }
    }
    EXPECT_TRUE(found) << reply;
}

TEST_F(GisCheck, CleanWithoutReplaceIsRefused)
{
    gapFixture();
    const std::size_t before = entityCount();
    auto refused = run("GIS COVERAGE CLEAN LAYERS lots gap=0.05");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("REPLACE"), std::string::npos);
    EXPECT_EQ(entityCount(), before);
}

TEST_F(GisCheck, CleanClosesAnEnclosedGapAndOneUndoRestoresIt)
{
    // The four lots cover 100 x 120 = 12000 m2 less the gap's 0.02 x 40 =
    // 0.8 m2. Cleaned, one of L and R takes the gap: 12000 m2, no gap left.
    gapFixture();
    EXPECT_NEAR(areaOn("lots"), 12000.0 - 0.8, 1e-9);
    const std::string reply = ok("GIS COVERAGE CLEAN LAYERS lots gap=0.05 REPLACE");
    EXPECT_NEAR(areaOn("lots"), 12000.0, 1e-9);
    EXPECT_EQ(record(reply, "coverage")->get("changed"), "1") << reply;
    EXPECT_NE(reply.find("boundaries moved"), std::string::npos);
    EXPECT_TRUE(problems(ok("GIS COVERAGE CHECK LAYERS lots gap=0.05")).empty());
    EXPECT_EQ(on("lots").size(), 4u);
    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_NEAR(areaOn("lots"), 12000.0 - 0.8, 1e-9);
    EXPECT_EQ(problems(ok("GIS COVERAGE CHECK LAYERS lots gap=0.05")).size(), 2u);
}

TEST_F(GisCheck, CleanRefusesADrawingChangedWhileItRan)
{
    // Prepared, then lot L moves before the apply: writing the cleaned
    // boundaries now would undo the move, so the apply refuses and changes
    // nothing.
    gapFixture();
    const EntityId left = on("lots").front().id;
    auto prepared =
        katana::app::geo::prepare(context, "GIS COVERAGE CLEAN LAYERS lots gap=0.05 REPLACE");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    auto apply = prepared->work({}, {});
    ASSERT_TRUE(apply.ok()) << apply.error().describe();
    ASSERT_TRUE(
        document.execute(katana::commands::moveEntities({left}, katana::math::Vec2(0.0, 1.0)))
            .ok());
    const std::vector<Entity> moved = on("lots");
    auto applied = (*apply)(context);
    ASSERT_FALSE(applied.ok());
    EXPECT_EQ(applied.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(on("lots"), moved);
}

TEST_F(GisCheck, MixedGeometryInScopeIsFilteredAndCounted)
{
    // check-coverage refuses anything but areas: the line and the point are
    // left out, counted and said, and the lot is checked.
    rect(0, 0, 50, 40, "lots");
    line(0, 50, 100, 50, "lots");
    add(katana::entity::PointGeometry{{5, 5}}, "lots");
    const std::string reply = ok("GIS COVERAGE CHECK LAYERS lots");
    const auto coverage = record(reply, "coverage");
    ASSERT_TRUE(coverage) << reply;
    EXPECT_EQ(coverage->get("areas"), "1");
    EXPECT_EQ(coverage->get("ignored"), "2");
    EXPECT_EQ(coverage->get("problems"), "0");
    EXPECT_NE(reply.find("are not areas"), std::string::npos);
}

TEST_F(GisCheck, CoverageMarkersAreTheBadEdges)
{
    rect(0, 0, 50, 40, "lots");
    rect(49, 0, 100, 40, "lots");
    ok("GIS COVERAGE CHECK LAYERS lots markers=coverage");
    const auto markers = on("coverage");
    ASSERT_EQ(markers.size(), 2u);
    for (const Entity& marker : markers) {
        EXPECT_TRUE(std::holds_alternative<Polyline2>(marker.geometry));
        EXPECT_EQ(text(marker, "gis.marker"), "coverage");
        EXPECT_EQ(text(marker, "gis.problem"), "overlap");
    }
}

TEST_F(GisCheck, WhatItCannotDoIsRefusedByName)
{
    rect(0, 0, 50, 40, "lots");
    const auto refused = [&](const std::string& line, ErrorCode code) {
        auto reply = run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, code) << line << ": " << reply.error().describe();
    };
    refused("GIS REPAIR DRAWING method=glue", ErrorCode::InvalidArgument);
    refused("GIS COVERAGE DRAWING", ErrorCode::InvalidArgument);
    refused("GIS COVERAGE CHECK DRAWING gap=-1", ErrorCode::InvalidArgument);
    refused("GIS COVERAGE CHECK DRAWING snap=1", ErrorCode::InvalidArgument); // CLEAN's word
    refused("GIS COVERAGE CLEAN DRAWING markers=m REPLACE", ErrorCode::InvalidArgument);
    refused("GIS CHECK DRAWING markers=/bad//layer", ErrorCode::InvalidArgument);
}

TEST(GisCheckContract, TheArgumentsTheVerbsBindAreGdals)
{
    using katana::geo_test::expectArgument;
    expectArgument({"vector", "check-geometry"}, "include-field", gp::ArgType::StringList, false);
    expectArgument({"vector", "make-valid"}, "method", gp::ArgType::String, false);
    expectArgument({"vector", "check-coverage"}, "include-valid", gp::ArgType::Boolean, false);
    expectArgument({"vector", "check-coverage"}, "maximum-gap-width", gp::ArgType::Real, false);
    expectArgument({"vector", "clean-coverage"}, "maximum-gap-width", gp::ArgType::Real, false);
    expectArgument({"vector", "clean-coverage"}, "snapping-distance", gp::ArgType::Real, false);
    expectArgument({"vector", "clean-coverage"}, "merge-strategy", gp::ArgType::String, false);
}

TEST(GisCheckSession, TheVerbsRunThroughTheSessionAndHelpNamesThem)
{
    katana::app::Session session(nullptr);
    EXPECT_TRUE(session.run("RECT 0,0 50,40"));
    EXPECT_TRUE(session.run("GIS CHECK DRAWING"));
    EXPECT_TRUE(session.run("GIS COVERAGE CHECK DRAWING"));
    EXPECT_FALSE(session.run("GIS COVERAGE CLEAN DRAWING"));
    const std::string help = katana::app::Session::helpText();
    for (const char* verb : {"GIS CHECK", "GIS REPAIR", "GIS COVERAGE CHECK"}) {
        EXPECT_NE(help.find(verb), std::string::npos) << verb;
    }
}

} // namespace
