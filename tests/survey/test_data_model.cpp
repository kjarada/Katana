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
    ASSERT_TRUE(network.point("B")->elevation.has_value());
    EXPECT_DOUBLE_EQ(*network.point("B")->elevation, 7.0);
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
//
// The trailing `{}` in the brace initialisations below is the SourceRecord: these
// observations were written here, not imported from anything, so they have no
// provenance. -Wmissing-field-initializers is on, and GCC applies it to
// designated initialisers too, so every field has to appear.

TEST(SurveyNetworkObservations, AddReturnsIndexAndStoresEveryKind)
{
    SurveyNetwork network = triangleNetwork();
    const std::vector<Observation> observations = {
        distance("A", "B", 100.0, 0.003),
        HorizontalAngleObservation{"A", "B", "C", kHalfPi, 1e-5, {}},
        VerticalAngleObservation{"A", "B", 0.1, 1e-5, 1.5, 1.8, {}},
        ZenithAngleObservation{"A", "B", 1.5, 1e-5, 1.5, 1.8, {}},
        AzimuthObservation{"A", "C", kHalfPi, 2e-5, {}},
        GnssBaselineObservation{"A", "B", 100.0, 0.0, 1.0, 0.005, 0.005, 0.01, {}},
        GnssPositionObservation{"C", 0.0, 100.0, 5.0, 0.01, 0.01, 0.02, {}},
        LevelDifferenceObservation{"A", "B", 1.234, 0.001, 250.0, {}},
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

    EXPECT_EQ(network.addObservation(HorizontalAngleObservation{"A", "B", "X", 1.0, 1e-5, {}})
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
        validateObservation(GnssBaselineObservation{"A", "B", 1, 1, 1, 0.01, 0.0, 0.01, {}}).ok());
    EXPECT_FALSE(
        validateObservation(GnssPositionObservation{"A", 1, 1, 1, 0.01, 0.01, -1.0, {}}).ok());
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

    EXPECT_EQ(code(HorizontalAngleObservation{"A", "B", "B", 1.0, 1e-5, {}}), invalid);
    EXPECT_EQ(code(HorizontalAngleObservation{"A", "A", "B", 1.0, 1e-5, {}}), invalid);
    EXPECT_EQ(code(HorizontalAngleObservation{"A", "B", "C", kNaN, 1e-5, {}}), invalid);

    EXPECT_EQ(code(ZenithAngleObservation{"A", "B", -0.1, 1e-5, 0, 0, {}}), invalid);
    EXPECT_EQ(code(ZenithAngleObservation{"A", "B", kPi + 0.1, 1e-5, 0, 0, {}}), invalid);
    EXPECT_EQ(code(ZenithAngleObservation{"A", "B", kHalfPi, 1e-5, 0, 0, {}}), std::nullopt);

    EXPECT_EQ(code(VerticalAngleObservation{"A", "B", kHalfPi + 0.1, 1e-5, 0, 0, {}}), invalid);
    EXPECT_EQ(code(VerticalAngleObservation{"A", "B", -kHalfPi, 1e-5, 0, 0, {}}), std::nullopt);
    EXPECT_EQ(code(VerticalAngleObservation{"A", "B", 0.1, 1e-5, kNaN, 0, {}}), invalid);

    EXPECT_EQ(code(AzimuthObservation{"A", "B", kInf, 1e-5, {}}), invalid);
    EXPECT_EQ(code(LevelDifferenceObservation{"A", "B", 1.0, 0.001, -1.0, {}}), invalid);
    EXPECT_EQ(code(LevelDifferenceObservation{"A", "B", kNaN, 0.001, 0.0, {}}), invalid);
    EXPECT_EQ(code(LevelDifferenceObservation{"A", "B", -1.0, 0.001, 0.0, {}}), std::nullopt);
}

TEST(SurveyNetworkObservations, NormalisesAzimuthsAndAnglesOnInsertion)
{
    SurveyNetwork network = triangleNetwork();
    ASSERT_TRUE(network.addObservation(AzimuthObservation{"A", "B", -kHalfPi, 1e-5, {}}).ok());
    ASSERT_TRUE(
        network.addObservation(HorizontalAngleObservation{"A", "B", "C", kTwoPi, 1e-5, {}}).ok());
    ASSERT_TRUE(
        network.addObservation(HorizontalAngleObservation{"A", "B", "C", 5.0 * kPi, 1e-5, {}}).ok());

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

// ---- provenance --------------------------------------------------------------

TEST(SurveyProvenance, ASuppliedPathIsReducedToItsNameSoAFileCannotChooseWhatIsRead)
{
    // The property under test is the security rule stated at sourceFileName():
    // whatever a file says, what comes back is a NAME and can never be walked.
    EXPECT_EQ(sourceFileName("job.gsi"), "job.gsi");
    EXPECT_EQ(sourceFileName("..\\..\\..\\Windows\\win.ini"), "win.ini");
    EXPECT_EQ(sourceFileName("../../etc/passwd"), "passwd");
    EXPECT_EQ(sourceFileName("/etc/passwd"), "passwd");
    EXPECT_EQ(sourceFileName("C:job.gsi"), "job.gsi");
    // An NTFS alternate data stream: the suffix is a plain name, and the part
    // that made it a path is gone.
    EXPECT_EQ(sourceFileName("job.gsi:hidden"), "hidden");

    // Nothing that is not a name comes back as one.
    EXPECT_EQ(sourceFileName(""), "");
    EXPECT_EQ(sourceFileName("."), "");
    EXPECT_EQ(sourceFileName(".."), "");
    EXPECT_EQ(sourceFileName("scans/"), "");
    EXPECT_EQ(sourceFileName("C:\\"), "");
}

TEST(SurveyProvenance, ASourceRecordNamesTheFileAndTheRecordWithinIt)
{
    SourceRecord source;
    source.manufacturer = "Leica";
    source.format = "GSI-16";
    source.fileName = "job.gsi";
    source.recordNumber = 412;

    EXPECT_TRUE(source.known());
    EXPECT_EQ(describeSource(source), "job.gsi record 412 (Leica GSI-16)");

    source.formatVersion = "16 byte words";
    EXPECT_EQ(describeSource(source), "job.gsi record 412 (Leica GSI-16 16 byte words)");
}

TEST(SurveyProvenance, ADefaultSourceRecordSaysTheValueWasNotImportedRatherThanNamingNothing)
{
    const SourceRecord none;
    EXPECT_FALSE(none.known());
    EXPECT_EQ(describeSource(none), "unknown source");

    SourceRecord recordOnly;
    recordOnly.recordNumber = 7; // a record number alone identifies nothing
    EXPECT_FALSE(recordOnly.known());
}

TEST(SurveyProvenance, ACalculatedCoordinateIsDistinguishableFromAnObservedOneWithoutMetadata)
{
    SurveyPoint observed = makePoint("STN-1", 100.0, 200.0, 10.0);
    observed.coordinateSource = CoordinateSource::FieldObserved;
    SurveyPoint calculated = observed;
    calculated.coordinateSource = CoordinateSource::Calculated;

    // Same coordinates, different fact about them - and the difference survives
    // value comparison, which a metadata convention would not guarantee.
    EXPECT_DOUBLE_EQ(observed.northing, calculated.northing);
    EXPECT_NE(observed, calculated);
    EXPECT_STRNE(toString(observed.coordinateSource), toString(calculated.coordinateSource));
    EXPECT_EQ(std::string(toString(CoordinateSource::Unknown)), "unknown");
    EXPECT_EQ(makePoint("P", 0.0, 0.0).coordinateSource, CoordinateSource::Unknown);
}

TEST(SurveyProvenance, AnObservationCarriesItsSourceWhicheverKindItIs)
{
    SourceRecord source;
    source.format = "GSI-16";
    source.fileName = "job.gsi";
    source.recordNumber = 12;

    Observation distanceObservation = distance("A", "B", 100.0, 0.005);
    setObservationSource(distanceObservation, source);
    EXPECT_EQ(observationSource(distanceObservation), source);

    GnssPositionObservation position;
    position.point = "A";
    position.sigmaNorthing = 0.01;
    position.sigmaEasting = 0.01;
    position.sigmaElevation = 0.02;
    Observation gnss = position;
    EXPECT_FALSE(observationSource(gnss).known());
    setObservationSource(gnss, source);
    EXPECT_EQ(observationSource(gnss).recordNumber, 12u);

    // Provenance is not part of what makes an observation valid: an observation
    // this program computed has none, and must still validate.
    EXPECT_TRUE(validateObservation(distanceObservation).ok());
}

// ---- declared coordinate system and units --------------------------------------

TEST(SurveyDeclaredCoordinateSystem, ASystemThatTheSourceDidNotStateStaysUnknown)
{
    const DeclaredCoordinateSystem nothingSaid;
    EXPECT_TRUE(nothingSaid.unknown);
    EXPECT_TRUE(nothingSaid.name.empty());
    EXPECT_EQ(nothingSaid.epsgCode, 0);

    // A blank declaration declares nothing, and a code of 0 is the field's own
    // "none given" marker rather than an EPSG code.
    EXPECT_TRUE(DeclaredCoordinateSystem::named("").unknown);
    EXPECT_TRUE(DeclaredCoordinateSystem::epsg(0).unknown);
    EXPECT_TRUE(DeclaredCoordinateSystem::epsg(-1).unknown);
}

TEST(SurveyDeclaredCoordinateSystem, WhatTheSourceDeclaredIsRecordedAndNotResolved)
{
    const DeclaredCoordinateSystem byName = DeclaredCoordinateSystem::named("MGA Zone 56");
    EXPECT_FALSE(byName.unknown);
    EXPECT_EQ(byName.name, "MGA Zone 56");
    EXPECT_EQ(byName.epsgCode, 0); // no code was stated, and none is invented

    const DeclaredCoordinateSystem byCode = DeclaredCoordinateSystem::epsg(28356, "MGA Zone 56");
    EXPECT_FALSE(byCode.unknown);
    EXPECT_EQ(byCode.epsgCode, 28356);
    EXPECT_EQ(byCode.name, "MGA Zone 56");
}

TEST(SurveyDeclaredUnits, UnitsAreRecordedAsDeclaredWhileTheModelStaysMetresAndRadians)
{
    const DeclaredUnits nothingSaid;
    EXPECT_EQ(nothingSaid.linear, LinearUnit::Unknown);
    EXPECT_EQ(nothingSaid.angular, AngularUnit::Unknown);

    EXPECT_EQ(std::string(toString(LinearUnit::UsSurveyFeet)), "US survey feet");
    EXPECT_EQ(std::string(toString(AngularUnit::Gons)), "gons");
    EXPECT_EQ(std::string(toString(AngularUnit::DegreesMinutesSeconds)),
              "degrees, minutes and seconds");
}

// ---- stations, features and projects -------------------------------------------

TEST(SurveyStations, ASetupWithNoRecordedOrientationIsNotASetupOrientedOnZero)
{
    SurveyStation unrecorded;
    unrecorded.setup = Station{"setup-1", "A", 1.532};
    SurveyStation orientedOnZero = unrecorded;
    orientedOnZero.backsightAzimuth = 0.0;

    EXPECT_FALSE(unrecorded.backsightAzimuth.has_value());
    EXPECT_NE(unrecorded, orientedOnZero);
}

namespace {

// Two points and one setup over the first of them, backsighting the second.
SurveyProject twoPointProject()
{
    SurveyProject project;
    project.points.push_back(makePoint("A", 0.0, 0.0, 10.0));
    project.points.push_back(makePoint("B", 100.0, 0.0, 12.0));

    SurveyStation station;
    station.setup = Station{"setup-1", "A", 1.532};
    station.backsightPointId = "B";
    station.backsightAzimuth = 0.0;
    station.observations.push_back(distance("A", "B", 100.0, 0.005));
    project.stations.push_back(std::move(station));
    return project;
}

} // namespace

TEST(SurveyProjects, AProjectWhoseReferencesAllResolveValidates)
{
    SurveyProject project = twoPointProject();
    SurveyFeature kerb;
    kerb.code = "KB";
    kerb.pointIds = {"A", "B"};
    project.features.push_back(kerb);
    project.observations.push_back(distance("B", "A", 100.0, 0.005));

    EXPECT_TRUE(validateProject(project).ok());
}

TEST(SurveyProjects, APointWithNoIdOrADuplicateOneIsRejected)
{
    SurveyProject empty;
    empty.points.push_back(makePoint("", 0.0, 0.0));
    EXPECT_EQ(validateProject(empty).error().code, ErrorCode::InvalidArgument);

    SurveyProject twice;
    twice.points.push_back(makePoint("A", 0.0, 0.0));
    twice.points.push_back(makePoint("A", 1.0, 1.0));
    EXPECT_EQ(validateProject(twice).error().code, ErrorCode::AlreadyExists);
}

TEST(SurveyProjects, APointWithANonFiniteCoordinateIsRejected)
{
    SurveyProject project;
    project.points.push_back(makePoint("A", kNaN, 0.0));
    EXPECT_EQ(validateProject(project).error().code, ErrorCode::InvalidArgument);

    SurveyProject infinite;
    infinite.points.push_back(makePoint("A", 0.0, 0.0, kInf));
    EXPECT_EQ(validateProject(infinite).error().code, ErrorCode::InvalidArgument);
}

TEST(SurveyProjects, ASetupOverAPointThatIsNotInTheProjectIsRejected)
{
    SurveyProject project = twoPointProject();
    project.stations.front().setup.pointId = "Z";
    const katana::core::Status status = validateProject(project);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::NotFound);
    EXPECT_NE(status.error().message.find("'Z'"), std::string::npos);
}

TEST(SurveyProjects, ABacksightPointThatIsNotInTheProjectIsRejected)
{
    SurveyProject project = twoPointProject();
    project.stations.front().backsightPointId = "Z";
    EXPECT_EQ(validateProject(project).error().code, ErrorCode::NotFound);

    // No backsight at all is not an error: a free setup records none.
    project.stations.front().backsightPointId.clear();
    EXPECT_TRUE(validateProject(project).ok());
}

TEST(SurveyProjects, AStatedBacksightAzimuthThatIsNotFiniteIsRejectedAndAFiniteOneIsKeptApart)
{
    // The azimuth a file states for the backsight and the circle set on it
    // are two values: a circle zeroed on a backsight 123 degrees away is the
    // usual case, and the two must not compare equal as one field.
    SurveyProject project = twoPointProject();
    project.stations.front().statedBacksightAzimuth = 123.0 * kPi / 180.0;
    EXPECT_TRUE(validateProject(project).ok());
    EXPECT_NE(project.stations.front(), twoPointProject().stations.front());
    for (const double bad : {kNaN, kInf}) {
        project.stations.front().statedBacksightAzimuth = bad;
        const katana::core::Status status = validateProject(project);
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
        EXPECT_NE(status.error().message.find("azimuth the file states"), std::string::npos)
            << status.error().message;
    }
}

TEST(SurveyProjects, DuplicateOrUnnamedSetupsAreRejected)
{
    SurveyProject project = twoPointProject();
    project.stations.push_back(project.stations.front());
    EXPECT_EQ(validateProject(project).error().code, ErrorCode::AlreadyExists);

    project.stations.back().setup.id.clear();
    EXPECT_EQ(validateProject(project).error().code, ErrorCode::InvalidArgument);
}

TEST(SurveyProjects, AnObservationTakenFromASetupIsValidatedLikeAnyOther)
{
    SurveyProject badValue = twoPointProject();
    badValue.stations.front().observations.push_back(distance("A", "B", 100.0, 0.0));
    EXPECT_EQ(validateProject(badValue).error().code, ErrorCode::InvalidSurveyObservation);

    SurveyProject unknownPoint = twoPointProject();
    unknownPoint.stations.front().observations.push_back(distance("A", "Z", 100.0, 0.005));
    EXPECT_EQ(validateProject(unknownPoint).error().code, ErrorCode::NotFound);

    SurveyProject loose = twoPointProject();
    loose.observations.push_back(distance("A", "Z", 100.0, 0.005));
    EXPECT_EQ(validateProject(loose).error().code, ErrorCode::NotFound);
}

TEST(SurveyProjects, AFeatureMustNameAtLeastOnePointAndOnlyPointsThatExist)
{
    SurveyProject noPoints = twoPointProject();
    SurveyFeature empty;
    empty.code = "KB";
    noPoints.features.push_back(empty);
    EXPECT_EQ(validateProject(noPoints).error().code, ErrorCode::InvalidArgument);

    SurveyProject dangling = twoPointProject();
    SurveyFeature kerb;
    kerb.name = "KB1";
    kerb.code = "KB";
    kerb.pointIds = {"A", "Z"};
    dangling.features.push_back(kerb);
    const katana::core::Status status = validateProject(dangling);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::NotFound);
    EXPECT_NE(status.error().context.find("KB1"), std::string::npos);
}

TEST(SurveyProjects, ATraverseIsNotCheckedAgainstTheProjectPointsBecauseItIsSelfContained)
{
    // A Traverse carries its own start and end coordinates, so its station ids
    // are names of its own. Requiring them to be project points would reject a
    // perfectly good traverse read from a file that lists no coordinates.
    SurveyProject project = twoPointProject();
    Traverse traverse;
    traverse.name = "TR1";
    traverse.kind = TraverseKind::Open;
    traverse.startAzimuth = 0.0;
    traverse.setups.push_back(TraverseSetup{"not-a-project-point", 0.0, 50.0});
    traverse.endStationId = "also-not-one";
    project.traverses.push_back(std::move(traverse));

    EXPECT_TRUE(validateProject(project).ok());
}
