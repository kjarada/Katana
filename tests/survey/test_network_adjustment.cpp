#include <gtest/gtest.h>

#include <limits>
#include <optional>
#include <string>
#include <utility>

#include "katana/survey/data_model.hpp"
#include "katana/survey/network.hpp"
#include "katana/survey/network_adjustment.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;

namespace {

SurveyPoint point(std::string id, std::optional<double> elevation)
{
    SurveyPoint p;
    p.id = std::move(id);
    p.elevation = elevation;
    return p;
}

LevelDifferenceObservation level(std::string from, std::string to, double rise, double sigma)
{
    LevelDifferenceObservation o;
    o.from = std::move(from);
    o.to = std::move(to);
    o.heightDifference = rise;
    o.sigma = sigma;
    return o;
}

// Benchmark A held at 100, and B levelled from it twice: rises of 1.0 and 1.5,
// each with sigma 0.5. Worked by hand: equal weights, so B is the plain mean,
// 100 + 1.25 = 101.25; the residuals (adjusted - observed) are +0.25 and
// -0.25; v^T P v = 2 * 0.25^2 / 0.5^2 = 0.5 on one degree of freedom. Every
// number is exact in binary.
SurveyNetwork twoRuns(std::optional<double> heightOfA, std::optional<double> startOfB,
                      ControlPoint control)
{
    SurveyNetwork network;
    EXPECT_TRUE(network.addPoint(point("A", heightOfA)).ok());
    EXPECT_TRUE(network.addPoint(point("B", startOfB)).ok());
    EXPECT_TRUE(network.addControlPoint(std::move(control)).ok());
    EXPECT_TRUE(network.addObservation(level("A", "B", 1.0, 0.5)).ok());
    EXPECT_TRUE(network.addObservation(level("A", "B", 1.5, 0.5)).ok());
    return network;
}

} // namespace

TEST(SurveyLevelNetwork, TwoRunsFromAHeldBenchmarkAverageToTheHandWorkedHeight)
{
    const auto result = adjustLevelNetwork(twoRuns(100.0, 101.0, ControlPoint::fixedVertical("A")));
    ASSERT_TRUE(result.ok()) << result.error().message;
    ASSERT_EQ(result->elevations.size(), 2u);
    EXPECT_EQ(result->elevations[0].pointId, "A");
    EXPECT_EQ(result->elevations[0].elevation, 100.0);
    EXPECT_EQ(result->elevations[1].pointId, "B");
    EXPECT_DOUBLE_EQ(result->elevations[1].elevation, 101.25);
    ASSERT_EQ(result->residuals.size(), 2u);
    EXPECT_DOUBLE_EQ(result->residuals[0].residual, 0.25);
    EXPECT_DOUBLE_EQ(result->residuals[1].residual, -0.25);
    EXPECT_EQ(result->statistics.degreesOfFreedom, 1u);
    EXPECT_DOUBLE_EQ(result->statistics.weightedSquaredResiduals, 0.5);
}

TEST(SurveyLevelNetwork, AFreePointWithNoHeightAdjustsToTheSameAnswer)
{
    // The model is linear, so where B starts is irrelevant - including when the
    // source never gave it a height at all.
    const auto result =
        adjustLevelNetwork(twoRuns(100.0, std::nullopt, ControlPoint::fixedVertical("A")));
    ASSERT_TRUE(result.ok()) << result.error().message;
    EXPECT_DOUBLE_EQ(result->elevations[1].elevation, 101.25);
}

TEST(SurveyLevelNetwork, AHeldBenchmarkWithNoHeightIsRefusedRatherThanTakenAsZero)
{
    // Taken as zero it would level B to 1.25 and report a perfect adjustment
    // onto the wrong datum.
    const auto held =
        adjustLevelNetwork(twoRuns(std::nullopt, 101.0, ControlPoint::fixedVertical("A")));
    ASSERT_FALSE(held.ok());
    EXPECT_EQ(held.error().code, ErrorCode::AdjustmentFailure);
    EXPECT_NE(held.error().message.find("'A'"), std::string::npos) << held.error().message;

    const auto weighted = adjustLevelNetwork(
        twoRuns(std::nullopt, 101.0, ControlPoint::weightedVertical("A", 0.001)));
    ASSERT_FALSE(weighted.ok());
    EXPECT_EQ(weighted.error().code, ErrorCode::AdjustmentFailure);
}

TEST(SurveyPointHeight, AnAbsentHeightIsValidAndANonFiniteOneIsNot)
{
    SurveyProject project;
    project.points.push_back(point("NOZ", std::nullopt));
    EXPECT_TRUE(validateProject(project).ok());

    project.points.push_back(point("BAD", std::numeric_limits<double>::infinity()));
    const auto status = validateProject(project);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);

    SurveyNetwork network;
    EXPECT_TRUE(network.addPoint(point("NOZ", std::nullopt)).ok());
    EXPECT_FALSE(network.addPoint(point("NAN", std::numeric_limits<double>::quiet_NaN())).ok());
}

TEST(SurveyUnits, EachDeclaredUnitIsItsStatutoryRatioAndUnknownIsRefused)
{
    ASSERT_TRUE(metresPer(LinearUnit::Metres).ok());
    EXPECT_EQ(*metresPer(LinearUnit::Metres), (katana::math::UnitRatio{1, 1}));
    EXPECT_EQ(*metresPer(LinearUnit::Feet), (katana::math::UnitRatio{381, 1250}));
    EXPECT_EQ(*metresPer(LinearUnit::UsSurveyFeet), (katana::math::UnitRatio{1200, 3937}));
    EXPECT_EQ(*metresPer(LinearUnit::Links), (katana::math::UnitRatio{12573, 62500}));
    const auto unknown = metresPer(LinearUnit::Unknown);
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::InvalidArgument);
}

TEST(SurveyUnpositionedPoints, AReferenceMayResolveToAPointTheSourceGaveNoCoordinatesFor)
{
    // A raw file: setup on 1, a shot to 2 whose coordinates the file never gives.
    SurveyProject project;
    SurveyPoint occupied;
    occupied.id = "1";
    project.points.push_back(occupied);
    UnpositionedPoint target;
    target.id = "2";
    target.code = "TREE";
    project.unpositionedPoints.push_back(target);
    SurveyStation station;
    station.setup.id = "S1";
    station.setup.pointId = "1";
    HorizontalAngleObservation angle;
    angle.at = "1";
    angle.from = "2";
    angle.to = "2";
    DistanceObservation distance;
    distance.from = "1";
    distance.to = "2";
    distance.distance = 12.5;
    distance.sigma = 0.002;
    station.observations.push_back(distance);
    project.stations.push_back(station);
    SurveyFeature line;
    line.code = "TREE";
    line.pointIds = {"1", "2"};
    project.features.push_back(line);
    EXPECT_TRUE(validateProject(project).ok()) << validateProject(project).error().message;

    // One id, two lists: refused, since a point cannot both have a position and not.
    SurveyPoint clash;
    clash.id = "2";
    project.points.push_back(clash);
    const auto both = validateProject(project);
    ASSERT_FALSE(both.ok());
    EXPECT_EQ(both.error().code, ErrorCode::AlreadyExists);

    project.points.pop_back();
    project.unpositionedPoints.push_back(UnpositionedPoint{});
    EXPECT_EQ(validateProject(project).error().code, ErrorCode::InvalidArgument);
}

// ---- Directions: one set per point, one orientation unknown ----------------------

namespace {

constexpr double kDegree = 3.14159265358979323846 / 180.0;

SurveyPoint placed(std::string id, double northing, double easting)
{
    SurveyPoint p;
    p.id = std::move(id);
    p.northing = northing;
    p.easting = easting;
    return p;
}

// A, B and C held 100 m north, east and south of P, and P (really at the
// origin) started at N 3, E -2. Read at P with the circle turned 30 degrees:
// reading = azimuth - 30, so A (azimuth 0) reads 330, B (90) 60, C (180) 150.
SurveyNetwork freeStation(double sigmaDirection)
{
    SurveyNetwork network;
    EXPECT_TRUE(network.addPoint(placed("P", 3.0, -2.0)).ok());
    EXPECT_TRUE(network.addPoint(placed("A", 100.0, 0.0)).ok());
    EXPECT_TRUE(network.addPoint(placed("B", 0.0, 100.0)).ok());
    EXPECT_TRUE(network.addPoint(placed("C", -100.0, 0.0)).ok());
    for (const char* id : {"A", "B", "C"}) {
        EXPECT_TRUE(network.addControlPoint(ControlPoint::fixedHorizontal(id)).ok());
    }
    EXPECT_TRUE(network.addObservation(HorizontalDirectionObservation{"P", "A", 330.0 * kDegree,
                                                                      sigmaDirection, {}, {}})
                    .ok());
    EXPECT_TRUE(network.addObservation(HorizontalDirectionObservation{"P", "B", 60.0 * kDegree,
                                                                      sigmaDirection, {}, {}})
                    .ok());
    EXPECT_TRUE(network.addObservation(HorizontalDirectionObservation{"P", "C", 150.0 * kDegree,
                                                                      sigmaDirection, {}, {}})
                    .ok());
    return network;
}

DistanceObservation horizontalDistance(std::string from, std::string to, double metres)
{
    DistanceObservation o;
    o.from = std::move(from);
    o.to = std::move(to);
    o.distance = metres;
    o.sigma = 0.002;
    o.kind = DistanceKind::Horizontal;
    return o;
}

} // namespace

TEST(SurveyHorizontalNetwork, ThreeDirectionsFixAFreePointAndTheOrientationOfItsSet)
{
    // Three readings for three unknowns (N, E and the set's orientation): the
    // point and the turn of the circle come back exactly, with nothing left
    // to check them.
    const auto result = adjustHorizontalNetwork(freeStation(1e-5));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result->parameters.size(), 3u);
    EXPECT_EQ(result->parameters[2], (AdjustedParameter{"P", CoordinateComponent::Orientation}));
    EXPECT_TRUE(result->unusedObservations.empty());
    EXPECT_EQ(result->statistics.degreesOfFreedom, 0u);
    const AdjustedStation& p = result->stations.front();
    ASSERT_EQ(p.pointId, "P");
    EXPECT_NEAR(p.position.northing, 0.0, 1e-9);
    EXPECT_NEAR(p.position.easting, 0.0, 1e-9);
    ASSERT_EQ(result->orientations.size(), 1u);
    EXPECT_EQ(result->orientations[0].pointId, "P");
    EXPECT_NEAR(result->orientations[0].orientation, 30.0 * kDegree, 1e-12);
    for (const ResidualRecord& residual : result->residuals) {
        EXPECT_NEAR(residual.residual, 0.0, 1e-12);
    }
}

TEST(SurveyHorizontalNetwork, ADirectionSetWithDistancesIsAdjustedToAnIndependentSolution)
{
    // The same three readings, and distances 3 mm long to A, 2 mm short to B
    // and exact to C, each to 2 mm; the readings to 1e-5 rad. Worked apart
    // from this code, by the normal equations of the same model solved by
    // Cramer's rule and iterated to 1e-13 m (a scratch script, not Katana):
    //   N -0.000642851429, E 0.000222220744, orientation 29.999877224694 deg,
    //   residuals (computed - observed) of the readings -0.0164", -0.8840",
    //   +0.9004", of the distances -2.3571, +1.7778, -0.6429 mm;
    //   v'Pv 2.65674994700 on 3 degrees of freedom.
    // By hand, roughly: A and C pull P 1.5 mm south with weight 2 / 0.002^2
    // = 5e5, the reading to B holds it where it is with (0.01 / 1e-5)^2 * 2/3
    // = 6.7e5 (the orientation takes a third), so it moves 1.5 * 5 / 11.7 =
    // 0.64 mm south.
    SurveyNetwork network = freeStation(1e-5);
    ASSERT_TRUE(network.addObservation(horizontalDistance("P", "A", 100.003)).ok());
    ASSERT_TRUE(network.addObservation(horizontalDistance("P", "B", 99.998)).ok());
    ASSERT_TRUE(network.addObservation(horizontalDistance("P", "C", 100.000)).ok());
    const auto result = adjustHorizontalNetwork(network);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->statistics.degreesOfFreedom, 3u);
    const AdjustedStation& p = result->stations.front();
    EXPECT_NEAR(p.position.northing, -0.000642851429, 1e-9);
    EXPECT_NEAR(p.position.easting, 0.000222220744, 1e-9);
    ASSERT_EQ(result->orientations.size(), 1u);
    EXPECT_NEAR(result->orientations[0].orientation, 29.999877224694 * kDegree, 1e-11);
    ASSERT_EQ(result->residuals.size(), 6u);
    const double arcSecond = kDegree / 3600.0;
    EXPECT_NEAR(result->residuals[0].residual, -0.0164 * arcSecond, 0.0001 * arcSecond);
    EXPECT_NEAR(result->residuals[1].residual, -0.8840 * arcSecond, 0.0001 * arcSecond);
    EXPECT_NEAR(result->residuals[2].residual, 0.9004 * arcSecond, 0.0001 * arcSecond);
    EXPECT_NEAR(result->residuals[3].residual, -0.0023571, 1e-7);
    EXPECT_NEAR(result->residuals[4].residual, 0.0017778, 1e-7);
    EXPECT_NEAR(result->residuals[5].residual, -0.0006429, 1e-7);
    EXPECT_NEAR(result->statistics.weightedSquaredResiduals, 2.65674994700, 1e-8);
    EXPECT_GT(result->orientations[0].sigma, 0.0);
}

TEST(SurveyHorizontalNetwork, ReadingsEitherSideOfZeroAreOneSet)
{
    // The circle turned 0.1 degrees: A, due north, reads 359.9, B 89.9 and C
    // 179.9 - one set with readings either side of zero. Azimuth less reading
    // is -359.9 for A and +0.1 for the others; the set's start is wrapped
    // about the first, so the orientation comes out 0.1 degrees, not half a
    // turn away.
    SurveyNetwork network;
    ASSERT_TRUE(network.addPoint(placed("P", 1.0, 1.0)).ok());
    ASSERT_TRUE(network.addPoint(placed("A", 100.0, 0.0)).ok());
    ASSERT_TRUE(network.addPoint(placed("B", 0.0, 100.0)).ok());
    ASSERT_TRUE(network.addPoint(placed("C", -100.0, 0.0)).ok());
    for (const char* id : {"A", "B", "C"}) {
        ASSERT_TRUE(network.addControlPoint(ControlPoint::fixedHorizontal(id)).ok());
    }
    for (const auto& [to, reading] :
         {std::pair{"A", 359.9}, std::pair{"B", 89.9}, std::pair{"C", 179.9}}) {
        ASSERT_TRUE(network
                        .addObservation(
                            HorizontalDirectionObservation{"P", to, reading * kDegree, 1e-5, {}, {}})
                        .ok());
    }
    const auto result = adjustHorizontalNetwork(network);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_NEAR(result->stations.front().position.northing, 0.0, 1e-9);
    EXPECT_NEAR(result->stations.front().position.easting, 0.0, 1e-9);
    ASSERT_EQ(result->orientations.size(), 1u);
    EXPECT_NEAR(result->orientations[0].orientation, 0.1 * kDegree, 1e-12);
}

TEST(SurveyHorizontalNetwork, APointOnTheCircleThroughItsThreeTargetsIsRankDeficient)
{
    // The danger circle: P at N 0, E -100 is on the circle through A, B and C
    // (radius 100 about the origin). Along the circle every angle between the
    // targets stays the same, so a move along it is a turn of the set, and
    // the readings cannot tell the two apart. Readings from P with the circle
    // at north: A at atan2(100, 100) = 45, B at atan2(200, 0) = 90, C at
    // atan2(100, -100) = 135 degrees; P started exactly there.
    SurveyNetwork network;
    ASSERT_TRUE(network.addPoint(placed("P", 0.0, -100.0)).ok());
    ASSERT_TRUE(network.addPoint(placed("A", 100.0, 0.0)).ok());
    ASSERT_TRUE(network.addPoint(placed("B", 0.0, 100.0)).ok());
    ASSERT_TRUE(network.addPoint(placed("C", -100.0, 0.0)).ok());
    for (const char* id : {"A", "B", "C"}) {
        ASSERT_TRUE(network.addControlPoint(ControlPoint::fixedHorizontal(id)).ok());
    }
    for (const auto& [to, reading] :
         {std::pair{"A", 45.0}, std::pair{"B", 90.0}, std::pair{"C", 135.0}}) {
        ASSERT_TRUE(network
                        .addObservation(
                            HorizontalDirectionObservation{"P", to, reading * kDegree, 1e-5, {}, {}})
                        .ok());
    }
    const auto result = adjustHorizontalNetwork(network);
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::AdjustmentFailure);
    EXPECT_NE(result.error().message.find("rank deficient"), std::string::npos)
        << result.error().message;
}
