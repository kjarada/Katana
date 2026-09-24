// The adjustments reduceAndAdjust drives: a traverse found in the setups, a
// network with a planted blunder, and a published level-net example.

#include <gtest/gtest.h>

#include <cmath>
#include <string>

#include "katana/survey/reduction.hpp"
#include "test_reduction_support.hpp"

using namespace katana::survey;
using namespace reduction_test;
using katana::core::ErrorCode;

namespace {

// A 100 m square loop A -> B -> C -> D -> A, oriented on R due north of A:
//   A (N 1000, E 1000) known   B (1000, 1100)   C (900, 1100)   D (900, 1000)
// Readings are the azimuths less each setup's circle orientation; the leg
// A -> B is measured 100.010 (a planted +10 mm), every other leg 100.000,
// every angle exact. S1 also reads D, which closes the loop on R.
SurveyProject squareLoop()
{
    SurveyProject project;
    project.points.push_back(point("A", 1000.0, 1000.0, 20.0));
    project.points.push_back(point("R", 1100.0, 1000.0, 20.0));
    for (const char* id : {"B", "C", "D"}) {
        project.unpositionedPoints.push_back(unpositioned(id));
    }
    const auto horizontal = [](std::string target, double direction,
                               std::optional<double> distance = std::nullopt) {
        Shot shot{std::move(target), 0, Face::Unknown, direction, {}, distance};
        shot.kind = DistanceKind::Horizontal;
        return shot;
    };
    // At A the circle reads azimuths; at B it reads azimuth - 270; at C
    // azimuth - 0; at D azimuth - 90.
    project.stations.push_back(setup("S1", "A", 1.5, "R",
                                     {horizontal("R", 0.0), horizontal("B", deg(90), 100.010),
                                      horizontal("D", deg(180))}));
    project.stations.push_back(
        setup("S2", "B", 1.5, "A", {horizontal("A", 0.0), horizontal("C", deg(270), 100.0)}));
    project.stations.push_back(
        setup("S3", "C", 1.5, "B", {horizontal("B", 0.0), horizontal("D", deg(270), 100.0)}));
    project.stations.push_back(
        setup("S4", "D", 1.5, "C", {horizontal("C", 0.0), horizontal("A", deg(270), 100.0)}));
    return project;
}

const MisclosureReport* findMisclosure(const ReductionReport& report, const std::string& prefix)
{
    for (const MisclosureReport& m : report.misclosures) {
        if (m.name.rfind(prefix, 0) == 0) {
            return &m;
        }
    }
    return nullptr;
}

} // namespace

TEST(ReductionTraverse, AClosedLoopShowsThePlantedMisclosureAndBowditchDistributesIt)
{
    // Latitudes 0, -100, 0, +100 close exactly; departures +100.010, 0, -100,
    // 0 leave +0.010 m east. Linear misclosure 10 mm over 400.010 m: 1 : 40001.
    // Angles: carried round the loop the azimuth of D -> A arrives at 0, and
    // the closing angle at A (D -> R) is 180, which gives 0 against R's
    // azimuth of 0: no angular misclosure.
    // Bowditch: each leg's departure correction is -0.010 * length / 400.010:
    //   A -> B -0.00250019, the others -0.00249994 each, so
    //   B E = 1100.010 - 0.00250019                    = 1100.00749981
    //   C E = 1100.00749981 - 0.00249994               = 1100.00499988
    //   D E = 1100.00499988 - 100 - 0.00249994         = 1000.00249994
    ReductionSettings settings = bareSettings();
    settings.method = AdjustmentMethod::Traverse;
    const auto outcome = reduceAndAdjust(squareLoop(), settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const MisclosureReport* misclosure = findMisclosure(outcome->report, "traverse from A");
    ASSERT_NE(misclosure, nullptr);
    ASSERT_TRUE(misclosure->angular.has_value());
    EXPECT_NEAR(*misclosure->angular, 0.0, 1e-12);
    EXPECT_NEAR(*misclosure->northing, 0.0, 1e-9);
    EXPECT_NEAR(*misclosure->easting, 0.010, 1e-9);
    EXPECT_NEAR(*misclosure->linear, 0.010, 1e-9);
    EXPECT_NEAR(misclosure->length, 400.010, 1e-9);
    EXPECT_NEAR(*misclosure->precisionRatio, 40001.0, 0.01);

    const ComputedPoint* b = findPoint(*outcome, "B");
    const ComputedPoint* c = findPoint(*outcome, "C");
    const ComputedPoint* d = findPoint(*outcome, "D");
    ASSERT_NE(b, nullptr);
    ASSERT_NE(c, nullptr);
    ASSERT_NE(d, nullptr);
    EXPECT_NEAR(b->easting, 1100.00749981, 1e-8);
    EXPECT_NEAR(b->northing, 1000.0, 1e-9);
    EXPECT_NEAR(c->easting, 1100.00499988, 1e-8);
    EXPECT_NEAR(c->northing, 900.0, 1e-9);
    EXPECT_NEAR(d->easting, 1000.00249994, 1e-8);
    EXPECT_EQ(b->method, ComputationMethod::TraverseBowditch);
    ASSERT_EQ(outcome->report.adjustments.size(), 1U);
    EXPECT_EQ(outcome->report.adjustments[0].redundancy, 3U);
}

TEST(ReductionTraverse, TheTransitRulePutsTheWholeDepartureMisclosureOnTheEastWestLegs)
{
    // Transit: departure corrections in proportion to |departure|: A -> B
    // takes 100.010 / 200.010 of -0.010 and C -> D 100 / 200.010; the
    // north-south legs take none. B E = 1100.010 - 0.00500025 = 1100.00499975.
    ReductionSettings settings = bareSettings();
    settings.method = AdjustmentMethod::Traverse;
    settings.traverseRule = TraverseRule::Transit;
    const auto outcome = reduceAndAdjust(squareLoop(), settings, {});
    ASSERT_TRUE(outcome.ok());
    const ComputedPoint* b = findPoint(*outcome, "B");
    ASSERT_NE(b, nullptr);
    EXPECT_NEAR(b->easting, 1100.00499975, 1e-8);
    EXPECT_EQ(b->method, ComputationMethod::TraverseTransit);
}

TEST(ReductionTraverse, TheLeastSquaresRuleReportsItsStatistics)
{
    ReductionSettings settings = bareSettings();
    settings.method = AdjustmentMethod::Traverse;
    settings.traverseRule = TraverseRule::LeastSquares;
    const auto outcome = reduceAndAdjust(squareLoop(), settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.adjustments.size(), 1U);
    const AdjustmentReport& adjustment = outcome->report.adjustments[0];
    // 3 new stations (6 unknowns); 4 distances + 5 angles = 9 observations.
    EXPECT_EQ(adjustment.unknowns, 6U);
    EXPECT_EQ(adjustment.redundancy, 3U);
    EXPECT_TRUE(adjustment.varianceFactor.has_value());
    EXPECT_FALSE(adjustment.ellipses.empty());
    EXPECT_EQ(findPoint(*outcome, "B")->method, ComputationMethod::TraverseLeastSquares);
}

namespace {

// A braced quadrilateral: A (1000, 1000) and B (1000, 1200) held; P (1150,
// 1100) and Q (850, 1100) to be found. Every setup reads every other point
// (direction and horizontal distance), except that P does not measure back
// to B, so the line B -> P is measured once. Each value carries a small
// alternating error (+-0.5 mm, +-1"), as real observations do; B -> P
// carries 30 mm more.
SurveyProject quadrilateral(double blunder)
{
    struct Mark {
        const char* id;
        double n;
        double e;
    };
    const Mark marks[] = {{"A", 1000, 1000}, {"B", 1000, 1200}, {"P", 1150, 1100}, {"Q", 850, 1100}};
    const double circle[] = {0.0, deg(30), deg(100), deg(200)};
    const char* backsight[] = {"B", "A", "A", "A"};
    SurveyProject project;
    project.points.push_back(point("A", 1000.0, 1000.0));
    project.points.push_back(point("B", 1000.0, 1200.0));
    project.unpositionedPoints.push_back(unpositioned("P"));
    project.unpositionedPoints.push_back(unpositioned("Q"));
    int sign = 1;
    for (int s = 0; s < 4; ++s) {
        std::vector<Shot> shots;
        for (int t = 0; t < 4; ++t) {
            if (t == s) {
                continue;
            }
            const double dn = marks[t].n - marks[s].n;
            const double de = marks[t].e - marks[s].e;
            const double azimuth = std::atan2(de, dn);
            double direction = katana::math::normalizeAngle(azimuth - circle[s] +
                                                            sign * arcSeconds(1.0));
            std::optional<double> distance = std::hypot(dn, de) + sign * 0.0005;
            sign = -sign;
            if (std::string(marks[s].id) == "B" && std::string(marks[t].id) == "P") {
                *distance += blunder;
            }
            if (std::string(marks[s].id) == "P" && std::string(marks[t].id) == "B") {
                distance.reset();
            }
            Shot shot{marks[t].id, 0, Face::Unknown, direction, {}, distance};
            shot.kind = DistanceKind::Horizontal;
            shots.push_back(shot);
        }
        project.stations.push_back(setup(std::string("S") + marks[s].id, marks[s].id, 1.5,
                                         backsight[s], shots));
    }
    return project;
}

ReductionSettings networkSettings(OutlierTest test, bool autoReject)
{
    ReductionSettings settings = bareSettings();
    settings.method = AdjustmentMethod::Network;
    settings.networkDimension = NetworkDimension::Horizontal;
    settings.control.push_back(ControlSelection{ControlPoint::fixedHorizontal("A")});
    settings.control.push_back(ControlSelection{ControlPoint::fixedHorizontal("B")});
    settings.outlierTest = test;
    settings.autoRejectOutliers = autoReject;
    return settings;
}

} // namespace

TEST(ReductionNetwork, ThePlantedBlunderHasTheLargestTauAndIsFlagged)
{
    // 8 angles + 11 distances = 19 observations, 4 unknowns: redundancy 15.
    // Pope's critical tau at alpha 0.001 for r = 15: t(14, 0.9995) = 4.140
    // (Student's t table), tau = 4.140 sqrt(15 / (14 + 4.140^2)) = 2.873.
    const auto outcome =
        reduceAndAdjust(quadrilateral(0.030), networkSettings(OutlierTest::Tau, false), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.adjustments.size(), 1U);
    const AdjustmentReport& adjustment = outcome->report.adjustments[0];
    EXPECT_EQ(adjustment.observations, 19U);
    EXPECT_EQ(adjustment.unknowns, 4U);
    EXPECT_EQ(adjustment.redundancy, 15U);
    ASSERT_TRUE(adjustment.globalTest.has_value());
    EXPECT_FALSE(adjustment.globalTest->passed); // 30 mm against 2.4 mm sigmas

    const ReportResidual* worst = nullptr;
    for (const ReportResidual& residual : adjustment.residuals) {
        if (residual.standardised &&
            (worst == nullptr || std::abs(*residual.standardised) > std::abs(*worst->standardised))) {
            worst = &residual;
        }
    }
    ASSERT_NE(worst, nullptr);
    EXPECT_EQ(worst->observation, "distance at SB: P");
    EXPECT_TRUE(worst->flagged);
    EXPECT_GT(std::abs(*worst->standardised), 2.873);
    bool namedCritical = false;
    for (const ReportMessage& warning : outcome->report.warnings) {
        namedCritical = namedCritical || warning.text.find("tau, critical 2.87") != std::string::npos;
    }
    EXPECT_TRUE(namedCritical);
}

TEST(ReductionNetwork, AutoRejectRemovesTheBlunderFirstAndLogsIt)
{
    // Baarda's critical w at alpha 0.001 is the normal quantile 3.2905.
    const auto outcome =
        reduceAndAdjust(quadrilateral(0.030), networkSettings(OutlierTest::Baarda, true), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const AdjustmentReport& adjustment = outcome->report.adjustments[0];
    ASSERT_FALSE(adjustment.rejectedOutliers.empty());
    EXPECT_EQ(adjustment.rejectedOutliers.front(), "distance at SB: P");
    EXPECT_EQ(adjustment.observations, 18U);
    const ReportObservation* row = findRow(outcome->report, "horizontal distance", "P", 1);
    ASSERT_NE(row, nullptr); // the second distance to P is B's
    EXPECT_EQ(row->stationId, "SB");
    EXPECT_TRUE(row->rejected);
    EXPECT_NE(row->rejectionReason.find("Baarda w, critical 3.29"), std::string::npos);
    // Without the blunder P lands within a millimetre of where it is.
    const ComputedPoint* p = findPoint(*outcome, "P");
    ASSERT_NE(p, nullptr);
    EXPECT_NEAR(p->northing, 1150.0, 0.001);
    EXPECT_NEAR(p->easting, 1100.0, 0.001);
    EXPECT_EQ(p->method, ComputationMethod::NetworkLeastSquares);
    ASSERT_TRUE(p->sigmaNorthing.has_value());
    EXPECT_GT(*p->sigmaNorthing, 0.0);
    EXPECT_FALSE(adjustment.ellipses.empty());
}

TEST(ReductionNetwork, WithoutControlTheNetworkIsRefusedWithTheDatumHint)
{
    ReductionSettings settings = networkSettings(OutlierTest::Baarda, false);
    settings.control.clear();
    const auto outcome = reduceAndAdjust(quadrilateral(0.0), settings, {});
    ASSERT_FALSE(outcome.ok());
    EXPECT_EQ(outcome.error().code, ErrorCode::AdjustmentFailure);
}

TEST(ReductionNetwork, SideShotsAreRadiatedFromTheAdjustedStationsNotAdjusted)
{
    SurveyProject project = quadrilateral(0.0);
    // A side shot from P: 10 m due north of P, reading = azimuth - 100 deg.
    project.unpositionedPoints.push_back(unpositioned("X"));
    Shot shot{"X", 0, Face::Unknown, katana::math::normalizeAngle(-deg(100)), {}, 10.0};
    shot.kind = DistanceKind::Horizontal;
    SurveyStation extra = setup("tmp", "P", 1.5, "A", {shot});
    for (Observation& observation : extra.observations) {
        project.stations[2].observations.push_back(observation);
    }
    const auto outcome =
        reduceAndAdjust(project, networkSettings(OutlierTest::Baarda, false), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    // Not in the adjustment: the observation count is unchanged.
    EXPECT_EQ(outcome->report.adjustments[0].observations, 19U);
    const ComputedPoint* p = findPoint(*outcome, "P");
    const ComputedPoint* x = findPoint(*outcome, "X");
    ASSERT_NE(x, nullptr);
    EXPECT_EQ(x->method, ComputationMethod::Radiation);
    // From the adjusted P (not the radiated one), within the noise of the
    // orientation: 10 m at a few seconds is well under a millimetre.
    EXPECT_NEAR(x->northing - p->northing, 10.0, 0.001);
    EXPECT_NEAR(x->easting - p->easting, 0.0, 0.001);
}

TEST(ReductionNetwork, TheLevelNetOfGhilaniAndWolfExample12_1AdjustsToThePublishedHeights)
{
    // Ghilani & Wolf, Adjustment Computations: Spatial Data Analysis, 4th ed.
    // (Wiley 2006), section 12.6, Example 12.1: A held at 437.596 m, six
    // levelled differences with their standard deviations. Published:
    //   B = 448.1087, C = 453.4685, D = 444.9436 (to 0.1 mm)
    //   v^T W v = 1.26976 from the residuals rounded to 0.1 mm, r = 6 - 3 = 3,
    //   so the variance factor is 1.26976 / 3 = 0.4233. (Unrounded, v^T W v
    //   is 1.27212 and the factor 0.4240; the book's printed S0 of 0.6575 is
    //   not the square root of either - sqrt(0.4233) = 0.6506 - and is taken
    //   to be a misprint.)
    SurveyProject project;
    project.points.push_back(point("A", 0.0, 0.0, 437.596));
    project.points.push_back(point("B", 10.0, 0.0, std::nullopt, CoordinateSource::Unknown));
    project.points.push_back(point("C", 10.0, 10.0, std::nullopt, CoordinateSource::Unknown));
    project.points.push_back(point("D", 0.0, 10.0, std::nullopt, CoordinateSource::Unknown));
    const auto level = [](const char* from, const char* to, double difference, double sigma) {
        return LevelDifferenceObservation{from, to, difference, sigma, 0.0, {}};
    };
    project.observations.push_back(level("A", "B", 10.509, 0.006));
    project.observations.push_back(level("B", "C", 5.360, 0.004));
    project.observations.push_back(level("C", "D", -8.523, 0.005));
    project.observations.push_back(level("D", "A", -7.348, 0.003));
    project.observations.push_back(level("B", "D", -3.167, 0.004));
    project.observations.push_back(level("A", "C", 15.881, 0.012));

    ReductionSettings settings;
    settings.method = AdjustmentMethod::Network;
    settings.networkDimension = NetworkDimension::Levels;
    settings.control.push_back(ControlSelection{ControlPoint::fixedVertical("A")});
    const auto outcome = reduceAndAdjust(project, settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.adjustments.size(), 1U);
    const AdjustmentReport& adjustment = outcome->report.adjustments[0];
    EXPECT_EQ(adjustment.redundancy, 3U);
    ASSERT_TRUE(adjustment.varianceFactor.has_value());
    EXPECT_NEAR(*adjustment.varianceFactor, 1.26976 / 3.0, 0.001);

    const ComputedPoint* b = findPoint(*outcome, "B");
    const ComputedPoint* c = findPoint(*outcome, "C");
    const ComputedPoint* d = findPoint(*outcome, "D");
    ASSERT_NE(b, nullptr);
    ASSERT_NE(c, nullptr);
    ASSERT_NE(d, nullptr);
    EXPECT_NEAR(*b->elevation, 448.1087, 0.00006);
    EXPECT_NEAR(*c->elevation, 453.4685, 0.00006);
    EXPECT_NEAR(*d->elevation, 444.9436, 0.00006);
    EXPECT_EQ(b->method, ComputationMethod::NetworkLeastSquares);
    // A levelled height on the file's own horizontal position.
    EXPECT_EQ(b->northing, 10.0);
}
