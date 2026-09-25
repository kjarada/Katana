// Dimensions of every kind and leaders (cad/annotation/dimension_build.hpp,
// cad/dimension_draw.hpp, cad/annotation/leader_draw.hpp; docs/annotation.md
// "Dimensions" and "Leaders and callouts"): what each kind measures and
// draws, paper-sized styles at several scales, chains, and callouts.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/cad/annotation/dimension_build.hpp"
#include "katana/cad/annotation/leader_draw.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/entity/text_block.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad;
using namespace katana::entity;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::math::kPi;
namespace ann = katana::cad::annotation;

namespace {

ann::AnchoredPoint free(double x, double y)
{
    return ann::AnchoredPoint{Point2(x, y), {}};
}

} // namespace

TEST(DimensionBuild, LinearReadsHorizontalOrVerticalFromWhereItIsDragged)
{
    // Points (0,0) and (10,4). Dragged above them: horizontal, 10.
    auto above = ann::linearDimension(free(0, 0), free(10, 4), Point2(5, 9));
    ASSERT_TRUE(above.ok());
    EXPECT_DOUBLE_EQ(above->angle, 0.0);
    EXPECT_DOUBLE_EQ(above->measurement(), 10.0);
    EXPECT_DOUBLE_EQ(above->offset, 9.0) << "the line through the dragged point, from start";
    // Beside them: vertical, 4.
    auto beside = ann::linearDimension(free(0, 0), free(10, 4), Point2(14, 2));
    ASSERT_TRUE(beside.ok());
    EXPECT_NEAR(beside->measurement(), 4.0, 1e-12);
    // At an angle of 30 degrees: (10, 4) . (cos 30, sin 30) = 8.660 + 2 = 10.660.
    auto rotated = ann::linearDimension(free(0, 0), free(10, 4), Point2(0, 10), 30.0 * kPi / 180.0);
    ASSERT_TRUE(rotated.ok());
    EXPECT_NEAR(rotated->measurement(), 10.0 * std::sqrt(3.0) / 2.0 + 2.0, 1e-12);
    EXPECT_FALSE(ann::linearDimension(free(0, 0), free(0, 5), Point2(3, 9), 0.0).ok())
        << "nothing to measure horizontally";
}

TEST(DimensionBuild, AngularMeasuresTheSideTheArcIsDraggedTo)
{
    auto inside = ann::angularDimension(free(0, 0), free(10, 0), free(0, 10), Point2(3, 3));
    ASSERT_TRUE(inside.ok());
    EXPECT_NEAR(inside->measurement(), 0.5 * kPi, 1e-12);
    EXPECT_NEAR(inside->offset, std::sqrt(18.0), 1e-12);
    auto outside = ann::angularDimension(free(0, 0), free(10, 0), free(0, 10), Point2(-3, -3));
    ASSERT_TRUE(outside.ok());
    EXPECT_NEAR(outside->measurement(), 1.5 * kPi, 1e-12) << "the reflex angle";
}

TEST(DimensionBuild, OrdinateTakesItsAxisFromTheLeader)
{
    // Leader straight up from the feature: the X ordinate.
    auto x = ann::ordinateDimension(free(100, 100), free(130, 110), Point2(130, 140));
    ASSERT_TRUE(x.ok());
    EXPECT_EQ(x->kind, DimensionKind::OrdinateX);
    EXPECT_DOUBLE_EQ(x->measurement(), 30.0);
    auto y = ann::ordinateDimension(free(100, 100), free(130, 110), Point2(160, 110));
    ASSERT_TRUE(y.ok());
    EXPECT_EQ(y->kind, DimensionKind::OrdinateY);
    EXPECT_DOUBLE_EQ(y->measurement(), 10.0);
}

TEST(DimensionBuild, BaselineAndContinuedChains)
{
    auto base = ann::linearDimension(free(0, 0), free(10, 0), Point2(5, 5), 0.0);
    ASSERT_TRUE(base.ok());
    auto baseline = ann::baselineDimensions(*base, {free(20, 0), free(35, 0)}, 3.0);
    ASSERT_TRUE(baseline.ok());
    ASSERT_EQ(baseline->size(), 2u);
    EXPECT_EQ((*baseline)[0].start, Point2(0, 0)) << "every one from the base's first point";
    EXPECT_DOUBLE_EQ((*baseline)[0].measurement(), 20.0);
    EXPECT_DOUBLE_EQ((*baseline)[0].offset, 8.0);
    EXPECT_DOUBLE_EQ((*baseline)[1].offset, 11.0) << "each a spacing further out";

    auto continued = ann::continuedDimensions(*base, {free(20, 2), free(35, -1)});
    ASSERT_TRUE(continued.ok());
    ASSERT_EQ(continued->size(), 2u);
    EXPECT_EQ((*continued)[0].start, Point2(10, 0)) << "from where the last one ended";
    EXPECT_DOUBLE_EQ((*continued)[0].measurement(), 10.0);
    EXPECT_DOUBLE_EQ((*continued)[1].measurement(), 15.0);
    // Every dimension line on the base's: y = 5.
    for (const DimensionGeometry& d : *continued) {
        const DimensionDrawing drawing = buildDimension(d, DimensionStyle{});
        EXPECT_NEAR(drawing.dimensionLine.start.y, 5.0, 1e-12);
    }
}

TEST(DimensionDraw, EveryKindDrawsAndShowsItsValue)
{
    DimensionStyle style;
    style.decimals = 2;
    DimensionGeometry angular;
    angular.kind = DimensionKind::Angular;
    angular.vertex = Point2(0, 0);
    angular.start = Point2(10, 0);
    angular.end = Point2(0, 10);
    angular.offset = 5.0;
    const DimensionDrawing a = buildDimension(angular, style);
    EXPECT_FALSE(a.hasDimensionLine);
    ASSERT_EQ(a.curves.size(), 1u);
    EXPECT_NEAR(a.curves[0].front().distanceTo(Point2(0, 0)), 5.0, 1e-12) << "the arc's radius";
    EXPECT_EQ(a.text, "90°00'00\"");

    DimensionGeometry radius;
    radius.kind = DimensionKind::Radius;
    radius.vertex = Point2(0, 0);
    radius.start = Point2(3, 4);
    EXPECT_EQ(buildDimension(radius, style).text, "R5.00");
    radius.kind = DimensionKind::Diameter;
    EXPECT_EQ(buildDimension(radius, style).text, "Ø10.00");
    radius.textOverride = "SEE NOTE";
    EXPECT_EQ(buildDimension(radius, style).text, "SEE NOTE") << "an override is verbatim";

    DimensionGeometry ordinate;
    ordinate.kind = DimensionKind::OrdinateX;
    ordinate.vertex = Point2(0, 0);
    ordinate.start = Point2(-7.5, 3);
    ordinate.end = Point2(-7.5, 10);
    EXPECT_EQ(buildDimension(ordinate, style).text, "-7.50");
}

TEST(DimensionDraw, APaperSizedStyleIsTheSameOnPaperAtEveryScale)
{
    DimensionStyle paper;
    paper.paperSized = true;
    paper.textHeight = 2.5;
    paper.arrowSize = 3.0;
    const DimensionGeometry dimension{Point2(0, 0), Point2(1000, 0), 50.0, ""};
    for (const double scale : {200.0, 1000.0, 2500.0}) {
        const DimensionDrawing drawing = buildDimension(dimension, paper, scale);
        EXPECT_DOUBLE_EQ(drawing.textHeight, 2.5 * scale / 1000.0) << "1:" << scale;
        // The closed arrow is arrowSize long: its tip to its back.
        ASSERT_FALSE(drawing.arrowFills.empty());
        const auto& head = drawing.arrowFills.front();
        const double length = head[0].distanceTo((head[1] + head[2]) * 0.5);
        EXPECT_NEAR(length, 3.0 * scale / 1000.0, 1e-9) << "1:" << scale;
    }
    // A model-unit style draws alike at every scale.
    const DimensionStyle model;
    EXPECT_DOUBLE_EQ(buildDimension(dimension, model, 200.0).textHeight,
                     buildDimension(dimension, model, 5000.0).textHeight);
}

TEST(DimensionDraw, TheAlignedDrawingIsWhatItWasBeforeTheKinds)
{
    // The construction pinned by test_dimension_draw.cpp: a dimension from
    // (0,0) to (10,0) offset 3 has its line at y = 3 and text above it.
    const DimensionDrawing d =
        buildDimension(DimensionGeometry{Point2(0, 0), Point2(10, 0), 3.0, ""}, DimensionStyle{});
    EXPECT_TRUE(d.hasDimensionLine);
    EXPECT_TRUE(d.curves.empty());
    EXPECT_EQ(d.dimensionLine.start, Point2(0, 3));
    EXPECT_EQ(d.dimensionLine.end, Point2(10, 3));
}

TEST(LeaderDraw, TheLandingRunsAwayFromTheLineAndTheNoteSitsBeyondIt)
{
    const Model model;
    const ann::TextMeasure measure = [](std::string_view line, const ann::TextFace&) {
        return 0.5 * static_cast<double>(characterCount(line));
    };
    // Tip at the origin, bend at (10, 10): heading right. At 1:1000 the
    // landing is 2.5 m, the note 2.5 m tall and half its height beyond.
    LeaderGeometry leader{.vertices = {Point2(0, 0), Point2(10, 10)}, .text = "AB"};
    const ann::Drawing right = ann::buildLeader(model, leader, 1000.0, measure);
    ASSERT_FALSE(right.strokes.empty());
    EXPECT_EQ(right.strokes.front().back(), Point2(12.5, 10)) << "the landing's end";
    ASSERT_EQ(right.texts.size(), 1u);
    EXPECT_NEAR(right.texts[0].origin.x, 12.5 + 1.25, 1e-12);
    ASSERT_EQ(right.fills.size(), 1u) << "the closed arrow";
    EXPECT_EQ(right.fills[0][0], Point2(0, 0)) << "its tip at the leader's tip";

    // Heading left: the landing and the note go left, the note right-justified.
    leader.vertices = {Point2(0, 0), Point2(-10, 10)};
    const ann::Drawing left = ann::buildLeader(model, leader, 1000.0, measure);
    EXPECT_EQ(left.strokes.front().back(), Point2(-12.5, 10));
    EXPECT_LT(left.texts[0].origin.x, -12.5);
}

TEST(LeaderDraw, CalloutsFrameTheirNote)
{
    const Model model;
    LeaderGeometry leader{.vertices = {Point2(0, 0), Point2(10, 10)}, .text = "12",
                          .callout = CalloutShape::Circle};
    const ann::Drawing balloon = ann::buildLeader(model, leader, 1000.0, ann::estimatedMeasure());
    ASSERT_EQ(balloon.outlines.size(), 1u);
    // The circle clears the note: every text box corner inside it.
    Box2 circle;
    for (const Point2& p : balloon.outlines[0]) {
        circle.expand(p);
    }
    const Point2 centre = circle.center();
    const double radius = 0.5 * circle.width();
    for (const Point2& corner : balloon.textBoxes.front()) {
        EXPECT_LT(corner.distanceTo(centre), radius);
    }
    EXPECT_NEAR(centre.x - radius, 12.5, 0.05) << "the landing ends on the circle";

    leader.callout = CalloutShape::Box;
    const ann::Drawing boxed = ann::buildLeader(model, leader, 1000.0, ann::estimatedMeasure());
    ASSERT_EQ(boxed.outlines.size(), 1u);
    ASSERT_EQ(boxed.masks.size(), 1u) << "the frame is filled behind the note";
}

TEST(LeaderDraw, ArrowSizesArePaperMillimetres)
{
    const Model model;
    const LeaderGeometry leader{.vertices = {Point2(0, 0), Point2(100, 0)}, .arrowSize = 2.0,
                                .landing = 0.0};
    for (const double scale : {500.0, 2000.0}) {
        const ann::Drawing d = ann::buildLeader(model, leader, scale, ann::estimatedMeasure());
        ASSERT_EQ(d.fills.size(), 1u);
        const auto& head = d.fills[0];
        EXPECT_NEAR(head[0].distanceTo((head[1] + head[2]) * 0.5), 2.0 * scale / 1000.0, 1e-9);
    }
}
