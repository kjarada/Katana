// The label placer (cad/annotation/label_layout.hpp, docs/annotation.md
// "The placer"): candidates in the style's order, overlap refused, displaced
// labels with leaders, suppression counted, the order fixed so the result
// does not depend on the input's order, and the scale deciding what fits.

#include <gtest/gtest.h>

#include <algorithm>
#include <random>

#include "katana/cad/annotation/label_layout.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/entity/text_block.hpp"

using namespace katana::cad::annotation;
using namespace katana::entity;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

TextMeasure halfPerCharacter()
{
    return [](std::string_view line, const TextFace&) {
        return 0.5 * static_cast<double>(characterCount(line));
    };
}

// A model with a point-number style and the points given, each labelled.
struct Scene {
    Model model;
    std::vector<EntityId> labels;
};

Scene pointsLabelled(const std::vector<Point2>& points, bool displace = true, int priority = 0)
{
    Scene scene;
    LabelStyle style;
    style.name = "pt";
    style.kind = LabelKind::Point;
    style.text = "{point}";
    style.displace = displace;
    style.priority = priority;
    EXPECT_TRUE(scene.model.labelStyles.add(style).ok());
    EntityId next = 1;
    for (std::size_t i = 0; i < points.size(); ++i) {
        Entity point;
        point.id = next++;
        point.geometry = PointGeometry{points[i]};
        point.properties["point"] = std::string("P") + std::to_string(i + 1);
        EXPECT_TRUE(scene.model.entities.insert(point).ok());
        Entity label;
        label.id = next++;
        label.geometry = LabelGeometry{.target = point.id, .style = "pt", .anchor = points[i]};
        EXPECT_TRUE(scene.model.entities.insert(label).ok());
        scene.labels.push_back(label.id);
    }
    return scene;
}

LabelLayoutOptions at(double scale)
{
    LabelLayoutOptions options;
    options.scale = scale;
    options.measure = halfPerCharacter();
    return options;
}

std::vector<std::vector<Point2>> boxes(const LabelLayout& layout)
{
    std::vector<std::vector<Point2>> all;
    for (const auto& placed : layout.placed) {
        all.push_back(placed.drawing.textBoxes.front());
    }
    return all;
}

} // namespace

TEST(LabelLayout, TheSeparatingAxisTestIsExact)
{
    const std::vector<Point2> square{Point2(0, 0), Point2(2, 0), Point2(2, 2), Point2(0, 2)};
    const std::vector<Point2> apart{Point2(3, 0), Point2(5, 0), Point2(5, 2), Point2(3, 2)};
    const std::vector<Point2> touching{Point2(2, 0), Point2(4, 0), Point2(4, 2), Point2(2, 2)};
    // A diamond whose box overlaps the square's but which misses it.
    const std::vector<Point2> diamond{Point2(3, 2.9), Point2(3.95, 3.85), Point2(3, 4.8),
                                      Point2(2.05, 3.85)};
    const std::vector<Point2> inside{Point2(0.5, 0.5), Point2(1, 0.5), Point2(1, 1)};
    EXPECT_FALSE(convexOverlap(square, apart));
    EXPECT_FALSE(convexOverlap(square, touching)) << "touching edges are apart";
    EXPECT_FALSE(convexOverlap(square, diamond));
    EXPECT_TRUE(convexOverlap(square, inside));
}

TEST(LabelLayout, TwoLabelsThatWouldCollideAreKeptApart)
{
    // Two points 1 m apart. At 1:1000 a 2.5 mm label is 2.5 m tall: their
    // upper-right places overlap, so the second takes another place.
    const Scene scene = pointsLabelled({Point2(0, 0), Point2(1, 0)});
    const LabelLayout layout =
        layoutLabels(scene.model, labelEntities(scene.model), at(1000.0));
    ASSERT_EQ(layout.placed.size(), 2u);
    EXPECT_EQ(layout.suppressed, 0u);
    EXPECT_EQ(layout.placed[0].candidate, 0u) << "the first placed takes its first place";
    EXPECT_NE(layout.placed[1].candidate, 0u);
    const auto all = boxes(layout);
    EXPECT_FALSE(convexOverlap(all[0], all[1]));

    // With avoidance off, both take their first place - and overlap.
    LabelLayoutOptions off = at(1000.0);
    off.avoidCollisions = false;
    const LabelLayout blind = layoutLabels(scene.model, labelEntities(scene.model), off);
    EXPECT_TRUE(convexOverlap(blind.placed[0].drawing.textBoxes.front(),
                              blind.placed[1].drawing.textBoxes.front()));
}

TEST(LabelLayout, TheScaleDecidesWhatFits)
{
    // The same two points 10 m apart: at 1:200 the labels are 0.5 m tall and
    // both sit at their first place; at 1:5000 they are 12.5 m and the second
    // must move.
    const Scene scene = pointsLabelled({Point2(0, 0), Point2(10, 0)});
    const LabelLayout large = layoutLabels(scene.model, labelEntities(scene.model), at(200.0));
    ASSERT_EQ(large.placed.size(), 2u);
    EXPECT_EQ(large.placed[1].candidate, 0u);
    const LabelLayout small = layoutLabels(scene.model, labelEntities(scene.model), at(5000.0));
    EXPECT_TRUE(small.placed.size() < 2 || small.placed[1].candidate != 0u);
}

TEST(LabelLayout, WithNoRoomALabelIsSuppressedAndCounted)
{
    // Five points at one place, a style that may not move its labels: the
    // first is placed, the other four have no room.
    const Scene scene = pointsLabelled(
        {Point2(0, 0), Point2(0, 0), Point2(0, 0), Point2(0, 0), Point2(0, 0)}, false);
    const LabelLayout layout = layoutLabels(scene.model, labelEntities(scene.model), at(1000.0));
    EXPECT_EQ(layout.considered, 5u);
    EXPECT_EQ(layout.placed.size(), 1u);
    EXPECT_EQ(layout.suppressed, 4u);
}

TEST(LabelLayout, ADisplacedLabelGetsALeaderBack)
{
    // A crowd of twelve at one point: once the eight places around it are
    // taken, labels move out to the displaced rings, each with a leader
    // from the point to its text.
    std::vector<Point2> crowd(12, Point2(0, 0));
    const Scene scene = pointsLabelled(crowd);
    const LabelLayout layout = layoutLabels(scene.model, labelEntities(scene.model), at(1000.0));
    ASSERT_GT(layout.displaced, 0u);
    for (const auto& placed : layout.placed) {
        if (placed.displaced) {
            ASSERT_FALSE(placed.drawing.strokes.empty());
            EXPECT_EQ(placed.drawing.strokes.front().front(), Point2(0, 0))
                << "the leader starts at the point labelled";
        }
    }
    const auto all = boxes(layout);
    for (std::size_t i = 0; i < all.size(); ++i) {
        for (std::size_t j = i + 1; j < all.size(); ++j) {
            EXPECT_FALSE(convexOverlap(all[i], all[j])) << i << " and " << j;
        }
    }
}

TEST(LabelLayout, TheResultDoesNotDependOnTheOrderLabelsArriveIn)
{
    std::vector<Point2> points;
    std::mt19937 generator(20260925);
    std::uniform_real_distribution<double> coordinate(0.0, 30.0);
    for (int i = 0; i < 40; ++i) {
        points.emplace_back(coordinate(generator), coordinate(generator));
    }
    const Scene scene = pointsLabelled(points);
    std::vector<const Entity*> labels = labelEntities(scene.model);
    const LabelLayout first = layoutLabels(scene.model, labels, at(1000.0));
    std::shuffle(labels.begin(), labels.end(), generator);
    const LabelLayout second = layoutLabels(scene.model, labels, at(1000.0));
    ASSERT_EQ(first.placed.size(), second.placed.size());
    EXPECT_EQ(first.suppressed, second.suppressed);
    for (std::size_t i = 0; i < first.placed.size(); ++i) {
        EXPECT_EQ(first.placed[i].label, second.placed[i].label);
        EXPECT_EQ(first.placed[i].candidate, second.placed[i].candidate);
        EXPECT_EQ(first.placed[i].drawing.texts.front().origin,
                  second.placed[i].drawing.texts.front().origin);
    }
}

TEST(LabelLayout, AHigherPriorityIsPlacedFirstAndKeepsItsPlace)
{
    Scene scene = pointsLabelled({Point2(0, 0), Point2(1, 0)});
    // The second point's label gets its own style of higher priority.
    LabelStyle urgent = *scene.model.labelStyles.find("pt");
    urgent.name = "urgent";
    urgent.priority = 10;
    ASSERT_TRUE(scene.model.labelStyles.add(urgent).ok());
    Entity second = *scene.model.entities.find(scene.labels[1]);
    std::get<LabelGeometry>(second.geometry).style = "urgent";
    ASSERT_TRUE(scene.model.entities.replace(second).ok());
    const LabelLayout layout = layoutLabels(scene.model, labelEntities(scene.model), at(1000.0));
    ASSERT_EQ(layout.placed.size(), 2u);
    EXPECT_EQ(layout.placed[0].label, scene.labels[1]) << "the urgent one first";
    EXPECT_EQ(layout.placed[0].candidate, 0u);
}

TEST(LabelLayout, TextKeepsOffTheLinesItIsGiven)
{
    const Scene scene = pointsLabelled({Point2(0, 0)});
    LabelLayoutOptions options = at(1000.0);
    // A line through where the upper-right label would go.
    options.linework.push_back(Segment2{Point2(1, 2), Point2(6, 2)});
    const LabelLayout layout = layoutLabels(scene.model, labelEntities(scene.model), options);
    ASSERT_EQ(layout.placed.size(), 1u);
    EXPECT_NE(layout.placed[0].candidate, 0u);
}

TEST(LabelLayout, ADraggedLabelIsWhereItWasPut)
{
    Scene scene = pointsLabelled({Point2(0, 0), Point2(0, 0)});
    Entity dragged = *scene.model.entities.find(scene.labels[1]);
    std::get<LabelGeometry>(dragged.geometry).position = Point2(1, 1);
    ASSERT_TRUE(scene.model.entities.replace(dragged).ok());
    const LabelLayout layout = layoutLabels(scene.model, labelEntities(scene.model), at(1000.0));
    ASSERT_EQ(layout.placed.size(), 2u);
    EXPECT_EQ(layout.placed[0].label, scene.labels[1]) << "a dragged label is placed first";
    Box2 box;
    for (const Point2& p : layout.placed[0].drawing.textBoxes.front()) {
        box.expand(p);
    }
    EXPECT_TRUE(box.contains(Point2(1, 1))) << "centred where it was put";
}

TEST(LabelLayout, SegmentLabelsSitBesideTheirSegmentReadingAlongIt)
{
    Model model;
    LabelStyle style;
    style.name = "bd";
    style.kind = LabelKind::Segment;
    style.text = "{distance:.1f}";
    ASSERT_TRUE(model.labelStyles.add(style).ok());
    Entity line;
    line.id = 1;
    line.geometry = Segment2{Point2(0, 0), Point2(0, 100)}; // north
    ASSERT_TRUE(model.entities.insert(line).ok());
    Entity label;
    label.id = 2;
    label.geometry = LabelGeometry{.target = 1, .style = "bd", .anchor = Point2(0, 50)};
    ASSERT_TRUE(model.entities.insert(label).ok());
    const LabelLayout layout = layoutLabels(model, labelEntities(model), at(1000.0));
    ASSERT_EQ(layout.placed.size(), 1u);
    const auto& run = layout.placed[0].drawing.texts.front();
    EXPECT_EQ(run.text, "100.0");
    EXPECT_NEAR(run.rotation, 0.5 * katana::math::kPi, 1e-12) << "reads up the line";
    // Above a line running north is its left: west of it.
    EXPECT_LT(run.origin.x, 0.0);
}

TEST(LabelLayout, ChainageTicksAreDrawnAtEveryMark)
{
    Model model;
    Alignment road;
    road.name = "road";
    road.horizontal.pis = {katana::geometry::AlignmentPI{Point2(0, 0)},
                           katana::geometry::AlignmentPI{Point2(100, 0)}};
    ASSERT_TRUE(model.alignments.add(road).ok());
    LabelStyle style;
    style.name = "ch";
    style.kind = LabelKind::Chainage;
    style.text = "{chainage:ch}";
    style.interval = 50.0;
    style.tickInterval = 10.0;
    ASSERT_TRUE(model.labelStyles.add(style).ok());
    Entity label;
    label.id = 1;
    label.geometry = LabelGeometry{.alignment = "road", .style = "ch"};
    ASSERT_TRUE(model.entities.insert(label).ok());
    const LabelLayout layout = layoutLabels(model, labelEntities(model), at(1000.0));
    // 0, 50, 100 labelled (each with its tick); 10..40 and 60..90 ticks only.
    EXPECT_EQ(layout.placed.size(), 3u);
    EXPECT_EQ(layout.marksOnly.size(), 8u);
    EXPECT_EQ(layout.placed[1].drawing.texts.front().text, "0+050.000");
}

TEST(LabelLayout, ALabelWhoseTargetIsGoneIsCountedNotPlaced)
{
    Scene scene = pointsLabelled({Point2(0, 0)});
    ASSERT_TRUE(scene.model.entities.remove(1).ok());
    const LabelLayout layout = layoutLabels(scene.model, labelEntities(scene.model), at(1000.0));
    EXPECT_TRUE(layout.placed.empty());
    EXPECT_EQ(layout.orphaned, 1u);
}

TEST(LabelLayout, ANoteIsKeptOutOfEvenWhenTheLabelWouldFitInsideIt)
{
    // A point with a large note over its upper right: the label's first
    // place lies wholly inside the note's box, crossing none of its edges -
    // the box's diagonals are what catch it.
    Scene scene = pointsLabelled({Point2(0, 0)});
    Entity note;
    note.id = 100;
    note.geometry = TextGeometry{.position = Point2(0.5, 0.5), .text = "NOTE", .height = 6.0};
    ASSERT_TRUE(scene.model.entities.insert(note).ok());

    LabelLayoutOptions options = at(1000.0);
    const LabelLayout blind = layoutLabels(scene.model, labelEntities(scene.model), options);
    ASSERT_EQ(blind.placed.size(), 1u);
    EXPECT_EQ(blind.placed[0].candidate, 0u) << "nothing to keep out of";

    options.linework = labelKeepOut(scene.model, options.scale, options.measure, 1000);
    ASSERT_FALSE(options.linework.empty());
    const LabelLayout layout = layoutLabels(scene.model, labelEntities(scene.model), options);
    ASSERT_EQ(layout.placed.size(), 1u);
    EXPECT_NE(layout.placed[0].candidate, 0u);
}

TEST(LabelLayout, TheKeepOutHoldsADimensionsLinesAndFigures)
{
    Model model;
    Entity dimension;
    dimension.id = 1;
    DimensionGeometry measured;
    measured.start = Point2(0, 0);
    measured.end = Point2(40, 0);
    measured.offset = 5.0;
    dimension.geometry = measured;
    ASSERT_TRUE(model.entities.insert(dimension).ok());
    // Its drawing, and what it keeps out: two extension lines, the dimension
    // line, and the figures' box - four edges and two diagonals.
    const auto drawing =
        katana::cad::buildDimension(std::get<DimensionGeometry>(dimension.geometry),
                                    katana::cad::resolveDimensionStyle(model, dimension));
    std::vector<Segment2> keepOut;
    appendKeepOut(drawing, keepOut, 1000);
    EXPECT_EQ(keepOut.size(), 9u);
    EXPECT_EQ(labelKeepOut(model, 1000.0, estimatedMeasure(), 1000).size(), 9u);
    // The limit is a limit.
    keepOut.clear();
    appendKeepOut(drawing, keepOut, 4);
    EXPECT_EQ(keepOut.size(), 4u);
    // A hidden entity is in nobody's way.
    Entity hidden = *model.entities.find(1);
    hidden.visible = false;
    ASSERT_TRUE(model.entities.replace(hidden).ok());
    EXPECT_TRUE(labelKeepOut(model, 1000.0, estimatedMeasure(), 1000).empty());
}

TEST(LabelLayout, AnAreaLabelMovesOffALineBeforeItTakesAMask)
{
    Model model;
    LabelStyle style;
    style.name = "lot";
    style.kind = LabelKind::Area;
    style.text = "{area:m2:.0f}";
    style.placement = LabelPlacement::Centroid;
    ASSERT_TRUE(model.labelStyles.add(style).ok());
    Entity lot;
    lot.id = 1;
    lot.geometry = Polyline2{{Point2(0, 0), Point2(40, 0), Point2(40, 25), Point2(0, 25)}, true};
    ASSERT_TRUE(model.entities.insert(lot).ok());
    // A kerb across the lot, through its middle.
    Entity kerb;
    kerb.id = 2;
    kerb.geometry = Segment2{Point2(0, 12.5), Point2(40, 12.5)};
    ASSERT_TRUE(model.entities.insert(kerb).ok());
    Entity label;
    label.id = 3;
    label.geometry = LabelGeometry{.target = 1, .style = "lot", .anchor = Point2(20, 12.5)};
    ASSERT_TRUE(model.entities.insert(label).ok());

    LabelLayoutOptions options = at(200.0);
    options.linework = labelKeepOut(model, options.scale, options.measure, 1000);
    const LabelLayout moved = layoutLabels(model, labelEntities(model), options);
    ASSERT_EQ(moved.placed.size(), 1u);
    EXPECT_NE(moved.placed[0].candidate, 0u) << "stepped off the kerb";
    EXPECT_TRUE(moved.placed[0].drawing.masks.empty()) << "clear, so nothing hidden";

    // A style that may not move its text keeps the centre, over a mask.
    LabelStyle fixed = style;
    fixed.displace = false;
    ASSERT_TRUE(model.labelStyles.update(fixed).ok());
    const LabelLayout masked = layoutLabels(model, labelEntities(model), options);
    ASSERT_EQ(masked.placed.size(), 1u);
    EXPECT_EQ(masked.placed[0].candidate, 0u);
    EXPECT_FALSE(masked.placed[0].drawing.masks.empty());
}
