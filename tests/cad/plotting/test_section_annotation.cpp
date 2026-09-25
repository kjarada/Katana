// What a section says besides its lines
// (include/katana/cad/plotting/section_annotation.hpp): the design and ground
// series, cut and fill between them, crossings' own levels and depths, and
// label placement. Sections are built by hand, sample by sample, so every
// expected number is worked on paper.

#include <gtest/gtest.h>

#include <cmath>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/plotting/section_annotation.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"

using katana::cad::LayerOverrides;
using katana::cad::Section;
using katana::cad::SectionCrossing;
using katana::cad::SectionSample;
using katana::cad::SectionSurface;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::Model;
using namespace katana::cad::plotting;

namespace {

SectionSurface series(std::string name, std::vector<std::pair<double, std::optional<double>>> at)
{
    SectionSurface surface;
    surface.name = std::move(name);
    for (const auto& [station, level] : at) {
        SectionSample sample;
        sample.station = station;
        sample.elevation = level;
        surface.samples.push_back(sample);
    }
    return surface;
}

EntityId addEntity(Model& model, katana::entity::Geometry geometry,
                   std::vector<std::optional<double>> heights, std::string layer = "WATER")
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = std::move(layer);
    katana::entity::setHeights(entity.properties, heights);
    auto id = model.entities.add(std::move(entity));
    EXPECT_TRUE(id.ok());
    return id.ok() ? *id : katana::entity::kInvalidEntityId;
}

SectionCrossing crossingOf(EntityId id, Point2 plan, double station, std::string layer = "WATER",
                           std::optional<double> draped = std::nullopt)
{
    SectionCrossing crossing;
    crossing.entity = id;
    crossing.layer = std::move(layer);
    crossing.plan = plan;
    crossing.station = station;
    crossing.elevation = draped;
    return crossing;
}

} // namespace

// ---- the series ------------------------------------------------------------------------

TEST(SectionAnnotation, TheDesignIsTheSeriesNamedDesignInAnyCase)
{
    EXPECT_TRUE(isDesignSeries("design ROAD"));
    EXPECT_TRUE(isDesignSeries("DESIGN"));
    EXPECT_TRUE(isDesignSeries("Design finished surface"));
    EXPECT_FALSE(isDesignSeries("GROUND"));
    EXPECT_FALSE(isDesignSeries("desi"));
    EXPECT_FALSE(isDesignSeries(""));

    Section section;
    section.surfaces = {series("GROUND", {}), series("ROCK", {}), series("design ROAD", {})};
    ASSERT_NE(designSeries(section), nullptr);
    EXPECT_EQ(designSeries(section)->name, "design ROAD");
    ASSERT_NE(groundSeries(section), nullptr);
    EXPECT_EQ(groundSeries(section)->name, "GROUND");
    section.surfaces = {series("design ROAD", {})};
    EXPECT_EQ(groundSeries(section), nullptr);
}

TEST(SectionAnnotation, ALevelIsStraightBetweenSamplesAndNothingInAGap)
{
    const SectionSurface ground =
        series("GROUND", {{0.0, 10.0}, {10.0, 12.0}, {20.0, std::nullopt}, {30.0, 15.0}});
    EXPECT_NEAR(*levelAt(ground, 5.0), 11.0, 1e-12);
    EXPECT_EQ(*levelAt(ground, 0.0), 10.0);
    EXPECT_EQ(*levelAt(ground, 30.0), 15.0);
    EXPECT_FALSE(levelAt(ground, 15.0)); // into the gap
    EXPECT_FALSE(levelAt(ground, 20.0));
    EXPECT_FALSE(levelAt(ground, -0.1)); // before the start
    EXPECT_FALSE(levelAt(ground, 30.1)); // past the end
    EXPECT_FALSE(levelAt(series("EMPTY", {}), 0.0));
    // A hair past either end - a range's end summed from a section's length
    // - reads the end; a millimetre past does not.
    EXPECT_EQ(*levelAt(ground, 30.0 + 1e-9), 15.0);
    EXPECT_EQ(*levelAt(ground, -1e-9), 10.0);
    EXPECT_FALSE(levelAt(ground, 30.001));
    EXPECT_FALSE(levelAt(ground, -0.001));
}

// ---- cut and fill ------------------------------------------------------------------------

TEST(SectionAnnotation, CutAndFillAreDesignLessGround)
{
    const SectionSurface design = series("design", {{0.0, 10.0}, {10.0, 10.0}});
    const SectionSurface ground = series("GROUND", {{0.0, 9.0}, {10.0, 11.0}});
    EXPECT_NEAR(*cutFillAt(design, ground, 0.0), 1.0, 1e-12);   // fill
    EXPECT_NEAR(*cutFillAt(design, ground, 10.0), -1.0, 1e-12); // cut
    EXPECT_NEAR(*cutFillAt(design, ground, 5.0), 0.0, 1e-12);
    EXPECT_FALSE(cutFillAt(design, ground, 11.0));

    EXPECT_EQ(cutFillText(0.25), "+0.250");
    EXPECT_EQ(cutFillText(-1.2), "-1.200");
    EXPECT_EQ(cutFillText(0.0), "0.000");
    EXPECT_EQ(cutFillText(-0.0004), "0.000"); // never "-0.000"
}

TEST(SectionAnnotation, WhereTheDesignCrossesTheGroundTheRegionSplits)
{
    // Design flat at 10 over 0..10; ground rising from 9 to 11. They cross
    // at 5: fill before (design above), cut after, each a triangle of
    // 5 x 1 / 2 = 2.5 square metres.
    const SectionSurface design = series("design", {{0.0, 10.0}, {10.0, 10.0}});
    const SectionSurface ground = series("GROUND", {{0.0, 9.0}, {10.0, 11.0}});
    const auto regions = earthworkRegions(design, ground);
    ASSERT_EQ(regions.size(), 2u);
    EXPECT_EQ(regions[0].kind, Earthwork::Fill);
    EXPECT_EQ(regions[1].kind, Earthwork::Cut);
    // The meeting point is in each outline once.
    EXPECT_EQ(regions[0].outline,
              (std::vector<Point2>{Point2(0.0, 10.0), Point2(5.0, 10.0), Point2(0.0, 9.0)}));
    EXPECT_EQ(regions[1].outline,
              (std::vector<Point2>{Point2(5.0, 10.0), Point2(10.0, 10.0), Point2(10.0, 11.0)}));
    EXPECT_NEAR(outlineArea(regions[0].outline), 2.5, 1e-12);
    EXPECT_NEAR(outlineArea(regions[1].outline), 2.5, 1e-12);

    // Only 2..8: 3 x 0.6 / 2 = 0.9 each side of the crossing.
    const auto within = earthworkRegions(design, ground, 2.0, 8.0);
    ASSERT_EQ(within.size(), 2u);
    EXPECT_NEAR(outlineArea(within[0].outline), 0.9, 1e-12);
    EXPECT_NEAR(outlineArea(within[1].outline), 0.9, 1e-12);
    EXPECT_NEAR(within[0].outline.front().x, 2.0, 1e-12);
    EXPECT_NEAR(within[1].outline[1].x, 8.0, 1e-12);
}

TEST(SectionAnnotation, PiecesOfOneKindAreOneRegionAndAGapEndsIt)
{
    // Design above the ground all the way, over samples of both at
    // different stations: one fill region.
    const SectionSurface design = series("design", {{0.0, 11.0}, {10.0, 12.0}, {20.0, 11.0}});
    const SectionSurface ground = series("GROUND", {{0.0, 10.0}, {5.0, 10.5}, {20.0, 10.0}});
    const auto one = earthworkRegions(design, ground);
    ASSERT_EQ(one.size(), 1u);
    EXPECT_EQ(one[0].kind, Earthwork::Fill);
    // Design at 0, 5, 10, 20 then ground back at 20, 10, 5, 0.
    EXPECT_EQ(one[0].outline.size(), 8u);
    // The area is the design's less the ground's, piece by piece: design
    // 0..10 = 115 and 10..20 = 115; ground 0..5 = 51.25 and 5..20 = 153.75.
    EXPECT_NEAR(outlineArea(one[0].outline), 230.0 - 205.0, 1e-9);

    // A gap in the ground at 10 splits it into two.
    const SectionSurface gappy =
        series("GROUND", {{0.0, 10.0}, {5.0, 10.5}, {10.0, std::nullopt}, {15.0, 10.0}, {20.0, 10.0}});
    const auto two = earthworkRegions(design, gappy);
    ASSERT_EQ(two.size(), 2u);
    EXPECT_NEAR(two[0].outline.back().x, 0.0, 1e-12);
    EXPECT_NEAR(two[1].outline.front().x, 15.0, 1e-12);
}

TEST(SectionAnnotation, DesignOnTheGroundHasNothingToShade)
{
    const SectionSurface design = series("design", {{0.0, 10.0}, {10.0, 12.0}});
    const SectionSurface ground = series("GROUND", {{0.0, 10.0}, {5.0, 11.0}, {10.0, 12.0}});
    EXPECT_TRUE(earthworkRegions(design, ground).empty());
    // Nor where they do not overlap.
    EXPECT_TRUE(earthworkRegions(design, series("GROUND", {{20.0, 1.0}, {30.0, 1.0}})).empty());
    EXPECT_TRUE(earthworkRegions(design, series("GROUND", {})).empty());
}

// ---- crossings ------------------------------------------------------------------------------

TEST(SectionAnnotation, ACrossingTakesItsEntitysOwnLevelWhereItCrosses)
{
    Model model;
    // A pipe from (0, 0) at 20 to (10, 0) at 22, crossed at x 2.5: 20.5.
    const EntityId pipe =
        addEntity(model, katana::geometry::Segment2{Point2(0.0, 0.0), Point2(10.0, 0.0)}, {20.0, 22.0});
    EXPECT_NEAR(*ownLevel(model, crossingOf(pipe, Point2(2.5, 0.0), 0.0)), 20.5, 1e-12);

    // A string with a level at each vertex, crossed on its second piece.
    katana::geometry::Polyline2 string;
    string.vertices = {Point2(0.0, 0.0), Point2(10.0, 0.0), Point2(10.0, 10.0)};
    const EntityId main = addEntity(model, string, {30.0, 31.0, 35.0});
    EXPECT_NEAR(*ownLevel(model, crossingOf(main, Point2(10.0, 4.0), 0.0)), 32.6, 1e-12);

    // A pit: one level for the whole circle.
    const EntityId pit =
        addEntity(model, katana::geometry::Circle2{Point2(50.0, 50.0), 1.0}, {18.25});
    EXPECT_EQ(*ownLevel(model, crossingOf(pit, Point2(51.0, 50.0), 0.0)), 18.25);

    // An arc at 10 at its start and 20 at its end, crossed halfway round.
    katana::geometry::Arc2 bend{Point2(0.0, 0.0), 5.0, 0.0, katana::math::kPi / 2.0};
    const EntityId curve = addEntity(model, bend, {10.0, 20.0});
    EXPECT_NEAR(*ownLevel(model, crossingOf(curve, bend.midpoint(), 0.0)), 15.0, 1e-9);

    // A line drawn flat has no level of its own; nor has one missing a level
    // at either end of the piece crossed, nor an entity that is gone.
    const EntityId kerb =
        addEntity(model, katana::geometry::Segment2{Point2(0.0, 5.0), Point2(10.0, 5.0)}, {});
    EXPECT_FALSE(ownLevel(model, crossingOf(kerb, Point2(5.0, 5.0), 0.0)));
    const EntityId half = addEntity(
        model, katana::geometry::Segment2{Point2(0.0, 7.0), Point2(10.0, 7.0)}, {20.0, std::nullopt});
    EXPECT_FALSE(ownLevel(model, crossingOf(half, Point2(5.0, 7.0), 0.0)));
    EXPECT_FALSE(ownLevel(model, crossingOf(987654, Point2(), 0.0)));
}

TEST(SectionAnnotation, AServiceIsNotedWithItsLevelAndDepth)
{
    Model model;
    const EntityId water = addEntity(
        model, katana::geometry::Segment2{Point2(0.0, -5.0), Point2(0.0, 5.0)}, {24.1});
    const EntityId power = addEntity(
        model, katana::geometry::Segment2{Point2(4.0, -5.0), Point2(4.0, 5.0)}, {36.0}, "power");
    const EntityId kerb =
        addEntity(model, katana::geometry::Segment2{Point2(8.0, -5.0), Point2(8.0, 5.0)}, {}, "KERB");
    Section section;
    section.surfaces = {series("design ROAD", {{0.0, 26.0}, {10.0, 26.0}}),
                        series("GROUND", {{0.0, 25.3}, {10.0, 29.3}})};

    // Buried 25.30 - 24.10 = 1.20 under the ground.
    const CrossingNote service =
        crossingNote(section, crossingOf(water, Point2(0.0, 0.0), 0.0, "WATER", 25.3), &model);
    EXPECT_TRUE(service.ownLevel);
    EXPECT_NEAR(*service.level, 24.1, 1e-12);
    EXPECT_NEAR(*service.depth, 1.2, 1e-12);
    EXPECT_EQ(service.text, "WATER RL 24.10 D 1.20");
    EXPECT_EQ(service.shortText, "WATER");

    // Overhead at 36.00 where the ground is 26.90: 9.10 above it.
    const CrossingNote overhead =
        crossingNote(section, crossingOf(power, Point2(4.0, 0.0), 4.0, "power", 26.9), &model);
    EXPECT_EQ(overhead.text, "POWER RL 36.00 H 9.10");

    // Draped on the ground: its level is the ground's, and it has no depth.
    const CrossingNote draped =
        crossingNote(section, crossingOf(kerb, Point2(8.0, 0.0), 8.0, "KERB", 28.5), &model);
    EXPECT_FALSE(draped.ownLevel);
    EXPECT_FALSE(draped.depth);
    EXPECT_EQ(draped.text, "KERB RL 28.50");

    // No surface under it and no level of its own: its layer alone.
    const CrossingNote bare =
        crossingNote(section, crossingOf(kerb, Point2(8.0, 0.0), 8.0, "FENCE"), &model);
    EXPECT_EQ(bare.text, "FENCE");
    EXPECT_FALSE(bare.level);

    // Without the model every level is the one the section found.
    EXPECT_EQ(crossingNote(section, crossingOf(water, Point2(0.0, 0.0), 0.0, "WATER", 25.3), nullptr)
                  .text,
              "WATER RL 25.30");
}

TEST(SectionAnnotation, TheLevelRangeTakesInBuriedServicesAndLeavesOutHiddenOnes)
{
    Model model;
    const EntityId sewer = addEntity(
        model, katana::geometry::Segment2{Point2(5.0, -5.0), Point2(5.0, 5.0)}, {17.5}, "SEWER");
    Section section;
    section.surfaces = {series("GROUND", {{0.0, 20.0}, {10.0, 22.0}, {20.0, 21.0}})};
    section.crossings = {crossingOf(sewer, Point2(5.0, 0.0), 5.0, "SEWER", 21.0)};

    const auto all = levelRange(section, -1e9, 1e9, &model);
    ASSERT_TRUE(all);
    EXPECT_EQ(all->first, 17.5);
    EXPECT_EQ(all->second, 22.0);
    // Between 12 and 20: the ground only, from 21.8 at 12 down to 21.
    const auto part = levelRange(section, 12.0, 20.0, &model);
    ASSERT_TRUE(part);
    EXPECT_NEAR(part->first, 21.0, 1e-12);
    EXPECT_NEAR(part->second, 21.8, 1e-12);
    // The sewer hidden, it does not count.
    LayerOverrides hidden;
    hidden.hide("SEWER");
    EXPECT_EQ(levelRange(section, -1e9, 1e9, &model, &hidden)->first, 20.0);
    // Nothing there: nothing.
    EXPECT_FALSE(levelRange(section, 30.0, 40.0, &model));
}

// ---- placing labels ---------------------------------------------------------------------------

TEST(SectionLabelPlacement, TheNearestTheMiddleIsKeptAndTheOtherDropped)
{
    const Box2 bounds(Point2(0.0, 0.0), Point2(100.0, 50.0));
    // Two labels in one place: the one with the lower priority wins,
    // whichever comes first.
    const std::vector<LabelCandidate> labels{
        {{Box2(Point2(10.0, 10.0), Point2(12.0, 30.0))}, 5.0},
        {{Box2(Point2(11.0, 10.0), Point2(13.0, 30.0))}, 1.0},
    };
    const auto placed = placeLabels(labels, bounds);
    EXPECT_FALSE(placed[0]);
    ASSERT_TRUE(placed[1]);
    EXPECT_EQ(*placed[1], 0u);
}

TEST(SectionLabelPlacement, ALabelTakesItsFirstFreePlace)
{
    const Box2 bounds(Point2(0.0, 0.0), Point2(100.0, 50.0));
    const std::vector<LabelCandidate> labels{
        {{Box2(Point2(10.0, 10.0), Point2(12.0, 30.0))}, 0.0},
        // Its first place crowds the first label; its second - the other
        // side of its line - does not; its third is never tried.
        {{Box2(Point2(12.1, 10.0), Point2(14.0, 30.0)), Box2(Point2(20.0, 10.0), Point2(22.0, 30.0)),
          Box2(Point2(40.0, 10.0), Point2(42.0, 30.0))},
         1.0},
        // Every place outside the bounds or crowded: dropped.
        {{Box2(Point2(95.0, 10.0), Point2(105.0, 30.0)), Box2(Point2(9.0, 29.0), Point2(11.0, 45.0))},
         2.0},
    };
    const auto placed = placeLabels(labels, bounds, {}, 0.3);
    ASSERT_TRUE(placed[0]);
    ASSERT_TRUE(placed[1]);
    EXPECT_EQ(*placed[1], 1u);
    EXPECT_FALSE(placed[2]);
}

TEST(SectionLabelPlacement, ObstaclesAreKeptClearAndTiesGoInOrder)
{
    const Box2 bounds(Point2(0.0, 0.0), Point2(100.0, 50.0));
    const std::vector<Box2> obstacles{Box2(Point2(0.0, 40.0), Point2(30.0, 50.0))};
    const std::vector<LabelCandidate> labels{
        {{Box2(Point2(5.0, 35.0), Point2(7.0, 45.0)), Box2(Point2(5.0, 5.0), Point2(7.0, 15.0))}, 0.0},
        {{Box2(Point2(6.0, 5.0), Point2(8.0, 15.0))}, 0.0},
        {{Box2(Point2(50.0, 5.0), Point2(52.0, 15.0))}, 0.0},
    };
    const auto placed = placeLabels(labels, bounds, obstacles);
    ASSERT_TRUE(placed[0]);
    EXPECT_EQ(*placed[0], 1u); // the key is in the way of its first place
    EXPECT_FALSE(placed[1]);   // the first, equal and earlier, took its place
    ASSERT_TRUE(placed[2]);
    // Boxes touching within the gap crowd each other; farther apart they do not.
    const std::vector<LabelCandidate> pair{{{Box2(Point2(0.0, 0.0), Point2(2.0, 2.0))}, 0.0},
                                           {{Box2(Point2(2.2, 0.0), Point2(4.0, 2.0))}, 1.0}};
    EXPECT_FALSE(placeLabels(pair, bounds, {}, 0.3)[1]);
    EXPECT_TRUE(placeLabels(pair, bounds, {}, 0.1)[1]);
}
