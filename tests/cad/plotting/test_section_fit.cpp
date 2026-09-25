// A section fitted to its viewport, its label steps and its layout
// (include/katana/cad/plotting/section_fit.hpp). Every expectation is worked
// by hand from the sheet scale ladder (1:1 ... 1:50 000), the exaggeration
// ladder 1, 2, 2.5, 4, 5, 8, 10, 20 and a fill of 0.9 unless a test says
// otherwise.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <string>
#include <vector>

#include "katana/cad/plotting/section_fit.hpp"

using katana::core::ErrorCode;
using namespace katana::cad::plotting;
using katana::geometry::Point2;

namespace {

SectionFitRequest request(double spanM, double depthM, double widthMm, double heightMm)
{
    SectionFitRequest r;
    r.spanM = spanM;
    r.depthM = depthM;
    r.plotWidthMm = widthMm;
    r.plotHeightMm = heightMm;
    return r;
}

void expectBox(const Box2& box, double x0, double y0, double x1, double y1)
{
    EXPECT_NEAR(box.min.x, x0, 1e-9);
    EXPECT_NEAR(box.min.y, y0, 1e-9);
    EXPECT_NEAR(box.max.x, x1, 1e-9);
    EXPECT_NEAR(box.max.y, y1, 1e-9);
}

} // namespace

// ---- the fit ---------------------------------------------------------------------------

TEST(SectionFit, TheScaleIsTheLargestAtWhichTheSpanFitsAndTheExaggerationTheLargestForTheDepth)
{
    // 200 m across 90% of 300 mm needs 1 : 200000 / 270 = 740.7: 1 : 750.
    // 5 m up 90% of 100 mm at 1 : 750: 5000 e / 750 <= 90 allows e <= 13.5,
    // so 10 (20 would be 133 mm).
    const auto fit = fitSection(request(200.0, 5.0, 300.0, 100.0));
    ASSERT_TRUE(fit.ok()) << fit.error().describe();
    EXPECT_EQ(fit->scale, 750.0);
    EXPECT_EQ(fit->exaggeration, 10.0);
}

TEST(SectionFit, AFlatLongSectionTakesTheTopOfTheLadder)
{
    // A 1 km road across 360 mm: 1 000 000 / 324 = 3086, 1 : 5000. 12 m of
    // relief at 1 : 5000 is 2.4 mm: 20 times is 48 mm, inside 81 mm.
    const auto fit = fitSection(request(1000.0, 12.0, 360.0, 90.0));
    ASSERT_TRUE(fit.ok());
    EXPECT_EQ(fit->scale, 5000.0);
    EXPECT_EQ(fit->exaggeration, 20.0);
}

TEST(SectionFit, ADeepNarrowSectionIsDrawnSmallerRatherThanOutOfItsPlot)
{
    // 10 m across 90 mm would be 1 : 111, so 1 : 125; but 30 m of depth at
    // 1 : 125 is 240 mm in a 36 mm high plot. At true scale the depth needs
    // 1 : 30000 / 36 = 833: 1 : 1000, at which even 2 times is 60 mm - so 1.
    const auto fit = fitSection(request(10.0, 30.0, 100.0, 40.0));
    ASSERT_TRUE(fit.ok());
    EXPECT_EQ(fit->scale, 1000.0);
    EXPECT_EQ(fit->exaggeration, 1.0);
}

TEST(SectionFit, NoDepthTakesTheFlatExaggeration)
{
    auto flat = request(40.0, 0.0, 180.0, 60.0);
    const auto cross = fitSection(flat);
    ASSERT_TRUE(cross.ok());
    EXPECT_EQ(cross->scale, 250.0); // 40 000 / 162 = 246.9
    EXPECT_EQ(cross->exaggeration, 1.0);
    flat.flatExaggeration = 10.0; // a long section's usual H 1:500 V 1:50
    EXPECT_EQ(fitSection(flat)->exaggeration, 10.0);
}

TEST(SectionFit, ASpanThatFitsExactlyIsNotPushedToTheNextScale)
{
    // 50 m across all of 100 mm is exactly 1 : 500; 4 m up all of 40 mm at
    // 1 : 500 allows exactly 5.
    auto exact = request(50.0, 4.0, 100.0, 40.0);
    exact.fill = 1.0;
    const auto fit = fitSection(exact);
    ASSERT_TRUE(fit.ok());
    EXPECT_EQ(fit->scale, 500.0);
    EXPECT_EQ(fit->exaggeration, 5.0);
}

TEST(SectionFit, BeyondTheLadderTheScaleIsTheOneNeeded)
{
    // 40 km across 90 mm at 90%: 1 : 40 000 000 / 81, past 1 : 50 000.
    const auto fit = fitSection(request(40000.0, 0.0, 90.0, 50.0));
    ASSERT_TRUE(fit.ok());
    EXPECT_NEAR(fit->scale, 40'000'000.0 / 81.0, 1e-3);
}

TEST(SectionFit, WhatCannotBeFittedIsRefused)
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    for (const double span : {0.0, -5.0, nan, inf}) {
        EXPECT_EQ(fitSection(request(span, 1.0, 100.0, 50.0)).error().code,
                  ErrorCode::InvalidArgument)
            << span;
    }
    EXPECT_EQ(fitSection(request(10.0, 1.0, 0.0, 50.0)).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(fitSection(request(10.0, 1.0, 100.0, -1.0)).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(fitSection(request(10.0, -1.0, 100.0, 50.0)).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(fitSection(request(10.0, nan, 100.0, 50.0)).error().code, ErrorCode::InvalidArgument);
    auto overfull = request(10.0, 1.0, 100.0, 50.0);
    overfull.fill = 1.5;
    EXPECT_EQ(fitSection(overfull).error().code, ErrorCode::InvalidArgument);
    overfull.fill = 0.0;
    EXPECT_EQ(fitSection(overfull).error().code, ErrorCode::InvalidArgument);
}

TEST(SectionFit, TheExaggerationIsTheLargestOfTheLadderThatFits)
{
    // The cross-section generator's case: 20 m deep at 1 : 500 in 98.8 mm
    // allows 2.47, so 2.
    EXPECT_EQ(fitExaggeration(20.0, 500.0, 98.8), 2.0);
    // Allowing exactly 2.5 takes 2.5; allowing 25 takes the top, 20.
    EXPECT_EQ(fitExaggeration(20.0, 500.0, 100.0), 2.5);
    EXPECT_EQ(fitExaggeration(2.0, 500.0, 100.0), 20.0);
    // Not even 1 fits: 1, the least there is.
    EXPECT_EQ(fitExaggeration(200.0, 500.0, 100.0), 1.0);
    // No depth: the flat value asked for.
    EXPECT_EQ(fitExaggeration(0.0, 500.0, 100.0), 1.0);
    EXPECT_EQ(fitExaggeration(0.0, 500.0, 100.0, 10.0), 10.0);
    // 12 m at 1 : 750 in 70 mm allows 4.375: not 4, which would print
    // V 1:187.5, but 2.5, V 1:300. At 1 : 500, 4 is V 1:125 and is taken.
    EXPECT_EQ(fitExaggeration(12.0, 750.0, 70.0), 2.5);
    EXPECT_EQ(fitExaggeration(12.0, 500.0, 100.0), 4.0);
    // A scale off the ladder has no whole vertical: the largest that fits.
    EXPECT_EQ(fitExaggeration(1.0, 1234.5, 100.0), 20.0);
    ASSERT_EQ(kSectionExaggerations.size(), 8u);
    EXPECT_EQ(kSectionExaggerations.back(), 20.0);
}

TEST(SectionFit, TheSpanIsMeasuredAroundAFixedCentre)
{
    EXPECT_EQ(sectionSpan(10.0, 20.0), 10.0);
    // Centred at 12 the plot must reach 20, 8 away, both ways: 16.
    EXPECT_EQ(sectionSpan(10.0, 20.0, 12.0), 16.0);
    EXPECT_EQ(sectionSpan(10.0, 20.0, 25.0), 30.0);
    EXPECT_EQ(sectionSpan(5.0, 5.0), 0.0);
    EXPECT_EQ(sectionSpan(20.0, 10.0), 0.0); // empty
}

// ---- label steps ---------------------------------------------------------------------

TEST(SectionLabelSteps, RoundStepsAreOneTwoAndFiveTimesAPowerOfTen)
{
    EXPECT_EQ(roundStepAtLeast(0.3), 0.5);
    EXPECT_EQ(roundStepAtLeast(6.0), 10.0);
    EXPECT_EQ(roundStepAtLeast(1.0), 1.0);
    EXPECT_EQ(roundStepAtLeast(2.0), 2.0);
    EXPECT_EQ(roundStepAtLeast(7.0), 10.0);
    EXPECT_NEAR(roundStepAtLeast(0.012), 0.02, 1e-15);
    EXPECT_EQ(roundStepAtLeast(0.0), 1.0);
    EXPECT_EQ(roundStepAtLeast(-3.0), 1.0);

    EXPECT_EQ(nextRoundStep(1.0), 2.0);
    EXPECT_EQ(nextRoundStep(2.0), 5.0);
    EXPECT_EQ(nextRoundStep(5.0), 10.0);
    EXPECT_EQ(nextRoundStep(20.0), 50.0);
    EXPECT_NEAR(nextRoundStep(0.5), 1.0, 1e-15);
    EXPECT_NEAR(nextRoundStep(0.01), 0.02, 1e-15);
}

TEST(SectionLabelSteps, AValueIsWrittenWithTheDecimalsItsStepNeeds)
{
    EXPECT_EQ(stepDecimals(1.0), 0);
    EXPECT_EQ(stepDecimals(50.0), 0);
    EXPECT_EQ(stepDecimals(0.5), 1);
    EXPECT_EQ(stepDecimals(0.2), 1);
    EXPECT_EQ(stepDecimals(0.05), 2);
    EXPECT_EQ(stepDecimals(0.25), 2);
    EXPECT_EQ(stepDecimals(0.001), 3);
    EXPECT_EQ(stepDecimals(0.0001), 3); // three at most

    EXPECT_EQ(stepText(120.0, 20.0), "120");
    EXPECT_EQ(stepText(12.5, 0.5), "12.5");
    EXPECT_EQ(stepText(-3.0, 1.0), "-3");
    EXPECT_EQ(stepText(-0.0001, 0.1), "0.0"); // never "-0.0"
    EXPECT_EQ(stepText(-0.4, 1.0), "0");
}

TEST(SectionLabelSteps, GridValuesAreWholeStepsInsideTheRange)
{
    EXPECT_EQ(gridValues(-7.0, 13.0, 5.0), (std::vector<double>{-5.0, 0.0, 5.0, 10.0}));
    EXPECT_EQ(gridValues(0.0, 10.0, 5.0), (std::vector<double>{0.0, 5.0, 10.0}));
    const auto tenths = gridValues(0.1, 0.35, 0.1);
    ASSERT_EQ(tenths.size(), 3u);
    EXPECT_NEAR(tenths[2], 0.3, 1e-15);
    EXPECT_TRUE(gridValues(1.0, 0.0, 1.0).empty());
    EXPECT_TRUE(gridValues(0.0, 1.0, 0.0).empty());
    EXPECT_EQ(gridValues(0.0, 1000.0, 1.0, 10).size(), 10u);
}

TEST(SectionLabelSteps, TheStepGrowsUntilTheWidestLabelClearsItsNeighbours)
{
    // A made-up font: every character 1.2 mm wide.
    const auto widest = [](double first, double last) {
        return [first, last](double step) {
            double w = 0.0;
            for (const double v : gridValues(first, last, step)) {
                w = std::max(w, 1.2 * static_cast<double>(stepText(v, step).size()));
            }
            return w;
        };
    };
    // 1 : 500 is 2 mm a metre. A 10 mm minimum gives 5 m; "1300" is
    // 4.8 mm, with 2 mm of gap 6.8 mm: 5 m holds.
    EXPECT_EQ(labelStep(2.0, 10.0, 2.0, widest(1000.0, 1300.0)), 5.0);
    // Chainages of 10 000 and more are 6 mm and, with the gap, 8 mm; at
    // 1 : 1000 (1 mm a metre) the minimum's 10 m step is 10 mm: holds. At
    // 1 : 2000 a 5 mm minimum gives 10 m, 5 mm: too tight; 20 m is 10 mm.
    EXPECT_EQ(labelStep(1.0, 10.0, 2.0, widest(10000.0, 12000.0)), 10.0);
    EXPECT_EQ(labelStep(0.5, 5.0, 2.0, widest(10000.0, 12000.0)), 20.0);
    // A label written across the axis passes its height: the minimum rules.
    EXPECT_EQ(labelStep(2.0, 10.0, 1.0, [](double) { return 1.4; }), 5.0);
    // Wide labels on a fine grid: 1 : 25 is 40 mm a metre and the minimum
    // gives 0.5 m; "1234.5" is 7.2 mm - 9.2 with the gap - inside 20 mm.
    EXPECT_EQ(labelStep(40.0, 10.0, 2.0, widest(1234.0, 1236.0)), 0.5);
    // Labels so wide that 5 mm steps crowd them go to 10 mm steps.
    EXPECT_EQ(labelStep(1.0, 5.0, 1.0, [](double) { return 8.0; }), 10.0);
}

// ---- the plot's layout ----------------------------------------------------------------

TEST(SectionLayout, ALongSectionKeepsItsBandUnderThePlot)
{
    // A long section across the A3 drawing area, above its title's 7 mm:
    // 23..410 x 167..287. Four band rows are 32 mm; the plot starts 1 mm
    // above them, 22 mm in for the row names, 2 mm short of the top and right.
    SectionLayoutRequest r;
    r.area = Box2(Point2(23.0, 167.0), Point2(410.0, 287.0));
    r.bandRows = 4;
    r.levelLabelMm = 9.0;
    const SectionLayout layout = sectionPlotLayout(r);
    EXPECT_TRUE(layout.banded);
    EXPECT_EQ(layout.leftMm, 22.0);
    expectBox(layout.plot, 45.0, 200.0, 408.0, 285.0);

    // Levels wider than the names widen the column: 25 mm and 1.6 of clearance.
    r.levelLabelMm = 25.0;
    const SectionLayout wide = sectionPlotLayout(r);
    EXPECT_NEAR(wide.leftMm, 26.6, 1e-12);
    EXPECT_NEAR(wide.plot.min.x, 49.6, 1e-12);
}

TEST(SectionLayout, ABandThatWouldCrushThePlotGivesWayToTheAxisValues)
{
    // 50 mm high: the band would leave 50 - 33 - 2 = 15 mm, under 30.
    SectionLayoutRequest r;
    r.area = Box2(Point2(0.0, 0.0), Point2(200.0, 50.0));
    r.bandRows = 4;
    r.levelLabelMm = 8.0;
    const SectionLayout layout = sectionPlotLayout(r);
    EXPECT_FALSE(layout.banded);
    EXPECT_NEAR(layout.leftMm, 9.6, 1e-12);
    expectBox(layout.plot, 9.6, 5.0, 198.0, 48.0);
}

TEST(SectionLayout, ACrossSectionLeavesRoomForItsCaptionAndItsLevels)
{
    SectionLayoutRequest r;
    r.area = Box2(Point2(100.0, 100.0), Point2(280.0, 160.0));
    r.caption = true;
    r.levelLabelMm = 7.4;
    const SectionLayout layout = sectionPlotLayout(r);
    EXPECT_FALSE(layout.banded);
    // Left 7.4 + 1.6 = 9; below 4 for the values, 4.5 for the caption, 1 gap.
    expectBox(layout.plot, 109.0, 109.5, 278.0, 158.0);
    // Short levels still get 6 mm.
    r.levelLabelMm = 1.0;
    EXPECT_NEAR(sectionPlotLayout(r).plot.min.x, 106.0, 1e-12);
}

TEST(SectionLayout, AnAreaTooSmallForAPlotHasNone)
{
    SectionLayoutRequest r;
    r.area = Box2(Point2(0.0, 0.0), Point2(20.0, 12.0));
    EXPECT_TRUE(sectionPlotLayout(r).plot.empty());
    r.area = Box2();
    EXPECT_TRUE(sectionPlotLayout(r).plot.empty());
}

// ---- a section viewport ----------------------------------------------------------------

TEST(SectionViewport, ItsSectionsAreDrawnAboveTheTitlesStrip)
{
    expectBox(sectionDrawingArea(Box2(Point2(23.0, 35.0), Point2(210.0, 145.0))), 23.0, 42.0, 210.0,
              145.0);
    // A rectangle lower than the strip keeps none of itself for sections.
    EXPECT_EQ(sectionDrawingArea(Box2(Point2(0.0, 0.0), Point2(50.0, 5.0))).height(), 0.0);
    EXPECT_TRUE(sectionDrawingArea(Box2()).empty());
}

TEST(SectionViewport, CrossSectionsAreCutAtTheStationsElseEveryIntervalToTheEnd)
{
    ViewportSource source;
    source.stations = {60.0, 20.0};
    EXPECT_EQ(viewportStations(source), (std::vector<double>{60.0, 20.0}));

    // Every 0.1 m from 0 to 0.3 lands on 0.3 itself: counted in whole
    // intervals, not added up (0.1 + 0.1 + 0.1 is 0.30000000000000004).
    source.stations.clear();
    source.chainageFrom = 0.0;
    source.chainageTo = 0.3;
    source.sectionInterval = 0.1;
    const std::vector<double> tenths = viewportStations(source);
    ASSERT_EQ(tenths.size(), 4u);
    EXPECT_EQ(tenths.back(), 3.0 * 0.1);

    // Every 20 m from 100 to 150: 100, 120, 140 - not past the end.
    source.chainageFrom = 100.0;
    source.chainageTo = 150.0;
    source.sectionInterval = 20.0;
    EXPECT_EQ(viewportStations(source), (std::vector<double>{100.0, 120.0, 140.0}));

    // At most kMaximumSectionRows, and none without an interval or a range.
    source.chainageTo = 1.0e6;
    source.sectionInterval = 1.0;
    EXPECT_EQ(viewportStations(source).size(), kMaximumSectionRows);
    source.sectionInterval = 0.0;
    EXPECT_TRUE(viewportStations(source).empty());
}

TEST(SectionViewport, AHalfWidthNotGivenIsTheDefault)
{
    ViewportSource source;
    EXPECT_EQ(viewportHalfWidth(source), kDefaultSectionHalfWidth);
    source.sectionHalfWidth = -5.0;
    EXPECT_EQ(viewportHalfWidth(source), kDefaultSectionHalfWidth);
    source.sectionHalfWidth = std::numeric_limits<double>::infinity();
    EXPECT_EQ(viewportHalfWidth(source), kDefaultSectionHalfWidth);
    source.sectionHalfWidth = 12.5;
    EXPECT_EQ(viewportHalfWidth(source), 12.5);
}
