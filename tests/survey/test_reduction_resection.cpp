// The resection of a free station: a setup on a point nothing else positions,
// computed from its pointings to points already placed (resectSetup,
// src/katana_survey/reduction_adjust.cpp; tried from placeSetups,
// src/katana_survey/reduction.cpp).
//
// Most cases share one free station R, really at N 1040, E 1030, H 52.345,
// instrument height 1.550, its circle turned so that azimuth = reading + 20
// degrees, and three marks 50 m from it, each at H 50.000 with a 1.700 target:
//
//   A  N 1000, E 1000   azimuth 180 + atan(3/4) = 216.869897646 deg, reading 196.869897646
//   B  N 1000, E 1060   azimuth 180 - atan(3/4) = 143.130102354 deg, reading 123.130102354
//   C  N 1090, E 1030   azimuth 0,                                   reading 340
//
// Each is observed on both faces with the zenith and slope distance that put
// it 50 m out and 2.345 m below the mark: the line of sight falls
// 50 - 52.345 - 1.550 + 1.700 = -2.195 m, so z = atan2(50, -2.195) and
// S = hypot(50, 2.195), and S sin z = 50, S cos z + 1.550 - 1.700 = -2.345.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/survey/reduction.hpp"
#include "test_reduction_support.hpp"

using namespace katana::survey;
using namespace reduction_test;

namespace {

constexpr double kStationNorthing = 1040.0;
constexpr double kStationEasting = 1030.0;
constexpr double kStationHeight = 52.345;
constexpr double kInstrumentHeight = 1.550;
constexpr double kTargetHeight = 1.700;

struct Mark {
    const char* id;
    double northing;
    double easting;
};

constexpr Mark kA{"A", 1000.0, 1000.0};
constexpr Mark kB{"B", 1000.0, 1060.0};
constexpr Mark kC{"C", 1090.0, 1030.0};

double azimuthFrom(double northing, double easting, const Mark& mark)
{
    return katana::math::normalizeAngle(
        std::atan2(mark.easting - easting, mark.northing - northing));
}

// Both faces of one pointing to `target`: the face-right reading half a turn
// on, the zenith as the model stores it (face-left equivalent).
void bothFaces(std::vector<Shot>& shots, const std::string& target, std::size_t& index,
               std::optional<double> reading, std::optional<double> zenith,
               std::optional<double> slope, double targetHeight)
{
    shots.push_back(Shot{target, index++, Face::Left, reading, zenith, slope, targetHeight});
    shots.push_back(
        Shot{target, index++, Face::Right,
             reading ? std::optional<double>(katana::math::normalizeAngle(*reading + deg(180)))
                     : std::nullopt,
             zenith, slope, targetHeight});
}

// What R observes of `mark`: the reading for the circle's 20 degrees, and -
// when `distance` - the zenith and slope distance of a line `horizontal` long
// that falls 2.195 m.
void observe(std::vector<Shot>& shots, std::size_t& index, const Mark& mark, bool distance,
             double horizontal = 50.0, double readingError = 0.0)
{
    const double reading = katana::math::normalizeAngle(
        azimuthFrom(kStationNorthing, kStationEasting, mark) - deg(20) + readingError);
    const double fall = -2.195;
    bothFaces(shots, mark.id, index, reading,
              distance ? std::optional<double>(std::atan2(horizontal, fall)) : std::nullopt,
              distance ? std::optional<double>(std::hypot(horizontal, fall)) : std::nullopt,
              kTargetHeight);
}

// The marks entered with coordinates; R and Q named only.
SurveyProject marksAnd(std::vector<Mark> marks, std::vector<Shot> shots)
{
    SurveyProject project;
    for (const Mark& mark : marks) {
        project.points.push_back(point(mark.id, mark.northing, mark.easting, 50.0));
    }
    project.unpositionedPoints.push_back(unpositioned("R"));
    project.unpositionedPoints.push_back(unpositioned("Q"));
    project.stations.push_back(setup("S1", "R", kInstrumentHeight, {}, shots));
    return project;
}

// Q from R: reading 0 (azimuth 20), 10 m level, target at the instrument's
// height - at N 1040 + 10 cos 20 = 1049.396926208, E 1030 + 10 sin 20 =
// 1033.420201433, and R's height.
void shootQ(std::vector<Shot>& shots, std::size_t& index)
{
    bothFaces(shots, "Q", index, 0.0, deg(90), 10.0, kInstrumentHeight);
}

std::size_t warningsWith(const ReductionReport& report, const std::string& words)
{
    std::size_t count = 0;
    for (const ReportMessage& warning : report.warnings) {
        count += warning.text.find(words) != std::string::npos ? 1 : 0;
    }
    return count;
}

std::string allWarnings(const ReductionReport& report)
{
    std::string text;
    for (const ReportMessage& warning : report.warnings) {
        text += warning.text + "\n";
    }
    return text;
}

} // namespace

TEST(ReductionResection, TwoPlacedPointsWithDistancesPositionAndOrientAStationNothingElsePlaces)
{
    // Two directions and two distances for north, east and the orientation:
    // R, its 20 degrees and Q come back as constructed. The heights are
    // 50 - (-2.345) = 52.345 from each (no curvature in these settings).
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true);
    observe(shots, index, kB, true);
    shootQ(shots, index);
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ReductionReport& report = outcome->report;

    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr) << allWarnings(report);
    EXPECT_EQ(r->method, ComputationMethod::Resection);
    EXPECT_NEAR(r->northing, kStationNorthing, 1e-8);
    EXPECT_NEAR(r->easting, kStationEasting, 1e-8);
    ASSERT_TRUE(r->elevation.has_value());
    EXPECT_NEAR(*r->elevation, kStationHeight, 1e-9);
    ASSERT_TRUE(report.setups[0].orientationCorrection.has_value());
    EXPECT_NEAR(*report.setups[0].orientationCorrection, deg(20), 1e-10);

    ASSERT_EQ(report.resections.size(), 1U);
    const ResectionReport& resection = report.resections[0];
    EXPECT_EQ(resection.stationId, "S1");
    EXPECT_EQ(resection.pointId, "R");
    EXPECT_EQ(resection.targets, (std::vector<std::string>{"A", "B"}));
    EXPECT_NEAR(resection.orientation, deg(20), 1e-10);
    EXPECT_EQ(resection.horizontal.redundancy, 1U); // 4 observations, 3 unknowns
    EXPECT_EQ(resection.horizontal.residuals.size(), 4U);
    ASSERT_TRUE(resection.height.has_value());
    EXPECT_EQ(resection.height->redundancy, 1U); // two heights of one unknown

    const ComputedPoint* q = findPoint(*outcome, "Q");
    ASSERT_NE(q, nullptr);
    EXPECT_NEAR(q->northing, 1049.396926208, 1e-8);
    EXPECT_NEAR(q->easting, 1033.420201433, 1e-8);
    EXPECT_NEAR(*q->elevation, kStationHeight, 1e-9);

    // Its targets were used, so they are not also checks of it.
    EXPECT_TRUE(report.misclosures.empty());
    EXPECT_EQ(warningsWith(report, "has no position"), 0U) << allWarnings(report);
}

TEST(ReductionResection, ThreeDirectionsAloneResectAStationOffTheCircleThroughThem)
{
    // Three readings for three unknowns: R is the centre of the circle through
    // A, B and C (all 50 m away), as far from the danger circle as it gets.
    // Nothing checks it, and with no distance there is no height.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, false);
    observe(shots, index, kB, false);
    observe(shots, index, kC, false);
    shootQ(shots, index);
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB, kC}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ReductionReport& report = outcome->report;

    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr) << allWarnings(report);
    EXPECT_NEAR(r->northing, kStationNorthing, 1e-8);
    EXPECT_NEAR(r->easting, kStationEasting, 1e-8);
    EXPECT_FALSE(r->elevation.has_value());
    EXPECT_NEAR(*report.setups[0].orientationCorrection, deg(20), 1e-10);
    ASSERT_EQ(report.resections.size(), 1U);
    EXPECT_EQ(report.resections[0].horizontal.redundancy, 0U);
    EXPECT_FALSE(report.resections[0].height.has_value());
    EXPECT_EQ(warningsWith(report, "its resection has no redundancy"), 1U) << allWarnings(report);
    EXPECT_EQ(warningsWith(report, "resected with no height"), 1U) << allWarnings(report);

    const ComputedPoint* q = findPoint(*outcome, "Q");
    ASSERT_NE(q, nullptr);
    EXPECT_NEAR(q->northing, 1049.396926208, 1e-8);
    EXPECT_NEAR(q->easting, 1033.420201433, 1e-8);
}

TEST(ReductionResection, ResidualsAndPrecisionMatchAnIndependentLeastSquares)
{
    // A, B and C with directions and distances, the distance to A 4 mm long
    // and the reading to B 6" high on both faces. Worked apart from this code
    // by a scratch script of the textbook model (Ghilani, Adjustment
    // Computations, ch. 15): unknowns N, E and the orientation w; readings
    // atan2(dE, dN) - w weighted 1 / (s^2 + (c / D)^2) with s = 3" / sqrt 2
    // (a face pair) and c = hypot(1, 1) mm over D = 50 m (50.004 to A);
    // distances weighted 1 / (hypot(2 mm, 2 ppm S)^2 + c^2); its own normal
    // equations, solved by Cramer's rule and iterated to 1e-13 m:
    //   N 1040.001316214, E 1030.000730846, w 19.999611938 deg
    //   residuals (computed - observed): reading A +0.5512", distance A
    //   -2.5085 mm, reading B +1.0668", distance B +0.6145 mm, reading C
    //   -1.6180", distance C -1.3162 mm; v'Pv 1.503460509 on 3 degrees of
    //   freedom, variance factor 0.501153503;
    //   a posteriori sN 0.8477 mm, sE 0.6832 mm, sw 2.5990".
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true, 50.004);
    observe(shots, index, kB, true, 50.0, arcSeconds(6.0));
    observe(shots, index, kC, true);
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB, kC}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.resections.size(), 1U);
    const ResectionReport& resection = outcome->report.resections[0];

    EXPECT_NEAR(resection.northing, 1040.001316214, 1e-8);
    EXPECT_NEAR(resection.easting, 1030.000730846, 1e-8);
    EXPECT_NEAR(resection.orientation, deg(19.999611938), 1e-10);
    const AdjustmentReport& horizontal = resection.horizontal;
    EXPECT_EQ(horizontal.redundancy, 3U);
    ASSERT_TRUE(horizontal.varianceFactor.has_value());
    EXPECT_NEAR(*horizontal.varianceFactor, 0.501153503, 1e-8);
    ASSERT_EQ(horizontal.residuals.size(), 6U);
    const double expected[6] = {arcSeconds(0.5512), -0.0025085, arcSeconds(1.0668),
                                0.0006145,          arcSeconds(-1.6180), -0.0013162};
    const char* labels[6] = {"direction at S1: A", "distance at S1: A", "direction at S1: B",
                             "distance at S1: B",  "direction at S1: C", "distance at S1: C"};
    for (std::size_t i = 0; i < 6; ++i) {
        const ReportResidual& residual = horizontal.residuals[i];
        EXPECT_EQ(residual.observation, labels[i]);
        EXPECT_EQ(residual.angular, i % 2 == 0) << labels[i];
        const double tolerance = residual.angular ? arcSeconds(0.0001) : 1e-7;
        EXPECT_NEAR(residual.residual, expected[i], tolerance) << labels[i];
    }
    EXPECT_NEAR(resection.sigmaNorthing, 0.0008477, 1e-7);
    EXPECT_NEAR(resection.sigmaEasting, 0.0006832, 1e-7);
    EXPECT_NEAR(resection.sigmaOrientation, arcSeconds(2.5990), arcSeconds(0.0001));

    // The station carries that precision onto the drawing.
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_NEAR(*r->sigmaNorthing, 0.0008477, 1e-7);
    EXPECT_NEAR(*r->sigmaEasting, 0.0006832, 1e-7);
}

TEST(ReductionResection, TheHeightIsTheWeightedMeanOfTrigonometricHeightsWithCurvature)
{
    // The default settings: curvature and refraction on, k = 0.13,
    // R = 6 371 000 m. C's mark is 6 mm higher than the others. By hand:
    //   B = (1 - k) / (2 R) = 0.87 / 12 742 000 = 6.82781353e-8
    //   Y = S sin z = 50, B Y^2 = 0.000170695338
    //   dh = S cos z + B Y^2 + HI - HT = -2.195 + 0.000170695 + 1.550 - 1.700
    //      = -2.344829305 to each mark
    //   R's height from A and from B = 50 + 2.344829305 = 52.344829305,
    //   from C 52.350829305. The three have the same zenith and distance, so
    //   the same weight: the plain mean, 52.346829305; residuals
    //   (computed - observed) -2, -2 and +4 mm; the a posteriori standard
    //   error of a mean of three, sqrt(sum v^2 / (n (n - 1))) =
    //   sqrt(24e-6 / 6) = 2 mm.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true);
    observe(shots, index, kB, true);
    observe(shots, index, kC, true);
    SurveyProject project = marksAnd({kA, kB, kC}, shots);
    project.points[2].elevation = 50.006;
    const auto outcome = reduceAndAdjust(project, ReductionSettings{}, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.resections.size(), 1U);
    const ResectionReport& resection = outcome->report.resections[0];
    ASSERT_TRUE(resection.elevation.has_value());
    EXPECT_NEAR(*resection.elevation, 52.346829305, 1e-9);
    ASSERT_TRUE(resection.sigmaElevation.has_value());
    EXPECT_NEAR(*resection.sigmaElevation, 0.002, 1e-9);
    ASSERT_TRUE(resection.height.has_value());
    ASSERT_EQ(resection.height->residuals.size(), 3U);
    EXPECT_NEAR(resection.height->residuals[0].residual, -0.002, 1e-9);
    EXPECT_NEAR(resection.height->residuals[1].residual, -0.002, 1e-9);
    EXPECT_NEAR(resection.height->residuals[2].residual, 0.004, 1e-9);
    EXPECT_EQ(resection.height->residuals[2].observation, "height difference at S1: C");

    // The curvature also shortens each 50 m by A X Y = (1 - k/2) / R * 2.195 * 50
    // = 0.016 mm, alike on all three: R moves by less than that.
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_NEAR(r->northing, kStationNorthing, 5e-5);
    EXPECT_NEAR(r->easting, kStationEasting, 5e-5);
    EXPECT_NEAR(*r->elevation, 52.346829305, 1e-9);
}

TEST(ReductionResection, TooFewPlacedPointsAreRefusedSayingWhatAResectionNeeds)
{
    // Two readings fix neither the station nor its orientation.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, false);
    observe(shots, index, kB, false);
    shootQ(shots, index);
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_EQ(findPoint(*outcome, "R"), nullptr);
    EXPECT_EQ(findPoint(*outcome, "Q"), nullptr);
    EXPECT_TRUE(outcome->report.resections.empty());
    EXPECT_EQ(warningsWith(outcome->report,
                           "Setup S1 stands on R, which has no position: nothing was computed "
                           "from it. It was not resected: it observes 2 placed points with a "
                           "direction (A and B), 0 of them with a distance as well, where a "
                           "resection needs two placed points observed with a direction and a "
                           "distance, or three observed with a direction."),
              1U)
        << allWarnings(outcome->report);
}

TEST(ReductionResection, PointsOnOnePositionCountOnce)
{
    // D is entered at A's coordinates: three readings, but two places.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, false);
    observe(shots, index, kB, false);
    const Mark d{"D", kA.northing, kA.easting};
    const double readingToD = katana::math::normalizeAngle(
        azimuthFrom(kStationNorthing, kStationEasting, d) - deg(20));
    bothFaces(shots, "D", index, readingToD, std::nullopt, std::nullopt, kTargetHeight);
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB, d}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_EQ(findPoint(*outcome, "R"), nullptr);
    EXPECT_EQ(warningsWith(outcome->report, "(A and D stand on one position, so count as one)"),
              1U)
        << allWarnings(outcome->report);
}

TEST(ReductionResection, AStationOnOneLineWithItsTargetsIsRefused)
{
    // A, B and C due south, south and north of R, on its meridian: every
    // angle between them is 0 or 180 degrees.
    const Mark a{"A", 1000.0, 1030.0};
    const Mark b{"B", 1020.0, 1030.0};
    const Mark c{"C", 1090.0, 1030.0};
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, a, false);
    observe(shots, index, b, false);
    observe(shots, index, c, false);
    const auto outcome = reduceAndAdjust(marksAnd({a, b, c}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_EQ(findPoint(*outcome, "R"), nullptr);
    EXPECT_EQ(warningsWith(outcome->report,
                           "It was not resected: it stands on one line with A, B and C, where "
                           "directions cannot fix it."),
              1U)
        << allWarnings(outcome->report);
}

TEST(ReductionResection, AStationOnTheDangerCircleIsRefused)
{
    // A, B and C on the circle of radius 100 about N 1000, E 1000, and R on
    // it too, at N 1000, E 900: it sees A at 45, B at 90 and C at 135
    // degrees, and so would any point of that arc - the two circles of the
    // construction are one, centre N 1000, E 1000.
    const Mark a{"A", 1100.0, 1000.0};
    const Mark b{"B", 1000.0, 1100.0};
    const Mark c{"C", 900.0, 1000.0};
    std::vector<Shot> shots;
    std::size_t index = 1;
    for (const auto& [mark, azimuth] :
         {std::pair{a, 45.0}, std::pair{b, 90.0}, std::pair{c, 135.0}}) {
        bothFaces(shots, mark.id, index, deg(azimuth - 20.0), std::nullopt, std::nullopt,
                  kTargetHeight);
    }
    const auto outcome = reduceAndAdjust(marksAnd({a, b, c}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_EQ(findPoint(*outcome, "R"), nullptr);
    EXPECT_EQ(warningsWith(outcome->report,
                           "It was not resected: it stands on the circle through A, B and C "
                           "(the danger circle), where directions to them cannot fix it."),
              1U)
        << allWarnings(outcome->report);
}

TEST(ReductionResection, AReadingThatIsNotANumberIsRefusedNotComputed)
{
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, false);
    observe(shots, index, kB, false);
    bothFaces(shots, "C", index, std::numeric_limits<double>::quiet_NaN(), std::nullopt,
              std::nullopt, kTargetHeight);
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB, kC}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_EQ(findPoint(*outcome, "R"), nullptr);
    EXPECT_EQ(warningsWith(outcome->report,
                           "It was not resected: what it measured to C, or where that point is, "
                           "is not a finite number."),
              1U)
        << allWarnings(outcome->report);
}

TEST(ReductionResection, AResectionWaitsForTargetsALaterSetupPlaces)
{
    // S1, first in the file, stands on R and observes T1 and T2, which only
    // S2 places: S2 stands on K (N 2000, E 2000, H 100), backsights L due
    // north at reading 0 (orientation 0), and radiates T1 50 m east and T2
    // 50 m south, level (HI = HT, zenith 90): T1 N 2000 E 2050, T2 N 1950
    // E 2000, both H 100. S1 is tried after S2, when nothing else is left,
    // and resects R - really N 1960, E 2060, H 100, its circle turned so that
    // azimuth = reading + 10 degrees - from T1 (azimuth atan2(-10, 40), 41.231 m)
    // and T2 (azimuth atan2(-60, -10), 60.828 m); then radiates Q at reading
    // 80 (azimuth 90), 10 m: N 1960, E 2070.
    const Mark t1{"T1", 2000.0, 2050.0};
    const Mark t2{"T2", 1950.0, 2000.0};
    const double rN = 1960.0;
    const double rE = 2060.0;
    SurveyProject project;
    project.points.push_back(point("K", 2000.0, 2000.0, 100.0));
    project.points.push_back(point("L", 2100.0, 2000.0, 100.0));
    for (const char* id : {"R", "T1", "T2", "Q"}) {
        project.unpositionedPoints.push_back(unpositioned(id));
    }
    std::vector<Shot> free;
    std::size_t index = 1;
    for (const Mark& mark : {t1, t2}) {
        const double horizontal = std::hypot(mark.northing - rN, mark.easting - rE);
        bothFaces(free, mark.id, index,
                  katana::math::normalizeAngle(azimuthFrom(rN, rE, mark) - deg(10)), deg(90),
                  horizontal, 1.5);
    }
    bothFaces(free, "Q", index, deg(80), deg(90), 10.0, 1.5);
    project.stations.push_back(setup("S1", "R", 1.5, {}, free));
    std::vector<Shot> known;
    index = 1;
    bothFaces(known, "L", index, 0.0, deg(90), 100.0, 1.5);
    bothFaces(known, "T1", index, deg(90), deg(90), 50.0, 1.5);
    bothFaces(known, "T2", index, deg(180), deg(90), 50.0, 1.5);
    project.stations.push_back(setup("S2", "K", 1.5, "L", known));

    const auto outcome = reduceAndAdjust(project, bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr) << allWarnings(outcome->report);
    EXPECT_EQ(r->method, ComputationMethod::Resection);
    EXPECT_NEAR(r->northing, rN, 1e-8);
    EXPECT_NEAR(r->easting, rE, 1e-8);
    EXPECT_NEAR(*r->elevation, 100.0, 1e-9);
    const ComputedPoint* q = findPoint(*outcome, "Q");
    ASSERT_NE(q, nullptr);
    EXPECT_NEAR(q->northing, 1960.0, 1e-8);
    EXPECT_NEAR(q->easting, 2070.0, 1e-8);
    ASSERT_EQ(outcome->report.resections.size(), 1U);
    EXPECT_EQ(outcome->report.resections[0].targets, (std::vector<std::string>{"T1", "T2"}));
}

TEST(ReductionResection, AResectionComesBeforeTheFilesOwnCoordinatesForTheStation)
{
    // R's own coordinates in the file, as a controller computed them, 0.2 m
    // off: the resection from A and B places it, not the file.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true);
    observe(shots, index, kB, true);
    SurveyProject project = marksAnd({kA, kB}, shots);
    project.unpositionedPoints.erase(project.unpositionedPoints.begin()); // R
    project.points.push_back(point("R", kStationNorthing + 0.2, kStationEasting, kStationHeight,
                                   CoordinateSource::Calculated));
    const auto outcome = reduceAndAdjust(project, bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_EQ(r->method, ComputationMethod::Resection);
    EXPECT_NEAR(r->northing, kStationNorthing, 1e-8);
    EXPECT_EQ(warningsWith(outcome->report, "the file's own coordinates for it were used"), 0U);
}

TEST(ReductionResection, ARefusedResectionFallsBackOnTheFilesOwnCoordinatesAndSaysWhy)
{
    // The danger circle again, R's coordinates in the file: they are used,
    // and the warning says why the resection was not.
    const Mark a{"A", 1100.0, 1000.0};
    const Mark b{"B", 1000.0, 1100.0};
    const Mark c{"C", 900.0, 1000.0};
    std::vector<Shot> shots;
    std::size_t index = 1;
    for (const auto& [mark, azimuth] :
         {std::pair{a, 45.0}, std::pair{b, 90.0}, std::pair{c, 135.0}}) {
        bothFaces(shots, mark.id, index, deg(azimuth), std::nullopt, std::nullopt, kTargetHeight);
    }
    SurveyProject project = marksAnd({a, b, c}, shots);
    project.unpositionedPoints.erase(project.unpositionedPoints.begin()); // R
    project.points.push_back(point("R", 1000.0, 900.0, 50.0, CoordinateSource::Calculated));
    const auto outcome = reduceAndAdjust(project, bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_TRUE(outcome->report.resections.empty());
    EXPECT_EQ(warningsWith(outcome->report,
                           "Setup S1 stands on R, which is not control and was not computed (its "
                           "resection was refused: it stands on the circle through A, B and C "
                           "(the danger circle), where directions to them cannot fix it); the "
                           "file's own coordinates for it were used."),
              1U)
        << allWarnings(outcome->report);
}

TEST(ReductionResection, AStationAnotherSetupRadiatesIsNotResected)
{
    // S1 on K radiates R; S2 on R backsights K and also reads A and B with
    // distances - enough for a resection, but R already has a position, so
    // S2 is oriented on K as before and its readings to A and B are checks.
    // K is 50 m due south of R (N 990, E 1030); S1 backsights A with its
    // circle at north, S2 reads K with its circle turned 20 degrees as it
    // reads A and B.
    std::vector<Shot> atK;
    std::size_t index = 1;
    const Mark k{"K", 990.0, 1030.0};
    bothFaces(atK, "A", index, azimuthFrom(k.northing, k.easting, kA), deg(90),
              std::hypot(kA.northing - k.northing, kA.easting - k.easting), 1.5);
    bothFaces(atK, "R", index, 0.0, deg(90), 50.0, 1.5);
    std::vector<Shot> atR;
    index = 1;
    bothFaces(atR, "K", index,
              katana::math::normalizeAngle(azimuthFrom(kStationNorthing, kStationEasting, k) -
                                           deg(20)),
              deg(90), 50.0, 1.5);
    observe(atR, index, kA, true);
    observe(atR, index, kB, true);
    SurveyProject project;
    project.points.push_back(point("K", k.northing, k.easting, 50.0));
    project.points.push_back(point("A", kA.northing, kA.easting, 50.0));
    project.points.push_back(point("B", kB.northing, kB.easting, 50.0));
    project.unpositionedPoints.push_back(unpositioned("R"));
    project.stations.push_back(setup("S1", "K", 1.5, "A", atK));
    project.stations.push_back(setup("S2", "R", 1.5, "K", atR));
    const auto outcome = reduceAndAdjust(project, bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* radiated = findPoint(*outcome, "R");
    ASSERT_NE(radiated, nullptr);
    EXPECT_EQ(radiated->method, ComputationMethod::Radiation);
    EXPECT_NEAR(radiated->northing, kStationNorthing, 1e-8);
    EXPECT_NEAR(radiated->easting, kStationEasting, 1e-8);
    EXPECT_TRUE(outcome->report.resections.empty());
}

TEST(ReductionResection, TheReportNamesTheResectionItsTargetsAndItsResiduals)
{
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true);
    observe(shots, index, kB, true);
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const std::string text = renderText(outcome->report);
    EXPECT_NE(text.find("Resections: setups positioned from the points they observe"),
              std::string::npos);
    EXPECT_NE(text.find("A, B"), std::string::npos);
    EXPECT_NE(text.find("Residuals: resection at S1 (horizontal)"), std::string::npos);
    EXPECT_NE(text.find("direction at S1: A"), std::string::npos);
    EXPECT_NE(text.find("Residuals: resection at S1 (heights)"), std::string::npos);
    EXPECT_NE(text.find("resection"), std::string::npos);
    // Its variance factor is not a network's: the Survey Jobs dialog reads
    // "Variance factor" rows out of the text, and there is no network here.
    EXPECT_EQ(text.find("Variance factor"), std::string::npos);
    const std::string html = renderHtml(outcome->report);
    EXPECT_NE(html.find("Resections: setups positioned from the points they observe"),
              std::string::npos);
}

TEST(ReductionResection, AResectedStationIsAdjustedWithTheRestOfANetwork)
{
    // The resection gives the network its approximate station; the network
    // then adjusts it with everything else (here the same observations, so
    // the same place), and says it did.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true);
    observe(shots, index, kB, true);
    observe(shots, index, kC, true);
    SurveyProject project = marksAnd({kA, kB, kC}, shots);
    ReductionSettings settings = bareSettings();
    settings.method = AdjustmentMethod::Network;
    for (const char* id : {"A", "B", "C"}) {
        settings.control.push_back(
            ControlSelection{ControlPoint::fixed3d(id), ControlOrigin::File});
    }
    const auto outcome = reduceAndAdjust(project, settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.resections.size(), 1U);
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_EQ(r->method, ComputationMethod::NetworkLeastSquares);
    EXPECT_NEAR(r->northing, kStationNorthing, 1e-7);
    EXPECT_NEAR(r->easting, kStationEasting, 1e-7);
}

TEST(ReductionResection, ATraverseAdjustmentDoesNotReplaceAResectionWithAnotherSetupsRadiation)
{
    // A traverse job: S1 on A (N 1000, E 1000) oriented on R due north,
    // S2 to S4 round the square B, C, D back to A (Bowditch moves them). And
    // a free station: S5 on F, really at N 1050, E 950, reads A (azimuth 135)
    // and R (azimuth 45), each 50 sqrt 2 = 70.710678 m, its circle at north,
    // and radiates Q 20 m west (N 1050, E 930); S6 on Q, oriented on A,
    // measures F 20 m east as 20.005 - a check 5 mm long. After the traverse
    // the side shots are radiated again from the adjusted stations; F is not
    // one of them - its resection placed it, from A and R, which the traverse
    // did not move - so it stays at N 1050, E 950, where S6's radiation would
    // put it at E 950.005.
    SurveyProject project;
    project.points.push_back(point("A", 1000.0, 1000.0, 20.0));
    project.points.push_back(point("R", 1100.0, 1000.0, 20.0));
    for (const char* id : {"B", "C", "D", "F", "Q"}) {
        project.unpositionedPoints.push_back(unpositioned(id));
    }
    const auto horizontal = [](std::string target, double direction,
                               std::optional<double> distance = std::nullopt) {
        Shot shot{std::move(target), 0, Face::Unknown, direction, {}, distance};
        shot.kind = DistanceKind::Horizontal;
        return shot;
    };
    project.stations.push_back(setup("S1", "A", 1.5, "R",
                                     {horizontal("R", 0.0), horizontal("B", deg(90), 100.010),
                                      horizontal("D", deg(180))}));
    project.stations.push_back(
        setup("S2", "B", 1.5, "A", {horizontal("A", 0.0), horizontal("C", deg(270), 100.0)}));
    project.stations.push_back(
        setup("S3", "C", 1.5, "B", {horizontal("B", 0.0), horizontal("D", deg(270), 100.0)}));
    project.stations.push_back(
        setup("S4", "D", 1.5, "C", {horizontal("C", 0.0), horizontal("A", deg(270), 100.0)}));
    const double diagonal = 50.0 * std::sqrt(2.0);
    project.stations.push_back(setup("S5", "F", 1.5, {},
                                     {horizontal("A", deg(135), diagonal),
                                      horizontal("R", deg(45), diagonal),
                                      horizontal("Q", deg(270), 20.0)}));
    const double toA = katana::math::normalizeAngle(std::atan2(1000.0 - 930.0, 1000.0 - 1050.0));
    project.stations.push_back(
        setup("S6", "Q", 1.5, "A", {horizontal("A", toA), horizontal("F", deg(90), 20.005)}));

    ReductionSettings settings = bareSettings();
    settings.method = AdjustmentMethod::Traverse;
    const auto outcome = reduceAndAdjust(project, settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.resections.size(), 1U);
    const ComputedPoint* f = findPoint(*outcome, "F");
    ASSERT_NE(f, nullptr);
    EXPECT_EQ(f->method, ComputationMethod::Resection);
    EXPECT_NEAR(f->northing, 1050.0, 1e-8);
    EXPECT_NEAR(f->easting, 950.0, 1e-8);
    const ComputedPoint* q = findPoint(*outcome, "Q");
    ASSERT_NE(q, nullptr);
    EXPECT_NEAR(q->easting, 930.0, 1e-8);
    // And the traverse was adjusted: B moved west by its Bowditch share.
    const ComputedPoint* b = findPoint(*outcome, "B");
    ASSERT_NE(b, nullptr);
    EXPECT_NEAR(b->easting, 1100.00749981, 1e-8);
}
