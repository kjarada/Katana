// Linetype dashing (PLAN.MD Phase 09).
//
// The failure this file exists to prevent is the one that LOOKS right: dashing
// in screen space instead of model space. A fixed pixel pattern renders a
// perfectly convincing dashed line, and is exactly backwards - the dashes stay
// the same size as you zoom, so a 10 m fence and a 10 km boundary get identical
// dashes. ZoomDoesNotChangeWhereTheDashesFall and
// ALongerPathGetsProportionallyMoreDashes are the two that catch it.

#include <gtest/gtest.h>

#include <cmath>
#include <vector>

#include "katana/cad/dashing.hpp"

using katana::cad::DashOptions;
using katana::cad::forEachDash;
using katana::cad::qtDashPattern;
using katana::cad::shouldDash;
using katana::entity::Linetype;
using katana::entity::LinetypeElement;
using katana::geometry::Point2;

namespace {

// 1 m dash, 0.5 m gap: a 1.5 m period.
Linetype dashed()
{
    Linetype linetype;
    linetype.name = "dashed";
    linetype.pattern = {LinetypeElement{1.0}, LinetypeElement{-0.5}};
    return linetype;
}

// Dash, gap, dot, gap - the DXF "dash dot" shape.
Linetype dashDot()
{
    Linetype linetype;
    linetype.name = "dashdot";
    linetype.pattern = {LinetypeElement{1.0}, LinetypeElement{-0.25}, LinetypeElement{0.0},
                        LinetypeElement{-0.25}};
    return linetype;
}

Linetype continuous()
{
    Linetype linetype;
    linetype.name = "continuous";
    return linetype;
}

struct Span {
    Point2 from;
    Point2 to;
    [[nodiscard]] double length() const { return from.distanceTo(to); }
};

// Collects the spans of a horizontal run of `length` metres along the x axis.
std::vector<Span> dashesAlongX(double length, const Linetype& linetype,
                               const DashOptions& options, bool* dashed = nullptr)
{
    const std::vector<Point2> points = {Point2(0.0, 0.0), Point2(length, 0.0)};
    std::vector<Span> spans;
    const bool laid = forEachDash(points, false, linetype, options,
                                  [&spans](const Point2& a, const Point2& b) {
                                      spans.push_back(Span{a, b});
                                  });
    if (dashed != nullptr) {
        *dashed = laid;
    }
    if (!laid) {
        spans.clear();
    }
    return spans;
}

DashOptions atScale(double viewScale)
{
    DashOptions options;
    options.viewScale = viewScale;
    return options;
}

} // namespace

TEST(Dashing, AContinuousLinetypeIsNeverDashed)
{
    EXPECT_FALSE(shouldDash(continuous(), atScale(100.0)));
    bool laid = true;
    const auto spans = dashesAlongX(10.0, continuous(), atScale(100.0), &laid);
    EXPECT_FALSE(laid) << "the caller must be told to draw it solid";
    EXPECT_TRUE(spans.empty());
}

TEST(Dashing, DashesAreLaidOutInModelUnits)
{
    // A 10 m run of a 1 m dash / 0.5 m gap pattern: dashes at 0-1, 1.5-2.5,
    // 3-4, 4.5-5.5, 6-7, 7.5-8.5, 9-10. Seven of them, every one exactly 1 m.
    const auto spans = dashesAlongX(10.0, dashed(), atScale(100.0));
    ASSERT_EQ(spans.size(), 7u);
    for (std::size_t i = 0; i < spans.size(); ++i) {
        EXPECT_NEAR(spans[i].from.x, static_cast<double>(i) * 1.5, 1e-9) << "span " << i;
        EXPECT_NEAR(spans[i].length(), 1.0, 1e-9) << "span " << i;
    }
}

TEST(Dashing, ZoomDoesNotChangeWhereTheDashesFall)
{
    // THE test. Model-space dashing gives identical spans at every zoom; a
    // screen-space implementation would give the same NUMBER of dashes at every
    // zoom instead, which is the opposite.
    const auto reference = dashesAlongX(20.0, dashed(), atScale(50.0));
    ASSERT_FALSE(reference.empty());

    for (const double viewScale : {10.0, 50.0, 200.0, 1000.0}) {
        const auto spans = dashesAlongX(20.0, dashed(), atScale(viewScale));
        ASSERT_EQ(spans.size(), reference.size()) << "viewScale " << viewScale;
        for (std::size_t i = 0; i < spans.size(); ++i) {
            EXPECT_NEAR(spans[i].from.x, reference[i].from.x, 1e-12)
                << "viewScale " << viewScale << " span " << i;
            EXPECT_NEAR(spans[i].length(), reference[i].length(), 1e-12);
        }
    }
}

TEST(Dashing, ALongerPathGetsProportionallyMoreDashes)
{
    // The other half of the same property: a 10 km boundary has a thousand
    // times the dashes of a 10 m fence, because a dash is a ground length.
    const auto shortRun = dashesAlongX(15.0, dashed(), atScale(100.0));
    const auto longRun = dashesAlongX(150.0, dashed(), atScale(100.0));

    ASSERT_FALSE(shortRun.empty());
    EXPECT_NEAR(static_cast<double>(longRun.size()) / static_cast<double>(shortRun.size()), 10.0,
                0.15);
    // And every dash is still the same ground length in both.
    for (const Span& span : longRun) {
        EXPECT_NEAR(span.length(), 1.0, 1e-9);
    }
}

TEST(Dashing, ThePatternRunsContinuouslyAcrossVertices)
{
    // DXF's $PLINEGEN default: the pattern carries on along the whole polyline
    // rather than restarting at each vertex. A restart would put a dash at
    // every corner and make the spacing depend on where the vertices are.
    const std::vector<Point2> bent = {Point2(0.0, 0.0), Point2(2.0, 0.0), Point2(2.0, 4.0)};
    std::vector<Span> spans;
    ASSERT_TRUE(forEachDash(bent, false, dashed(), atScale(100.0),
                            [&spans](const Point2& a, const Point2& b) {
                                spans.push_back(Span{a, b});
                            }));

    // Total pen-down length must be the same fraction of the path length as
    // the dash is of the period (1.0 of 1.5), give or take the final partial.
    double penDown = 0.0;
    for (const Span& span : spans) {
        penDown += span.length();
    }
    const double pathLength = 6.0;
    EXPECT_NEAR(penDown / pathLength, 1.0 / 1.5, 0.12);

    // A span crossing the corner is split into two, so no span may cut the
    // corner: every one must lie on the path.
    for (const Span& span : spans) {
        const bool onFirst = std::abs(span.from.y) < 1e-9 && std::abs(span.to.y) < 1e-9;
        const bool onSecond =
            std::abs(span.from.x - 2.0) < 1e-9 && std::abs(span.to.x - 2.0) < 1e-9;
        EXPECT_TRUE(onFirst || onSecond)
            << "a span cut the corner: (" << span.from.x << "," << span.from.y << ") to ("
            << span.to.x << "," << span.to.y << ")";
    }
}

TEST(Dashing, AClosedPathIncludesItsClosingSegment)
{
    const std::vector<Point2> square = {Point2(0.0, 0.0), Point2(10.0, 0.0), Point2(10.0, 10.0),
                                        Point2(0.0, 10.0)};
    std::vector<Span> open;
    std::vector<Span> closed;
    ASSERT_TRUE(forEachDash(square, false, dashed(), atScale(100.0),
                            [&open](const Point2& a, const Point2& b) {
                                open.push_back(Span{a, b});
                            }));
    ASSERT_TRUE(forEachDash(square, true, dashed(), atScale(100.0),
                            [&closed](const Point2& a, const Point2& b) {
                                closed.push_back(Span{a, b});
                            }));
    EXPECT_GT(closed.size(), open.size()) << "the fourth side must be dashed too";
}

TEST(Dashing, APatternTooFineToResolveIsDrawnSolidRatherThanAsAPalerLine)
{
    // Below about a pixel per element, every dash and gap falls inside one
    // pixel and antialiasing turns the line a uniformly paler colour. The user
    // sees the wrong COLOUR, not a dash pattern, so the honest answer is a
    // solid line.
    EXPECT_TRUE(shouldDash(dashed(), atScale(10.0)))   // 0.5 m gap = 5 px
        << "zoomed in, the pattern is resolvable";
    EXPECT_FALSE(shouldDash(dashed(), atScale(1.0)))   // 0.5 m gap = 0.5 px
        << "zoomed out, it is not";

    bool laid = true;
    const auto spans = dashesAlongX(1000.0, dashed(), atScale(0.5), &laid);
    EXPECT_FALSE(laid);
    EXPECT_TRUE(spans.empty());
}

TEST(Dashing, AnUnreasonableSpanCountFallsBackToSolidInsteadOfTruncating)
{
    // A truncating implementation silently shortens the line: a 900 m fence
    // drawn as 300 m, with no error anywhere. All-or-nothing instead.
    DashOptions options = atScale(1000.0);
    options.maximumSpans = 50;

    bool laid = true;
    const auto spans = dashesAlongX(10000.0, dashed(), options, &laid);
    EXPECT_FALSE(laid) << "the caller must be told to draw the whole path solid";
    EXPECT_TRUE(spans.empty()) << "nothing may have been emitted";

    // Raising the budget makes the same path dash.
    options.maximumSpans = 100000;
    EXPECT_TRUE(dashesAlongX(10000.0, dashed(), options, &laid).size() > 1000u);
    EXPECT_TRUE(laid);
}

TEST(Dashing, ADotIsEmittedAsAZeroLengthSpan)
{
    const auto spans = dashesAlongX(10.0, dashDot(), atScale(100.0));
    ASSERT_FALSE(spans.empty());

    std::size_t dots = 0;
    std::size_t dashes = 0;
    for (const Span& span : spans) {
        if (span.length() < 1e-12) {
            ++dots;
        } else {
            ++dashes;
            EXPECT_NEAR(span.length(), 1.0, 1e-9);
        }
    }
    EXPECT_GT(dots, 3u) << "the dots in the pattern must actually be emitted";
    EXPECT_GT(dashes, 3u);
}

TEST(Dashing, DegenerateInputProducesNothingRatherThanSpinning)
{
    DashOptions options = atScale(100.0);
    std::vector<Span> spans;
    const auto collect = [&spans](const Point2& a, const Point2& b) {
        spans.push_back(Span{a, b});
    };

    EXPECT_FALSE(forEachDash({}, false, dashed(), options, collect));
    EXPECT_FALSE(forEachDash({Point2(0, 0)}, false, dashed(), options, collect));
    // Two coincident points: a path with no length.
    EXPECT_FALSE(forEachDash({Point2(1, 1), Point2(1, 1)}, false, dashed(), options, collect));
    // A pattern that is all dots has no length either; it must not loop.
    Linetype dots;
    dots.name = "dots";
    dots.pattern = {LinetypeElement{0.0}, LinetypeElement{0.0}};
    EXPECT_FALSE(forEachDash({Point2(0, 0), Point2(10, 0)}, false, dots, options, collect));
    EXPECT_TRUE(spans.empty());

    // A nonsensical view scale is refused rather than dividing by it.
    EXPECT_FALSE(shouldDash(dashed(), atScale(0.0)));
    EXPECT_FALSE(shouldDash(dashed(), atScale(-5.0)));
    EXPECT_FALSE(shouldDash(dashed(), atScale(std::nan(""))));
}

// ---- the Qt conversion -------------------------------------------------------------

TEST(Dashing, QtDashUnitsArePenWidthsNotPixels)
{
    // QPen multiplies the array by the pen width when it draws, so a 2 px pen
    // doubles every entry. Forgetting the division renders perfectly and makes
    // every pattern as many times too long as the pen is wide - and the
    // viewport draws at 1.5 and 2 px, so 1.5x and 2x wrong.
    const DashOptions options = atScale(100.0); // 1 m dash = 100 px

    const auto atOnePixel = qtDashPattern(dashed(), options, 1.0);
    ASSERT_EQ(atOnePixel.size(), 2u);
    EXPECT_NEAR(atOnePixel[0], 100.0, 1e-9) << "1 m at 100 px/m with a 1 px pen";
    EXPECT_NEAR(atOnePixel[1], 50.0, 1e-9);

    const auto atTwoPixels = qtDashPattern(dashed(), options, 2.0);
    ASSERT_EQ(atTwoPixels.size(), 2u);
    EXPECT_NEAR(atTwoPixels[0], 50.0, 1e-9) << "a 2 px pen must HALVE every entry";
    EXPECT_NEAR(atTwoPixels[1], 25.0, 1e-9);

    // A cosmetic pen (width 0) is drawn one pixel wide, so it divides by 1.
    const auto cosmetic = qtDashPattern(dashed(), options, 0.0);
    ASSERT_EQ(cosmetic.size(), 2u);
    EXPECT_NEAR(cosmetic[0], 100.0, 1e-9);
}

TEST(Dashing, TheQtPatternScalesWithZoomBecausePixelsDo)
{
    // The mirror of ZoomDoesNotChangeWhereTheDashesFall: the MODEL layout is
    // fixed, so the PIXEL pattern must grow with the view scale. A constant Qt
    // array at every zoom would be the screen-space bug.
    const auto near = qtDashPattern(dashed(), atScale(200.0), 1.0);
    const auto far = qtDashPattern(dashed(), atScale(100.0), 1.0);
    ASSERT_EQ(near.size(), 2u);
    ASSERT_EQ(far.size(), 2u);
    EXPECT_NEAR(near[0] / far[0], 2.0, 1e-9) << "twice the zoom, twice the pixels per dash";
}

TEST(Dashing, TheQtPatternIsEmptyWhenTheLineShouldBeSolid)
{
    EXPECT_TRUE(qtDashPattern(continuous(), atScale(100.0), 1.0).empty());
    EXPECT_TRUE(qtDashPattern(dashed(), atScale(0.5), 1.0).empty()) << "too fine to resolve";
}

TEST(Dashing, TheQtPatternAlwaysHasAnEvenLength)
{
    // QPen alternates on/off from on, so an odd count inverts the meaning of
    // every entry when the array repeats. validate() makes a well-formed
    // pattern even, but a project written elsewhere is not ours to trust.
    Linetype odd;
    odd.name = "odd";
    odd.pattern = {LinetypeElement{1.0}, LinetypeElement{-0.5}, LinetypeElement{1.0}};
    const auto qt = qtDashPattern(odd, atScale(100.0), 1.0);
    ASSERT_FALSE(qt.empty());
    EXPECT_EQ(qt.size() % 2, 0u);
    for (const double value : qt) {
        EXPECT_GT(value, 0.0) << "QPen rejects a zero-length entry";
    }
}

TEST(Dashing, ADotBecomesAVisibleMarkNotAZero)
{
    // A zero-length on-span draws nothing under a flat cap, so a dot has to
    // become the thinnest mark Qt will actually render.
    const auto qt = qtDashPattern(dashDot(), atScale(100.0), 2.0);
    ASSERT_EQ(qt.size(), 4u);
    for (const double value : qt) {
        EXPECT_GT(value, 0.0);
    }
    EXPECT_LT(qt[2], qt[0]) << "the dot must be far shorter than the dash";
}
