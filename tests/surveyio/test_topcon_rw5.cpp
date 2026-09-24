// The RW5 reader (src/katana_surveyio/topcon_rw5.cpp).
//
// Fixtures are hand-built from the TDS Raw Data Record Specification (Survey
// Pro 3.6) and the Carlson SurvCE RW5 format 3.03; gnss.rw5 is made of the
// SurvCE 3.03 document's own sample records, and survce250_us_feet.rw5 is the
// sample job printed in Carlson's "SurvCE Version 2.50 Raw File records". Every
// expected value is worked below from the record text, not taken from the
// reader's output.

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
    // Each distance carries the offset in force when it was measured.
    const auto distances =
        observationsTo<survey::DistanceObservation>(result.project.stations.at(0), "10");
    ASSERT_FALSE(distances.empty());
    ASSERT_TRUE(distances[0].target.prismConstant.has_value());
    EXPECT_DOUBLE_EQ(*distances[0].target.prismConstant, 0.0254);
    EXPECT_EQ(distances[0].target.prismConstantState, survey::CorrectionState::Unknown);
    EXPECT_EQ(settings.atmosphericPpmState, survey::CorrectionState::Unknown);
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(result, "no atmospheric settings"));
}

TEST(TopconRw5, AnEdmOffsetChangedMidSetupIsCarriedByTheShotsAfterItOnly)
{
    const std::string bytes = "MO,AD0,UN1,SF1.0,EC0,EO0.0,AU0\n"
                              "OC,OP1,N 0.0,E 0.0,EL0.0\n"
                              "SS,OP1,FP2,AR0.0000,ZE90.0000,SD10.000\n"
                              "MO,AD0,UN1,SF1.0,EC0,EO-1.0,AU0\n"
                              "SS,OP1,FP3,AR90.0000,ZE90.0000,SD10.000\n";
    const auto result = topcon_test::read(kRw5, bytes, "prism.rw5");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyStation& station = result->project.stations.at(0);
    // The setup began under EO0.0.
    EXPECT_DOUBLE_EQ(*station.instrument.prismConstant, 0.0);
    const auto before = observationsTo<survey::DistanceObservation>(station, "2");
    const auto after = observationsTo<survey::DistanceObservation>(station, "3");
    ASSERT_TRUE(before.size() == 1 && after.size() == 1);
    EXPECT_DOUBLE_EQ(*before[0].target.prismConstant, 0.0);
    // EO-1.0 inch = -0.0254 m.
    EXPECT_DOUBLE_EQ(*after[0].target.prismConstant, -0.0254);
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

TEST(TopconRw5, NoFieldOrNoteIsDroppedWithoutAWordAndOffCentreShotsAreKept)
{
    const std::string bytes = "MO,AD0,UN1,SF1.0,EC0,EO0.0,AU0\n"                            // 1
                              "OC,OP1,N 0.0,E 0.0,EL0.0\n"                                  // 2
                              "BK,OP1,BP2,BC0.0000,--sun behind\n"                          // 3
                              "SS,OP1,FP3,AR10.0000,AZ20.0000,ZE90.0000,SD5.000,FE12.345,QQ7\n"
                              "OF,AR11.0000,ZE90.0000,SD5.100,--tree centre\n"              // 5
                              "SS,OP1,FP4,AR30.0000,ZE90.0000,SD6.000,QQ8\n";               // 6
    const auto result = topcon_test::read(kRw5, bytes, "fields.rw5");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsRead, 6u);
    EXPECT_EQ(result->recordsSkipped, 0u);
    const survey::SurveyStation& station = result->project.stations.at(0);
    // A backsight has no use for a note; it is kept with the setup.
    EXPECT_EQ(station.metadata.at("notes"), "BK note: sun behind");
    // AR and AZ on one shot: AR (10 00' 00") is read, AZ named, not imported.
    EXPECT_TRUE(topcon_test::anyWarningContains(*result, 4, "gives AR and also AZ"));
    const auto direction = observationsTo<survey::HorizontalDirectionObservation>(station, "3");
    ASSERT_EQ(direction.size(), 1u);
    EXPECT_NEAR(direction[0].direction, dms(10, 0, 0), kAngleTolerance);
    EXPECT_TRUE(observationsTo<survey::AzimuthObservation>(station, "3").empty());
    // FE, the collector's foresight elevation, stays with the point.
    EXPECT_EQ(topcon_test::unpositioned(result->project, "3")
                  ->metadata.at("foresight elevation (collector, m)"),
              "12.345");
    // QQ is in no specification: warned where first met, then counted.
    EXPECT_TRUE(topcon_test::anyWarningContains(*result, 4, "field 'QQ' (value '7')"));
    EXPECT_FALSE(topcon_test::anyWarningContains(*result, 6, "field 'QQ'"));
    EXPECT_TRUE(topcon_test::anyWarningContains(
        *result, 0, "'QQ' field of SS records was not imported on 2 records, the first record 4"));
    // The off-centre shot is kept, as written, with the setup.
    EXPECT_EQ(station.metadata.at("off-centre shot, record 5 (file units)"),
              "AR 11.0000, ZE 90.0000, SD 5.100, note tree centre");
    EXPECT_TRUE(topcon_test::anyWarningContains(*result, 5, "not applied"));
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
    // G4 BVARP: the base end is the antenna reference point, AG1.000 above
    // the mark. RVAPC: the rover end is the phase centre, LS,HR2.000 above.
    EXPECT_EQ(vector->fromAntenna.method, survey::AntennaHeightMethod::Vertical);
    EXPECT_DOUBLE_EQ(vector->fromAntenna.height, 1.0);
    EXPECT_EQ(vector->toAntenna.method, survey::AntennaHeightMethod::PhaseCentre);
    EXPECT_DOUBLE_EQ(vector->toAntenna.height, 2.0);
    EXPECT_EQ(vector->source.recordNumber, 6u);
    EXPECT_EQ(topcon_test::unpositioned(result.project, "3")->metadata.at("GNSS vector note"),
              "2013/12/06 15:44:50,(Average) - Base ID read at rover: 0");
}

namespace {

// WGS 84 geodetic to geocentric, the textbook closed form (a = 6378137 m,
// 1/f = 298.257223563), written out here so that a vector can be checked
// against the two positions a file gives for its ends without the reader.
survey::GeocentricCoordinate wgs84Geocentric(double latitude, double longitude, double height)
{
    constexpr double a = 6378137.0;
    constexpr double f = 1.0 / 298.257223563;
    const double e2 = f * (2.0 - f);
    const double n = a / std::sqrt(1.0 - e2 * std::sin(latitude) * std::sin(latitude));
    return {(n + height) * std::cos(latitude) * std::cos(longitude),
            (n + height) * std::cos(latitude) * std::sin(longitude),
            (n * (1.0 - e2) + height) * std::sin(latitude)};
}

const survey::GnssGeocentricBaselineObservation* firstVector(const ReadResult& result)
{
    for (const survey::Observation& observation : result.project.observations) {
        if (const auto* vector = std::get_if<survey::GnssGeocentricBaselineObservation>(&observation)) {
            return vector;
        }
    }
    return nullptr;
}

} // namespace

TEST(TopconRw5, ASurvCeVectorIsInMetresWhateverTheJobsDistanceUnit)
{
    // Carlson's own sample from "SurvCE Version 2.50 Raw File records"
    // (5/4/2010), a US-survey-feet job (MO UN2). The document: "The DX, DY and
    // DZ values are phase center to phase center. ALL THE VALUES ARE ALWAYS IN
    // METERS."
    const ReadResult result = readFixture("survce250_us_feet.rw5");
    EXPECT_EQ(result.project.units.linear, survey::LinearUnit::UsSurveyFeet);
    const survey::GnssGeocentricBaselineObservation* vector = firstVector(result);
    ASSERT_NE(vector, nullptr);
    EXPECT_EQ(vector->from, "733");
    EXPECT_EQ(vector->to, "BWC1+A");
    EXPECT_DOUBLE_EQ(vector->delta.x, 5692.192);
    EXPECT_DOUBLE_EQ(vector->delta.y, 6823.564);
    EXPECT_DOUBLE_EQ(vector->delta.z, 12978.073);
    // The variances are square metres too; they and the vector agree.
    EXPECT_DOUBLE_EQ(vector->covariance.xx, 0.00863666);
    // The file itself says metres: its base (BP) and rover (GPS) positions,
    // both to the phase centre, are 15,728.71 m apart through the earth -
    //   base  30 16' 08.94090052" N, 97 47' 13.43999946" W, h 175.4530 m
    //   rover 30 24' 16.17091114" N, 97 44' 16.79812958" W, h 231.637722 m
    // - and sqrt(5692.192^2 + 6823.564^2 + 12978.073^2) = 15,728.71 m, the
    // components agreeing to 0.7 mm. Read as US feet the vector is 4,794 m.
    const survey::GeocentricCoordinate base =
        wgs84Geocentric(dms(30, 16, 8.94090052), -dms(97, 47, 13.43999946), 175.4530);
    const survey::GeocentricCoordinate rover =
        wgs84Geocentric(dms(30, 24, 16.17091114), -dms(97, 44, 16.79812958), 231.637722);
    EXPECT_NEAR(vector->delta.x, rover.x - base.x, 0.002);
    EXPECT_NEAR(vector->delta.y, rover.y - base.y, 0.002);
    EXPECT_NEAR(vector->delta.z, rover.z - base.z, 0.002);
}

TEST(TopconRw5, ASurvCeVectorRunsFromPhaseCentreToPhaseCentreWithTheRodHeightAtTheRover)
{
    // [SCE] 2.50: "You will get the rod height of the rover from the LS record
    // prior to the vector records. The LS,HR value is from phase center to the
    // ground ... THE UNITS CAN BE FEET OR METERS"; and of the BP record: "add
    // [AG and PA] together to get the Phase Center to Ground value", always
    // in metres.
    const ReadResult result = readFixture("survce250_us_feet.rw5");
    const survey::GnssGeocentricBaselineObservation* vector = firstVector(result);
    ASSERT_NE(vector, nullptr);
    // LS,HR6.9344 US survey feet = 6.9344 * 1200 / 3937 = 2.1136 m.
    EXPECT_DOUBLE_EQ(vector->toAntenna.height, 6.9344 * 1200.0 / 3937.0);
    EXPECT_EQ(vector->toAntenna.method, survey::AntennaHeightMethod::PhaseCentre);
    // AG2.000 + PA0.114 = 2.114 m, the base's phase centre above its mark.
    EXPECT_DOUBLE_EQ(vector->fromAntenna.height, 2.114);
    EXPECT_EQ(vector->fromAntenna.method, survey::AntennaHeightMethod::PhaseCentre);
}

TEST(TopconRw5, AG4RecordMovesAVectorEndBetweenThePhaseCentreAndTheReferencePoint)
{
    const std::string bytes =
        "MO,AD0,UN1,SF1.00000000,EC0,EO0.0,AU0\n"                                          // 1
        "BP,PN001,LA40.364883159071,LN-3.420590114521,EL765.6980,AG1.000,PA0.084,ATARP,"
        "SRROVER,--\n"                                                                     // 2
        "LS,HR2.000\n"                                                                     // 3
        "G0,2013/12/06 15:44:50,(Average) - Base ID read at rover: 001\n"                  // 4
        "G1,BP001,PN3,DX-3444.43582,DY-4918.56294,DZ3862.46787\n"                          // 5
        "G4,BVAPC,RVAPC\n"                                                                 // 6
        "G0,2013/12/06 15:49:50,(Average) - Base ID read at rover: 001\n"                  // 7
        "G1,BP001,PN4,DX-3440.00000,DY-4910.00000,DZ3860.00000\n"                          // 8
        "G4,BVARP,RVARP\n";                                                                // 9
    const auto result = topcon_test::read(kRw5, bytes, "ends.rw5");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    std::vector<survey::GnssGeocentricBaselineObservation> vectors;
    for (const survey::Observation& observation : result->project.observations) {
        if (const auto* vector = std::get_if<survey::GnssGeocentricBaselineObservation>(&observation)) {
            vectors.push_back(*vector);
        }
    }
    ASSERT_EQ(vectors.size(), 2u);
    // BVAPC on a base whose AG is to the reference point (ATARP): the phase
    // centre is AG1.000 + PA0.084 = 1.084 m above the mark, not 1.000.
    EXPECT_DOUBLE_EQ(vectors[0].fromAntenna.height, 1.084);
    EXPECT_EQ(vectors[0].fromAntenna.method, survey::AntennaHeightMethod::PhaseCentre);
    EXPECT_DOUBLE_EQ(vectors[0].toAntenna.height, 2.0);
    EXPECT_EQ(vectors[0].toAntenna.method, survey::AntennaHeightMethod::PhaseCentre);
    // BVARP: AG itself, 1.000 m.
    EXPECT_DOUBLE_EQ(vectors[1].fromAntenna.height, 1.0);
    EXPECT_EQ(vectors[1].fromAntenna.method, survey::AntennaHeightMethod::Vertical);
    // RVARP: the vector ends at the rover's reference point but HR is to its
    // phase centre ([SCE] 3.03: "GPS heights always to be recorded to phase
    // center"), and no record gives the offset between them: the height is
    // not passed off as either, and the record says so.
    EXPECT_DOUBLE_EQ(vectors[1].toAntenna.height, 2.0);
    EXPECT_EQ(vectors[1].toAntenna.method, survey::AntennaHeightMethod::Other);
    EXPECT_TRUE(topcon_test::anyWarningContains(*result, 9, "phase centre"));
}

TEST(TopconRw5, ABaselineNoteEndsWhereItsSolutionClassAndPrecisionsBegin)
{
    // [TDS] record 30, the one record whose note is not its last field:
    // "BL,DC%s,PN%s,DX%s,DY%s,DZ%s,--%s,GM%s,CL%s,HP%s,VP%s".
    const std::string bytes =
        "MO,AD0,UN1,SF1.00000000,EC0,EO0.0,AU0\n"                                          // 1
        "BP,PN1,LA40.364883159071,LN-3.420590114521,HT765.698,SG0\n"                       // 2
        "BL,DC2,PN2,DX10.000,DY20.000,DZ30.000,--CP fence, north,GM4,CL1,HP0.012,VP0.020\n" // 3
        "BL,DC2,PN3,DX11.000,DY21.000,DZ31.000,--CP,GM3,CL2,HP0.015,VP0.025\n";           // 4
    const auto result = topcon_test::read(kRw5, bytes, "baselines.rw5");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    std::vector<survey::GnssGeocentricBaselineObservation> vectors;
    for (const survey::Observation& observation : result->project.observations) {
        if (const auto* vector = std::get_if<survey::GnssGeocentricBaselineObservation>(&observation)) {
            vectors.push_back(*vector);
        }
    }
    ASSERT_EQ(vectors.size(), 2u);
    // [TDS] GM: 4 RTKFixed, 3 RTKFloat.
    EXPECT_EQ(vectors[0].solution, survey::GnssSolution::Fixed);
    EXPECT_EQ(vectors[1].solution, survey::GnssSolution::Float);
    const survey::UnpositionedPoint* two = topcon_test::unpositioned(result->project, "2");
    const survey::UnpositionedPoint* three = topcon_test::unpositioned(result->project, "3");
    ASSERT_TRUE(two != nullptr && three != nullptr);
    // The note keeps its own comma; the fields after it are fields.
    EXPECT_EQ(two->code, "CP");
    EXPECT_EQ(two->description, "CP fence, north");
    EXPECT_EQ(three->code, "CP");
    EXPECT_EQ(two->metadata.at("horizontal precision (m)"), "0.012");
    EXPECT_EQ(two->metadata.at("vertical precision (m)"), "0.02");
    EXPECT_EQ(three->metadata.at("horizontal precision (m)"), "0.015");
    // [TDS] CL: 1 Normal, 2 Control; DC 2 ModeRover.
    EXPECT_EQ(two->metadata.at("classification"), "normal");
    EXPECT_EQ(three->metadata.at("classification"), "control");
    EXPECT_EQ(two->metadata.at("GNSS derivation"), "rover");
    // Both points string into one CP feature.
    const survey::SurveyFeature* control = topcon_test::feature(result->project, "CP");
    ASSERT_NE(control, nullptr);
    EXPECT_EQ(control->pointIds, (std::vector<std::string>{"2", "3"}));
    EXPECT_EQ(result->project.features.size(), 1u);
    for (const ReadWarning& warning : result->warnings) {
        EXPECT_EQ(warning.message.find("not imported"), std::string::npos) << warning.message;
    }
}

TEST(TopconRw5, EveryValueOfEveryOffCentreShotIsKeptWithItsSetup)
{
    // [TDS] record 16 writes "OF,OL%s,--Right Angle Offset", "OF,HD%s,--
    // Horizontal Distance Offset", "OF,LR%s,--Left / Right Offset" and
    // "OF,VD%s,--Elevation Offset" beside [SCE]'s AR, ZE and SD.
    const std::string bytes = "MO,AD0,UN1,SF1.0,EC0,EO0.0,AU0\n"                   // 1
                              "OC,OP1,N 0.0,E 0.0,EL0.0\n"                         // 2
                              "SS,OP1,FP9,AR10.0000,ZE90.0000,SD5.000\n"           // 3
                              "OF,OL0.500,--Right Angle Offset\n"                  // 4
                              "SS,OP1,FP8,AR20.0000,ZE90.0000,SD6.000\n"           // 5
                              "OF,OL0.750,--Right Angle Offset\n"                  // 6
                              "OF,HD1.250,--Horizontal Distance Offset\n"          // 7
                              "OF,LR-0.300,--Left / Right Offset\n"                // 8
                              "OF,VD0.100,--Elevation Offset\n";                   // 9
    const auto result = topcon_test::read(kRw5, bytes, "offsets.rw5");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyStation& station = result->project.stations.at(0);
    const auto& metadata = station.metadata;
    EXPECT_EQ(metadata.at("off-centre shot, record 4 (file units)"),
              "OL 0.500, note Right Angle Offset");
    EXPECT_EQ(metadata.at("off-centre shot, record 6 (file units)"),
              "OL 0.750, note Right Angle Offset");
    EXPECT_EQ(metadata.at("off-centre shot, record 7 (file units)"),
              "HD 1.250, note Horizontal Distance Offset");
    EXPECT_EQ(metadata.at("off-centre shot, record 8 (file units)"),
              "LR -0.300, note Left / Right Offset");
    EXPECT_EQ(metadata.at("off-centre shot, record 9 (file units)"),
              "VD 0.100, note Elevation Offset");
    for (const ReadWarning& warning : result->warnings) {
        EXPECT_EQ(warning.message.find("specifications define"), std::string::npos)
            << warning.message;
    }
}

TEST(TopconRw5, EveryFieldTheSpecificationsDefineIsReadNotWarnedAboutAsUnknown)
{
    // One record of every type the reader imports, with every field [TDS]
    // 3.6 and [SCE] 3.03 give that type (a shot's alternatives on separate
    // shots, as the specifications allow one of each group).
    const std::string bytes =
        "JB,NMALL,DT09-24-2026,TM10:00:00\n"
        "MO,AD0,UN1,SF1.00000000,EC0,EO0.0,AU0\n"
        "CS,CO4,ZGUTM,ZNZone 56,DNWGS84\n"
        "OC,OP1,N 1000.0,E 2000.0,EL50.0,--CP\n"
        "BK,OP1,BP2,BS0.0000,BC0.0000\n"
        "LS,HI1.500,HR1.800\n"
        "TR,OP1,FP3,AR10.0000,ZE90.0000,SD5.000,--A\n"
        "SS,OP1,FP4,AZ20.0000,VA1.0000,SD6.000,--A\n"
        "OB,OP1,FP5,AL30.0000,CE0.500,HD7.000,--A\n"
        "SS,OP1,FP6,BRN40.0000E,ZE90.0000,SD8.000,FE51.000,--A\n"
        "SS,OP1,FP7,DR5.0000,ZE90.0000,SD9.000,--A\n"
        "SS,OP1,FP8,DL5.0000,ZE90.0000,SD9.500,--A\n"
        "BD,OP1,FP2,AR0.0000,ZE90.0000,SD10.000,--A\n"
        "BR,OP1,FP2,AR180.0000,ZE270.0000,SD10.000,--A\n"
        "FD,OP1,FP3,AR10.0000,ZE90.0000,SD5.000,--A\n"
        "FR,OP1,FP3,AR190.0000,ZE270.0000,SD5.000,--A\n"
        "SK,OP1,FP9,AR50.0000,ZE90.0000,SD11.000,--A\n"
        "RB,OP1,BP2,AR0.0000,ZE90.0000,SD10.000,HR1.800,--A\n"
        "RF,OP1,FP3,AR10.0000,ZE90.0000,SD5.000,HR1.800,--A\n"
        "OF,AR11.0000,ZE90.0000,SD5.100\n"
        "OF,OL0.500,--Right Angle Offset\n"
        "OF,HD1.250,--Horizontal Distance Offset\n"
        "OF,LR-0.300,--Left / Right Offset\n"
        "OF,VD0.100,--Elevation Offset\n"
        "SP,PN20,N 1010.0,E 2010.0,EL51.0,--B\n"
        "AP,PN21,N 1011.0,E 2011.0,EL51.1,--B\n"
        "GS,PN22,N 1012.0,E 2012.0,EL51.2,--B\n"
        "GR,PN23,N 1013.0,E 2013.0,EL51.3,--B\n"
        "FC,PN20,FNKB\n"
        "AT,TNcolour,TVred\n"
        "DP,PN21\n"
        "SP,PN24,N 1014.0,E 2014.0,EL51.4\n"
        "EP,TM12:30:45,LA40.364883159071,LN-3.420590114521,HT765.698,RH0.010,RV0.020,DH0.9,"
        "DV1.4,GM4,CL1\n"
        "AH,DC2,MA2.000,ME1,RA2.084\n"
        "EQ,DC2,RXR10,RS123,AN5,AI1,ATTRMR10,TS456,TA0.000,HO0.000,VO0.000\n"
        "BP,PN30,LA40.364883159071,LN-3.420590114521,HT765.698,SG0\n"
        "BL,DC2,PN31,DX10.000,DY20.000,DZ30.000,--CP,GM4,CL1,HP0.012,VP0.020\n"
        "CV,DC2,SV8,SC1.0,XX0.0001,XY0.00001,XZ0.00001,YY0.0001,YZ0.00001,ZZ0.0002\n"
        "BP,PN40,LA40.364883159071,LN-3.420590114521,EL765.6980,AG1.000,PA0.084,ATARP,"
        "SRROVER,--BASE\n"
        "GPS,PN41,LA42.214630920,LN-71.081409184,EL-21.8459,--CP\n"
        "G0,2013/12/06 15:44:50,(Average) - Base ID read at rover: 40\n"
        "G1,BP40,PN42,DX-3444.43582,DY-4918.56294,DZ3862.46787\n"
        "G2,VX0.00013303,VY0.00008997,VZ0.00014449\n"
        "G3,XY-0.00005549,XZ0.00010642,YZ-0.00005627\n"
        "G4,BVARP,RVAPC\n";
    const auto result = topcon_test::read(kRw5, bytes, "every_field.rw5");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsSkipped, 0u);
    for (const ReadWarning& warning : result->warnings) {
        EXPECT_EQ(warning.message.find("field '"), std::string::npos)
            << "record " << warning.record << ": " << warning.message;
        EXPECT_EQ(warning.message.find("was not imported on"), std::string::npos)
            << warning.message;
    }
    // Where the fields no import needs went.
    const auto& project = result->project;
    EXPECT_EQ(project.metadata.at("coordinate system option"), "chosen from library");
    EXPECT_EQ(project.metadata.at("rover antenna tape adjustment (m)"), "0");
    EXPECT_EQ(project.metadata.at("rover antenna number"), "5");
    EXPECT_EQ(topcon_test::point(project, "24")->metadata.at("GNSS time"), "12:30:45");
    EXPECT_EQ(topcon_test::point(project, "24")->metadata.at("classification"), "normal");
    EXPECT_EQ(topcon_test::unpositioned(project, "30")->metadata.at("setup group"), "0");
    EXPECT_EQ(topcon_test::unpositioned(project, "31")->metadata.at("minimum satellites"), "8");
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
