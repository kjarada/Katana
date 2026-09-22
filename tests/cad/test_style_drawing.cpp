// A 12d definition turned into geometry (PLAN.MD 20.3, slice 2).

#include <gtest/gtest.h>

#include <cmath>
#include <numbers>

#include "katana/cad/style_drawing.hpp"

using katana::cad::StyleDrawing;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StrokeText;
using katana::entity::StyleUnits;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

constexpr double kPi = std::numbers::pi;

LineStyle definition(std::vector<Stroke> strokes, StyleUnits units = StyleUnits::World)
{
    LineStyle style;
    style.name = "S";
    style.units = units;
    style.strokes = std::move(strokes);
    return style;
}

Stroke move(double x, double y)
{
    return Stroke{StrokeOp::Move, Point2(x, y)};
}

Stroke draw(double x, double y)
{
    return Stroke{StrokeOp::Draw, Point2(x, y)};
}

// Total length of everything drawn, which is what a pattern repeating along a
// line multiplies.
double drawnLength(const StyleDrawing& drawing)
{
    double total = 0.0;
    for (const auto& stroke : drawing.strokes) {
        for (std::size_t i = 1; i < stroke.path.vertices.size(); ++i) {
            total += stroke.path.vertices[i - 1].distanceTo(stroke.path.vertices[i]);
        }
    }
    return total;
}

} // namespace

TEST(StyleDrawing, AMoveStartsARunAndADrawExtendsIt)
{
    // Two separate strokes: a 3-long run and a 1-long one.
    const LineStyle style = definition({move(0, 0), draw(3, 0), move(5, 0), draw(5, 1)});
    const StyleDrawing drawing = katana::cad::linestyleDrawing(
        style, Polyline2{{Point2(0, 0), Point2(100, 0)}, false});
    ASSERT_FALSE(drawing.strokes.empty());
    EXPECT_EQ(drawing.strokes[0].path.vertices.size(), 2u);
    EXPECT_EQ(drawing.strokes[0].path.vertices[0], Point2(0, 0));
    EXPECT_EQ(drawing.strokes[0].path.vertices[1], Point2(3, 0));
    // The period is the span of the strokes, 5, so 100/5 + 1 = 21 instances,
    // each drawing 3 + 1.
    EXPECT_EQ(drawing.strokes.size(), 21u * 2u);
    EXPECT_NEAR(drawnLength(drawing), 21.0 * 4.0, 1e-9);
}

TEST(StyleDrawing, TheStatedLengthIsThePeriodRatherThanTheStrokesSpan)
{
    LineStyle style = definition({move(0, 0), draw(1, 0)});
    style.length = 10.0;
    const StyleDrawing drawing = katana::cad::linestyleDrawing(
        style, Polyline2{{Point2(0, 0), Point2(50, 0)}, false});
    EXPECT_EQ(drawing.strokes.size(), 6u) << "at 0, 10, 20, 30, 40 and 50";
    EXPECT_EQ(drawing.strokes[1].path.vertices[0], Point2(10, 0));
}

TEST(StyleDrawing, ThePatternsYIsAcrossTheLineAndBendsWithIt)
{
    // A tick 1 to the left of the line. The line turns a right angle at
    // (10,0) and is 20 long, so with a period of 10 the three instances sit
    // at the start, exactly ON the corner, and at the end.
    LineStyle style = definition({move(0, 0), draw(0, 1)});
    style.length = 10.0;
    const StyleDrawing drawing = katana::cad::linestyleDrawing(
        style, Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10)}, false});
    ASSERT_EQ(drawing.strokes.size(), 3u);
    // Along +x, left is +y.
    EXPECT_EQ(drawing.strokes[0].path.vertices[0], Point2(0, 0));
    EXPECT_EQ(drawing.strokes[0].path.vertices[1], Point2(0, 1));
    // On the corner the direction is genuinely two-valued, and the OUTGOING
    // leg is taken - the pattern is being laid forwards, so what is about to
    // be drawn belongs to the leg it is about to be drawn along. Along +y,
    // left is -x.
    EXPECT_EQ(drawing.strokes[1].path.vertices[0], Point2(10, 0));
    const Point2 atCorner = drawing.strokes[1].path.vertices[1];
    EXPECT_NEAR(atCorner.x, 9.0, 1e-9);
    EXPECT_NEAR(atCorner.y, 0.0, 1e-9);
    // And at the end of the second leg, still across it.
    const Point2 atEnd = drawing.strokes[2].path.vertices[1];
    EXPECT_NEAR(atEnd.x, 9.0, 1e-9);
    EXPECT_NEAR(atEnd.y, 10.0, 1e-9);
}

TEST(StyleDrawing, APaperStyleIsMeasuredInPlotMillimetresAndAWorldStyleIsNot)
{
    const std::vector<Stroke> strokes{move(0, 0), draw(2, 0)};
    const Polyline2 line{{Point2(0, 0), Point2(100, 0)}, false};
    // 500 model units to the millimetre is a 1:500 plot.
    const StyleDrawing paper =
        katana::cad::linestyleDrawing(definition(strokes, StyleUnits::Paper), line, 500.0);
    const StyleDrawing world =
        katana::cad::linestyleDrawing(definition(strokes, StyleUnits::World), line, 500.0);
    EXPECT_EQ(paper.strokes[0].path.vertices[1], Point2(1000, 0)) << "2 mm at 1:500 is 1000 m";
    EXPECT_EQ(world.strokes[0].path.vertices[1], Point2(2, 0)) << "2 model units, whatever the plot";
}

TEST(StyleDrawing, ASymbolIsPlacedScaledAndRotated)
{
    // A unit square from -1 to 1: two wide, so `size` 4 doubles it.
    LineStyle style = definition({move(-1, -1), draw(1, -1), draw(1, 1), draw(-1, 1), draw(-1, -1)});
    style.atVertices = true;
    const StyleDrawing plain = katana::cad::symbolDrawing(style, Point2(10, 20));
    EXPECT_EQ(plain.strokes[0].path.vertices[0], Point2(9, 19));

    const StyleDrawing scaled = katana::cad::symbolDrawing(style, Point2(0, 0), 4.0);
    EXPECT_EQ(scaled.strokes[0].path.vertices[0], Point2(-2, -2)) << "size is a width";

    const StyleDrawing turned = katana::cad::symbolDrawing(style, Point2(0, 0), 0.0, kPi / 2.0);
    const Point2 corner = turned.strokes[0].path.vertices[0];
    EXPECT_NEAR(corner.x, 1.0, 1e-12) << "(-1,-1) turned a quarter turn is (1,-1)";
    EXPECT_NEAR(corner.y, -1.0, 1e-12);
}

TEST(StyleDrawing, ASymbolGoesOnEveryVertexAndALinestyleAlongTheLine)
{
    LineStyle symbol = definition({move(0, 0), draw(1, 0)});
    symbol.atVertices = true;
    const Polyline2 line{{Point2(0, 0), Point2(10, 0), Point2(20, 0)}, false};
    EXPECT_EQ(katana::cad::styleDrawing(symbol, line).strokes.size(), 3u) << "one per vertex";

    LineStyle along = definition({move(0, 0), draw(1, 0)});
    along.length = 5.0;
    EXPECT_EQ(katana::cad::styleDrawing(along, line).strokes.size(), 5u) << "0 5 10 15 20";
}

TEST(StyleDrawing, ATwoPointStyleStretchesBetweenItsAnchorsWithoutShearing)
{
    // Anchors 4 apart; drawn across a span of 8 turned a quarter turn, so
    // everything doubles and turns with it.
    LineStyle style = definition({move(0, 0), draw(4, 0), draw(4, 1)}, StyleUnits::TwoPoint);
    style.anchor1 = Point2(0, 0);
    style.anchor2 = Point2(4, 0);
    const StyleDrawing drawing =
        katana::cad::twoPointDrawing(style, Point2(0, 0), Point2(0, 8));
    ASSERT_EQ(drawing.strokes.size(), 1u);
    const auto& points = drawing.strokes[0].path.vertices;
    ASSERT_EQ(points.size(), 3u);
    EXPECT_NEAR(points[1].x, 0.0, 1e-12);
    EXPECT_NEAR(points[1].y, 8.0, 1e-12) << "the second anchor lands on the second point";
    // (4,1) scaled by 2 and turned a quarter turn is (-2, 8).
    EXPECT_NEAR(points[2].x, -2.0, 1e-12) << "scaled equally in both axes, not sheared";
    EXPECT_NEAR(points[2].y, 8.0, 1e-12);
}

TEST(StyleDrawing, AColourCommandSetsThePenForWhatFollowsAndViewColourMeansTheEntitys)
{
    LineStyle style = definition({
        Stroke{StrokeOp::Pen, {}, 0.0, 0.0, 0.0, Stroke::kNoText, "pen 035"},
        move(0, 0),
        draw(1, 0),
        Stroke{StrokeOp::Pen, {}, 0.0, 0.0, 0.0, Stroke::kNoText, "view_colour"},
        move(1, 0),
        draw(2, 0),
    });
    style.atVertices = true;
    const StyleDrawing drawing = katana::cad::symbolDrawing(style, Point2(0, 0));
    ASSERT_EQ(drawing.strokes.size(), 2u);
    EXPECT_EQ(drawing.strokes[0].pen, "pen 035");
    EXPECT_EQ(drawing.strokes[1].pen, "") << "view_colour is the entity's own colour";
}

TEST(StyleDrawing, ACircleAnArcAndADotAreDrawnAboutTheCurrentPoint)
{
    Stroke circle;
    circle.op = StrokeOp::Circle;
    circle.radius = 2.0;
    Stroke arc;
    arc.op = StrokeOp::Arc;
    arc.radius = 1.0;
    arc.startAngle = 0.0;
    arc.endAngle = 180.0;
    Stroke dot;
    dot.op = StrokeOp::Dot;

    LineStyle style = definition({move(5, 5), circle, arc, dot});
    style.atVertices = true;
    const StyleDrawing drawing = katana::cad::symbolDrawing(style, Point2(0, 0));
    ASSERT_EQ(drawing.strokes.size(), 3u);

    EXPECT_TRUE(drawing.strokes[0].path.closed);
    for (const Point2& point : drawing.strokes[0].path.vertices) {
        EXPECT_NEAR(point.distanceTo(Point2(5, 5)), 2.0, 1e-12) << "a circle about the pen";
    }
    // A half turn of radius 1 from (6,5) round to (4,5).
    const auto& arcPoints = drawing.strokes[1].path.vertices;
    EXPECT_NEAR(arcPoints.front().x, 6.0, 1e-12);
    EXPECT_NEAR(arcPoints.back().x, 4.0, 1e-12);
    EXPECT_NEAR(arcPoints.back().y, 5.0, 1e-12);

    ASSERT_EQ(drawing.strokes[2].path.vertices.size(), 1u) << "a dot is a point mark";
    EXPECT_EQ(drawing.strokes[2].path.vertices[0], Point2(5, 5));
}

TEST(StyleDrawing, TextTakesTheDirectionOfTheLineItSitsOn)
{
    LineStyle style = definition({move(0, 0)});
    StrokeText text;
    text.text = "WM";
    text.height = 1.5;
    text.justify = "middle-centre";
    text.font = "Arial";
    style.texts.push_back(text);
    Stroke mark;
    mark.op = StrokeOp::Text;
    mark.text = 0;
    style.strokes.push_back(mark);
    style.length = 10.0;

    // A line going straight up: the text turns with it.
    const StyleDrawing drawing = katana::cad::linestyleDrawing(
        style, Polyline2{{Point2(0, 0), Point2(0, 30)}, false});
    ASSERT_FALSE(drawing.texts.empty());
    EXPECT_EQ(drawing.texts[0].text, "WM");
    EXPECT_EQ(drawing.texts[0].height, 1.5);
    EXPECT_NEAR(drawing.texts[0].angle, kPi / 2.0, 1e-12);
    EXPECT_EQ(drawing.texts[0].justify, "middle-centre");
    EXPECT_EQ(drawing.texts.size(), 4u) << "at 0, 10, 20 and 30";
}

TEST(StyleDrawing, ADefinitionWithNoExtentIsDrawnOnceRatherThanEndlessly)
{
    // A single tick across the line has no length along it, so there is no
    // period to repeat at. Drawing it once beats not returning.
    const LineStyle style = definition({move(0, 0), draw(0, 1)});
    const StyleDrawing drawing = katana::cad::linestyleDrawing(
        style, Polyline2{{Point2(0, 0), Point2(1000, 0)}, false});
    EXPECT_EQ(drawing.strokes.size(), 1u);
}

TEST(StyleDrawing, AVeryShortPeriodOnAVeryLongLineIsTruncatedRatherThanRunningAway)
{
    LineStyle style = definition({move(0, 0), draw(0.001, 0)});
    style.length = 0.001;
    // 30 km of traverse at a millimetre a time would be thirty million
    // instances. It is capped, and what comes back is still a drawing.
    const StyleDrawing drawing = katana::cad::linestyleDrawing(
        style, Polyline2{{Point2(0, 0), Point2(30000, 0)}, false});
    EXPECT_EQ(drawing.strokes.size(), 20000u);
}

TEST(StyleDrawing, DegenerateInputGivesNothingRatherThanNonsense)
{
    const LineStyle style = definition({move(0, 0), draw(1, 0)});
    EXPECT_TRUE(katana::cad::linestyleDrawing(style, Polyline2{}).empty());
    EXPECT_TRUE(katana::cad::styleDrawing(style, Polyline2{}).empty());
    // Every vertex in one place: no direction to lay a pattern along.
    EXPECT_TRUE(katana::cad::linestyleDrawing(
                    style, Polyline2{{Point2(5, 5), Point2(5, 5), Point2(5, 5)}, false})
                    .empty());
    // A definition with no strokes draws nothing, from anywhere.
    EXPECT_TRUE(katana::cad::symbolDrawing(definition({}), Point2(0, 0)).empty());
}

TEST(StyleDrawing, TheFactorScalesEveryCoordinateAndTheOriginIsWhereTheDefinitionIsAnchored)
{
    LineStyle style = definition({move(1, 1), draw(3, 1)});
    style.factor = 2.0;
    style.origin = Point2(1, 1);
    style.atVertices = true;
    const StyleDrawing drawing = katana::cad::symbolDrawing(style, Point2(100, 200));
    // (1,1) is the origin, so it lands on the point; (3,1) is 2 further in x,
    // doubled by the factor.
    EXPECT_EQ(drawing.strokes[0].path.vertices[0], Point2(100, 200));
    EXPECT_EQ(drawing.strokes[0].path.vertices[1], Point2(104, 200));
}
