// A plan's coordinate grid, worked out on paper (plan_grid.hpp): the
// automatic interval, the label text, where the lines, crosses, ticks and
// labels fall - square and rotated - what the labels keep off, the JSON form
// and the undoable edit.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <limits>
#include <string>

#include "katana/cad/document.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/plan_grid.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/math/numerics.hpp"

using katana::cad::Document;
using namespace katana::cad::plotting;
using katana::core::ErrorCode;

namespace {

// A 200 x 100 mm plan at 1:1000 over (305 230, 6 250 410): the ground from E
// 305 130 to 305 330 and N 6 250 360 to 6 250 460.
Viewport gridded(GridStyle style, double interval, double rotation = 0.0)
{
    Viewport viewport;
    viewport.id = "vp1";
    viewport.kind = ViewportKind::Plan;
    viewport.rect = Box2(Point2(100.0, 100.0), Point2(300.0, 200.0));
    viewport.scale = 1000.0;
    viewport.centre = Point2(305230.0, 6250410.0);
    viewport.rotation = rotation;
    viewport.gridStyle = style;
    viewport.gridInterval = interval;
    return viewport;
}

PlanGrid gridOf(const Viewport& viewport, const PlanGridOptions& options = {})
{
    auto grid = planGrid(viewport, storedPlacement(viewport), options);
    EXPECT_TRUE(grid.ok()) << (grid.ok() ? "" : grid.error().describe());
    return grid.ok() ? *grid : PlanGrid{};
}

bool overlap(const Box2& a, const Box2& b)
{
    return a.min.x < b.max.x && b.min.x < a.max.x && a.min.y < b.max.y && b.min.y < a.max.y;
}

bool onBorder(const Point2& p, const Box2& r)
{
    constexpr double kTolerance = 1e-9;
    const bool inside = p.x >= r.min.x - kTolerance && p.x <= r.max.x + kTolerance &&
                        p.y >= r.min.y - kTolerance && p.y <= r.max.y + kTolerance;
    return inside && (std::abs(p.x - r.min.x) < kTolerance || std::abs(p.x - r.max.x) < kTolerance ||
                      std::abs(p.y - r.min.y) < kTolerance || std::abs(p.y - r.max.y) < kTolerance);
}

std::vector<std::string> labelTexts(const PlanGrid& grid, ViewportEdge edge)
{
    std::vector<std::string> out;
    for (const GridLabel& label : grid.labels) {
        if (label.edge == edge) {
            out.push_back(label.text);
        }
    }
    return out;
}

} // namespace

TEST(PlanGrid, TheAutomaticIntervalIsTheRoundSpacingNearestFiftyMillimetresOnThePaper)
{
    EXPECT_DOUBLE_EQ(automaticGridInterval(500.0), 20.0);   // 40 mm apart
    EXPECT_DOUBLE_EQ(automaticGridInterval(1000.0), 50.0);  // 50 mm
    EXPECT_DOUBLE_EQ(automaticGridInterval(2000.0), 100.0); // 50 mm
    EXPECT_DOUBLE_EQ(automaticGridInterval(250.0), 10.0);   // 40 mm
    EXPECT_DOUBLE_EQ(automaticGridInterval(200.0), 10.0);   // 50 mm
    // 1:750 lies between 20 m (27 mm) and 50 m (67 mm); 67 mm is nearer 50
    // by ratio.
    EXPECT_DOUBLE_EQ(automaticGridInterval(750.0), 50.0);
    EXPECT_DOUBLE_EQ(automaticGridInterval(1.0), 0.05);
    EXPECT_EQ(automaticGridInterval(0.0), 0.0);
    EXPECT_EQ(automaticGridInterval(-500.0), 0.0);
    EXPECT_EQ(automaticGridInterval(std::numeric_limits<double>::quiet_NaN()), 0.0);
    EXPECT_EQ(automaticGridInterval(std::numeric_limits<double>::infinity()), 0.0);
    // Every scale of the ladder: 40 to 67 mm apart (1:75, 1:150 and 1:750
    // fall between 1-2-5 steps; 67 mm is nearer 50 than 27 mm is).
    for (const double scale : katana::cad::kSheetScales) {
        const double interval = automaticGridInterval(scale);
        const double spacing = interval * 1000.0 / scale;
        EXPECT_GE(spacing, 40.0 - 1e-9) << "1:" << scale;
        EXPECT_LE(spacing, 200.0 / 3.0 + 1e-9) << "1:" << scale;
        const double first = interval / std::pow(10.0, std::floor(std::log10(interval) + 1e-12));
        EXPECT_TRUE(std::abs(first - 1.0) < 1e-9 || std::abs(first - 2.0) < 1e-9 ||
                    std::abs(first - 5.0) < 1e-9)
            << interval;
    }
}

TEST(PlanGrid, CoordinatesAreGroupedByThousandsWithDecimalsOnlyWhenTheIntervalNeedsThem)
{
    EXPECT_EQ(gridDecimals(20.0), 0);
    EXPECT_EQ(gridDecimals(1.0), 0);
    EXPECT_EQ(gridDecimals(0.5), 1);
    EXPECT_EQ(gridDecimals(2.5), 1);
    EXPECT_EQ(gridDecimals(0.1), 1);
    EXPECT_EQ(gridDecimals(0.25), 2);
    EXPECT_EQ(gridDecimals(0.05), 2);
    EXPECT_EQ(gridDecimals(0.001), 3);
    EXPECT_EQ(gridDecimals(0.0001), 3);

    EXPECT_EQ(groupedCoordinate(305200.0, 0), "305 200");
    EXPECT_EQ(groupedCoordinate(6250400.0, 0), "6 250 400");
    EXPECT_EQ(groupedCoordinate(100000.0, 0), "100 000");
    EXPECT_EQ(groupedCoordinate(999.0, 0), "999");
    EXPECT_EQ(groupedCoordinate(1000.0, 0), "1 000");
    EXPECT_EQ(groupedCoordinate(-1200.0, 0), "-1 200");
    EXPECT_EQ(groupedCoordinate(-305200.5, 1), "-305 200.5");
    EXPECT_EQ(groupedCoordinate(12.5, 1), "12.5");
    EXPECT_EQ(groupedCoordinate(1234567.25, 2), "1 234 567.25");
    // Nothing prints as "-0".
    EXPECT_EQ(groupedCoordinate(-0.0, 0), "0");
    EXPECT_EQ(groupedCoordinate(-0.4, 0), "0");
    EXPECT_EQ(groupedCoordinate(-0.04, 1), "0.0");
}

TEST(PlanGrid, TheStylesHaveStableNames)
{
    for (const GridStyle style :
         {GridStyle::None, GridStyle::Ticks, GridStyle::Crosses, GridStyle::Lines}) {
        EXPECT_EQ(gridStyleFrom(toString(style)), style);
    }
    EXPECT_EQ(toString(GridStyle::Crosses), "crosses");
    EXPECT_FALSE(gridStyleFrom("dots").has_value());
    EXPECT_FALSE(gridStyleFrom("").has_value());
}

TEST(PlanGrid, PaperAndGroundConvertBothWaysAndTheFootprintIsTheGroundUnderTheRectangle)
{
    const Viewport viewport = gridded(GridStyle::Lines, 50.0, 0.3);
    const PlanPlacement at = storedPlacement(viewport);
    for (const Point2 paper : {Point2(100.0, 100.0), Point2(212.5, 187.25), Point2(300.0, 150.0)}) {
        const Point2 back = planWorldToPaper(viewport, at, planPaperToWorld(viewport, at, paper));
        EXPECT_NEAR(back.x, paper.x, 1e-6);
        EXPECT_NEAR(back.y, paper.y, 1e-6);
    }
    // The rectangle's centre is the viewport's centre on the ground.
    const Point2 middle = planPaperToWorld(viewport, at, Point2(200.0, 150.0));
    EXPECT_NEAR(middle.x, 305230.0, 1e-9);
    EXPECT_NEAR(middle.y, 6250410.0, 1e-9);

    const Viewport square = gridded(GridStyle::Lines, 50.0);
    const auto corners = planFootprint(square, storedPlacement(square));
    ASSERT_EQ(corners.size(), 4u);
    EXPECT_NEAR(corners[0].x, 305130.0, 1e-9);
    EXPECT_NEAR(corners[0].y, 6250360.0, 1e-9);
    EXPECT_NEAR(corners[2].x, 305330.0, 1e-9);
    EXPECT_NEAR(corners[2].y, 6250460.0, 1e-9);

    Viewport unplaced = square;
    unplaced.rect = Box2{};
    EXPECT_TRUE(planFootprint(unplaced, storedPlacement(unplaced)).empty());
    EXPECT_TRUE(planFootprint(square, {0.0, Point2()}).empty());
}

TEST(PlanGrid, ASquareGridsLinesLieAtRoundCoordinatesRightAcrossTheViewport)
{
    const PlanGrid grid = gridOf(gridded(GridStyle::Lines, 50.0));
    EXPECT_EQ(grid.style, GridStyle::Lines);
    EXPECT_DOUBLE_EQ(grid.interval, 50.0);
    EXPECT_EQ(grid.decimals, 0);
    // Eastings 305 150 to 305 300, then northings 6 250 400 and 6 250 450.
    ASSERT_EQ(grid.lines.size(), 6u);
    const double eastings[] = {305150.0, 305200.0, 305250.0, 305300.0};
    for (std::size_t i = 0; i < 4; ++i) {
        const GridLine& line = grid.lines[i];
        EXPECT_EQ(line.axis, GridAxis::Easting);
        EXPECT_DOUBLE_EQ(line.value, eastings[i]);
        const double x = 200.0 + (eastings[i] - 305230.0);
        EXPECT_NEAR(line.from.x, x, 1e-6);
        EXPECT_NEAR(line.from.y, 100.0, 1e-9);
        EXPECT_NEAR(line.to.x, x, 1e-6);
        EXPECT_NEAR(line.to.y, 200.0, 1e-9);
        EXPECT_EQ(line.fromEdge, ViewportEdge::Bottom);
        EXPECT_EQ(line.toEdge, ViewportEdge::Top);
    }
    EXPECT_EQ(grid.lines[4].axis, GridAxis::Northing);
    EXPECT_DOUBLE_EQ(grid.lines[4].value, 6250400.0);
    EXPECT_NEAR(grid.lines[4].from.x, 100.0, 1e-9);
    EXPECT_NEAR(grid.lines[4].from.y, 140.0, 1e-6);
    EXPECT_NEAR(grid.lines[4].to.x, 300.0, 1e-9);
    EXPECT_EQ(grid.lines[4].fromEdge, ViewportEdge::Left);
    EXPECT_EQ(grid.lines[4].toEdge, ViewportEdge::Right);
    EXPECT_NEAR(grid.lines[5].from.y, 190.0, 1e-6);

    // Lines are stroked whole, and nothing else.
    ASSERT_EQ(grid.strokes.size(), grid.lines.size());
    for (std::size_t i = 0; i < grid.lines.size(); ++i) {
        EXPECT_EQ(grid.strokes[i].from, grid.lines[i].from);
        EXPECT_EQ(grid.strokes[i].to, grid.lines[i].to);
    }
    EXPECT_TRUE(grid.crossings.empty());
}

TEST(PlanGrid, LabelsReadTheCoordinateAlongEveryEdgeInsideTheViewport)
{
    const Viewport viewport = gridded(GridStyle::Lines, 50.0);
    const PlanGrid grid = gridOf(viewport);
    const std::vector<std::string> across = {"E 305 150", "E 305 200", "E 305 250", "E 305 300"};
    const std::vector<std::string> up = {"N 6 250 400", "N 6 250 450"};
    EXPECT_EQ(labelTexts(grid, ViewportEdge::Bottom), across);
    EXPECT_EQ(labelTexts(grid, ViewportEdge::Top), across);
    EXPECT_EQ(labelTexts(grid, ViewportEdge::Left), up);
    EXPECT_EQ(labelTexts(grid, ViewportEdge::Right), up);
    ASSERT_EQ(grid.labels.size(), 12u);
    // Bottom first, then left, top and right.
    EXPECT_EQ(grid.labels.front().edge, ViewportEdge::Bottom);
    EXPECT_EQ(grid.labels.back().edge, ViewportEdge::Right);

    for (const GridLabel& label : grid.labels) {
        EXPECT_TRUE(viewport.rect.contains(label.box)) << label.text;
        EXPECT_EQ(label.horizontal, HorizontalJustify::Centre);
        switch (label.edge) {
        case ViewportEdge::Bottom:
            // Along the edge, 0.8 mm in, standing on the anchor.
            EXPECT_DOUBLE_EQ(label.angleDegrees, 0.0);
            EXPECT_NEAR(label.anchor.y, 100.8, 1e-9);
            EXPECT_EQ(label.vertical, VerticalJustify::Bottom);
            break;
        case ViewportEdge::Top:
            EXPECT_DOUBLE_EQ(label.angleDegrees, 0.0);
            EXPECT_NEAR(label.anchor.y, 199.2, 1e-9);
            EXPECT_EQ(label.vertical, VerticalJustify::Top);
            break;
        case ViewportEdge::Left:
            // Up the side, its head at the border.
            EXPECT_DOUBLE_EQ(label.angleDegrees, 90.0);
            EXPECT_NEAR(label.anchor.x, 100.8, 1e-9);
            EXPECT_EQ(label.vertical, VerticalJustify::Top);
            break;
        case ViewportEdge::Right:
            EXPECT_DOUBLE_EQ(label.angleDegrees, 90.0);
            EXPECT_NEAR(label.anchor.x, 299.2, 1e-9);
            EXPECT_EQ(label.vertical, VerticalJustify::Bottom);
            break;
        }
    }
    // Centred on its line: E 305 200 is at x 170, N 6 250 400 at y 140.
    EXPECT_NEAR(grid.labels[1].anchor.x, 170.0, 1e-6);
    EXPECT_NEAR(grid.labels[4].anchor.y, 140.0, 1e-6);
    // No two labels share any paper.
    for (std::size_t i = 0; i < grid.labels.size(); ++i) {
        for (std::size_t j = i + 1; j < grid.labels.size(); ++j) {
            EXPECT_FALSE(overlap(grid.labels[i].box, grid.labels[j].box))
                << grid.labels[i].text << " / " << grid.labels[j].text;
        }
    }
}

TEST(PlanGrid, LabelsKeepOffTheFurnitureAndOffEachOther)
{
    const Viewport viewport = gridded(GridStyle::Lines, 50.0);
    // A title in the bottom-left corner, 80 x 10 mm: the labels of E 305 150
    // and E 305 200 would sit on it.
    PlanGridOptions options;
    options.keepOut.push_back(Box2(Point2(100.0, 100.0), Point2(180.0, 110.0)));
    const PlanGrid grid = gridOf(viewport, options);
    EXPECT_EQ(labelTexts(grid, ViewportEdge::Bottom),
              (std::vector<std::string>{"E 305 250", "E 305 300"}));
    for (const GridLabel& label : grid.labels) {
        EXPECT_FALSE(overlap(label.box, options.keepOut.front())) << label.text;
    }
    // The lines themselves are not cut: the title knocks them out when drawn.
    EXPECT_EQ(grid.lines.size(), 6u);

    // Labels as wide as the spacing, 50 mm (50.8 with their knock-out):
    // E 305 150 at x 120 would stick out past the left edge, E 305 250 would
    // overlap E 305 200, so every other one is left out along the bottom.
    PlanGridOptions wide;
    wide.labelWidthMm = [](std::string_view) { return 50.0; };
    const PlanGrid crowded = gridOf(viewport, wide);
    EXPECT_EQ(labelTexts(crowded, ViewportEdge::Bottom),
              (std::vector<std::string>{"E 305 200", "E 305 300"}));
    // Up the sides N 6 250 450, at y 190, would stick out past the top.
    EXPECT_EQ(labelTexts(crowded, ViewportEdge::Left),
              (std::vector<std::string>{"N 6 250 400"}));
    for (std::size_t i = 0; i < crowded.labels.size(); ++i) {
        EXPECT_TRUE(viewport.rect.contains(crowded.labels[i].box));
        for (std::size_t j = i + 1; j < crowded.labels.size(); ++j) {
            EXPECT_FALSE(overlap(crowded.labels[i].box, crowded.labels[j].box));
        }
    }
}

TEST(PlanGrid, TwoLabelsCloserThanTheSpacingAreNotBothKept)
{
    // Along the bottom E 305 150, 305 200 ... sit 50 mm apart. Labels 48 mm
    // wide leave 2 mm between their centres' neighbours less 0.8 mm of
    // knock-out: 1.2 mm of paper, which reads as one label run on into the
    // next. The default 1.5 mm keeps every other one; asked for no spacing,
    // each fits.
    const Viewport viewport = gridded(GridStyle::Lines, 50.0);
    PlanGridOptions close;
    close.labelWidthMm = [](std::string_view) { return 48.0; };
    const PlanGrid spaced = gridOf(viewport, close);
    EXPECT_EQ(labelTexts(spaced, ViewportEdge::Bottom),
              (std::vector<std::string>{"E 305 200", "E 305 300"}));
    for (std::size_t i = 0; i < spaced.labels.size(); ++i) {
        for (std::size_t j = i + 1; j < spaced.labels.size(); ++j) {
            EXPECT_FALSE(overlap(spaced.labels[i].box.inflated(close.labelSpacingMm),
                                 spaced.labels[j].box))
                << spaced.labels[i].text << " / " << spaced.labels[j].text;
        }
    }
    close.labelSpacingMm = 0.0;
    EXPECT_EQ(labelTexts(gridOf(viewport, close), ViewportEdge::Bottom),
              (std::vector<std::string>{"E 305 200", "E 305 250", "E 305 300"}));
}

TEST(PlanGrid, TicksComeInFromTheBorderAndTheirLabelsStandPastThem)
{
    const Viewport viewport = gridded(GridStyle::Ticks, 50.0);
    const PlanGrid grid = gridOf(viewport);
    ASSERT_EQ(grid.lines.size(), 6u);
    ASSERT_EQ(grid.strokes.size(), 12u);
    // E 305 150 at x 120: 2.5 mm up from the bottom, 2.5 mm down from the top.
    EXPECT_EQ(grid.strokes[0], (GridSegment{Point2(120.0, 100.0), Point2(120.0, 102.5)}));
    EXPECT_NEAR(grid.strokes[1].from.y, 200.0, 1e-9);
    EXPECT_NEAR(grid.strokes[1].to.y, 197.5, 1e-9);
    for (const GridSegment& tick : grid.strokes) {
        EXPECT_TRUE(onBorder(tick.from, viewport.rect));
        EXPECT_NEAR((tick.to - tick.from).length(), 2.5, 1e-9);
        EXPECT_TRUE(viewport.rect.contains(tick.to));
    }
    // 2.5 mm of tick and 0.8 mm of gap.
    for (const GridLabel& label : grid.labels) {
        if (label.edge == ViewportEdge::Bottom) {
            EXPECT_NEAR(label.anchor.y, 103.3, 1e-9);
        }
        if (label.edge == ViewportEdge::Right) {
            EXPECT_NEAR(label.anchor.x, 296.7, 1e-9);
        }
    }
}

TEST(PlanGrid, CrossesMarkEveryIntersectionWhollyInsideTheViewport)
{
    const Viewport viewport = gridded(GridStyle::Crosses, 50.0);
    const PlanGrid grid = gridOf(viewport);
    // Four eastings by two northings.
    ASSERT_EQ(grid.crossings.size(), 8u);
    ASSERT_EQ(grid.strokes.size(), 16u);
    EXPECT_NEAR(grid.crossings[0].x, 120.0, 1e-6);
    EXPECT_NEAR(grid.crossings[0].y, 140.0, 1e-6);
    EXPECT_NEAR(grid.crossings[1].y, 190.0, 1e-6);
    // Each 3 mm across, along the grid.
    EXPECT_NEAR(grid.strokes[0].from.x, 118.5, 1e-6);
    EXPECT_NEAR(grid.strokes[0].to.x, 121.5, 1e-6);
    EXPECT_NEAR(grid.strokes[1].from.y, 138.5, 1e-6);
    EXPECT_NEAR(grid.strokes[1].to.y, 141.5, 1e-6);

    // An intersection 1 mm from the border is left out, not cut in half:
    // move the ground so E 305 150 falls at x 101.
    Viewport near = viewport;
    near.centre.x = 305150.0 + 99.0;
    const PlanGrid edge = gridOf(near);
    for (const Point2& crossing : edge.crossings) {
        EXPECT_GE(crossing.x, 101.5);
    }
    EXPECT_EQ(edge.crossings.size(), 6u);
    // The line is there; its label, centred 1 mm from the edge, would stick
    // out past it, so the first label along the bottom is the next line's.
    ASSERT_EQ(edge.lines.size(), 6u);
    EXPECT_NEAR(edge.lines[0].from.x, 101.0, 1e-6);
    EXPECT_EQ(edge.labels.front().text, "E 305 200");
}

TEST(PlanGrid, ARotatedGridRunsAskewAndEveryLineEndsOnTheBorderAtItsCoordinate)
{
    const double rotation = katana::math::kPi / 6.0; // 30 degrees
    const Viewport viewport = gridded(GridStyle::Lines, 20.0, rotation);
    const PlanPlacement at = storedPlacement(viewport);
    const PlanGrid grid = gridOf(viewport);
    ASSERT_FALSE(grid.lines.empty());

    // World north on the paper is 30 degrees clockwise of up.
    const Point2 north(std::sin(rotation), std::cos(rotation));
    std::size_t eastings = 0;
    for (const GridLine& line : grid.lines) {
        EXPECT_TRUE(onBorder(line.from, viewport.rect));
        EXPECT_TRUE(onBorder(line.to, viewport.rect));
        EXPECT_NEAR(std::fmod(std::abs(line.value), 20.0), 0.0, 1e-6);
        const Point2 a = planPaperToWorld(viewport, at, line.from);
        const Point2 b = planPaperToWorld(viewport, at, line.to);
        const double along = line.axis == GridAxis::Easting ? 0.0 : 1.0;
        EXPECT_NEAR(along == 0.0 ? a.x : a.y, line.value, 1e-6);
        EXPECT_NEAR(along == 0.0 ? b.x : b.y, line.value, 1e-6);
        const Point2 d = (line.to - line.from) * (1.0 / (line.to - line.from).length());
        if (line.axis == GridAxis::Easting) {
            ++eastings;
            EXPECT_NEAR(std::abs(d.x * north.x + d.y * north.y), 1.0, 1e-9);
        } else {
            EXPECT_NEAR(std::abs(d.x * north.x + d.y * north.y), 0.0, 1e-9);
        }
    }
    // Every easting that crosses the viewport is there: one whose value lies
    // strictly between the ground under two of the rectangle's corners.
    const auto corners = planFootprint(viewport, at);
    std::size_t expected = 0;
    for (double e = std::floor(305000.0 / 20.0) * 20.0; e < 305500.0; e += 20.0) {
        const auto [low, high] = std::ranges::minmax(
            {corners[0].x - e, corners[1].x - e, corners[2].x - e, corners[3].x - e});
        expected += low < 0.0 && high > 0.0 ? 1 : 0;
    }
    EXPECT_EQ(eastings, expected);

    // Every label is inside, clear of the others, and centred where its line
    // meets its edge.
    for (std::size_t i = 0; i < grid.labels.size(); ++i) {
        const GridLabel& label = grid.labels[i];
        EXPECT_TRUE(viewport.rect.contains(label.box)) << label.text;
        EXPECT_EQ(label.text.substr(0, 2), label.axis == GridAxis::Easting ? "E " : "N ");
        const auto line = std::ranges::find_if(grid.lines, [&](const GridLine& l) {
            return l.axis == label.axis && l.value == label.value;
        });
        ASSERT_NE(line, grid.lines.end());
        const Point2 end = line->fromEdge == label.edge ? line->from : line->to;
        const bool across = label.edge == ViewportEdge::Bottom || label.edge == ViewportEdge::Top;
        EXPECT_NEAR(across ? label.anchor.x : label.anchor.y, across ? end.x : end.y, 1e-9);
        for (std::size_t j = i + 1; j < grid.labels.size(); ++j) {
            EXPECT_FALSE(overlap(label.box, grid.labels[j].box));
        }
    }
    EXPECT_FALSE(grid.labels.empty());
}

TEST(PlanGrid, AnIntervalOfLessThanAMetreWritesItsDecimals)
{
    Viewport viewport = gridded(GridStyle::Ticks, 0.5);
    viewport.scale = 10.0; // 0.5 m is 50 mm
    viewport.centre = Point2(305230.25, 6250410.0);
    const PlanGrid grid = gridOf(viewport);
    EXPECT_EQ(grid.decimals, 1);
    EXPECT_EQ(labelTexts(grid, ViewportEdge::Top),
              (std::vector<std::string>{"E 305 229.5", "E 305 230.0", "E 305 230.5",
                                        "E 305 231.0"}));
}

TEST(PlanGrid, AnAutomaticGridTakesTheRoundSpacingForTheScale)
{
    Viewport viewport = gridded(GridStyle::Crosses, 0.0);
    viewport.scale = 500.0;
    const PlanGrid grid = gridOf(viewport);
    EXPECT_DOUBLE_EQ(grid.interval, 20.0);
    // The same viewport drawn at an automatic scale of 1:2000.
    const auto rescaled = planGrid(viewport, {2000.0, viewport.centre});
    ASSERT_TRUE(rescaled.ok());
    EXPECT_DOUBLE_EQ(rescaled->interval, 100.0);
}

TEST(PlanGrid, NoGridIsEmptyAndAGridThatCannotBeDrawnIsRefusedWithWhy)
{
    const Viewport off = gridded(GridStyle::None, 0.0);
    const auto none = planGrid(off, storedPlacement(off));
    ASSERT_TRUE(none.ok());
    EXPECT_EQ(*none, PlanGrid{});

    // 1 m at 1:10000 is 0.1 mm apart.
    Viewport dense = gridded(GridStyle::Lines, 1.0);
    dense.scale = 10000.0;
    const auto refused = planGrid(dense, storedPlacement(dense));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("1 m grid at 1:10000 is 0.1 mm apart"), std::string::npos)
        << refused.error().message;

    Viewport unplaced = gridded(GridStyle::Lines, 50.0);
    unplaced.rect = Box2{};
    EXPECT_EQ(planGrid(unplaced, storedPlacement(unplaced)).error().code,
              ErrorCode::InvalidArgument);
    const Viewport lines = gridded(GridStyle::Lines, 50.0);
    EXPECT_FALSE(planGrid(lines, {0.0, lines.centre}).ok());
    EXPECT_FALSE(planGrid(lines, {std::numeric_limits<double>::infinity(), lines.centre}).ok());
    Viewport negative = gridded(GridStyle::Lines, -5.0);
    EXPECT_FALSE(planGrid(negative, storedPlacement(negative)).ok());
    Viewport nan = gridded(GridStyle::Lines, std::numeric_limits<double>::quiet_NaN());
    EXPECT_FALSE(planGrid(nan, storedPlacement(nan)).ok());
}

TEST(PlanGrid, TheSameViewportAlwaysGivesTheSameGrid)
{
    const Viewport viewport = gridded(GridStyle::Ticks, 25.0, 0.4);
    EXPECT_EQ(gridOf(viewport), gridOf(viewport));
}

TEST(PlanGrid, TheGridIsStoredInTheSheetsJsonOnlyWhenItIsNotTheDefault)
{
    SheetSet set;
    Sheet sheet;
    sheet.id = "s1";
    sheet.viewports.push_back(gridded(GridStyle::Crosses, 25.0));
    Viewport plain = gridded(GridStyle::None, 0.0);
    plain.id = "vp2";
    sheet.viewports.push_back(plain);
    set.sheets.push_back(sheet);

    const auto json = sheetSetToJson(set);
    ASSERT_TRUE(json.ok());
    EXPECT_NE(json->find("\"grid_style\":\"crosses\""), std::string::npos) << *json;
    EXPECT_NE(json->find("\"grid_interval\":25"), std::string::npos) << *json;
    // Once each: the plain viewport writes neither.
    EXPECT_EQ(json->find("grid_style", json->find("grid_style") + 1), std::string::npos);
    EXPECT_EQ(json->find("grid_interval", json->find("grid_interval") + 1), std::string::npos);
    const auto back = sheetSetFromJson(*json);
    ASSERT_TRUE(back.ok());
    EXPECT_EQ(*back, set);

    for (const GridStyle style : {GridStyle::Ticks, GridStyle::Lines}) {
        set.sheets[0].viewports[0].gridStyle = style;
        set.sheets[0].viewports[0].gridInterval = 0.25;
        const auto again = sheetSetFromJson(*sheetSetToJson(set));
        ASSERT_TRUE(again.ok());
        EXPECT_EQ(*again, set);
    }

    const auto unknown = sheetSetFromJson(
        R"({"format": "katana-sheets", "version": 1, "sheets": [{"id": "s1", "viewports":
            [{"id": "vp1", "kind": "plan", "grid_style": "dots"}]}]})");
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::ParseFailure);
    EXPECT_NE(unknown.error().context.find("dots"), std::string::npos);
}

TEST(PlanGrid, SettingAPlansGridIsOneUndoableStep)
{
    Document document;
    Sheet sheet;
    sheet.id = "s1";
    sheet.viewports.push_back(gridded(GridStyle::None, 0.0));
    Viewport legend;
    legend.id = "vp2";
    legend.kind = ViewportKind::Legend;
    sheet.viewports.push_back(legend);
    ASSERT_TRUE(addSheet(document, sheet).ok());
    const auto viewport = [&document] { return document.sheetSet().sheets.at(0).viewports.at(0); };
    const std::size_t before = document.history().undoCount();

    ASSERT_TRUE(setPlanGrid(document, "vp1", GridStyle::Lines, 25.0).ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(viewport().gridStyle, GridStyle::Lines);
    EXPECT_DOUBLE_EQ(viewport().gridInterval, 25.0);
    // The same again records nothing.
    ASSERT_TRUE(setPlanGrid(document, "vp1", GridStyle::Lines, 25.0).ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);

    // Refused, and nothing changes.
    const auto legendGrid = setPlanGrid(document, "vp2", GridStyle::Lines);
    ASSERT_FALSE(legendGrid.ok());
    EXPECT_EQ(legendGrid.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(setPlanGrid(document, "vp9", GridStyle::Lines).error().code, ErrorCode::NotFound);
    EXPECT_EQ(setPlanGrid(document, "vp1", GridStyle::Ticks, -1.0).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(setPlanGrid(document, "vp1", GridStyle::Ticks,
                          std::numeric_limits<double>::infinity())
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(viewport().gridStyle, GridStyle::Lines);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(viewport().gridStyle, GridStyle::None);
    EXPECT_DOUBLE_EQ(viewport().gridInterval, 0.0);
}
