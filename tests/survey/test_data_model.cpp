#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <optional>
#include <string>
#include <vector>

#include "katana/math/numerics.hpp"
#include "katana/survey/data_model.hpp"
#include "katana/survey/network.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;

namespace {

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();
constexpr double kInf = std::numeric_limits<double>::infinity();

SurveyPoint makePoint(std::string id, double northing, double easting, double elevation = 0.0)
{
    SurveyPoint point;
    point.id = std::move(id);
    point.northing = northing;
    point.easting = easting;
    point.elevation = elevation;
    return point;
}

SurveyNetwork triangleNetwork()
{
    SurveyNetwork network;
    EXPECT_TRUE(network.addPoint(makePoint("A", 0.0, 0.0)).ok());
    EXPECT_TRUE(network.addPoint(makePoint("B", 100.0, 0.0)).ok());
    EXPECT_TRUE(network.addPoint(makePoint("C", 0.0, 100.0)).ok());
    return network;
}

DistanceObservation distance(std::string from, std::string to, double value, double sigma)
{
    DistanceObservation observation;
    observation.from = std::move(from);
    observation.to = std::move(to);
    observation.distance = value;
    observation.sigma = sigma;
    return observation;
}

} // namespace

// ---- points ----------------------------------------------------------------

TEST(SurveyNetworkPoints, AddFindAndKeepInsertionOrder)
{
    SurveyNetwork network;
    SurveyPoint point = makePoint("STN-7", 5000123.456, 500987.654, 312.5);
    point.code = "IP";
    point.description = "iron pin";
    point.metadata["instrument"] = "TS16";
    ASSERT_TRUE(network.addPoint(point).ok());
    ASSERT_TRUE(network.addPoint(makePoint("A", 1.0, 2.0)).ok());
    ASSERT_TRUE(network.addPoint(makePoint("M", 3.0, 4.0)).ok());

    // Insertion order, not alphabetical and not hash order.
    ASSERT_EQ(network.points().size(), 3u);
    EXPECT_EQ(network.points()[0].id, "STN-7");
    EXPECT_EQ(network.points()[1].id, "A");
    EXPECT_EQ(network.points()[2].id, "M");

    const auto found = network.point("STN-7");
    ASSERT_TRUE(found.ok());
    EXPECT_EQ(*found, point); // every field survives, including metadata
    EXPECT_EQ(found->position(), (Coordinate2{5000123.456, 500987.654}));
    EXPECT_EQ(network.pointIndex("M"), std::optional<std::size_t>(2));
    EXPECT_TRUE(network.containsPoint("A"));
    EXPECT_FALSE(network.containsPoint("a")); // ids are case sensitive
    EXPECT_EQ(network.point("missing").error().code, ErrorCode::NotFound);
    EXPECT_FALSE(network.pointIndex("missing").has_value());
}

TEST(SurveyNetworkPoints, RejectsDuplicateEmptyAndNonFinite)
{
    SurveyNetwork network;
    ASSERT_TRUE(network.addPoint(makePoint("A", 1.0, 2.0)).ok());
    EXPECT_EQ(network.addPoint(makePoint("A", 9.0, 9.0)).error().code, ErrorCode::AlreadyExists);
    EXPECT_EQ(network.addPoint(makePoint("", 1.0, 2.0)).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(network.addPoint(makePoint("B", kNaN, 2.0)).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(network.addPoint(makePoint("B", 1.0, kInf)).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(network.addPoint(makePoint("B", 1.0, 2.0, kNaN)).error().code,
              ErrorCode::InvalidArgument);

    // A rejected call leaves the network as it was.
    ASSERT_EQ(network.points().size(), 1u);
    EXPECT_DOUBLE_EQ(network.point("A")->northing, 1.0);
}

TEST(SurveyNetworkPoints, UpdateReplacesAndValidates)
{
    SurveyNetwork network = triangleNetwork();
    ASSERT_TRUE(network.updatePoint(makePoint("B", 100.25, -0.5, 7.0)).ok());
    EXPECT_DOUBLE_EQ(network.point("B")->northing, 100.25);
    EXPECT_DOUBLE_EQ(network.point("B")->elevation, 7.0);
    EXPECT_EQ(network.pointIndex("B"), std::optional<std::size_t>(1)); // position kept

    EXPECT_EQ(network.updatePoint(makePoint("Z", 0.0, 0.0)).error().code, ErrorCode::NotFound);
    EXPECT_EQ(network.updatePoint(makePoint("B", kNaN, 0.0)).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_DOUBLE_EQ(network.point("B")->northing, 100.25);
}

TEST(SurveyNetworkPoints, RemoveRebuildsTheIndexAndRefusesReferencedPoints)
{
    SurveyNetwork network = triangleNetwork();
    ASSERT_TRUE(network.addObservation(distance("B", "C", 141.42, 0.005)).ok());

    const auto referenced = network.removePoint("B");
    ASSERT_FALSE(referenced.ok());
    EXPECT_EQ(referenced.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(network.points().size(), 3u);

    // "A" is unreferenced; the points after it must still be found afterwards.
    ASSERT_TRUE(network.removePoint("A").ok());
    EXPECT_FALSE(network.containsPoint("A"));
    EXPECT_EQ(network.pointIndex("B"), std::optional<std::size_t>(0));
    EXPECT_EQ(network.pointIndex("C"), std::optional<std::size_t>(1));
    EXPECT_DOUBLE_EQ(network.point("C")->easting, 100.0);

    EXPECT_EQ(network.removePoint("A").error().code, ErrorCode::NotFound);

    ASSERT_TRUE(network.removeObservation(0).ok());
    EXPECT_TRUE(network.removePoint("B").ok());
}

// ---- observations ----------------------------------------------------------

TEST(SurveyNetworkObservations, AddReturnsIndexAndStoresEveryKind)
{
    SurveyNetwork network = triangleNetwork();
    const std::vector<Observation> observations = {
        distance("A", "B", 100.0, 0.003),
        HorizontalAngleObservation{"A", "B", "C", kHalfPi, 1e-5},
        VerticalAngleObservation{"A", "B", 0.1, 1e-5, 1.5, 1.8},
        ZenithAngleObservation{"A", "B", 1.5, 1e-5, 1.5, 1.8},
        AzimuthObservation{"A", "C", kHalfPi, 2e-5},
        GnssBaselineObservation{"A", "B", 100.0, 0.0, 1.0, 0.005, 0.005, 0.01},
        GnssPositionObservation{"C", 0.0, 100.0, 5.0, 0.01, 0.01, 0.02},
        LevelDifferenceObservation{"A", "B", 1.234, 0.001, 250.0},
    };
    for (std::size_t i = 0; i < observations.size(); ++i) {
        const auto index = network.addObservation(observations[i]);
        ASSERT_TRUE(index.ok()) << observationKindName(observations[i]);
        EXPECT_EQ(*index, i);
    }
    ASSERT_EQ(network.observations().size(), observations.size());
    EXPECT_EQ(network.observations(), observations);

    EXPECT_EQ(observationKindName(observations[0]), "horizontal distance");
    EXPECT_EQ(observationKindName(observations[7]), "level difference");
    EXPECT_EQ(referencedPoints(observations[1]), (std::vector<std::string>{"A", "B", "C"}));
    EXPECT_EQ(referencedPoints(observations[6]), (std::vector<std::string>{"C"}));
}

TEST(SurveyNetworkObservations, RejectsUnknownStations)
{
    SurveyNetwork network = triangleNetwork();
    const auto result = network.addObservation(distance("A", "NOPE", 10.0, 0.01));
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::NotFound);
    EXPECT_NE(result.error().context.find("NOPE"), std::string::npos); // names the culprit

    EXPECT_EQ(network.addObservation(HorizontalAngleObservation{"A", "B", "X", 1.0, 1e-5})
                  .error()
                  .code,
              ErrorCode::NotFound);
    EXPECT_TRUE(network.observations().empty());
}

TEST(SurveyNetworkObservations, RejectsNonPositiveAndNonFiniteSigma)
{
    SurveyNetwork network = triangleNetwork();
    for (const double sigma : {0.0, -0.01, kNaN, kInf}) {
        const auto result = network.addObservation(distance("A", "B", 100.0, sigma));
        ASSERT_FALSE(result.ok()) << sigma;
        EXPECT_EQ(result.error().code, ErrorCode::InvalidSurveyObservation) << sigma;
    }
    // Each of the three GNSS sigmas is checked on its own.
    EXPECT_FALSE(
        validateObservation(GnssBaselineObservation{"A", "B", 1, 1, 1, 0.01, 0.0, 0.01}).ok());
    EXPECT_FALSE(
        validateObservation(GnssPositionObservation{"A", 1, 1, 1, 0.01, 0.01, -1.0}).ok());
    EXPECT_TRUE(network.observations().empty());
}

TEST(SurveyNetworkObservations, RejectsBadValues)
{
    const auto code = [](const Observation& observation) {
        const auto status = validateObservation(observation);
        return status.ok() ? std::optional<ErrorCode>{} : std::optional(status.error().code);
    };
    const auto invalid = std::optional(ErrorCode::InvalidSurveyObservation);

    EXPECT_EQ(code(distance("A", "B", -5.0, 0.01)), invalid); // negative distance
    EXPECT_EQ(code(distance("A", "B", 0.0, 0.01)), invalid);  // zero length line
    EXPECT_EQ(code(distance("A", "B", kNaN, 0.01)), invalid);
    EXPECT_EQ(code(distance("A", "B", kInf, 0.01)), invalid);
    EXPECT_EQ(code(distance("A", "A", 5.0, 0.01)), invalid); // observes itself
    EXPECT_EQ(code(distance("", "B", 5.0, 0.01)), invalid);  // empty id

    EXPECT_EQ(code(HorizontalAngleObservation{"A", "B", "B", 1.0, 1e-5}), invalid);
    EXPECT_EQ(code(HorizontalAngleObservation{"A", "A", "B", 1.0, 1e-5}), invalid);
    EXPECT_EQ(code(HorizontalAngleObservation{"A", "B", "C", kNaN, 1e-5}), invalid);

    EXPECT_EQ(code(ZenithAngleObservation{"A", "B", -0.1, 1e-5, 0, 0}), invalid);
    EXPECT_EQ(code(ZenithAngleObservation{"A", "B", kPi + 0.1, 1e-5, 0, 0}), invalid);
    EXPECT_EQ(code(ZenithAngleObservation{"A", "B", kHalfPi, 1e-5, 0, 0}), std::nullopt);

    EXPECT_EQ(code(VerticalAngleObservation{"A", "B", kHalfPi + 0.1, 1e-5, 0, 0}), invalid);
    EXPECT_EQ(code(VerticalAngleObservation{"A", "B", -kHalfPi, 1e-5, 0, 0}), std::nullopt);
    EXPECT_EQ(code(VerticalAngleObservation{"A", "B", 0.1, 1e-5, kNaN, 0}), invalid);

    EXPECT_EQ(code(AzimuthObservation{"A", "B", kInf, 1e-5}), invalid);
    EXPECT_EQ(code(LevelDifferenceObservation{"A", "B", 1.0, 0.001, -1.0}), invalid);
    EXPECT_EQ(code(LevelDifferenceObservation{"A", "B", kNaN, 0.001, 0.0}), invalid);
    EXPECT_EQ(code(LevelDifferenceObservation{"A", "B", -1.0, 0.001, 0.0}), std::nullopt);
}

TEST(SurveyNetworkObservations, NormalisesAzimuthsAndAnglesOnInsertion)
{
    SurveyNetwork network = triangleNetwork();
    ASSERT_TRUE(network.addObservation(AzimuthObservation{"A", "B", -kHalfPi, 1e-5}).ok());
    ASSERT_TRUE(
        network.addObservation(HorizontalAngleObservation{"A", "B", "C", kTwoPi, 1e-5}).ok());
    ASSERT_TRUE(
        network.addObservation(HorizontalAngleObservation{"A", "B", "C", 5.0 * kPi, 1e-5}).ok());

    // -90° is 270°; a full turn is 0; 900° is 180°.
    EXPECT_NEAR(std::get<AzimuthObservation>(network.observations()[0]).azimuth, 1.5 * kPi, 1e-15);
    EXPECT_DOUBLE_EQ(std::get<HorizontalAngleObservation>(network.observations()[1]).angle, 0.0);
    EXPECT_NEAR(std::get<HorizontalAngleObservation>(network.observations()[2]).angle, kPi,
                1e-14);
}

TEST(SurveyNetworkObservations, RemoveShiftsIndicesAndChecksRange)
{
    SurveyNetwork network = triangleNetwork();
    ASSERT_TRUE(network.addObservation(distance("A", "B", 100.0, 0.01)).ok());
    ASSERT_TRUE(network.addObservation(distance("A", "C", 100.0, 0.02)).ok());
    ASSERT_TRUE(network.addObservation(distance("B", "C", 141.0, 0.03)).ok());

    ASSERT_TRUE(network.removeObservation(0).ok());
    ASSERT_EQ(network.observations().size(), 2u);
    EXPECT_DOUBLE_EQ(std::get<DistanceObservation>(network.observations()[0]).sigma, 0.02);
    EXPECT_EQ(network.removeObservation(2).error().code, ErrorCode::NotFound);
}

// ---- control and setups ----------------------------------------------------

TEST(SurveyNetworkControl, AddValidateRemove)
{
    SurveyNetwork network = triangleNetwork();
    ASSERT_TRUE(network.addControlPoint(ControlPoint::fixedHorizontal("A")).ok());
    ASSERT_TRUE(network.addControlPoint(ControlPoint::weightedHorizontal("B", 0.01, 0.02)).ok());

    const auto control = network.controlFor("B");
    ASSERT_TRUE(control.has_value());
    EXPECT_EQ(control->northing.constraint, ControlConstraint::Weighted);
    EXPECT_DOUBLE_EQ(control->easting.sigma, 0.02);
    EXPECT_EQ(control->elevation.constraint, ControlConstraint::Free);
    EXPECT_FALSE(network.controlFor("C").has_value());

    EXPECT_EQ(network.addControlPoint(ControlPoint::fixed3d("A")).error().code,
              ErrorCode::AlreadyExists);
    EXPECT_EQ(network.addControlPoint(ControlPoint::fixed3d("Z")).error().code,
              ErrorCode::NotFound);

    ControlPoint nothing;
    nothing.pointId = "C";
    EXPECT_EQ(network.addControlPoint(nothing).error().code, ErrorCode::InvalidArgument);
    for (const double sigma : {0.0, -0.01, kNaN}) {
        EXPECT_EQ(network.addControlPoint(ControlPoint::weightedVertical("C", sigma)).error().code,
                  ErrorCode::InvalidArgument)
            << sigma;
    }
    EXPECT_EQ(network.controlPoints().size(), 2u);

    // Control pins the point until it is removed.
    EXPECT_EQ(network.removePoint("A").error().code, ErrorCode::InvalidState);
    ASSERT_TRUE(network.removeControlPoint("A").ok());
    EXPECT_EQ(network.removeControlPoint("A").error().code, ErrorCode::NotFound);
    EXPECT_TRUE(network.removePoint("A").ok());
}

TEST(SurveyNetworkControl, FactoriesConstrainTheAdvertisedComponents)
{
    const ControlPoint vertical = ControlPoint::fixedVertical("BM");
    EXPECT_EQ(vertical.northing.constraint, ControlConstraint::Free);
    EXPECT_EQ(vertical.easting.constraint, ControlConstraint::Free);
    EXPECT_EQ(vertical.elevation.constraint, ControlConstraint::Fixed);

    const ControlPoint full = ControlPoint::fixed3d("P");
    EXPECT_EQ(full.northing.constraint, ControlConstraint::Fixed);
    EXPECT_EQ(full.easting.constraint, ControlConstraint::Fixed);
    EXPECT_EQ(full.elevation.constraint, ControlConstraint::Fixed);
}

TEST(SurveyNetworkStations, AddValidateRemove)
{
    SurveyNetwork network = triangleNetwork();
    ASSERT_TRUE(network.addStation(Station{"setup-1", "A", 1.532}).ok());
    ASSERT_TRUE(network.addStation(Station{"setup-2", "A", 1.601}).ok()); // re-occupation

    EXPECT_EQ(network.addStation(Station{"setup-1", "B", 1.5}).error().code,
              ErrorCode::AlreadyExists);
    EXPECT_EQ(network.addStation(Station{"", "B", 1.5}).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(network.addStation(Station{"s", "Z", 1.5}).error().code, ErrorCode::NotFound);
    EXPECT_EQ(network.addStation(Station{"s", "B", kNaN}).error().code,
              ErrorCode::InvalidArgument);
    ASSERT_EQ(network.stations().size(), 2u);
    EXPECT_DOUBLE_EQ(network.stations()[1].instrumentHeight, 1.601);

    EXPECT_EQ(network.removePoint("A").error().code, ErrorCode::InvalidState);
    ASSERT_TRUE(network.removeStation("setup-1").ok());
    ASSERT_TRUE(network.removeStation("setup-2").ok());
    EXPECT_EQ(network.removeStation("setup-2").error().code, ErrorCode::NotFound);
    EXPECT_TRUE(network.removePoint("A").ok());
}

TEST(SurveyNetworkValues, CopiesAreIndependent)
{
    SurveyNetwork original = triangleNetwork();
    SurveyNetwork copy = original;
    ASSERT_TRUE(copy.addPoint(makePoint("D", 5.0, 5.0)).ok());
    ASSERT_TRUE(copy.removePoint("A").ok());

    EXPECT_EQ(original.points().size(), 3u);
    EXPECT_TRUE(original.containsPoint("A"));
    EXPECT_FALSE(original.containsPoint("D"));
    EXPECT_EQ(copy.pointIndex("D"), std::optional<std::size_t>(2));
}
