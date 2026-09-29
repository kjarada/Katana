#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/math/numerics.hpp"
#include "katana/survey/data_model.hpp"
#include "katana/survey/network.hpp"
#include "katana/survey/network_adjustment.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;
using katana::math::kHalfPi;
using katana::math::kPi;

namespace {

SurveyPoint positioned(std::string id, double northing, double easting)
{
    SurveyPoint point;
    point.id = std::move(id);
    point.northing = northing;
    point.easting = easting;
    return point;
}

} // namespace

TEST(RawObservations, NewMembersDefaultToNotStated)
{
    const Pointing pointing;
    EXPECT_EQ(pointing.index, 0U);
    EXPECT_EQ(pointing.face, Face::Unknown);

    const TargetInfo target;
    EXPECT_FALSE(target.prismConstant.has_value());
    EXPECT_EQ(target.prismConstantState, CorrectionState::Unknown);

    const InstrumentSettings instrument;
    EXPECT_FALSE(instrument.prismConstant.has_value());
    EXPECT_FALSE(instrument.atmosphericPpm.has_value());
    EXPECT_EQ(instrument.atmosphericPpmState, CorrectionState::Unknown);
    EXPECT_EQ(instrument.scaleFactorState, CorrectionState::Unknown);
    EXPECT_FALSE(instrument.time.known());

    const SurveyStation station;
    EXPECT_EQ(station.instrument, InstrumentSettings{});

    const SurveyProject project;
    EXPECT_TRUE(project.controlPoints.empty());
    EXPECT_TRUE(project.gnssSessions.empty());
}

TEST(RawObservations, AnAggregateWrittenBeforeThePointingExistedStillMeansTheSame)
{
    // Positional initialisation as every test and importer wrote it before
    // Pointing and TargetInfo were added after `source`.
    const DistanceObservation distance{"A", "B", 10.0, 0.002, DistanceKind::Slope, 1.5, 1.6, {}};
    EXPECT_EQ(distance.pointing, Pointing{});
    EXPECT_EQ(distance.target, TargetInfo{});
    EXPECT_TRUE(validateObservation(distance).ok());
}

TEST(RawObservations, AHorizontalDirectionIsARawCircleReadingAtOneSetup)
{
    HorizontalDirectionObservation direction;
    direction.at = "S1";
    direction.to = "P7";
    direction.direction = -kHalfPi; // a reading given as -90 degrees
    direction.sigma = 1e-5;
    direction.pointing = Pointing{3, Face::Right};
    const Observation observation = direction;

    EXPECT_EQ(observationKindName(observation), "horizontal direction");
    EXPECT_EQ(referencedPoints(observation), (std::vector<std::string>{"S1", "P7"}));
    EXPECT_TRUE(validateObservation(observation).ok());
    // -pi/2 wrapped into [0, 2 pi) is 3 pi / 2.
    const Observation normalised = normalizedObservation(observation);
    const auto& wrapped = std::get<HorizontalDirectionObservation>(normalised);
    EXPECT_NEAR(wrapped.direction, 1.5 * kPi, 1e-15);
    ASSERT_NE(observationPointing(observation), nullptr);
    EXPECT_EQ(observationPointing(observation)->index, 3U);
    EXPECT_EQ(observationPointing(observation)->face, Face::Right);

    direction.sigma = 0.0;
    EXPECT_EQ(validateObservation(direction).error().code, ErrorCode::InvalidSurveyObservation);
}

TEST(RawObservations, OnlyTotalStationObservationsBelongToAPointing)
{
    EXPECT_EQ(observationPointing(AzimuthObservation{"A", "B", 1.0, 1e-5, {}}), nullptr);
    EXPECT_EQ(observationPointing(LevelDifferenceObservation{"A", "B", 1.0, 0.001, 0.0, {}}),
              nullptr);
    EXPECT_NE(observationPointing(ZenithAngleObservation{"A", "B", 1.5, 1e-5, 1.5, 1.6, {}}),
              nullptr);
}

TEST(RawObservations, AGnssGlobalPositionIsEitherGeocentricOrGeodeticNeverBoth)
{
    GnssGlobalPositionObservation position;
    position.point = "BASE";
    EXPECT_FALSE(validateObservation(position).ok()); // neither

    position.geocentric = GeocentricCoordinate{-4052051.0, 4212836.0, -2545106.0};
    EXPECT_TRUE(validateObservation(position).ok());
    EXPECT_EQ(observationKindName(position), "GNSS geocentric position");
    EXPECT_EQ(referencedPoints(position), (std::vector<std::string>{"BASE"}));

    position.geodetic = GeodeticCoordinate{-0.5, 2.3, 40.0};
    EXPECT_FALSE(validateObservation(position).ok()); // both

    position.geocentric.reset();
    EXPECT_TRUE(validateObservation(position).ok());
    position.geodetic->latitude = kHalfPi + 0.01; // beyond a pole
    EXPECT_FALSE(validateObservation(position).ok());
}

TEST(RawObservations, AGnssCovarianceIsOptionalButWhenStatedItsVariancesArePositive)
{
    GnssGeocentricBaselineObservation baseline;
    baseline.from = "BASE";
    baseline.to = "R1";
    baseline.delta = GeocentricCoordinate{120.0, -35.5, 8.25};
    EXPECT_TRUE(validateObservation(baseline).ok()); // no covariance stated
    EXPECT_EQ(referencedPoints(baseline), (std::vector<std::string>{"BASE", "R1"}));

    baseline.covariance.xy = 1e-6; // stated, with a zero diagonal
    EXPECT_FALSE(validateObservation(baseline).ok());
    baseline.covariance = GnssCovariance3{4e-6, 4e-6, 9e-6, 1e-6, 0.0, 0.0};
    EXPECT_TRUE(validateObservation(baseline).ok());
}

TEST(RawObservations, ControlNamedByAFileMustBeAPositionedPointOfTheProject)
{
    SurveyProject project;
    project.points = {positioned("CP1", 1000.0, 2000.0)};
    project.unpositionedPoints = {UnpositionedPoint{"P9", {}, {}, {}, {}}};
    project.controlPoints = {ControlPoint::fixedHorizontal("CP1")};
    EXPECT_TRUE(validateProject(project).ok());

    project.controlPoints.push_back(ControlPoint::fixedHorizontal("P9"));
    EXPECT_EQ(validateProject(project).error().code, ErrorCode::NotFound);

    project.controlPoints = {ControlPoint::weightedVertical("CP1", 0.0)};
    EXPECT_EQ(validateProject(project).error().code, ErrorCode::InvalidArgument);
}

TEST(RawObservations, AStationWithRawDirectionsAndGnssPassesProjectValidation)
{
    SurveyProject project;
    project.points = {positioned("S1", 0.0, 0.0)};
    project.unpositionedPoints = {UnpositionedPoint{"P1", {}, {}, {}, {}}};
    SurveyStation station;
    station.setup = Station{"SETUP1", "S1", 1.55};
    station.instrument.make = "Leica";
    station.instrument.atmosphericPpmState = CorrectionState::Applied;
    station.observations = {
        HorizontalDirectionObservation{"S1", "P1", 0.25, 1e-5, {}, Pointing{1, Face::Left}},
        ZenithAngleObservation{"S1", "P1", 1.6, 1e-5, 1.55, 1.8, {}, Pointing{1, Face::Left}},
    };
    project.stations = {station};
    GnssGlobalPositionObservation base;
    base.point = "S1";
    base.geodetic = GeodeticCoordinate{-0.59, 2.64, 45.0};
    project.observations = {base};
    EXPECT_TRUE(validateProject(project).ok()) << validateProject(project).error().describe();
}

TEST(RawObservations, TheHorizontalAdjustmentSetsAGlobalGnssValueAsideAndTakesADirection)
{
    // A global GNSS value needs the reduction's conversion to grid first; an
    // adjustment handed one must set it aside and say so, not crash. A
    // direction it takes, as one of the set read at A with an orientation
    // unknown of its own (network_adjustment.hpp) - here that unknown and
    // the reading add nothing else. B is due north of A, so the reading 0.3
    // there turns into an orientation of 0 - 0.3 = 2 pi - 0.3.
    SurveyNetwork network;
    ASSERT_TRUE(network.addPoint(positioned("A", 0.0, 0.0)).ok());
    ASSERT_TRUE(network.addPoint(positioned("B", 100.0, 0.0)).ok());
    ASSERT_TRUE(network.addControlPoint(ControlPoint::fixedHorizontal("A")).ok());
    ASSERT_TRUE(network.addObservation(
                    DistanceObservation{"A", "B", 100.0, 0.002, DistanceKind::Horizontal, 0.0, 0.0, {}}).ok());
    ASSERT_TRUE(network.addObservation(AzimuthObservation{"A", "B", 0.0, 1e-5, {}}).ok());
    ASSERT_TRUE(
        network.addObservation(HorizontalDirectionObservation{"A", "B", 0.3, 1e-5, {}, {}}).ok());
    GnssGlobalPositionObservation global;
    global.point = "B";
    global.geodetic = GeodeticCoordinate{-0.59, 2.64, 45.0};
    ASSERT_TRUE(network.addObservation(global).ok());
    const auto result = adjustHorizontalNetwork(network);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->unusedObservations, (std::vector<std::size_t>{3}));
    ASSERT_EQ(result->orientations.size(), 1U);
    EXPECT_EQ(result->orientations[0].pointId, "A");
    EXPECT_NEAR(result->orientations[0].orientation, 2.0 * kPi - 0.3, 1e-12);
}

TEST(RawObservations, TimestampsPrintAsIso8601WithTheirTimeSystem)
{
    EXPECT_EQ(toString(SurveyTimestamp{}), "");
    EXPECT_EQ(toString(SurveyTimestamp{2024, 3, 5, 10, 15, 30.5, "GPS"}),
              "2024-03-05T10:15:30.5 GPS");
    EXPECT_EQ(toString(SurveyTimestamp{1999, 12, 31, 23, 59, 5.25, ""}), "1999-12-31T23:59:05.25");
}

TEST(RawObservations, NewEnumsHaveWordsForTheReport)
{
    EXPECT_STREQ(toString(Face::Left), "face left");
    EXPECT_STREQ(toString(CorrectionState::Applied), "applied by the instrument");
    EXPECT_STREQ(toString(AntennaHeightMethod::Slant), "slant");
    EXPECT_STREQ(toString(GnssSolution::Float), "float");
}
