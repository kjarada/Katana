// The RW5 reader (src/katana_surveyio/topcon_rw5.cpp).
//
// Fixtures are hand-built from the TDS Raw Data Record Specification (Survey
// Pro 3.6) and the Carlson SurvCE RW5 format 3.03; gnss.rw5 is made of the
// SurvCE document's own sample records. Every expected value is worked below
// from the record text, not taken from the reader's output.

#include <gtest/gtest.h>

#include <chrono>
#include <cmath>
#include <iostream>
#include <numbers>
#include <string>

#include "katana/core/text.hpp"
#include "surveyio/topcon_test_support.hpp"

using namespace katana::surveyio;
namespace survey = katana::survey;
using topcon_test::dms;
using topcon_test::fixture;
using topcon_test::observationsTo;

namespace {

constexpr std::string_view kRw5 = "tds-rw5";
constexpr double kAngleTolerance = 1e-12; // radians: far below 0.001"
constexpr double kPi = std::numbers::pi;

ReadResult readFixture(std::string_view name)
{
    const std::string bytes = fixture(std::filesystem::path("rw5") / name);
    auto result = topcon_test::read(kRw5, bytes, name);
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(result).value() : ReadResult{};
}

} // namespace

TEST(TopconRw5, IsRegisteredAsAnImportableFormatThatReadsSetupsObservationsAndGnss)
{
    const auto format = formatRegistry().find(kRw5);
    ASSERT_TRUE(format.ok());
    EXPECT_TRUE(format->canImport);
    EXPECT_FALSE(format->canExport);
    EXPECT_EQ(format->parserVersion, "1.0");
    EXPECT_EQ(format->extensions, std::vector<std::string>{"rw5"});
    EXPECT_TRUE(format->reads.observations && format->reads.stations && format->reads.points &&
                format->reads.features && format->reads.instrumentSettings && format->reads.gnss);
    EXPECT_NE(formatRegistry().reader(kRw5), nullptr);
}

TEST(TopconRw5, PackedDegreesAreReadAsDegreesMinutesAndSecondsNotAsADecimal)
{
    const ReadResult result = readFixture("setup_metres.rw5");
    ASSERT_EQ(result.project.stations.size(), 1u);
    const auto directions = observationsTo<survey::HorizontalDirectionObservation>(
        result.project.stations[0], "10");
    ASSERT_EQ(directions.size(), 3u); // SS, FD and FR to point 10
    // AR90.3333 is 90 33' 33" (= 326013"), not 90.3333 degrees.
    EXPECT_NEAR(directions[0].direction, dms(90, 33, 33), kAngleTolerance);
    // FR AR270.3336 is 270 33' 36".
    EXPECT_NEAR(directions[2].direction, dms(270, 33, 36), kAngleTolerance);
}

TEST(TopconRw5, AnOccupyRecordBecomesOneSetupWithCoordinatesHeightsAndBacksight)
{
    const ReadResult result = readFixture("setup_metres.rw5");
    const survey::SurveyProject& project = result.project;
    EXPECT_EQ(project.name, "KATANA TEST");
    EXPECT_EQ(project.units.linear, survey::LinearUnit::Metres);
    EXPECT_EQ(project.units.angular, survey::AngularUnit::DegreesMinutesSeconds);
    ASSERT_EQ(project.stations.size(), 1u);
    const survey::SurveyStation& station = project.stations[0];
    EXPECT_EQ(station.setup.id, "1");
    EXPECT_EQ(station.setup.pointId, "1");
    // LS,HI1.550 came after the OC record but before any shot.
    EXPECT_DOUBLE_EQ(station.setup.instrumentHeight, 1.55);
    EXPECT_EQ(station.backsightPointId, "2");
    ASSERT_TRUE(station.backsightAzimuth.has_value());
    EXPECT_DOUBLE_EQ(*station.backsightAzimuth, 0.0); // BC0.0000
    EXPECT_EQ(station.metadata.at("backsight azimuth (radians)"), "0"); // BS0.0000
    EXPECT_EQ(station.metadata.at("notes"), "Setup check ok");

    const survey::SurveyPoint* occupied = topcon_test::point(project, "1");
    ASSERT_NE(occupied, nullptr);
    EXPECT_DOUBLE_EQ(occupied->northing, 1000.0);
    EXPECT_DOUBLE_EQ(occupied->easting, 2000.0);
    ASSERT_TRUE(occupied->elevation.has_value());
    EXPECT_DOUBLE_EQ(*occupied->elevation, 50.0);
    EXPECT_EQ(occupied->source.recordNumber, 3u); // the SP record, first to position it
    // Shot targets with no coordinates stay unpositioned.
    EXPECT_NE(topcon_test::unpositioned(project, "10"), nullptr);
    EXPECT_NE(topcon_test::unpositioned(project, "12"), nullptr);
    EXPECT_EQ(topcon_test::point(project, "10"), nullptr);
}

TEST(TopconRw5, ASideshotGivesDirectionZenithAndSlopeDistanceSharingOnePointing)
{
    const ReadResult result = readFixture("setup_metres.rw5");
    const survey::SurveyStation& station = result.project.stations.at(0);
    const auto directions = observationsTo<survey::HorizontalDirectionObservation>(station, "10");
    const auto zeniths = observationsTo<survey::ZenithAngleObservation>(station, "10");
    const auto distances = observationsTo<survey::DistanceObservation>(station, "10");
    ASSERT_FALSE(directions.empty());
    ASSERT_FALSE(zeniths.empty());
    ASSERT_FALSE(distances.empty());
    // SS,OP1,FP10 is the second shot of the setup: pointing 2, face left
    // (ZE91.1015 is below 180 degrees).
    const survey::Pointing expected{2, survey::Face::Left};
    EXPECT_EQ(directions[0].pointing, expected);
    EXPECT_EQ(zeniths[0].pointing, expected);
    EXPECT_EQ(distances[0].pointing, expected);
    EXPECT_NEAR(zeniths[0].angle, dms(91, 10, 15), kAngleTolerance);
    EXPECT_DOUBLE_EQ(distances[0].distance, 25.55);
    EXPECT_EQ(distances[0].kind, survey::DistanceKind::Slope);
    EXPECT_DOUBLE_EQ(distances[0].instrumentHeight, 1.55);
    EXPECT_DOUBLE_EQ(distances[0].targetHeight, 1.8);
    EXPECT_DOUBLE_EQ(zeniths[0].targetHeight, 1.8);
    // Default precision: 3" and 2 mm + 2 ppm -> hypot(0.002, 2e-6 * 25.55).
    EXPECT_DOUBLE_EQ(directions[0].sigma, 3.0 * kPi / 648000.0);
    EXPECT_DOUBLE_EQ(distances[0].sigma, std::hypot(0.002, 2e-6 * 25.55));
    EXPECT_EQ(distances[0].source.recordNumber, 9u);
    EXPECT_EQ(distances[0].source.fileName, "setup_metres.rw5");
}

TEST(TopconRw5, DirectAndReverseRecordsCarryTheirFaceAndReverseZenithsAreFolded)
{
    const ReadResult result = readFixture("setup_metres.rw5");
    const survey::SurveyStation& station = result.project.stations.at(0);
    const auto zeniths = observationsTo<survey::ZenithAngleObservation>(station, "2");
    ASSERT_EQ(zeniths.size(), 3u); // SS, BD, BR
    EXPECT_EQ(zeniths[1].pointing, (survey::Pointing{5, survey::Face::Left}));  // BD
    EXPECT_EQ(zeniths[2].pointing, (survey::Pointing{6, survey::Face::Right})); // BR
    // BR ZE270.3458: 360 00' 00" - 270 34' 58" = 89 25' 02" (1296000" - 974098").
    EXPECT_NEAR(zeniths[2].angle, dms(89, 25, 2), kAngleTolerance);
    // SS to 11 at ZE271.3000 is a single face-right shot: 88 30' 00".
    const auto eleven = observationsTo<survey::ZenithAngleObservation>(station, "11");
    ASSERT_EQ(eleven.size(), 1u);
    EXPECT_EQ(eleven[0].pointing.face, survey::Face::Right);
    EXPECT_NEAR(eleven[0].angle, dms(88, 30, 0), kAngleTolerance);
    // FR ZE268.4952: 360 - 268 49' 52" = 91 10' 08".
    const auto ten = observationsTo<survey::ZenithAngleObservation>(station, "10");
    ASSERT_EQ(ten.size(), 3u);
    EXPECT_NEAR(ten[2].angle, dms(91, 10, 8), kAngleTolerance);
    EXPECT_EQ(ten[2].pointing.face, survey::Face::Right);
}

TEST(TopconRw5, AZeroSlopeDistanceIsAWarningAndNoDistanceNotAZeroLengthObservation)
{
    const ReadResult result = readFixture("setup_metres.rw5");
    const survey::SurveyStation& station = result.project.stations.at(0);
    EXPECT_TRUE(observationsTo<survey::DistanceObservation>(station, "12").empty());
    EXPECT_EQ(observationsTo<survey::ZenithAngleObservation>(station, "12").size(), 1u);
    EXPECT_TRUE(topcon_test::anyWarningContains(result, 12, "no distance measured"));
    // LS,HR2.000 (record 11) is the target height from then on.
    EXPECT_DOUBLE_EQ(observationsTo<survey::ZenithAngleObservation>(station, "12")[0].targetHeight,
                     2.0);
}

TEST(TopconRw5, TheModeRecordSetsScaleCurvatureAndThePrismConstantInMetres)
{
    const ReadResult result = readFixture("setup_metres.rw5");
    const survey::InstrumentSettings& settings = result.project.stations.at(0).instrument;
    // SF0.99960000, which the collector applies when it computes, not to a
    // recorded distance.
    ASSERT_TRUE(settings.scaleFactor.has_value());
    EXPECT_DOUBLE_EQ(*settings.scaleFactor, 0.9996);
    EXPECT_EQ(settings.scaleFactorState, survey::CorrectionState::NotApplied);
    // EC1: a recorded HD or CE may include it; raw readings never do.
    EXPECT_EQ(settings.curvatureRefractionState, survey::CorrectionState::Unknown);
    // EO1.0 inch = 0.0254 m, state not stated by the format.
    ASSERT_TRUE(settings.prismConstant.has_value());
    EXPECT_DOUBLE_EQ(*settings.prismConstant, 0.0254);
    EXPECT_EQ(settings.prismConstantState, survey::CorrectionState::Unknown);
    EXPECT_EQ(settings.atmosphericPpmState, survey::CorrectionState::Unknown);
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(result, "no atmospheric settings"));
}

TEST(TopconRw5, DescriptionsBecomeCodesAndStringPointsIntoFeaturesInObservationOrder)
{
    const ReadResult result = readFixture("setup_metres.rw5");
    const survey::SurveyProject& project = result.project;
    const survey::SurveyFeature* kerb = topcon_test::feature(project, "KB1");
    ASSERT_NE(kerb, nullptr);
    // 10 then 11; the FD and FR shots to 10 add no second vertex.
    EXPECT_EQ(kerb->pointIds, (std::vector<std::string>{"10", "11"}));
    const survey::SurveyFeature* control = topcon_test::feature(project, "CP");
    ASSERT_NE(control, nullptr);
    EXPECT_EQ(control->pointIds, (std::vector<std::string>{"1", "2"}));
    const survey::UnpositionedPoint* tree = topcon_test::unpositioned(project, "12");
    ASSERT_NE(tree, nullptr);
    EXPECT_EQ(tree->code, "TREE");
    // The note runs to the end of the line, commas and all.
    EXPECT_EQ(tree->description, "TREE oak, 30 cm");
    EXPECT_EQ(topcon_test::point(project, "1")->description, "CP control");
}

TEST(TopconRw5, FeetAndGradsAreConvertedAndSouthAzimuthsTurnedThroughHalfACircle)
{
    const ReadResult result = readFixture("feet_grads.rw5");
    const survey::SurveyProject& project = result.project;
    EXPECT_EQ(project.units.linear, survey::LinearUnit::Feet);
    EXPECT_EQ(project.units.angular, survey::AngularUnit::Gons);
    const survey::SurveyPoint* occupied = topcon_test::point(project, "100");
    ASSERT_NE(occupied, nullptr);
    EXPECT_DOUBLE_EQ(occupied->northing, 5000.0 * 0.3048); // 1524 m
    EXPECT_DOUBLE_EQ(*occupied->elevation, 100.0 * 0.3048);
    const survey::SurveyStation& station = project.stations.at(0);
    EXPECT_DOUBLE_EQ(station.setup.instrumentHeight, 5.0 * 0.3048);
    // BS250 gon, south azimuth: 250 + 200 = 450 gon = 50 gon = pi / 4.
    const auto backsight = katana::core::parseFiniteDouble(
        station.metadata.at("backsight azimuth (radians)"));
    ASSERT_TRUE(backsight.has_value());
    EXPECT_NEAR(*backsight, 50.0 * kPi / 200.0, kAngleTolerance);
    const auto direction = observationsTo<survey::HorizontalDirectionObservation>(station, "102");
    ASSERT_EQ(direction.size(), 1u);
    EXPECT_NEAR(direction[0].direction, kPi / 2.0, kAngleTolerance); // AR100 gon
    const auto distance = observationsTo<survey::DistanceObservation>(station, "102");
    ASSERT_EQ(distance.size(), 1u);
    EXPECT_DOUBLE_EQ(distance[0].distance, 100.0 * 0.3048); // 30.48 m
    EXPECT_DOUBLE_EQ(distance[0].targetHeight, 6.0 * 0.3048);
    // TR with AZ50 gon (south) is an azimuth of 250 gon = 1.25 pi, VA10 gon
    // an elevation of pi / 20, and HD200 ft a horizontal 60.96 m.
    const auto azimuth = observationsTo<survey::AzimuthObservation>(station, "103");
    ASSERT_EQ(azimuth.size(), 1u);
    EXPECT_NEAR(azimuth[0].azimuth, 1.25 * kPi, kAngleTolerance);
    const auto vertical = observationsTo<survey::VerticalAngleObservation>(station, "103");
    ASSERT_EQ(vertical.size(), 1u);
    EXPECT_NEAR(vertical[0].angle, kPi / 20.0, kAngleTolerance);
    const auto flat = observationsTo<survey::DistanceObservation>(station, "103");
    ASSERT_EQ(flat.size(), 1u);
    EXPECT_EQ(flat[0].kind, survey::DistanceKind::Horizontal);
    EXPECT_DOUBLE_EQ(flat[0].distance, 200.0 * 0.3048);
}

TEST(TopconRw5, UsSurveyFeetAreConvertedWithTheUsSurveyFoot)
{
    const std::string bytes = "MO,AD0,UN2,SF1.0,EC0,EO0.0,AU0\n"
                              "SP,PN7,N 3937.000,E 7874.000,EL39.370\n";
    const auto result = topcon_test::read(kRw5, bytes, "us.rw5");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyPoint* stored = topcon_test::point(result->project, "7");
    ASSERT_NE(stored, nullptr);
    // 1 US survey foot = 1200/3937 m, so 3937 ft = 1200 m exactly.
    EXPECT_NEAR(stored->northing, 1200.0, 1e-9);
    EXPECT_NEAR(stored->easting, 2400.0, 1e-9);
    EXPECT_NEAR(*stored->elevation, 12.0, 1e-9);
    EXPECT_EQ(result->project.units.linear, survey::LinearUnit::UsSurveyFeet);
}

TEST(TopconRw5, AFileWithNoModeRecordIsRefusedAtItsFirstDistanceRatherThanGuessed)
{
    const std::string bytes = "JB,NMX\nOC,OP1,N 10.0,E 20.0,EL1.0\n";
    const auto result = topcon_test::read(kRw5, bytes, "nomode.rw5");
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, katana::core::ErrorCode::FileImportFailure);
    EXPECT_NE(result.error().message.find("record 2"), std::string::npos);
    EXPECT_NE(result.error().message.find("feet"), std::string::npos);
}

TEST(TopconRw5, APointStoredTwiceKeepsItsFirstCoordinatesAndWarnsWithBoth)
{
    const ReadResult result = readFixture("journal.rw5");
    const survey::SurveyPoint* first = topcon_test::point(result.project, "1");
    ASSERT_NE(first, nullptr);
    EXPECT_DOUBLE_EQ(first->northing, 100.0);
    EXPECT_EQ(first->metadata.at("coordinates restated at record 3"), "N 100.005, E 200, elevation 10");
    EXPECT_TRUE(topcon_test::anyWarningContains(result, 3, "record 2"));
}

TEST(TopconRw5, ABacksightTakenAgainAfterShotsStartsANewSetupOnTheSamePoint)
{
    const ReadResult result = readFixture("journal.rw5");
    ASSERT_EQ(result.project.stations.size(), 2u);
    EXPECT_EQ(result.project.stations[0].setup.id, "1");
    EXPECT_EQ(result.project.stations[1].setup.id, "1 (2)");
    EXPECT_EQ(result.project.stations[1].setup.pointId, "1");
    // BC90.0000 on the second.
    EXPECT_NEAR(*result.project.stations[1].backsightAzimuth, kPi / 2.0, kAngleTolerance);
    EXPECT_EQ(observationsTo<survey::HorizontalDirectionObservation>(result.project.stations[1],
                                                                     "4")
                  .size(),
              1u);
}

TEST(TopconRw5, AChangeInElevationBecomesAGroundToGroundLevelDifference)
{
    const ReadResult result = readFixture("journal.rw5");
    const auto differences =
        observationsTo<survey::LevelDifferenceObservation>(result.project.stations.at(1), "5");
    ASSERT_EQ(differences.size(), 1u);
    // CE1.250 with no LS record: HI 0 and HR 0, so 1.25 + 0 - 0.
    EXPECT_DOUBLE_EQ(differences[0].heightDifference, 1.25);
    EXPECT_DOUBLE_EQ(differences[0].length, 30.0);
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(result, "no instrument heights"));
}

TEST(TopconRw5, RecordsKatanaDoesNotImportAreWarnedByNumberAndCounted)
{
    const ReadResult result = readFixture("journal.rw5");
    EXPECT_TRUE(topcon_test::anyWarningContains(result, 9, "SL record not imported"));
    EXPECT_TRUE(topcon_test::anyWarningContains(result, 10, "not a record type"));
    EXPECT_EQ(result.recordsSkipped, 2u);
    EXPECT_EQ(result.recordsRead, 9u);
}

TEST(TopconRw5, PastTenThousandWarningsTheRestAreCountedInOneWarningNotListed)
{
    // Two good records, then 25,000 of a type the specification does not
    // define: lines 3 to 25,002. The first 10,000 (lines 3 to 10,002) are
    // listed; the other 15,000, from line 10,003, are one closing warning.
    std::string bytes = "MO,AD0,UN1,SF1.0,EC0,EO0.0,AU0\nSP,PN1,N 1.0,E 2.0,EL3.0\n";
    for (int line = 0; line < 25000; ++line) {
        bytes += "ZZ,XX1\n";
    }
    const auto result = topcon_test::read(kRw5, bytes, "noisy.rw5");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsRead, 2u);
    EXPECT_EQ(result->recordsSkipped, 25000u);
    ASSERT_EQ(result->warnings.size(), 10001u);
    EXPECT_EQ(result->warnings[9999].record, 10002u);
    EXPECT_EQ(result->warnings.back().record, 0u);
    EXPECT_NE(result->warnings.back().message.find("15000 more warnings, from record 10003 on"),
              std::string::npos)
        << result->warnings.back().message;
}

TEST(TopconRw5, GnssPositionsAreGeodeticInWgs84FromDegreesMinutesSeconds)
{
    const ReadResult result = readFixture("gnss.rw5");
    const auto& observations = result.project.observations;
    ASSERT_EQ(observations.size(), 3u); // BP, GPS, G1
    const auto* base = std::get_if<survey::GnssGlobalPositionObservation>(&observations[0]);
    ASSERT_NE(base, nullptr);
    EXPECT_EQ(base->point, "001");
    // LA40.364883159071 = 40 36' 48.83159071"; LN-3.420590114521 is west.
    EXPECT_NEAR(base->geodetic->latitude, dms(40, 36, 48.83159071), kAngleTolerance);
    EXPECT_NEAR(base->geodetic->longitude, -dms(3, 42, 5.90114521), kAngleTolerance);
    EXPECT_DOUBLE_EQ(base->geodetic->ellipsoidalHeight, 765.698);
    // ATARP: the height AG1.000 is to the antenna reference point.
    EXPECT_DOUBLE_EQ(base->antenna.height, 1.0);
    EXPECT_EQ(base->antenna.method, survey::AntennaHeightMethod::Vertical);
    EXPECT_EQ(base->referenceFrame, "WGS 84");

    const auto* rover = std::get_if<survey::GnssGlobalPositionObservation>(&observations[1]);
    ASSERT_NE(rover, nullptr);
    EXPECT_EQ(rover->point, "701");
    EXPECT_NEAR(rover->geodetic->latitude, dms(42, 21, 46.3092), kAngleTolerance);
    EXPECT_NEAR(rover->geodetic->longitude, -dms(71, 8, 14.09184), kAngleTolerance);
    EXPECT_DOUBLE_EQ(rover->geodetic->ellipsoidalHeight, -21.8459);
    // The rod height in force (LS,HR2.000) is the phase-centre height.
    EXPECT_DOUBLE_EQ(rover->antenna.height, 2.0);
    EXPECT_EQ(rover->antenna.method, survey::AntennaHeightMethod::PhaseCentre);
    EXPECT_EQ(topcon_test::unpositioned(result.project, "701")->code, "CP");
}

TEST(TopconRw5, GnssVectorRecordsBecomeOneGeocentricBaselineWithItsCovariance)
{
    const ReadResult result = readFixture("gnss.rw5");
    const auto* vector =
        std::get_if<survey::GnssGeocentricBaselineObservation>(&result.project.observations.at(2));
    ASSERT_NE(vector, nullptr);
    EXPECT_EQ(vector->from, "0");
    EXPECT_EQ(vector->to, "3");
    EXPECT_DOUBLE_EQ(vector->delta.x, -3444.43582);
    EXPECT_DOUBLE_EQ(vector->delta.y, -4918.56294);
    EXPECT_DOUBLE_EQ(vector->delta.z, 3862.46787);
    EXPECT_DOUBLE_EQ(vector->covariance.xx, 0.00013303);
    EXPECT_DOUBLE_EQ(vector->covariance.yy, 0.00008997);
    EXPECT_DOUBLE_EQ(vector->covariance.zz, 0.00014449);
    EXPECT_DOUBLE_EQ(vector->covariance.xy, -0.00005549);
    EXPECT_DOUBLE_EQ(vector->covariance.xz, 0.00010642);
    EXPECT_DOUBLE_EQ(vector->covariance.yz, -0.00005627);
    EXPECT_EQ(vector->fromAntenna.method, survey::AntennaHeightMethod::Vertical);   // BVARP
    EXPECT_EQ(vector->toAntenna.method, survey::AntennaHeightMethod::PhaseCentre); // RVAPC
    EXPECT_EQ(vector->source.recordNumber, 6u);
    EXPECT_EQ(topcon_test::unpositioned(result.project, "3")->metadata.at("GNSS vector note"),
              "2013/12/06 15:44:50,(Average) - Base ID read at rover: 0");
}

TEST(TopconRw5, DetectionIdentifiesEachRw5FixtureAndNoOtherFormatClaimsThem)
{
    for (const char* name : {"setup_metres.rw5", "feet_grads.rw5", "gnss.rw5", "journal.rw5"}) {
        const std::string bytes = fixture(std::filesystem::path("rw5") / name);
        const Detection detection = detectFormat(probeOf(bytes, name));
        ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << name << ": "
                                                                     << detection.summary();
        EXPECT_EQ(detection.format()->id, kRw5) << name;
    }
    // Without its extension an RW5 file is still recognised by its records.
    const std::string bytes = fixture("rw5/setup_metres.rw5");
    const Detection renamed = detectFormat(probeOf(bytes, "export.txt"));
    ASSERT_EQ(renamed.outcome(), DetectionOutcome::Identified) << renamed.summary();
    EXPECT_EQ(renamed.format()->id, kRw5);
}

TEST(TopconRw5, TheProbeRulesOutCoordinateFilesAndGtsRawData)
{
    const FormatRegistry& registry = formatRegistry();
    for (const char* name : {"survey_points_pnezd.csv", "survey_points_headerless.txt",
                             "survey_points_headerless_penz.txt", "survey_points_pnezd_utm30s.csv",
                             "gts/setup.gt7"}) {
        const std::string bytes = fixture(name);
        for (const auto& probed : registry.probeAll(probeOf(bytes, name))) {
            if (probed.formatId == kRw5) {
                EXPECT_LT(probed.signature.confidence, 0.5) << name << ": "
                                                            << probed.signature.evidence;
            }
        }
    }
}

TEST(TopconRw5, TheProbeDoesNotClaimLeicaTrimbleRinexOrLandXmlFiles)
{
    for (const topcon_test::ForeignSample& sample : topcon_test::kForeignSamples) {
        const ProbeInput input = probeOf(sample.bytes, sample.name);
        for (const auto& probed : formatRegistry().probeAll(input)) {
            if (probed.formatId == kRw5) {
                EXPECT_LT(probed.signature.confidence, 0.5) << sample.name << ": "
                                                            << probed.signature.evidence;
            }
        }
        const Detection detection = detectFormat(input);
        if (detection.format()) {
            EXPECT_NE(detection.format()->id, kRw5) << sample.name;
        }
    }
}

TEST(TopconRw5, EveryTruncationAndRandomBytesGiveAnErrorOrAResultNeverACrash)
{
    std::size_t reads = 0;
    for (const char* name : {"setup_metres.rw5", "feet_grads.rw5", "gnss.rw5", "journal.rw5"}) {
        reads += topcon_test::readEveryTruncationAndNoise(
            kRw5, fixture(std::filesystem::path("rw5") / name), name);
    }
    EXPECT_GT(reads, 3000u);
}

// A synthetic job: setups of radial shots. Sized by the environment variable
// KATANA_SURVEYIO_THROUGHPUT_MB (a small default keeps the suite quick); the
// rate is printed for the record, and the counts checked.
TEST(TopconRw5, AReadOfALargeSyntheticJobReadsEveryRecordAndReportsItsRate)
{
    std::size_t megabytes = 2;
    if (const char* wanted = std::getenv("KATANA_SURVEYIO_THROUGHPUT_MB")) {
        megabytes = static_cast<std::size_t>(std::strtoul(wanted, nullptr, 10));
    }
    std::string bytes = "JB,NMTHROUGHPUT,DT09-24-2026,TM10:00:00\n"
                        "MO,AD0,UN1,SF1.00000000,EC0,EO0.0,AU0\n";
    std::size_t setups = 0;
    std::size_t shots = 0;
    while (bytes.size() < megabytes * 1024 * 1024) {
        ++setups;
        const std::string station = "S" + std::to_string(setups);
        bytes += "OC,OP" + station + ",N 5000.0000,E 5000.0000,EL100.000,--CP\n";
        bytes += "BK,OP" + station + ",BPB" + std::to_string(setups) + ",BS0.0000,BC0.0000\n";
        bytes += "LS,HI1.550,HR1.800\n";
        for (int shot = 0; shot < 100; ++shot) {
            ++shots;
            bytes += "SS,OP" + station + ",FP" + std::to_string(shots) + ",AR" +
                     std::to_string(shot * 3) + ".1530,ZE91.1015,SD" +
                     std::to_string(10 + shot) + ".2345,--KB" + std::to_string(shot % 7) + "\n";
        }
    }
    // The reader on its own, then the whole door (readSurvey adds the size
    // caps and survey::validateProject, which is not this reader's cost).
    const FormatReader reader = formatRegistry().reader(kRw5);
    ASSERT_NE(reader, nullptr);
    double readerSeconds = 0.0;
    {
        const auto start = std::chrono::steady_clock::now();
        const auto alone = reader(bytes, "throughput.rw5", ReadOptions{});
        readerSeconds =
            std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
        EXPECT_TRUE(alone.ok());
    } // freed here, outside both timings
    const auto start = std::chrono::steady_clock::now();
    const auto result = topcon_test::read(kRw5, bytes, "throughput.rw5");
    const double seconds =
        std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsRead, 2 + setups * 3 + shots);
    EXPECT_EQ(result->recordsSkipped, 0u);
    EXPECT_EQ(result->project.stations.size(), setups);
    const double mb = static_cast<double>(bytes.size()) / (1024.0 * 1024.0);
    std::cout << "RW5 throughput: " << mb << " MB, " << shots << " shots; reader alone "
              << readerSeconds << " s, " << mb / readerSeconds << " MB/s; readSurvey (with "
              << "validateProject) " << seconds << " s, " << mb / seconds << " MB/s\n";
}
