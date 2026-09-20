#include <gtest/gtest.h>

#include <cmath>
#include <limits>

#include "katana/survey/leveling.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;

namespace {

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();

// Level loop from BM A (100.000) back to BM A, hand computed:
//   setup   BS      FS      HI = elev + BS     elevation = HI - FS
//     1    1.500   1.200    101.500            TP1 100.300
//     2    1.800   0.900    102.100            TP2 101.200
//     3    0.700   1.894    101.900            A   100.006
//   sum BS 4.000, sum FS 3.994: sum BS - sum FS = +0.006 = last - first.
//   Closing on A = 100.000: misclosure = 100.006 - 100.000 = +0.006.
LevelRun loopRun()
{
    LevelRun run;
    run.name = "loop";
    run.startElevation = 100.0;
    run.setups = {{"TP1", 1.500, 1.200, 100.0}, {"TP2", 1.800, 0.900, 200.0},
                  {"A", 0.700, 1.894, 300.0}};
    run.closingElevation = 100.0;
    return run;
}

} // namespace

TEST(SurveyLeveling, HeightOfInstrumentAndElevations)
{
    const auto result = computeLevelRun(loopRun());
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result->points.size(), 3u);

    const double heights[] = {101.500, 102.100, 101.900};
    const double elevations[] = {100.300, 101.200, 100.006};
    const char* const ids[] = {"TP1", "TP2", "A"};
    for (std::size_t i = 0; i < 3; ++i) {
        EXPECT_EQ(result->points[i].pointId, ids[i]);
        EXPECT_NEAR(result->points[i].heightOfInstrument, heights[i], 1e-12) << i;
        EXPECT_NEAR(result->points[i].elevation, elevations[i], 1e-12) << i;
        // No adjustment requested: nothing is corrected.
        EXPECT_DOUBLE_EQ(result->points[i].correction, 0.0);
        EXPECT_DOUBLE_EQ(result->points[i].adjustedElevation, result->points[i].elevation);
    }
    EXPECT_NEAR(result->sumBacksights, 4.000, 1e-12);
    EXPECT_NEAR(result->sumForesights, 3.994, 1e-12);
    EXPECT_NEAR(result->totalDistance, 600.0, 1e-12);

    ASSERT_TRUE(result->misclosure.has_value());
    EXPECT_NEAR(*result->misclosure, 0.006, 1e-12);
    // The page check of every level book: sum BS - sum FS = last - first elevation.
    EXPECT_NEAR(result->sumBacksights - result->sumForesights,
                result->points.back().elevation - 100.0, 1e-12);
}

TEST(SurveyLeveling, AdjustmentBySetups)
{
    // -0.006 over 3 setups: -0.002 per setup, accumulating.
    const auto result = computeLevelRun(loopRun(), LevelAdjustment::BySetups);
    ASSERT_TRUE(result.ok());
    const double corrections[] = {-0.002, -0.004, -0.006};
    const double adjusted[] = {100.298, 101.196, 100.000};
    for (std::size_t i = 0; i < 3; ++i) {
        EXPECT_NEAR(result->points[i].correction, corrections[i], 1e-12) << i;
        EXPECT_NEAR(result->points[i].adjustedElevation, adjusted[i], 1e-12) << i;
    }
    EXPECT_NEAR(*result->misclosure, 0.006, 1e-12); // the raw misclosure is still reported
}

TEST(SurveyLeveling, AdjustmentByDistance)
{
    // Distances 100, 200, 300 (total 600): cumulative 100, 300, 600.
    //   -0.006 * 100/600 = -0.001   -0.006 * 300/600 = -0.003   -0.006 * 600/600 = -0.006
    const auto result = computeLevelRun(loopRun(), LevelAdjustment::ByDistance);
    ASSERT_TRUE(result.ok());
    const double corrections[] = {-0.001, -0.003, -0.006};
    const double adjusted[] = {100.299, 101.197, 100.000};
    for (std::size_t i = 0; i < 3; ++i) {
        EXPECT_NEAR(result->points[i].correction, corrections[i], 1e-12) << i;
        EXPECT_NEAR(result->points[i].adjustedElevation, adjusted[i], 1e-12) << i;
    }
}

TEST(SurveyLeveling, LineBetweenTwoBenchmarks)
{
    // BM1 50.000 -> BM2 known 52.347. One setup per turning point.
    //   HI 51.250 -> TP 50.120 ; HI 52.780 -> BM2 52.350. Misclosure +0.003.
    LevelRun run;
    run.startElevation = 50.0;
    run.setups = {{"TP", 1.250, 1.130, 0.0}, {"BM2", 2.660, 0.430, 0.0}};
    run.closingElevation = 52.347;
    const auto result = computeLevelRun(run, LevelAdjustment::BySetups);
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(result->points[0].elevation, 50.120, 1e-12);
    EXPECT_NEAR(result->points[1].elevation, 52.350, 1e-12);
    EXPECT_NEAR(*result->misclosure, 0.003, 1e-12);
    EXPECT_NEAR(result->points[0].adjustedElevation, 50.1185, 1e-12); // -0.0015
    EXPECT_NEAR(result->points[1].adjustedElevation, 52.347, 1e-12);
}

TEST(SurveyLeveling, OpenRunHasNoMisclosure)
{
    LevelRun run = loopRun();
    run.closingElevation.reset();
    const auto result = computeLevelRun(run);
    ASSERT_TRUE(result.ok());
    EXPECT_FALSE(result->misclosure.has_value());
    EXPECT_NEAR(result->points.back().elevation, 100.006, 1e-12);

    // There is nothing to distribute, and saying so beats silently doing nothing.
    const auto adjusted = computeLevelRun(run, LevelAdjustment::BySetups);
    ASSERT_FALSE(adjusted.ok());
    EXPECT_EQ(adjusted.error().code, ErrorCode::InvalidArgument);
}

TEST(SurveyLeveling, RejectsBadInput)
{
    LevelRun empty;
    EXPECT_EQ(computeLevelRun(empty).error().code, ErrorCode::InvalidArgument);

    LevelRun badReading = loopRun();
    badReading.setups[1].backsight = kNaN;
    EXPECT_FALSE(computeLevelRun(badReading).ok());

    LevelRun badStart = loopRun();
    badStart.startElevation = kNaN;
    EXPECT_FALSE(computeLevelRun(badStart).ok());

    LevelRun badClosing = loopRun();
    badClosing.closingElevation = kNaN;
    EXPECT_FALSE(computeLevelRun(badClosing).ok());

    LevelRun negativeDistance = loopRun();
    negativeDistance.setups[0].distance = -1.0;
    EXPECT_FALSE(computeLevelRun(negativeDistance).ok());

    LevelRun noDistances = loopRun();
    for (LevelSetup& setup : noDistances.setups) {
        setup.distance = 0.0;
    }
    EXPECT_TRUE(computeLevelRun(noDistances, LevelAdjustment::BySetups).ok());
    EXPECT_EQ(computeLevelRun(noDistances, LevelAdjustment::ByDistance).error().code,
              ErrorCode::InvalidArgument);
}

TEST(SurveyLeveling, AllowableMisclosure)
{
    // 12 mm * sqrt(K km), expressed in metres.
    EXPECT_NEAR(*allowableLevelMisclosure(0.012, 1000.0), 0.012, 1e-15);
    EXPECT_NEAR(*allowableLevelMisclosure(0.012, 4000.0), 0.024, 1e-15);
    EXPECT_NEAR(*allowableLevelMisclosure(0.012, 600.0), 0.012 * std::sqrt(0.6), 1e-15);
    EXPECT_DOUBLE_EQ(*allowableLevelMisclosure(0.012, 0.0), 0.0);

    // The 0.006 loop above over 0.6 km is inside 12 mm sqrt(K) = 9.3 mm.
    EXPECT_LT(0.006, *allowableLevelMisclosure(0.012, 600.0));

    EXPECT_FALSE(allowableLevelMisclosure(-0.012, 1000.0).ok());
    EXPECT_FALSE(allowableLevelMisclosure(0.012, -1.0).ok());
    EXPECT_FALSE(allowableLevelMisclosure(kNaN, 1000.0).ok());

    // 1 mm per sqrt(km) over 250 m: 0.001 * sqrt(0.25) = 0.5 mm.
    EXPECT_NEAR(*levelDifferenceSigma(0.001, 250.0), 0.0005, 1e-16);
}
