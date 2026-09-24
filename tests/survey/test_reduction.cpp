// reduceAndAdjust end to end: face pairs, orientation and radiation over two
// setups, the reduced project, re-running with changed settings, and errors.

#include <gtest/gtest.h>

#include <cmath>
#include <string>

#include "katana/survey/reduction.hpp"
#include "test_reduction_support.hpp"

using namespace katana::survey;
using namespace reduction_test;
using katana::core::ErrorCode;

namespace {

// Two setups, both faces, hand-worked (k = 0.13, R = 6 371 000 m):
//
// S1 on A (N 1000, E 1000, H 50), HI 1.5, backsight B (N 1100, E 1000): grid
// azimuth A -> B = 0.
//   B   FL 10 00 00, FR 190 00 02  -> mean 10 00 01
//   orientation = 0 - 10 00 01 = -10 00 01
//   P1  FL 100 00 01, FR 280 00 01 -> mean 100 00 01 -> azimuth 90 00 00
//       zenith 90 both faces, slope 50.000 both, HT 1.5
//       X = 0, Y = 50: HD = 50 exactly; dH = B Y^2 = 6.8278135e-8 * 2500
//       = 0.000170695; P1 = (N 1000, E 1050, H 50.000170695)
// S2 on P1, HI 1.6, backsight A: azimuth P1 -> A = 270.
//   A   FL 0 00 00, FR 180 00 00 -> mean 0; slope 50.000 at zenith 90
//   orientation = 270 00 00
//   P2  FL 90 00 00, FR 270 00 00 -> mean 90 -> azimuth 0 (north)
//       zenith 89 00 00 both faces, slope 100.000 both, HT 1.6
//       X = 100 cos 89 = 1.745240644, Y = 99.984769516
//       HD = Y - A X Y = 99.984769516 - 0.000025609 = 99.984743907
//       dH = X + B Y^2 = 1.745240644 + 0.000682573 = 1.745923217
//   P2 = (N 1099.984743907, E 1050, H 51.746093912)
// The backsight check at S2: 50.000 measured against 50.000 computed = 0.
SurveyProject twoSetups()
{
    SurveyProject project;
    project.points.push_back(point("A", 1000.0, 1000.0, 50.0));
    project.points.push_back(point("B", 1100.0, 1000.0, 50.0));
    project.unpositionedPoints.push_back(unpositioned("P1"));
    project.unpositionedPoints.push_back(unpositioned("P2"));
    project.stations.push_back(
        setup("S1", "A", 1.5, "B",
              {Shot{"B", 1, Face::Left, deg(10), {}, {}},
               Shot{"P1", 2, Face::Left, deg(100, 0, 1), deg(90), 50.0, 1.5},
               Shot{"P1", 3, Face::Right, deg(280, 0, 1), deg(90), 50.0, 1.5},
               Shot{"B", 4, Face::Right, deg(190, 0, 2), {}, {}}}));
    project.stations.push_back(
        setup("S2", "P1", 1.6, "A",
              {Shot{"A", 1, Face::Left, deg(0), deg(90), 50.0, 1.6},
               Shot{"P2", 2, Face::Left, deg(90), deg(89), 100.0, 1.6},
               Shot{"P2", 3, Face::Right, deg(270), deg(89), 100.0, 1.6},
               Shot{"A", 4, Face::Right, deg(180), deg(90), 50.0, 1.6}}));
    return project;
}

} // namespace

TEST(Reduction, TwoSetupsOnBothFacesRadiateToTheHandWorkedCoordinates)
{
    const auto outcome = reduceAndAdjust(twoSetups(), ReductionSettings{}, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ReductionReport& report = outcome->report;

    ASSERT_EQ(report.setups.size(), 2U);
    EXPECT_NEAR(*report.setups[0].backsightReading, deg(10, 0, 1), 1e-12);
    EXPECT_NEAR(*report.setups[0].orientationCorrection, -deg(10, 0, 1), 1e-12);
    EXPECT_NEAR(*report.setups[1].orientationCorrection, -deg(90), 1e-12); // 270 as signed
    EXPECT_NEAR(*report.setups[1].backsightDistanceDifference, 0.0, 1e-9);

    const ComputedPoint* p1 = findPoint(*outcome, "P1");
    const ComputedPoint* p2 = findPoint(*outcome, "P2");
    ASSERT_NE(p1, nullptr);
    ASSERT_NE(p2, nullptr);
    EXPECT_NEAR(p1->northing, 1000.0, 1e-9);
    EXPECT_NEAR(p1->easting, 1050.0, 1e-9);
    EXPECT_NEAR(*p1->elevation, 50.000170695, 1e-9);
    EXPECT_EQ(p1->method, ComputationMethod::Radiation);
    EXPECT_NEAR(p2->northing, 1099.984743907, 1e-8);
    EXPECT_NEAR(p2->easting, 1050.0, 1e-8);
    EXPECT_NEAR(*p2->elevation, 51.746093912, 1e-8);

    // Every raw observation has its row, in order: 4 directions + 2 zeniths +
    // 2 distances at S1 = 8, then its height difference; 4 + 4 + 4 at S2 and
    // two height differences (A and P2).
    std::size_t raw = 0;
    for (const ReportObservation& row : report.observations) {
        raw += row.kind != "height difference" ? 1 : 0;
    }
    EXPECT_EQ(raw, 8U + 12U);

    // Face pairs: B, P1 at S1; A, P2 at S2. The B pair's spread is 2".
    ASSERT_EQ(report.facePairs.size(), 4U);
    EXPECT_NEAR(*report.facePairs[0].horizontalSpread, arcSeconds(2.0), 1e-12);
    EXPECT_TRUE(report.facePairs[0].withinTolerance);
}

TEST(Reduction, AFacePairMeansTheReadingsAndReportsTheIndexErrorAndSpreads)
{
    // FL 45 00 10 / FR 225 00 04: FR - 180 = 45 00 04, mean 45 00 07, spread 6".
    // Zenith FL 88 00 20, FR raw 271 59 50 stored folded as 88 00 10:
    //   mean 88 00 15, index error (FL - FR') / 2 = 5", spread 10".
    // Slope 100.003 / 100.001: mean 100.002, spread 2 mm.
    SurveyProject project;
    project.points.push_back(point("A", 0.0, 0.0, 10.0));
    project.points.push_back(point("B", 100.0, 0.0, 10.0));
    project.unpositionedPoints.push_back(unpositioned("T"));
    project.stations.push_back(
        setup("S", "A", 1.5, "B",
              {Shot{"B", 1, Face::Left, 0.0, {}, {}},
               Shot{"T", 2, Face::Left, deg(45, 0, 10), deg(88, 0, 20), 100.003, 1.5},
               Shot{"T", 3, Face::Right, deg(225, 0, 4), deg(88, 0, 10), 100.001, 1.5}}));
    const auto outcome = reduceAndAdjust(project, bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ReductionReport& report = outcome->report;
    ASSERT_EQ(report.facePairs.size(), 1U);
    const FacePairCheck& pair = report.facePairs[0];
    EXPECT_NEAR(*pair.horizontalSpread, arcSeconds(6.0), 1e-11);
    EXPECT_NEAR(*pair.zenithSpread, arcSeconds(10.0), 1e-11);
    EXPECT_NEAR(*pair.distanceSpread, 0.002, 1e-9);
    EXPECT_TRUE(pair.withinTolerance);

    const ReportObservation* left = findRow(report, "horizontal direction", "T", 0);
    const ReportObservation* right = findRow(report, "horizontal direction", "T", 1);
    ASSERT_NE(left, nullptr);
    ASSERT_NE(right, nullptr);
    EXPECT_NEAR(findCorrection(*left, CorrectionKind::FaceMean)->amount, -arcSeconds(3.0), 1e-11);
    // FR: 225 00 04 -> 45 00 07 is -180 + 3".
    EXPECT_NEAR(findCorrection(*right, CorrectionKind::FaceMean)->amount,
                -deg(180) + arcSeconds(3.0), 1e-11);
    // Oriented on B (reading 0, azimuth 0): the azimuth is the mean reading.
    EXPECT_NEAR(*left->reduced, deg(45, 0, 7), 1e-11);

    const ReportObservation* zenith = findRow(report, "zenith angle", "T", 0);
    ASSERT_NE(zenith, nullptr);
    EXPECT_NEAR(*zenith->reduced, deg(88, 0, 15), 1e-11);
    EXPECT_NEAR(findCorrection(*zenith, CorrectionKind::FaceMean)->amount, -arcSeconds(5.0),
                1e-11);
}

TEST(Reduction, APairOutsideTheToleranceIsFlaggedAndExcludedOnlyWhenTheSettingsSaySo)
{
    // 15" between the faces against a 10" tolerance.
    SurveyProject project;
    project.points.push_back(point("A", 0.0, 0.0));
    project.points.push_back(point("B", 100.0, 0.0));
    project.unpositionedPoints.push_back(unpositioned("T"));
    project.stations.push_back(
        setup("S", "A", 1.5, "B",
              {Shot{"B", 1, Face::Left, 0.0, {}, {}},
               Shot{"T", 2, Face::Left, deg(45), deg(90), 100.0},
               Shot{"T", 3, Face::Right, deg(225, 0, 15), deg(90), 100.0}}));
    ReductionSettings settings = bareSettings();
    const auto used = reduceAndAdjust(project, settings, {});
    ASSERT_TRUE(used.ok());
    ASSERT_EQ(used->report.facePairs.size(), 1U);
    EXPECT_FALSE(used->report.facePairs[0].withinTolerance);
    EXPECT_NE(findPoint(*used, "T"), nullptr);

    settings.faceTolerances.excludeOutside = true;
    const auto excluded = reduceAndAdjust(project, settings, {});
    ASSERT_TRUE(excluded.ok());
    EXPECT_EQ(findPoint(*excluded, "T"), nullptr);
    const ReportObservation* row = findRow(excluded->report, "horizontal direction", "T");
    ASSERT_NE(row, nullptr);
    EXPECT_TRUE(row->rejected);
    EXPECT_NE(row->rejectionReason.find("tolerance"), std::string::npos);
}

TEST(Reduction, FaceLeftOnlyReportsTheFaceRightPointingsAsUnused)
{
    ReductionSettings settings;
    settings.faces = FaceHandling::FaceLeftOnly;
    const auto outcome = reduceAndAdjust(twoSetups(), settings, {});
    ASSERT_TRUE(outcome.ok());
    std::size_t unused = 0;
    for (const ReportObservation& row : outcome->report.observations) {
        if (row.rejected) {
            EXPECT_EQ(row.pointing.face, Face::Right);
            ++unused;
        }
    }
    EXPECT_EQ(unused, 1U + 3U + 3U + 3U); // S1: B (1) + P1 (3); S2: A (3) + P2 (3)
    // P1 from face left only: 100 00 01 - 10 00 00 = 90 00 01, not 90 00 00.
    const ComputedPoint* p1 = findPoint(*outcome, "P1");
    ASSERT_NE(p1, nullptr);
    EXPECT_NEAR(p1->northing, 1000.0 + 50.0 * std::cos(deg(90, 0, 1)), 1e-9);
}

TEST(Reduction, TheReducedProjectCarriesTheComputedPointsAsCalculatedAndIsValid)
{
    const auto outcome = reduceAndAdjust(twoSetups(), ReductionSettings{}, {});
    ASSERT_TRUE(outcome.ok());
    const SurveyProject& reduced = outcome->reduced;
    EXPECT_TRUE(reduced.unpositionedPoints.empty());
    bool found = false;
    for (const SurveyPoint& p : reduced.points) {
        if (p.id == "P2") {
            found = true;
            EXPECT_EQ(p.coordinateSource, CoordinateSource::Calculated);
        }
        if (p.id == "A") {
            EXPECT_EQ(p.coordinateSource, CoordinateSource::Entered); // held, not computed
        }
    }
    EXPECT_TRUE(found);
    // Oriented directions became azimuths, slope distances horizontal ones.
    std::size_t azimuths = 0;
    for (const Observation& observation : reduced.stations[0].observations) {
        azimuths += std::holds_alternative<AzimuthObservation>(observation) ? 1 : 0;
        if (const auto* distance = std::get_if<DistanceObservation>(&observation)) {
            EXPECT_EQ(distance->kind, DistanceKind::Horizontal);
        }
    }
    EXPECT_EQ(azimuths, 2U); // B and P1, one per face pair
    EXPECT_TRUE(validateProject(reduced).ok()) << validateProject(reduced).error().describe();
}

TEST(Reduction, ChangingTheRefractionCoefficientChangesOnlyTheHeightsAndTheCurvatureTerms)
{
    const auto first = reduceAndAdjust(twoSetups(), ReductionSettings{}, {});
    ReductionSettings changed;
    changed.refractionCoefficient = 0.2;
    ReductionContext context;
    context.previous = first->points;
    const auto second = reduceAndAdjust(twoSetups(), changed, context);
    ASSERT_TRUE(first.ok());
    ASSERT_TRUE(second.ok());

    // Directions and zeniths: identical. Distances and heights: only where a
    // curvature term is.
    ASSERT_EQ(first->report.observations.size(), second->report.observations.size());
    for (std::size_t i = 0; i < first->report.observations.size(); ++i) {
        const ReportObservation& a = first->report.observations[i];
        const ReportObservation& b = second->report.observations[i];
        if (a.angular) {
            EXPECT_EQ(a.reduced, b.reduced) << a.kind << " " << a.to;
        }
    }
    // P1's height: B Y^2 with k 0.2 is 0.8 / 12742000 * 2500 = 0.000156961,
    // against 0.000170695 with k 0.13: the shift is -0.000013734 m.
    const ComputedPoint* p1 = findPoint(*second, "P1");
    ASSERT_NE(p1, nullptr);
    EXPECT_NEAR(*p1->elevation, 50.000156961, 1e-9);
    for (const CoordinateReport& coordinate : second->report.coordinates) {
        if (coordinate.pointId == "P1") {
            ASSERT_TRUE(coordinate.shiftElevation.has_value());
            EXPECT_NEAR(*coordinate.shiftElevation, -0.000013734, 1e-9);
            EXPECT_NEAR(*coordinate.shiftNorthing, 0.0, 1e-12);
            EXPECT_NEAR(*coordinate.shiftEasting, 0.0, 1e-12);
        }
    }
    EXPECT_EQ(second->report.settings.refractionCoefficient, 0.2);
}

TEST(Reduction, AFixedPpmMovesEveryTargetRadiallyAndLeavesTheAnglesAlone)
{
    // +100 ppm on P1's 50 m: 5 mm further east. P2's 100 m line grows by
    // 10 mm along its azimuth (north) and starts from the moved P1.
    ReductionSettings settings;
    settings.atmospheric = AtmosphericCorrection::Fixed;
    settings.fixedPpm = 100.0;
    const auto base = reduceAndAdjust(twoSetups(), ReductionSettings{}, {});
    const auto scaled = reduceAndAdjust(twoSetups(), settings, {});
    ASSERT_TRUE(base.ok());
    ASSERT_TRUE(scaled.ok());
    const ComputedPoint* p1 = findPoint(*scaled, "P1");
    const ComputedPoint* p1Base = findPoint(*base, "P1");
    EXPECT_NEAR(p1->easting - p1Base->easting, 0.005, 1e-9);
    const ComputedPoint* p2 = findPoint(*scaled, "P2");
    const ComputedPoint* p2Base = findPoint(*base, "P2");
    EXPECT_NEAR(p2->northing - p2Base->northing, 100.0 * std::sin(deg(89)) * 1e-4, 1e-6);
}

TEST(Reduction, ControlMissingFromTheFileAndTheDrawingIsNotFound)
{
    ReductionSettings settings;
    settings.control.push_back(ControlSelection{ControlPoint::fixed3d("NOPE"), ControlOrigin::File});
    const auto outcome = reduceAndAdjust(twoSetups(), settings, {});
    ASSERT_FALSE(outcome.ok());
    EXPECT_EQ(outcome.error().code, ErrorCode::NotFound);
}

TEST(Reduction, ControlFromTheDrawingHoldsThePointButIsNotReturnedToBeDrawnAgain)
{
    SurveyProject project = twoSetups();
    // B only on the drawing: the file names it without coordinates.
    project.points.erase(project.points.begin() + 1);
    project.unpositionedPoints.push_back(unpositioned("B"));
    ReductionContext context;
    context.drawingPoints.push_back(point("B", 1100.0, 1000.0, 50.0));
    ReductionSettings settings;
    settings.control.push_back(ControlSelection{ControlPoint::fixed3d("A"), ControlOrigin::File});
    settings.control.push_back(
        ControlSelection{ControlPoint::fixed3d("B"), ControlOrigin::Drawing});
    const auto outcome = reduceAndAdjust(project, settings, context);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_EQ(findPoint(*outcome, "B"), nullptr);
    const ComputedPoint* p1 = findPoint(*outcome, "P1");
    ASSERT_NE(p1, nullptr);
    EXPECT_NEAR(p1->easting, 1050.0, 1e-9);
    const ComputedPoint* a = findPoint(*outcome, "A");
    ASSERT_NE(a, nullptr);
    EXPECT_EQ(a->method, ComputationMethod::Control);
    EXPECT_EQ(a->sigmaNorthing, 0.0);
}

TEST(Reduction, ASetupWithNoOrientationRadiatesNothingAndSaysWhy)
{
    SurveyProject project = twoSetups();
    project.stations[0].backsightPointId.clear();
    const auto outcome = reduceAndAdjust(project, ReductionSettings{}, {});
    ASSERT_TRUE(outcome.ok());
    EXPECT_EQ(findPoint(*outcome, "P1"), nullptr);
    bool said = false;
    for (const ReportMessage& warning : outcome->report.warnings) {
        said = said || warning.text.find("S1 has no backsight") != std::string::npos;
    }
    EXPECT_TRUE(said);
}

TEST(Reduction, AGnssPositionIsConvertedThroughTheContextAndServesAsAStation)
{
    SurveyProject project = twoSetups();
    // A is now known only from GNSS.
    project.points.erase(project.points.begin());
    project.unpositionedPoints.push_back(unpositioned("A"));
    GnssGlobalPositionObservation gnss;
    gnss.point = "A";
    gnss.geodetic = GeodeticCoordinate{0.5, 2.0, 80.0};
    project.observations.push_back(gnss);
    ReductionContext context;
    context.geodeticToGrid = [](const GeodeticCoordinate& g) -> std::optional<GridPosition> {
        (void)g;
        return GridPosition{1000.0, 1000.0, 80.0};
    };
    context.geoidSeparation = [](double, double) -> std::optional<double> { return 30.0; };
    const auto outcome = reduceAndAdjust(project, ReductionSettings{}, context);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* a = findPoint(*outcome, "A");
    ASSERT_NE(a, nullptr);
    EXPECT_EQ(a->method, ComputationMethod::Gnss);
    EXPECT_NEAR(*a->elevation, 50.0, 1e-12); // 80 ellipsoidal - 30 geoid
    const ComputedPoint* p1 = findPoint(*outcome, "P1");
    ASSERT_NE(p1, nullptr);
    EXPECT_NEAR(p1->easting, 1050.0, 1e-9);
}

TEST(Reduction, IdenticalInputsGiveIdenticalOutcomes)
{
    const auto a = reduceAndAdjust(twoSetups(), ReductionSettings{}, {});
    const auto b = reduceAndAdjust(twoSetups(), ReductionSettings{}, {});
    ASSERT_TRUE(a.ok());
    ASSERT_TRUE(b.ok());
    EXPECT_EQ(a->points, b->points);
    EXPECT_EQ(a->report, b->report);
    EXPECT_EQ(a->reduced, b->reduced);
}
