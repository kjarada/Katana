#include <gtest/gtest.h>

#include <limits>
#include <optional>
#include <string>

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
