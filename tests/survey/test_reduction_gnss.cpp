// GNSS vectors in the reduction: an RTK controller's earth-centred vectors
// (what a Trimble job's ECEF deltas and a Topcon RW5 vector are) converted to
// grid at their base, radiated, checked, and adjusted as baselines.
//
// The projection in these tests is one a hand can follow: grid north is +Y,
// grid east is +X and the height is +Z, about a base at X0, Y0, Z0 that maps
// to N 5000, E 3000, h 100. A real projection bends and scales; the
// reduction only ever takes the difference of two converted points, so a
// linear one tests exactly what the reduction does with it.

#include <gtest/gtest.h>

#include <cmath>
#include <string>

#include "test_reduction_support.hpp"

using namespace reduction_test;

namespace {

constexpr double kX0 = -4600000.0;
constexpr double kY0 = 2600000.0;
constexpr double kZ0 = -3600000.0;

std::optional<GridPosition> linearGrid(const GeocentricCoordinate& c)
{
    return GridPosition{5000.0 + (c.y - kY0), 3000.0 + (c.x - kX0), 100.0 + (c.z - kZ0)};
}

GnssGlobalPositionObservation globalAt(std::string point, double x, double y, double z)
{
    GnssGlobalPositionObservation position;
    position.point = std::move(point);
    position.geocentric = GeocentricCoordinate{x, y, z};
    position.source = SourceRecord{"Test", "synthetic", "1", "rtk.job", 1};
    return position;
}

GnssGeocentricBaselineObservation vectorOf(std::string from, std::string to, double dx, double dy,
                                           double dz)
{
    GnssGeocentricBaselineObservation vector;
    vector.from = std::move(from);
    vector.to = std::move(to);
    vector.delta = GeocentricCoordinate{dx, dy, dz};
    vector.source = SourceRecord{"Test", "synthetic", "1", "rtk.job", 2};
    return vector;
}

GnssAntenna antenna(double height, AntennaHeightMethod method)
{
    GnssAntenna a;
    a.height = height;
    a.method = method;
    return a;
}

// B1 (N 5000, E 3000, H 60) with its global position at X0, Y0, Z0; B2
// (N 5100, E 3000, H 60) with its global position 100 m further along +Y.
// R has no coordinates: only vectors reach it.
SurveyProject twoBases()
{
    SurveyProject project;
    project.points.push_back(point("B1", 5000.0, 3000.0, 60.0));
    project.points.push_back(point("B2", 5100.0, 3000.0, 60.0));
    project.unpositionedPoints.push_back(unpositioned("R"));
    project.observations.push_back(globalAt("B1", kX0, kY0, kZ0));
    project.observations.push_back(globalAt("B2", kX0, kY0 + 100.0, kZ0));
    return project;
}

ReductionContext linearContext()
{
    ReductionContext context;
    context.geocentricToGrid = linearGrid;
    return context;
}

bool warned(const ReductionReport& report, const std::string& words)
{
    for (const ReportMessage& warning : report.warnings) {
        if (warning.text.find(words) != std::string::npos) {
            return true;
        }
    }
    return false;
}

} // namespace

TEST(ReductionGnss, AVectorFromABaseWithAGlobalPositionRadiatesTheRoverMarkToMark)
{
    // B1 -> R: dX 30, dY 40, dZ 2, so in grid dN = +40, dE = +30 and the
    // ellipsoidal height rises 2.000.
    //   geoid N = 40 + 0.001 (n - 5000): 40.000 at B1, 40.040 at R, so the
    //   orthometric rise is 2.000 - 0.040 = 1.960;
    //   antennas to the phase centre, base 1.500, rover 2.000: +1.500 - 2.000;
    //   dH mark to mark = 1.960 - 0.500 = 1.460.
    // R = N 5040.000, E 3030.000, H 60.000 + 1.460 = 61.460.
    SurveyProject project = twoBases();
    GnssGeocentricBaselineObservation vector = vectorOf("B1", "R", 30.0, 40.0, 2.0);
    vector.fromAntenna = antenna(1.5, AntennaHeightMethod::PhaseCentre);
    vector.toAntenna = antenna(2.0, AntennaHeightMethod::PhaseCentre);
    project.observations.push_back(vector);
    ReductionContext context = linearContext();
    context.geoidSeparation = [](double northing, double) -> std::optional<double> {
        return 40.0 + 0.001 * (northing - 5000.0);
    };

    const auto outcome = reduceAndAdjust(project, bareSettings(), context);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_NEAR(r->northing, 5040.0, 1e-9);
    EXPECT_NEAR(r->easting, 3030.0, 1e-9);
    ASSERT_TRUE(r->elevation.has_value());
    EXPECT_NEAR(*r->elevation, 61.460, 1e-9);
    EXPECT_EQ(r->method, ComputationMethod::Gnss);

    // The vector's row: its length in space, sqrt(30^2 + 40^2 + 2^2) =
    // sqrt(2504) = 50.039984, to its grid length 50.000.
    const ReportObservation* row = findRow(outcome->report, "GNSS geocentric baseline", "R");
    ASSERT_NE(row, nullptr);
    EXPECT_NEAR(row->raw, 50.0399840, 1e-6);
    ASSERT_TRUE(row->reduced.has_value());
    EXPECT_NEAR(*row->reduced, 50.0, 1e-9);
    EXPECT_FALSE(row->rejected);
    // Its height row: 2.000 ellipsoidal, -0.040 geoid, -0.500 antennas.
    const ReportObservation* height = findRow(outcome->report, "GNSS height difference", "R");
    ASSERT_NE(height, nullptr);
    EXPECT_NEAR(height->raw, 2.0, 1e-9);
    ASSERT_EQ(height->corrections.size(), 2U);
    EXPECT_NEAR(height->corrections[0].amount, -0.040, 1e-9);
    EXPECT_EQ(height->corrections[1].kind, CorrectionKind::InstrumentAndTargetHeight);
    EXPECT_NEAR(height->corrections[1].amount, -0.5, 1e-12);
    EXPECT_NEAR(*height->reduced, 1.460, 1e-9);
}

TEST(ReductionGnss, AVectorWhoseBaseHasNoGlobalPositionIsRejectedWithTheReasonNotDropped)
{
    SurveyProject project = twoBases();
    project.points.push_back(point("B3", 4000.0, 3000.0, 60.0)); // grid only
    project.observations.push_back(vectorOf("B3", "R", 30.0, 40.0, 2.0));
    const auto outcome = reduceAndAdjust(project, bareSettings(), linearContext());
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ReportObservation* row = findRow(outcome->report, "GNSS geocentric baseline", "R");
    ASSERT_NE(row, nullptr);
    EXPECT_TRUE(row->rejected);
    EXPECT_NE(row->rejectionReason.find("no global position"), std::string::npos);
    EXPECT_TRUE(warned(outcome->report, "base B3 no global"));
    EXPECT_EQ(findPoint(*outcome, "R"), nullptr);
}

TEST(ReductionGnss, WithoutACoordinateSystemEveryVectorIsRejectedAndTheReportSaysWhy)
{
    SurveyProject project = twoBases();
    project.observations.push_back(vectorOf("B1", "R", 30.0, 40.0, 2.0));
    const auto outcome = reduceAndAdjust(project, bareSettings(), {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ReportObservation* row = findRow(outcome->report, "GNSS geocentric baseline", "R");
    ASSERT_NE(row, nullptr);
    EXPECT_TRUE(row->rejected);
    EXPECT_TRUE(warned(outcome->report, "1 GNSS vector(s) were not used"));
}

TEST(ReductionGnss, ASlantAntennaHeightKeepsTheHorizontalPositionAndLeavesTheHeightOut)
{
    SurveyProject project = twoBases();
    GnssGeocentricBaselineObservation vector = vectorOf("B1", "R", 30.0, 40.0, 2.0);
    vector.fromAntenna = antenna(1.5, AntennaHeightMethod::Vertical);
    vector.toAntenna = antenna(1.62, AntennaHeightMethod::Slant);
    project.observations.push_back(vector);
    const auto outcome = reduceAndAdjust(project, bareSettings(), linearContext());
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_NEAR(r->northing, 5040.0, 1e-9);
    EXPECT_FALSE(r->elevation.has_value());
    const ReportObservation* height = findRow(outcome->report, "GNSS height difference", "R");
    ASSERT_NE(height, nullptr);
    EXPECT_TRUE(height->rejected);
    EXPECT_TRUE(warned(outcome->report, "not a vertical one"));
}

TEST(ReductionGnss, ASecondVectorToTheSameRoverIsACheckWithItsMisclosure)
{
    // B2 -> R: dX 30, dY -59.990, dZ 1.020 puts R at N 5100 - 59.990 =
    // 5040.010: 10 mm north of where B1's vector put it, 20 mm higher.
    SurveyProject project = twoBases();
    project.observations.push_back(vectorOf("B1", "R", 30.0, 40.0, 1.0));
    project.observations.push_back(vectorOf("B2", "R", 30.0, -59.990, 1.020));
    const auto outcome = reduceAndAdjust(project, bareSettings(), linearContext());
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_NEAR(r->northing, 5040.0, 1e-9); // the first vector placed it
    ASSERT_EQ(outcome->report.misclosures.size(), 1U);
    const MisclosureReport& check = outcome->report.misclosures[0];
    EXPECT_EQ(check.name, "R by GNSS vector from B2");
    EXPECT_NEAR(*check.northing, 0.010, 1e-9);
    EXPECT_NEAR(*check.easting, 0.0, 1e-9);
    EXPECT_NEAR(*check.height, 0.020, 1e-9);
}

TEST(ReductionGnss, TwoVectorsToOneRoverAdjustToTheirMeanWithTheHandWorkedStatistics)
{
    // As above, adjusted: horizontal and levels, both bases held.
    // Horizontal: each vector gives two equations (N, E) at the a-priori
    // 10 mm; unknowns R's N and E. 4 - 2 = redundancy 2.
    //   R = mean: N 5040.005, E 3030.000; v(N) = +/-0.005, v(E) = 0.
    //   v'Pv = 2 (0.005 / 0.010)^2 = 0.5; variance factor 0.5 / 2 = 0.25.
    //   Qxx(R) = sigma^2 / 2 = 5e-5 m^2 per axis; scaled by 0.25:
    //   sigma(N) = sigma(E) = sqrt(1.25e-5) = 0.0035355.
    //   The 95 % ellipse scale for two dimensions: sqrt(-2 ln 0.05) =
    //   sqrt(5.991465) = 2.447747.
    // Levels: 1.000 and 1.020 at the a-priori 20 mm; one unknown.
    //   R = 61.010; v = +/-0.010; v'Pv = 2 (0.010 / 0.020)^2 = 0.5, r = 1.
    SurveyProject project = twoBases();
    project.observations.push_back(vectorOf("B1", "R", 30.0, 40.0, 1.0));
    project.observations.push_back(vectorOf("B2", "R", 30.0, -59.990, 1.020));
    ReductionSettings settings = bareSettings();
    settings.method = AdjustmentMethod::Network;
    settings.networkDimension = NetworkDimension::HorizontalAndLevels;
    settings.control = {ControlSelection{ControlPoint::fixed3d("B1")},
                        ControlSelection{ControlPoint::fixed3d("B2")}};

    const auto outcome = reduceAndAdjust(project, settings, linearContext());
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_NEAR(r->northing, 5040.005, 1e-9);
    EXPECT_NEAR(r->easting, 3030.0, 1e-9);
    EXPECT_NEAR(*r->elevation, 61.010, 1e-9);
    EXPECT_EQ(r->method, ComputationMethod::NetworkLeastSquares);
    ASSERT_TRUE(r->sigmaNorthing.has_value());
    EXPECT_NEAR(*r->sigmaNorthing, 0.0035355, 1e-7);

    ASSERT_EQ(outcome->report.adjustments.size(), 2U);
    const AdjustmentReport& horizontal = outcome->report.adjustments[0];
    EXPECT_EQ(horizontal.observations, 4U);
    EXPECT_EQ(horizontal.unknowns, 2U);
    EXPECT_EQ(horizontal.redundancy, 2U);
    ASSERT_TRUE(horizontal.varianceFactor.has_value());
    // To 1e-7: the vectors end on Y values of 2.6e6 m, whose last digit is
    // 5e-10 m, a relative 1e-7 of the 5 mm residuals.
    EXPECT_NEAR(*horizontal.varianceFactor, 0.25, 1e-7);
    ASSERT_EQ(horizontal.ellipses.size(), 1U);
    EXPECT_EQ(horizontal.ellipses[0].pointId, "R");
    EXPECT_NEAR(horizontal.ellipses[0].confidenceScale, 2.447747, 1e-6);
    EXPECT_NEAR(horizontal.ellipses[0].standard.semiMajor, 0.0035355, 1e-7);

    const AdjustmentReport& levels = outcome->report.adjustments[1];
    EXPECT_EQ(levels.redundancy, 1U);
    ASSERT_TRUE(levels.varianceFactor.has_value());
    EXPECT_NEAR(*levels.varianceFactor, 0.5, 1e-9);
    // Without a geoid the heights are ellipsoidal, and the report says so.
    EXPECT_TRUE(warned(outcome->report, "no geoid separation"));
}

TEST(ReductionGnss, AWeightedControlHeightMovesByItsShareOfTheMisclosure)
{
    // As above, but B2 held horizontally and WEIGHTED in height at 20 mm.
    // Minimise (R - 61.000)^2 + (R - B2 - 1.020)^2 + (B2 - 60.000)^2, all
    // at 20 mm:
    //   dR:  2R - B2 = 62.020      dB2: -R + 2 B2 = 58.980
    //   3R = 2 * 62.020 + 58.980 = 183.020: R = 61.006667, B2 = 59.993333.
    //   v = +0.006667, -0.006667 (the vectors), -0.006667 (B2's height);
    //   v'Pv = 3 (0.006667 / 0.020)^2 = 0.333333, r = 3 - 2 = 1.
    SurveyProject project = twoBases();
    project.observations.push_back(vectorOf("B1", "R", 30.0, 40.0, 1.0));
    project.observations.push_back(vectorOf("B2", "R", 30.0, -59.990, 1.020));
    ReductionSettings settings = bareSettings();
    settings.method = AdjustmentMethod::Network;
    settings.networkDimension = NetworkDimension::HorizontalAndLevels;
    ControlPoint b2 = ControlPoint::fixedHorizontal("B2");
    b2.elevation = ControlComponent{ControlConstraint::Weighted, 0.020};
    settings.control = {ControlSelection{ControlPoint::fixed3d("B1")}, ControlSelection{b2}};

    const auto outcome = reduceAndAdjust(project, settings, linearContext());
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    EXPECT_NEAR(*findPoint(*outcome, "R")->elevation, 61.006667, 1e-6);
    EXPECT_NEAR(*findPoint(*outcome, "B2")->elevation, 59.993333, 1e-6);
    // Horizontally B2 is held: it does not move.
    EXPECT_NEAR(findPoint(*outcome, "B2")->northing, 5100.0, 1e-12);
    const AdjustmentReport& levels = outcome->report.adjustments[1];
    EXPECT_EQ(levels.unknowns, 2U);
    EXPECT_EQ(levels.redundancy, 1U);
    EXPECT_NEAR(*levels.varianceFactor, 0.333333, 1e-6);
    bool controlResidual = false;
    for (const ReportResidual& residual : levels.residuals) {
        if (residual.observation == "control height B2") {
            controlResidual = true;
            EXPECT_NEAR(residual.residual, -0.006667, 1e-6);
        }
    }
    EXPECT_TRUE(controlResidual);
}

TEST(ReductionGnss, TheFilesCovarianceIsTurnedToNorthEastAndUpAtTheBase)
{
    // A base on the equator at the prime meridian, X = a = 6 378 137: there
    // north is +Z, east is +Y and up is +X, so a covariance of
    // XX 4e-4, YY 1e-4, ZZ 9e-6 is sigma north sqrt(ZZ) = 0.003,
    // east sqrt(YY) = 0.010, up sqrt(XX) = 0.020.
    constexpr double a = 6378137.0;
    SurveyProject project;
    project.points.push_back(point("B1", 0.0, 0.0, 10.0));
    project.points.push_back(point("B2", 100.0, 0.0, 10.0));
    project.unpositionedPoints.push_back(unpositioned("R"));
    project.observations.push_back(globalAt("B1", a, 0.0, 0.0));
    project.observations.push_back(globalAt("B2", a, 0.0, 100.0));
    GnssGeocentricBaselineObservation first = vectorOf("B1", "R", 0.0, 30.0, 40.0);
    first.covariance = GnssCovariance3{4e-4, 1e-4, 9e-6, 0.0, 0.0, 0.0};
    project.observations.push_back(first);
    project.observations.push_back(vectorOf("B2", "R", 0.0, 30.0, -60.0));
    ReductionContext context;
    context.geocentricToGrid = [](const GeocentricCoordinate& c) -> std::optional<GridPosition> {
        return GridPosition{c.z, c.y, c.x - 6378137.0 + 10.0};
    };
    ReductionSettings settings = bareSettings();
    settings.method = AdjustmentMethod::Network;
    settings.control = {ControlSelection{ControlPoint::fixedHorizontal("B1")},
                        ControlSelection{ControlPoint::fixedHorizontal("B2")}};

    const auto outcome = reduceAndAdjust(project, settings, context);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const AdjustmentReport& horizontal = outcome->report.adjustments.at(0);
    std::optional<double> north;
    std::optional<double> east;
    for (const ReportResidual& residual : horizontal.residuals) {
        if (residual.observation == "GNSS vector B1 -> R (north)") {
            north = residual.sigma;
        } else if (residual.observation == "GNSS vector B1 -> R (east)") {
            east = residual.sigma;
        }
    }
    ASSERT_TRUE(north.has_value());
    ASSERT_TRUE(east.has_value());
    EXPECT_NEAR(*north, 0.003, 1e-12);
    EXPECT_NEAR(*east, 0.010, 1e-12);
}

TEST(ReductionGnss, AGeodeticBasePositionIsTakenToXyzOnTheGrs80Ellipsoid)
{
    // Latitude 0, longitude 0, height 0 is X = a = 6 378 137, Y = Z = 0;
    // latitude 90 is Z = b = a (1 - f) = 6 356 752.314140.
    SurveyProject project;
    project.points.push_back(point("B1", 0.0, 0.0, 10.0));
    project.unpositionedPoints.push_back(unpositioned("R"));
    GnssGlobalPositionObservation base;
    base.point = "B1";
    base.geodetic = GeodeticCoordinate{0.0, 0.0, 0.0};
    project.observations.push_back(base);
    project.observations.push_back(vectorOf("B1", "R", 0.0, 30.0, 40.0));
    std::vector<GeocentricCoordinate> asked;
    ReductionContext context;
    context.geocentricToGrid = [&asked](const GeocentricCoordinate& c) -> std::optional<GridPosition> {
        asked.push_back(c);
        return GridPosition{c.z, c.y, 0.0};
    };
    const auto outcome = reduceAndAdjust(project, bareSettings(), context);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    // The base's position is converted, then the rover's, base + delta.
    ASSERT_GE(asked.size(), 2U);
    EXPECT_DOUBLE_EQ(asked[0].x, 6378137.0);
    EXPECT_DOUBLE_EQ(asked[0].y, 0.0);
    EXPECT_DOUBLE_EQ(asked[0].z, 0.0);
    EXPECT_DOUBLE_EQ(asked.back().x, 6378137.0);
    EXPECT_DOUBLE_EQ(asked.back().y, 30.0);
    EXPECT_DOUBLE_EQ(asked.back().z, 40.0);
    const ComputedPoint* r = findPoint(*outcome, "R");
    ASSERT_NE(r, nullptr);
    EXPECT_NEAR(r->northing, 40.0, 1e-9);
    EXPECT_NEAR(r->easting, 30.0, 1e-9);

    project.observations[0] = [] {
        GnssGlobalPositionObservation pole;
        pole.point = "B1";
        pole.geodetic = GeodeticCoordinate{katana::math::kHalfPi, 0.0, 0.0};
        return pole;
    }();
    asked.clear();
    ASSERT_TRUE(reduceAndAdjust(project, bareSettings(), context).ok());
    ASSERT_FALSE(asked.empty());
    EXPECT_NEAR(asked[0].z, 6356752.314140, 1e-6);
}
