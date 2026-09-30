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

#include <array>
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
    // (a face pair) and c = 1 mm, the TARGET's centring - the instrument is
    // the unknown, so its own centring is common to every pointing - over
    // D = 50 m (50.004 to A); distances weighted 1 / (hypot(2 mm, 2 ppm S)^2
    // + c^2); its own normal equations, inverted by cofactors and iterated to
    // 1e-13 m:
    //   N 1040.001297101, E 1030.000631310, w 19.999589125 deg
    //   residuals (computed - observed): reading A +0.3521", distance A
    //   -2.5835 mm, reading B +0.7731", distance B +0.6589 mm, reading C
    //   -1.1253", distance C -1.2971 mm; v'Pv 1.847118723 on 3 degrees of
    //   freedom, variance factor 0.615706241;
    //   a posteriori sN 0.7752 mm, sE 0.5766 mm, sw 2.1546".
    // R stands over a mark (instrument height 1.55), so its precision adds
    // the instrument's 1 mm centring: sN hypot(0.7752, 1) = 1.2653 mm, sE
    // hypot(0.5766, 1) = 1.1543 mm.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true, 50.004);
    observe(shots, index, kB, true, 50.0, arcSeconds(6.0));
    observe(shots, index, kC, true);
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB, kC}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.resections.size(), 1U);
    const ResectionReport& resection = outcome->report.resections[0];

    EXPECT_NEAR(resection.northing, 1040.001297101, 1e-8);
    EXPECT_NEAR(resection.easting, 1030.000631310, 1e-8);
    EXPECT_NEAR(resection.orientation, deg(19.999589125), 1e-10);
    const AdjustmentReport& horizontal = resection.horizontal;
    EXPECT_EQ(horizontal.redundancy, 3U);
    ASSERT_TRUE(horizontal.varianceFactor.has_value());
    EXPECT_NEAR(*horizontal.varianceFactor, 0.615706241, 1e-8);
    ASSERT_EQ(horizontal.residuals.size(), 6U);
    const double expected[6] = {arcSeconds(0.3521), -0.0025835, arcSeconds(0.7731),
                                0.0006589,          arcSeconds(-1.1253), -0.0012971};
    const char* labels[6] = {"direction at S1: A", "distance at S1: A", "direction at S1: B",
                             "distance at S1: B",  "direction at S1: C", "distance at S1: C"};
    for (std::size_t i = 0; i < 6; ++i) {
        const ReportResidual& residual = horizontal.residuals[i];
        EXPECT_EQ(residual.observation, labels[i]);
        EXPECT_EQ(residual.angular, i % 2 == 0) << labels[i];
        const double tolerance = residual.angular ? arcSeconds(0.0001) : 1e-7;
        EXPECT_NEAR(residual.residual, expected[i], tolerance) << labels[i];
    }
    EXPECT_NEAR(resection.sigmaNorthing, 0.0012653, 1e-7);
    EXPECT_NEAR(resection.sigmaEasting, 0.0011543, 1e-7);
    EXPECT_NEAR(resection.sigmaOrientation, arcSeconds(2.1546), arcSeconds(0.0001));

    // The station carries that precision onto the drawing.
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_NEAR(*r->sigmaNorthing, 0.0012653, 1e-7);
    EXPECT_NEAR(*r->sigmaEasting, 0.0011543, 1e-7);
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
    //   sqrt(24e-6 / 6) = 2 mm. The instrument height, measured once to the
    //   settings' 2 mm, is in every height difference alike, so it is in none
    //   of their weights and is added to the mark's height after: sigma
    //   hypot(2, 2) = 2.828427 mm.
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
    EXPECT_NEAR(*resection.sigmaElevation, 0.002828427, 1e-9);
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

TEST(ReductionResection, MarksCloserThanTheirCentringCountAsOnePosition)
{
    // A1 1 mm east of A: less than the standard deviation of the difference
    // of two targets each centred to the settings' 1 mm (sqrt 2 x 1 mm =
    // 1.41 mm), which no pointing to them can resolve - so three readings
    // are two positions, too few, said as such rather than tried.
    const Mark a1{"A1", kA.northing, kA.easting + 0.001};
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, false);
    observe(shots, index, a1, false);
    observe(shots, index, kB, false);
    const auto outcome = reduceAndAdjust(marksAnd({kA, a1, kB}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_EQ(findPoint(*outcome, "R"), nullptr);
    EXPECT_EQ(warningsWith(outcome->report, "(A and A1 stand on one position, so count as one)"),
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
    // north: the resection from A and B places it, not the file - and the
    // file's coordinates are not dropped unseen: they are a check of the
    // resection (resection less file: -0.2 m north, 0 east, and in height
    // 52.345 - 52.345 = 0), and a warning says how far they were.
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
    const ReductionReport& report = outcome->report;
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_EQ(r->method, ComputationMethod::Resection);
    EXPECT_NEAR(r->northing, kStationNorthing, 1e-8);
    EXPECT_EQ(warningsWith(report, "the file's own coordinates for it were used"), 0U);

    EXPECT_EQ(warningsWith(report, "Setup S1 stands on R: its resection from A and B places it "
                                   "200.0 mm from the file's own coordinates for it, which were "
                                   "not used"),
              1U)
        << allWarnings(report);
    const MisclosureReport* check = nullptr;
    for (const MisclosureReport& misclosure : report.misclosures) {
        if (misclosure.name == "R by resection at setup S1, against the file's own coordinates") {
            check = &misclosure;
        }
    }
    ASSERT_NE(check, nullptr);
    EXPECT_NEAR(*check->northing, -0.2, 1e-8);
    EXPECT_NEAR(*check->easting, 0.0, 1e-8);
    EXPECT_NEAR(*check->linear, 0.2, 1e-8);
    ASSERT_TRUE(check->height.has_value());
    EXPECT_NEAR(*check->height, 0.0, 1e-9);
    ASSERT_EQ(report.resections.size(), 1U);
    ASSERT_TRUE(report.resections[0].fileNorthingDifference.has_value());
    EXPECT_NEAR(*report.resections[0].fileNorthingDifference, -0.2, 1e-8);
    EXPECT_NEAR(*report.resections[0].fileEastingDifference, 0.0, 1e-8);
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

// ---- Geometry that does not fix the station, read with real errors ------------------
//
// The refusals above are of exact readings, where the closed form finds the
// geometry exactly. Readings carry their errors: at an arc second a station
// on the danger circle, on one line with its targets, or with two targets a
// few millimetres apart, is not exactly any of them, and a least squares
// puts it wherever the errors do - tens of metres off. Each of these is
// refused with its geometry named, and leaves nothing behind: where its least
// squares solves, by the station's own a-priori precision (resectSetup: its
// ellipse at 95 % reaches past where the least squares' linear model holds);
// where the least squares fails - rank deficient at the start the closed
// form gives - by that. Each test has a case of the first kind (a
// resectSetup without the precision test places those stations).

namespace {

// Nothing of a refused resection's least squares is left: no warning of a
// flagged residual, no observation marked rejected.
void expectNothingLeftBehind(const ReductionOutcome& outcome)
{
    EXPECT_EQ(warningsWith(outcome.report, "flagged by"), 0U) << allWarnings(outcome.report);
    for (const ReportObservation& row : outcome.report.observations) {
        EXPECT_FALSE(row.rejected) << row.kind << " " << row.to << ": " << row.rejectionReason;
    }
}

} // namespace

TEST(ReductionResection, AStationOnTheDangerCircleReadWithUnequalErrorsIsRefusedNotPlaced)
{
    // The danger circle of AStationOnTheDangerCircleIsRefused - R at N 1000,
    // E 900 on the circle of radius 100 about N 1000, E 1000 through A, B and
    // C - but read with the errors field readings have: -2", +0.5", +1" (which
    // the resection placed 110 to 150 m off before it judged its precision),
    // and 0.1", 0, -0.1" (51.8 m off).
    const Mark a{"A", 1100.0, 1000.0};
    const Mark b{"B", 1000.0, 1100.0};
    const Mark c{"C", 900.0, 1000.0};
    for (const std::array<double, 3>& errors :
         {std::array<double, 3>{-2.0, 0.5, 1.0}, std::array<double, 3>{0.1, 0.0, -0.1}}) {
        std::vector<Shot> shots;
        std::size_t index = 1;
        const std::array<std::pair<Mark, double>, 3> readings{
            {{a, 45.0}, {b, 90.0}, {c, 135.0}}};
        for (std::size_t i = 0; i < 3; ++i) {
            bothFaces(shots, readings[i].first.id, index,
                      deg(readings[i].second - 20.0) + arcSeconds(errors[i]), std::nullopt,
                      std::nullopt, kTargetHeight);
        }
        shootQ(shots, index);
        const auto outcome = reduceAndAdjust(marksAnd({a, b, c}, shots), bareSettings(), {});
        ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
        EXPECT_EQ(findPoint(*outcome, "R"), nullptr) << errors[0];
        EXPECT_EQ(findPoint(*outcome, "Q"), nullptr) << errors[0];
        EXPECT_TRUE(outcome->report.resections.empty());
        EXPECT_EQ(warningsWith(outcome->report,
                               "It was not resected: it stands near the circle through A, B and "
                               "C (the danger circle)"),
                  1U)
            << allWarnings(outcome->report);
        expectNothingLeftBehind(*outcome);
    }
}

TEST(ReductionResection, AStationOnOneLineWithItsTargetsReadWithErrorsIsRefused)
{
    // A 100 m and B 200 m due north of R, C 100 m due south, read +1", -1",
    // +0.5" off (its least squares fails) and -2", +0.5", +1" off (it solves,
    // 304 m along the meridian, and the precision refuses it); before, such
    // a station was placed 25 to 88 m along the line.
    const Mark a{"A", 1140.0, 1030.0};
    const Mark b{"B", 1240.0, 1030.0};
    const Mark c{"C", 940.0, 1030.0};
    for (const std::array<double, 3>& errors :
         {std::array<double, 3>{1.0, -1.0, 0.5}, std::array<double, 3>{-2.0, 0.5, 1.0}}) {
        std::vector<Shot> shots;
        std::size_t index = 1;
        observe(shots, index, a, false, 100.0, arcSeconds(errors[0]));
        observe(shots, index, b, false, 200.0, arcSeconds(errors[1]));
        observe(shots, index, c, false, 100.0, arcSeconds(errors[2]));
        const auto outcome = reduceAndAdjust(marksAnd({a, b, c}, shots), bareSettings(), {});
        ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
        EXPECT_EQ(findPoint(*outcome, "R"), nullptr) << errors[0];
        EXPECT_EQ(warningsWith(outcome->report,
                               "It was not resected: it stands nearly on one line with A, B and C"),
                  1U)
            << allWarnings(outcome->report);
        expectNothingLeftBehind(*outcome);
    }
}

TEST(ReductionResection, TwoMarksAFewMillimetresApartDoNotFixAStationWithTheirDistances)
{
    // M and M2 5 mm apart, 158 m from R (122 m north, 100 m east), each read
    // with a direction and an exact distance - four observations of three
    // unknowns, but the station can turn about the pair almost freely: read
    // +1" and -2" off, its least squares solves somewhere on that turn and
    // the precision refuses it (before, 29.6 m off). Then M read a second
    // time, 20" off the first: the repeated reading is redundancy the turn
    // cannot absorb, so its least squares flags both readings of M before
    // the precision refuses the station - and the refusal takes the flags
    // back.
    const Mark m{"M", 1162.0, 1130.0};
    const Mark m2{"M2", 1162.0, 1130.005};
    const double toM = std::hypot(122.0, 100.0);
    const double toM2 = std::hypot(122.0, 100.005);
    for (const bool again : {false, true}) {
        std::vector<Shot> shots;
        std::size_t index = 1;
        observe(shots, index, m, true, toM, again ? 0.0 : arcSeconds(1.0));
        observe(shots, index, m2, true, toM2, again ? 0.0 : arcSeconds(-2.0));
        if (again) {
            observe(shots, index, m, true, toM, arcSeconds(20.0));
        }
        const auto outcome = reduceAndAdjust(marksAnd({m, m2}, shots), bareSettings(), {});
        ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
        EXPECT_EQ(findPoint(*outcome, "R"), nullptr) << again;
        EXPECT_EQ(warningsWith(outcome->report,
                               "It was not resected: M and M2 are nearly at one place (5.0 mm "
                               "apart, "),
                  1U)
            << allWarnings(outcome->report);
        EXPECT_EQ(warningsWith(outcome->report, "at the precision of its observations"), 1U)
            << allWarnings(outcome->report);
        expectNothingLeftBehind(*outcome);
    }
}

TEST(ReductionResection, TwoOfThreeTargetsAFewMillimetresApartDoNotFixAStationByDirections)
{
    // Three readings, but A and A2 are 5 mm apart, so two directions in all:
    // read +1", -1" and +0.5" off (placed up to 156 m off before).
    const Mark a2{"A2", 1000.0, 1000.005};
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, false, 50.0, arcSeconds(1.0));
    observe(shots, index, a2, false, 50.0, arcSeconds(-1.0));
    observe(shots, index, kC, false, 50.0, arcSeconds(0.5));
    const auto outcome = reduceAndAdjust(marksAnd({kA, a2, kC}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_EQ(findPoint(*outcome, "R"), nullptr);
    EXPECT_EQ(warningsWith(outcome->report,
                           "It was not resected: A and A2 are nearly at one place (5.0 mm apart"),
              1U)
        << allWarnings(outcome->report);
    expectNothingLeftBehind(*outcome);
}

TEST(ReductionResection, AWeakButSoundGeometryIsPlacedAndFlaggedWithItsPrecision)
{
    // R 20 m inside the danger circle of the test above - at N 1000, E 920 -
    // read exactly by direction only. Its geometry fixes it, weakly. By a
    // separate script (normal equations of the three readings, each weighted
    // 1 / ((3"/sqrt 2)^2 + (1 mm / D)^2) with D 128.062 m to A and C, 180 m
    // to B): the a-priori standard ellipse's semi-major axis 21.893 mm along
    // the meridian (azimuth 0), 53.588 mm at 95 % (x 2.4477), far inside the
    // 650.8 mm (128.062 sqrt(2 x 2.6635")) its linear model holds within, so
    // it is placed; the least precise line of position is B's, 180 m x
    // 2.4110" = 2.104 mm, so the dilution is 21.893 / 2.104 = 10.405, above
    // kWeakResectionDilution: flagged weak, and a warning says why.
    const Mark a{"A", 1100.0, 1000.0};
    const Mark b{"B", 1000.0, 1100.0};
    const Mark c{"C", 900.0, 1000.0};
    const double rN = 1000.0;
    const double rE = 920.0;
    std::vector<Shot> shots;
    std::size_t index = 1;
    for (const Mark& mark : {a, b, c}) {
        bothFaces(shots, mark.id, index,
                  katana::math::normalizeAngle(azimuthFrom(rN, rE, mark) - deg(20)), std::nullopt,
                  std::nullopt, kTargetHeight);
    }
    const auto outcome = reduceAndAdjust(marksAnd({a, b, c}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr) << allWarnings(outcome->report);
    EXPECT_NEAR(r->northing, rN, 1e-8);
    EXPECT_NEAR(r->easting, rE, 1e-8);
    ASSERT_EQ(outcome->report.resections.size(), 1U);
    const ResectionReport& resection = outcome->report.resections[0];
    EXPECT_NEAR(resection.aprioriEllipse.standard.semiMajor, 0.021893, 1e-6);
    EXPECT_NEAR(resection.aprioriEllipse.confidenceScale, 2.4477, 1e-4);
    EXPECT_NEAR(resection.dilution, 10.405, 1e-3);
    EXPECT_TRUE(resection.weakGeometry);
    EXPECT_EQ(warningsWith(outcome->report,
                           "Setup S1: its resection from A, B and C has a weak geometry - at the "
                           "precision of its observations its station is uncertain by 53.6 mm "
                           "(95 %) along azimuth 0 deg, 10.4 times the standard deviation of its "
                           "least precise observation"),
              1U)
        << allWarnings(outcome->report);
    // A sound geometry is not flagged: the shared R of the tests above, its
    // three marks all round it with distances (the same script: semi-major
    // axis 0.988 mm, least precise line a distance's hypot(2 mm, 2 ppm S,
    // 1 mm) = 2.238 mm, dilution 0.441).
    std::vector<Shot> round;
    index = 1;
    observe(round, index, kA, true);
    observe(round, index, kB, true);
    observe(round, index, kC, true);
    const auto sound = reduceAndAdjust(marksAnd({kA, kB, kC}, round), bareSettings(), {});
    ASSERT_TRUE(sound.ok()) << sound.error().describe();
    ASSERT_EQ(sound->report.resections.size(), 1U);
    EXPECT_FALSE(sound->report.resections[0].weakGeometry);
    EXPECT_NEAR(sound->report.resections[0].dilution, 0.441388, 1e-6);
    EXPECT_EQ(warningsWith(sound->report, "weak geometry"), 0U);
}

TEST(ReductionResection, ADistanceThatIsNotPositiveIsRefused)
{
    // C's horizontal distance recorded as 0: nothing it can be fitted to.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true);
    observe(shots, index, kB, true);
    Shot zero{"C", index++, Face::Left,
              katana::math::normalizeAngle(azimuthFrom(kStationNorthing, kStationEasting, kC) -
                                           deg(20)),
              std::nullopt, 0.0, kTargetHeight};
    zero.kind = DistanceKind::Horizontal;
    shots.push_back(zero);
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB, kC}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_EQ(findPoint(*outcome, "R"), nullptr);
    EXPECT_EQ(warningsWith(outcome->report,
                           "It was not resected: its distance to C is not a positive number."),
              1U)
        << allWarnings(outcome->report);
}

TEST(ReductionResection, ALeastSquaresThatFailsIsRefusedWithItsReason)
{
    // The perturbed resection above, allowed one Gauss-Newton iteration: the
    // closed-form start is millimetres off, so one correction does not reach
    // the adjustment's convergence threshold, and the adjustment refuses.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true, 50.004);
    observe(shots, index, kB, true, 50.0, arcSeconds(6.0));
    observe(shots, index, kC, true);
    ReductionSettings settings = bareSettings();
    settings.maxIterations = 1;
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB, kC}, shots), settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_EQ(findPoint(*outcome, "R"), nullptr);
    EXPECT_EQ(warningsWith(outcome->report,
                           "It was not resected: its least squares cannot fix it (the adjustment "
                           "did not converge"),
              1U)
        << allWarnings(outcome->report);
    expectNothingLeftBehind(*outcome);
}

// ---- The distances' factors ------------------------------------------------------------
//
// The resection fits horizontal distances to grid coordinates, so each takes
// phase B's factors, at the approximate station, before it is fitted. Here
// the marks are 50 m from R on the grid and the distances were measured on
// the ground, 50 m divided by the factor; with the factor applied R comes
// back where the readings were made from, and every distance fits.

namespace {

void expectResectedExactly(const ReductionOutcome& outcome)
{
    const ComputedPoint* r = findPoint(outcome, "R");
    ASSERT_NE(r, nullptr) << allWarnings(outcome.report);
    EXPECT_NEAR(r->northing, kStationNorthing, 1e-8);
    EXPECT_NEAR(r->easting, kStationEasting, 1e-8);
    ASSERT_EQ(outcome.report.resections.size(), 1U);
    for (const ReportResidual& residual : outcome.report.resections[0].horizontal.residuals) {
        EXPECT_NEAR(residual.residual, 0.0, residual.angular ? 1e-10 : 1e-8)
            << residual.observation;
    }
}

SurveyProject groundDistances(double factor)
{
    std::vector<Shot> shots;
    std::size_t index = 1;
    for (const Mark& mark : {kA, kB, kC}) {
        observe(shots, index, mark, true, 50.0 / factor);
    }
    return marksAnd({kA, kB, kC}, shots);
}

} // namespace

TEST(ReductionResection, ItsDistancesTakeTheCombinedFactor)
{
    // 50 / 0.9996 = 50.020008 m on the ground; x 0.9996 = 50 m on the grid.
    ReductionSettings settings = bareSettings();
    settings.useCombinedFactor = true;
    settings.combinedFactor = 0.9996;
    const auto outcome = reduceAndAdjust(groundDistances(0.9996), settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    expectResectedExactly(*outcome);
}

TEST(ReductionResection, ItsDistancesTakeAFixedGridScaleFactor)
{
    // 50 / 1.0004 = 49.980008 m on the ground; x 1.0004 = 50 m on the grid.
    ReductionSettings settings = bareSettings();
    settings.gridScale = GridScale::Fixed;
    settings.fixedGridScaleFactor = 1.0004;
    const auto outcome = reduceAndAdjust(groundDistances(1.0004), settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    expectResectedExactly(*outcome);
}

TEST(ReductionResection, ItsDistancesTakeTheHeightReductionAtTheStationsHeight)
{
    // To the geoid: R / (R + h), h the line's mean height - the station's
    // 52.345 (from its targets' heights less the height differences, before
    // it is placed) + the instrument's 1.550 + half the measured vertical
    // -2.195 = 52.7975 m; 6 371 000 / 6 371 052.7975 = 0.999991713, so the
    // ground distance is 50 / 0.999991713 = 50.000414358 m.
    ReductionSettings settings = bareSettings();
    settings.heightReduction = HeightReduction::Geoid;
    const double factor = 6371000.0 / (6371000.0 + 52.7975);
    const auto outcome = reduceAndAdjust(groundDistances(factor), settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    expectResectedExactly(*outcome);
}

TEST(ReductionResection, GroundDistancesFittedToAProjectedGridAreSaid)
{
    // The drawing's projection gives a point scale factor of 0.9998 (a
    // context function, constant here), and the distances were measured on
    // the ground: 50 / 0.9998 = 50.010002 m. With no grid scale set - the
    // import's default - the resection fits each 10 mm too long, more than a
    // distance's standard deviation (hypot(2 mm, 2 ppm x 50 m, 1 mm) =
    // 2.24 mm): it says so, and records the factor. With the grid scale from
    // the projection the distances are reduced to the grid, R comes back
    // exactly, and there is nothing to say.
    ReductionContext context;
    context.gridScaleFactor = [](double, double, double) -> std::optional<double> {
        return 0.9998;
    };
    const SurveyProject project = groundDistances(0.9998);
    const auto ground = reduceAndAdjust(project, bareSettings(), context);
    ASSERT_TRUE(ground.ok()) << ground.error().describe();
    EXPECT_EQ(warningsWith(ground->report,
                           "Setup S1: its resection fitted distances measured on the ground to "
                           "its targets' grid coordinates"),
              1U)
        << allWarnings(ground->report);
    ASSERT_EQ(ground->report.resections.size(), 1U);
    ASSERT_TRUE(ground->report.resections[0].unappliedScaleFactor.has_value());
    EXPECT_EQ(*ground->report.resections[0].unappliedScaleFactor, 0.9998);

    ReductionSettings settings = bareSettings();
    settings.gridScale = GridScale::FromProjection;
    const auto grid = reduceAndAdjust(project, settings, context);
    ASSERT_TRUE(grid.ok()) << grid.error().describe();
    expectResectedExactly(*grid);
    EXPECT_FALSE(grid->report.resections[0].unappliedScaleFactor.has_value());
    EXPECT_EQ(warningsWith(grid->report, "measured on the ground"), 0U);
}

// ---- The weights: orientation and height ---------------------------------------------

TEST(ReductionResection, TheOrientationIsTheWeightedMeanOfItsDirectionsAtTheStation)
{
    // A and B 50 m from R and D 200 m due north of it (N 1240, E 1030); A's
    // distance 4 mm long, B read 6" high and D 4" low; Q shot at reading 0,
    // 100 m, level. By the separate script of
    // ResidualsAndPrecisionMatchAnIndependentLeastSquares' model: R at
    // N 1040.001304459, E 1030.001806901 and the orientation 20.000671740
    // deg - which is the weighted mean of azimuth less reading at R, each
    // reading weighted 1 / ((3"/sqrt 2)^2 + (1 mm / D)^2), so D's 200 m line
    // counts more than A's and B's 50 m ones (the plain mean would be
    // 20.000746554 deg). So Q = R + 100 (cos w, sin w): N 1133.970165544,
    // E 1064.204922934 (the plain mean would put it 0.13 mm away, at
    // N 1133.970120884, E 1064.205045633).
    const Mark d{"D", 1240.0, 1030.0};
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true, 50.004);
    observe(shots, index, kB, true, 50.0, arcSeconds(6.0));
    observe(shots, index, d, true, 200.0, arcSeconds(-4.0));
    bothFaces(shots, "Q", index, 0.0, deg(90), 100.0, kInstrumentHeight);
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB, d}, shots), bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr) << allWarnings(outcome->report);
    EXPECT_NEAR(r->northing, 1040.001304459, 1e-8);
    EXPECT_NEAR(r->easting, 1030.001806901, 1e-8);
    ASSERT_TRUE(outcome->report.setups[0].orientationCorrection.has_value());
    EXPECT_NEAR(*outcome->report.setups[0].orientationCorrection, deg(20.000671740), 1e-10);
    const ComputedPoint* q = findPoint(*outcome, "Q");
    ASSERT_NE(q, nullptr);
    EXPECT_NEAR(q->northing, 1133.970165544, 1e-8);
    EXPECT_NEAR(q->easting, 1064.204922934, 1e-8);
}

TEST(ReductionResection, TheHeightIsAWeightedMeanOverSightsOfDifferentLengths)
{
    // The default settings (curvature and refraction, k = 0.13): A and B
    // 50 m from R, D 200 m due north with its mark 6 mm high (50.006). By
    // hand, as TheHeightIsTheWeightedMeanOfTrigonometricHeightsWithCurvature
    // (B = 6.82781353e-8): dh = -2.195 + B Y^2 + 1.550 - 1.700, -2.344829305
    // to A and B (Y = 50) and -2.342268875 to D (Y = 200); R's height from A
    // and B 52.344829305, from D 50.006 + 2.342268875 = 52.348268875. Each is
    // weighted 1 / ((S sin z s_z)^2 + (cos z s_S)^2 + (2 mm)^2) - s_z =
    // 3"/sqrt 2 for a face pair, s_S = hypot(2 mm, 2 ppm S), the target's
    // height the 2 mm, the instrument's being common to all three - which the
    // separate script puts at 2.066915 mm for A and B and 2.869024 mm for D.
    // The weighted mean: 52.345537986 (the plain mean would be 52.345975828);
    // residuals (computed - observed) -0.708681, -0.708681 and +2.730889 mm;
    // a posteriori sigma sqrt(v'Pv / 2 / sum p) = 0.983700 mm, and with the
    // instrument height's 2 mm, hypot = 2.228826 mm.
    const Mark d{"D", 1240.0, 1030.0};
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true);
    observe(shots, index, kB, true);
    observe(shots, index, d, true, 200.0);
    SurveyProject project = marksAnd({kA, kB, d}, shots);
    project.points[2].elevation = 50.006;
    const auto outcome = reduceAndAdjust(project, ReductionSettings{}, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.resections.size(), 1U);
    const ResectionReport& resection = outcome->report.resections[0];
    ASSERT_TRUE(resection.elevation.has_value());
    EXPECT_NEAR(*resection.elevation, 52.345537986, 1e-9);
    ASSERT_TRUE(resection.height.has_value());
    ASSERT_EQ(resection.height->residuals.size(), 3U);
    EXPECT_NEAR(resection.height->residuals[0].residual, -0.000708681, 1e-9);
    EXPECT_NEAR(resection.height->residuals[1].residual, -0.000708681, 1e-9);
    EXPECT_NEAR(resection.height->residuals[2].residual, 0.002730889, 1e-9);
    ASSERT_TRUE(resection.sigmaElevation.has_value());
    EXPECT_NEAR(*resection.sigmaElevation, 0.002228826, 1e-9);
}

TEST(ReductionResection, AResectedSetupFollowsItsStationWhenANetworkMovesIt)
{
    // R resected exactly from A, B and C, with Q shot 10 m from it. A second
    // setup, on K 50 m due south of R (N 990, E 1030), backsights A and reads
    // R by direction alone - so it places nothing, and R is resected - 20"
    // east of R. The network, A, B, C and K held, moves R toward that
    // reading; Q, a side shot of R's setup, is radiated again from where R
    // now stands, on the orientation R's resection gives there: the weighted
    // mean of azimuth less reading over its directions, here all to 50 m
    // lines and so equally weighted - worked below from the adjusted R, a
    // property of the result.
    const Mark k{"K", 990.0, 1030.0};
    std::vector<Shot> atR;
    std::size_t index = 1;
    observe(atR, index, kA, true);
    observe(atR, index, kB, true);
    observe(atR, index, kC, true);
    shootQ(atR, index);
    std::vector<Shot> atK;
    index = 1;
    bothFaces(atK, "A", index, azimuthFrom(k.northing, k.easting, kA), std::nullopt, std::nullopt,
              kTargetHeight);
    bothFaces(atK, "R", index, arcSeconds(20.0), std::nullopt, std::nullopt, kTargetHeight);
    SurveyProject project;
    for (const Mark& mark : {kA, kB, kC, k}) {
        project.points.push_back(point(mark.id, mark.northing, mark.easting, 50.0));
    }
    project.unpositionedPoints.push_back(unpositioned("R"));
    project.unpositionedPoints.push_back(unpositioned("Q"));
    project.stations.push_back(setup("S1", "R", kInstrumentHeight, {}, atR));
    project.stations.push_back(setup("S2", "K", 1.5, "A", atK));
    ReductionSettings settings = bareSettings();
    settings.method = AdjustmentMethod::Network;
    for (const char* id : {"A", "B", "C", "K"}) {
        settings.control.push_back(
            ControlSelection{ControlPoint::fixed3d(id), ControlOrigin::File});
    }
    const auto outcome = reduceAndAdjust(project, settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.resections.size(), 1U) << allWarnings(outcome->report);
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_EQ(r->method, ComputationMethod::NetworkLeastSquares);
    // It reaches the case: the network did move R.
    EXPECT_GT(std::hypot(r->northing - kStationNorthing, r->easting - kStationEasting), 1e-4);

    std::optional<double> first;
    double offsets = 0.0;
    for (const Mark& mark : {kA, kB, kC}) {
        const double reading = katana::math::normalizeAngle(
            azimuthFrom(kStationNorthing, kStationEasting, mark) - deg(20));
        const double value = katana::math::normalizeAngle(
            azimuthFrom(r->northing, r->easting, mark) - reading);
        if (!first) {
            first = value;
        }
        offsets += katana::math::normalizeAngleSigned(value - *first);
    }
    const double orientation = katana::math::normalizeAngleSigned(*first + offsets / 3.0);
    ASSERT_TRUE(outcome->report.setups[0].orientationCorrection.has_value());
    EXPECT_NEAR(*outcome->report.setups[0].orientationCorrection, orientation, 1e-12);
    const ComputedPoint* q = findPoint(*outcome, "Q");
    ASSERT_NE(q, nullptr);
    EXPECT_NEAR(q->northing, r->northing + 10.0 * std::cos(orientation), 1e-9);
    EXPECT_NEAR(q->easting, r->easting + 10.0 * std::sin(orientation), 1e-9);
}

// ---- Automatic rejection -------------------------------------------------------------

TEST(ReductionResection, AGrossReadingIsRejectedAndTheStationOrientationAndShotsComeFromTheRest)
{
    // A, B and C 50 m from R and D 200 m due north, all with distances; B
    // read 60" high; automatic rejection on. B's direction is the worst
    // flagged observation and is rejected; everything else is exact, so R is
    // where it was made from (N 1040, E 1030), the orientation 20 deg - the
    // rejected reading takes no part in it - and Q, read at 0, 100 m level:
    // N 1040 + 100 cos 20 = 1133.969262079, E 1030 + 100 sin 20 =
    // 1064.202014333.
    const Mark d{"D", 1240.0, 1030.0};
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true);
    observe(shots, index, kB, true, 50.0, arcSeconds(60.0));
    observe(shots, index, kC, true);
    observe(shots, index, d, true, 200.0);
    bothFaces(shots, "Q", index, 0.0, deg(90), 100.0, kInstrumentHeight);
    ReductionSettings settings = bareSettings();
    settings.autoRejectOutliers = true;
    const auto outcome = reduceAndAdjust(marksAnd({kA, kB, kC, d}, shots), settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.resections.size(), 1U);
    const ResectionReport& resection = outcome->report.resections[0];
    EXPECT_EQ(resection.horizontal.rejectedOutliers,
              (std::vector<std::string>{"direction at S1: B"}));
    std::size_t rejected = 0;
    for (const ReportResidual& residual : resection.horizontal.residuals) {
        if (residual.rejected) {
            ++rejected;
            EXPECT_EQ(residual.observation, "direction at S1: B");
        }
    }
    EXPECT_EQ(rejected, 1U);
    const ReportObservation* row = findRow(outcome->report, "horizontal direction", "B");
    ASSERT_NE(row, nullptr);
    EXPECT_TRUE(row->rejected);

    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_NEAR(r->northing, kStationNorthing, 1e-8);
    EXPECT_NEAR(r->easting, kStationEasting, 1e-8);
    EXPECT_NEAR(resection.orientation, deg(20), 1e-10);
    EXPECT_NEAR(*outcome->report.setups[0].orientationCorrection, deg(20), 1e-10);
    const ComputedPoint* q = findPoint(*outcome, "Q");
    ASSERT_NE(q, nullptr);
    EXPECT_NEAR(q->northing, 1133.969262079, 1e-8);
    EXPECT_NEAR(q->easting, 1064.202014333, 1e-8);
}

// ---- The file's resection block ------------------------------------------------------

TEST(ReductionResection, ACheckAfterTheFilesResectionBlockChecksTheStationAndDoesNotMoveIt)
{
    // The first test's A, B and C, exact, both faces - records 1 to 6 - in
    // the block the file marks (kResectionEndMetadata "record 7", its 129),
    // then a check to A (record 7) read 20" high and 10 mm long, as a
    // controller takes one after the block. The resection is of the block:
    // R exactly where the readings were made from, redundancy 3 (six
    // observations, three unknowns), one check. The check is a misclosure of
    // the station, radiated on the resection's orientation (20 deg): A' = R +
    // 50.010 (cos, sin)(216.869897646 deg + 20") = N 999.994909652,
    // E 999.990120856, so -5.090348 mm N, -9.879144 mm E, 11.113466 mm; in
    // height 0 (its dh -2.345 as the block's). Without the key the check is
    // the resection's fourth pointing, and R moves.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true);
    observe(shots, index, kB, true);
    observe(shots, index, kC, true);
    shots.push_back(Shot{"A", index++, Face::Left,
                         katana::math::normalizeAngle(
                             azimuthFrom(kStationNorthing, kStationEasting, kA) - deg(20) +
                             arcSeconds(20.0)),
                         std::atan2(50.010, -2.195), std::hypot(50.010, 2.195), kTargetHeight});
    SurveyProject project = marksAnd({kA, kB, kC}, shots);
    project.stations[0].metadata[kResectionEndMetadata] = "record 7";
    const auto outcome = reduceAndAdjust(project, bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_NEAR(r->northing, kStationNorthing, 1e-8);
    EXPECT_NEAR(r->easting, kStationEasting, 1e-8);
    ASSERT_EQ(outcome->report.resections.size(), 1U);
    EXPECT_EQ(outcome->report.resections[0].horizontal.redundancy, 3U);
    EXPECT_EQ(outcome->report.resections[0].checks, 1U);
    const MisclosureReport* check = nullptr;
    for (const MisclosureReport& misclosure : outcome->report.misclosures) {
        if (misclosure.name == "A from setup S1") {
            check = &misclosure;
        }
    }
    ASSERT_NE(check, nullptr);
    EXPECT_NEAR(*check->northing, -0.005090348, 1e-9);
    EXPECT_NEAR(*check->easting, -0.009879144, 1e-9);
    EXPECT_NEAR(*check->linear, 0.011113466, 1e-9);
    ASSERT_TRUE(check->height.has_value());
    EXPECT_NEAR(*check->height, 0.0, 1e-9);

    project.stations[0].metadata.erase(kResectionEndMetadata);
    const auto unmarked = reduceAndAdjust(project, bareSettings(), {});
    ASSERT_TRUE(unmarked.ok()) << unmarked.error().describe();
    const ComputedPoint* moved = findPoint(*unmarked, "R");
    ASSERT_NE(moved, nullptr);
    EXPECT_GT(std::hypot(moved->northing - kStationNorthing, moved->easting - kStationEasting),
              1e-4);
    ASSERT_EQ(unmarked->report.resections.size(), 1U);
    EXPECT_EQ(unmarked->report.resections[0].horizontal.redundancy, 5U);
    EXPECT_EQ(unmarked->report.resections[0].checks, 0U);
}

TEST(ReductionResection, ABlockEndThatNamesNoRecordLeavesEveryPointingToTheResection)
{
    // The block and check of the test above, the key's value "record" with
    // no number, empty, or not a record: the reduction cannot tell where the
    // block ends, so every pointing is the resection's, as for a file that
    // marks none - redundancy 5 (eight observations, three unknowns), where
    // "record 7" gives 3.
    std::vector<Shot> shots;
    std::size_t index = 1;
    observe(shots, index, kA, true);
    observe(shots, index, kB, true);
    observe(shots, index, kC, true);
    shots.push_back(Shot{"A", index++, Face::Left,
                         katana::math::normalizeAngle(
                             azimuthFrom(kStationNorthing, kStationEasting, kA) - deg(20)),
                         std::atan2(50.0, -2.195), std::hypot(50.0, 2.195), kTargetHeight});
    for (const char* value : {"record", "", "record -3", "record 7"}) {
        SurveyProject project = marksAnd({kA, kB, kC}, shots);
        project.stations[0].metadata[kResectionEndMetadata] = value;
        const auto outcome = reduceAndAdjust(project, bareSettings(), {});
        ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
        ASSERT_EQ(outcome->report.resections.size(), 1U) << value;
        EXPECT_EQ(outcome->report.resections[0].horizontal.redundancy,
                  std::string(value) == "record 7" ? 3U : 5U)
            << value;
    }
}

TEST(ReductionResection, ASecondBlockOnAStationAlreadyResectedSaysWhatItCouldBeOrientedOn)
{
    // S1 resects R from A and B; S2, a second block on R with no backsight,
    // reads A and B again and shoots Q2. R has a position when S2 is tried,
    // so S2 is not resected, and a setup on a placed station is not oriented
    // on the points it observes (docs/survey.md, Not done): Q2 is not
    // placed, and the warning names the placed points S2 observes, so it
    // does not read as a setup that observed nothing known.
    std::vector<Shot> first;
    std::size_t index = 1;
    observe(first, index, kA, true);
    observe(first, index, kB, true);
    std::vector<Shot> second;
    index = 1;
    observe(second, index, kA, true);
    observe(second, index, kB, true);
    bothFaces(second, "Q2", index, 0.0, deg(90), 10.0, kInstrumentHeight);
    SurveyProject project = marksAnd({kA, kB}, first);
    project.unpositionedPoints.push_back(unpositioned("Q2"));
    project.stations.push_back(setup("S2", "R", kInstrumentHeight, {}, second));
    const auto outcome = reduceAndAdjust(project, bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_NE(findPoint(*outcome, "R"), nullptr);
    EXPECT_EQ(findPoint(*outcome, "Q2"), nullptr);
    EXPECT_EQ(warningsWith(outcome->report,
                           "Setup S2 has no backsight; it observes 2 placed points (A, B), but a "
                           "setup on a placed station is not oriented on the points it observes, "
                           "so its directions are not oriented and its targets are not "
                           "radiated."),
              1U)
        << allWarnings(outcome->report);
}
