// What a surface built from the drawing is built from
// (include/katana/cad/geo/surface_input.hpp, docs/terrain.md "Surfaces on
// every front end"): the rules the window's Surface From Drawing kept, moved
// to katana_cad so katana_cli, katana_mcp and the window share them, with the
// one change - an entity with no height is left out, not put on the datum.

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/geo/surface_input.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/selection.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/model.hpp"

namespace {

using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Point3;

Entity point(double x, double y, std::optional<double> z)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{Point2(x, y)};
    katana::entity::setHeights(entity.properties, {z});
    return entity;
}

Entity polyline(std::vector<Point2> vertices, bool closed,
                const std::vector<std::optional<double>>& heights)
{
    Entity entity;
    katana::geometry::Polyline2 line;
    line.vertices = std::move(vertices);
    line.closed = closed;
    entity.geometry = line;
    katana::entity::setHeights(entity.properties, heights);
    return entity;
}

// The entities into a drawing, one step, and their ids in order.
std::vector<EntityId> draw(katana::cad::Document& document, std::vector<Entity> entities)
{
    EXPECT_TRUE(document.execute(katana::commands::createEntities(std::move(entities))).ok());
    return document.lastCreatedEntities();
}

// The window's Surface From Drawing as it was before the rules moved
// (src/katana_qt/main_window.cpp, 2026-09-26), kept verbatim as the oracle:
// what the moved rules must give wherever the entities carry heights. It read
// every drawn entity; an entity with no height property at all went to the
// datum.
katana::terrain::TinInput windowRules(const katana::entity::Model& model)
{
    katana::terrain::TinInput input;
    model.entities.forEach([&](const Entity& entity) {
        if (!katana::cad::isDrawn(model, entity, katana::cad::kNoLayerOverrides)) {
            return;
        }
        const bool carriesHeights =
            entity.properties.contains(std::string(katana::entity::kElevationProperty)) ||
            entity.properties.contains(std::string(katana::entity::kElevationsProperty));
        const auto heightAt = [&](const std::vector<std::optional<double>>& heights,
                                  std::size_t index) -> std::optional<double> {
            if (!carriesHeights) {
                return 0.0;
            }
            return heights[index];
        };
        if (const auto* at = std::get_if<katana::entity::PointGeometry>(&entity.geometry)) {
            const auto heights = katana::entity::heightsOf(entity.properties, 1);
            if (const auto z = heightAt(heights, 0)) {
                input.points.push_back(Point3(at->position.x, at->position.y, *z));
            }
        } else if (const auto* line = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
            const auto heights = katana::entity::heightsOf(entity.properties, line->vertices.size());
            katana::terrain::Breakline breakline;
            const auto flush = [&] {
                if (breakline.vertices.size() >= 2) {
                    input.breaklines.push_back(breakline);
                }
                breakline.vertices.clear();
            };
            bool whole = true;
            for (std::size_t i = 0; i < line->vertices.size(); ++i) {
                const auto& vertex = line->vertices[i];
                const auto z = heightAt(heights, i);
                if (!z) {
                    whole = false;
                    flush();
                    continue;
                }
                breakline.vertices.push_back(Point3(vertex.x, vertex.y, *z));
                input.points.push_back(Point3(vertex.x, vertex.y, *z));
            }
            breakline.closed = line->closed && whole;
            flush();
        }
    });
    return input;
}

std::vector<EntityId> everyId(const katana::entity::Model& model)
{
    std::vector<EntityId> ids;
    model.entities.forEach([&](const Entity& entity) { ids.push_back(entity.id); });
    return ids;
}

void expectSamePoints(const std::vector<Point3>& a, const std::vector<Point3>& b)
{
    ASSERT_EQ(a.size(), b.size());
    for (std::size_t i = 0; i < a.size(); ++i) {
        EXPECT_EQ(a[i].x, b[i].x) << i;
        EXPECT_EQ(a[i].y, b[i].y) << i;
        EXPECT_EQ(a[i].z, b[i].z) << i;
    }
}

TEST(SurfaceInput, FourLevelledPointsAreFourSurveyPointsAndNoBreakline)
{
    katana::cad::Document document;
    const auto ids = draw(document, {point(0, 0, 100.0), point(10, 0, 101.0),
                                     point(10, 10, 102.0), point(0, 10, 101.0)});
    const auto input = katana::cad::geo::surfaceInput(document.model(), ids);
    ASSERT_EQ(input.input.points.size(), 4u);
    EXPECT_EQ(input.input.points[2].z, 102.0);
    EXPECT_TRUE(input.input.breaklines.empty());
    EXPECT_EQ(input.used, 4u);
    EXPECT_EQ(input.heightlessVertices, 0u);
    EXPECT_TRUE(input.skipped.empty());
}

TEST(SurfaceInput, AHeightlessPointIsLeftOutAndCounted)
{
    katana::cad::Document document;
    const auto ids = draw(document, {point(0, 0, 100.0), point(10, 0, 101.0),
                                     point(10, 10, 102.0), point(5, 5, std::nullopt)});
    const auto input = katana::cad::geo::surfaceInput(document.model(), ids);
    EXPECT_EQ(input.input.points.size(), 3u);
    EXPECT_EQ(input.used, 3u);
    EXPECT_EQ(input.heightlessVertices, 1u);
    ASSERT_TRUE(input.skipped.contains("heightless"));
    EXPECT_EQ(input.skipped.at("heightless"), 1u);
    for (const Point3& kept : input.input.points) {
        EXPECT_FALSE(kept.x == 5.0 && kept.y == 5.0) << "the heightless point is not at z = 0";
    }
}

TEST(SurfaceInput, ANullVertexBreaksTheBreaklineAndARingThatLostOneIsNotClosed)
{
    katana::cad::Document document;
    // A ring of four with its second vertex unsurveyed: the first vertex is a
    // run of one (no breakline), the third and fourth a run of two, and the
    // ring does not close across the gap.
    const auto ids = draw(document, {polyline({Point2(0, 0), Point2(10, 0), Point2(10, 10),
                                               Point2(0, 10)},
                                              true, {1.0, std::nullopt, 3.0, 4.0})});
    const auto input = katana::cad::geo::surfaceInput(document.model(), ids);
    EXPECT_EQ(input.input.points.size(), 3u);
    EXPECT_EQ(input.heightlessVertices, 1u);
    ASSERT_EQ(input.input.breaklines.size(), 1u);
    EXPECT_EQ(input.input.breaklines[0].vertices.size(), 2u);
    EXPECT_FALSE(input.input.breaklines[0].closed);
    EXPECT_EQ(input.used, 1u);
}

TEST(SurfaceInput, AWholeLevelledRingStaysClosed)
{
    katana::cad::Document document;
    const auto ids = draw(document, {polyline({Point2(0, 0), Point2(10, 0), Point2(10, 10)}, true,
                                              {1.0, 2.0, 3.0})});
    const auto input = katana::cad::geo::surfaceInput(document.model(), ids);
    ASSERT_EQ(input.input.breaklines.size(), 1u);
    EXPECT_TRUE(input.input.breaklines[0].closed);
    EXPECT_EQ(input.input.breaklines[0].vertices.size(), 3u);
}

TEST(SurfaceInput, ALineIsABreaklineThroughItsTwoEnds)
{
    katana::cad::Document document;
    Entity line;
    line.geometry = katana::geometry::Segment2{Point2(0, 0), Point2(10, 0)};
    katana::entity::setHeights(line.properties, {5.0, 6.0});
    const auto ids = draw(document, {line});
    const auto input = katana::cad::geo::surfaceInput(document.model(), ids);
    ASSERT_EQ(input.input.breaklines.size(), 1u);
    EXPECT_EQ(input.input.breaklines[0].vertices[1].z, 6.0);
    EXPECT_EQ(input.input.points.size(), 2u);
}

TEST(SurfaceInput, WhatNoSurfaceIsBuiltFromIsSkippedByItsTypeAndCounted)
{
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("TEXT 0,0 2.5 \"LOT 7\"").ok());
    ASSERT_TRUE(interpreter.run("CIRCLE 5,5 2").ok());
    ASSERT_TRUE(interpreter.run("CIRCLE 9,9 1").ok());
    const auto input = katana::cad::geo::surfaceInput(document.model(), everyId(document.model()));
    EXPECT_TRUE(input.input.points.empty());
    EXPECT_EQ(input.skipped.at("text"), 1u);
    EXPECT_EQ(input.skipped.at("circle"), 2u);
    EXPECT_EQ(input.used, 0u);
}

TEST(SurfaceInput, OnLevelledDataTheRulesGiveExactlyWhatTheWindowGave)
{
    katana::cad::Document document;
    (void)draw(document,
               {point(0, 0, 100.0), point(20, 0, 101.5), point(20, 20, 99.25),
                polyline({Point2(2, 2), Point2(8, 2), Point2(8, 8)}, false, {100.1, 100.2, 100.3}),
                polyline({Point2(12, 12), Point2(18, 12), Point2(18, 18), Point2(12, 18)}, true,
                         {99.0, 99.5, std::nullopt, 99.75}),
                polyline({Point2(3, 15), Point2(9, 15), Point2(9, 19)}, true,
                         {98.0, 98.0, 98.0})});
    const auto oracle = windowRules(document.model());
    const auto input = katana::cad::geo::surfaceInput(document.model(), everyId(document.model()));
    expectSamePoints(input.input.points, oracle.points);
    ASSERT_EQ(input.input.breaklines.size(), oracle.breaklines.size());
    for (std::size_t i = 0; i < oracle.breaklines.size(); ++i) {
        SCOPED_TRACE(i);
        expectSamePoints(input.input.breaklines[i].vertices, oracle.breaklines[i].vertices);
        EXPECT_EQ(input.input.breaklines[i].closed, oracle.breaklines[i].closed);
    }
    // By hand: 3 levelled points, then 3 + 3 + 3 levelled vertices; 3
    // breaklines (the ring that lost a vertex splits in two runs, of 2 and 1).
    // A point with no height at all is where the rules part (the next test).
    EXPECT_EQ(input.input.points.size(), 12u);
    EXPECT_EQ(input.input.breaklines.size(), 3u);
}

TEST(SurfaceInput, OnTheSitePlanTheHeightlessAreLeftOutWhereTheWindowPutThemOnTheDatum)
{
    // samples/site_plan is a 2D drawing: no entity carries a height. The
    // window made its three polylines' 13 vertices (5 + 4 + 4, LIST) a flat
    // surface at z = 0; the rules now leave them out and count them, with the
    // six lines' 12 ends. Counted by hand from LIST: 3 polylines and 6 lines
    // heightless, 1 arc, 3 circles, 4 texts, 3 dimensions.
    const std::filesystem::path copy =
        std::filesystem::temp_directory_path() / "katana-surface-input-site-plan";
    std::error_code error;
    std::filesystem::remove_all(copy, error);
    std::filesystem::copy(std::filesystem::path(KATANA_GIS_SAMPLES) / ".." / "site_plan", copy,
                          std::filesystem::copy_options::recursive, error);
    ASSERT_FALSE(error) << error.message();
    {
        katana::cad::Document document;
        katana::cad::CommandInterpreter interpreter(document);
        ASSERT_TRUE(interpreter.run("OPEN \"" + copy.generic_string() + "\"").ok());
        ASSERT_EQ(document.model().entities.size(), 20u);

        const auto oracle = windowRules(document.model());
        EXPECT_EQ(oracle.points.size(), 13u);
        EXPECT_EQ(oracle.breaklines.size(), 3u);
        for (const Point3& datum : oracle.points) {
            EXPECT_EQ(datum.z, 0.0);
        }

        const auto input =
            katana::cad::geo::surfaceInput(document.model(), everyId(document.model()));
        EXPECT_TRUE(input.input.points.empty());
        EXPECT_TRUE(input.input.breaklines.empty());
        EXPECT_EQ(input.heightlessVertices, 25u);
        EXPECT_EQ(input.skipped.at("heightless"), 9u);
        EXPECT_EQ(input.skipped.at("arc"), 1u);
        EXPECT_EQ(input.skipped.at("circle"), 3u);
        EXPECT_EQ(input.skipped.at("text"), 4u);
        EXPECT_EQ(input.skipped.at("dimension"), 3u);
    }
    std::filesystem::remove_all(copy, error);
}

} // namespace
