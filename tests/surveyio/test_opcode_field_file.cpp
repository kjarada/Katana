// The opcode field file reader (src/katana_surveyio/opcode_field_file.cpp).
//
// data/fld/setup.fld is hand-built from the format's published record list
// ("Field File Format", sections 1.2, 1.3 and 1.8: opcodes 02, 03, 04, 05,
// 06, 07, 09, 29, 41, 72, 73 and 100), in the layout of the field files it
// was written against: the empty column after every opcode, CRLF line ends.
// Every expected value below is worked from the record text: circle readings
// are decimal degrees, so 45.00000000 is pi / 4.

#include <gtest/gtest.h>

#include <numbers>
#include <string>
#include <variant>

#include "surveyio/topcon_test_support.hpp"

using namespace katana::surveyio;
namespace survey = katana::survey;
using katana::core::ErrorCode;
using topcon_test::fixture;
using topcon_test::observationsTo;

namespace {

constexpr std::string_view kId = "opcode-field-file";
constexpr double kPi = std::numbers::pi;
constexpr double kAngleTolerance = 1e-12;

double degrees(double value) { return value * kPi / 180.0; }

ReadResult readSetup()
{
    const std::string bytes = fixture("fld/setup.fld");
    auto result = topcon_test::read(kId, bytes, "setup.fld");
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(result).value() : ReadResult{};
}

bool warned(const ReadResult& result, std::size_t record, std::string_view words)
{
    for (const ReadWarning& warning : result.warnings) {
        if (warning.record == record && warning.message.find(words) != std::string::npos) {
            return true;
        }
    }
    return false;
}

// A file of tab-separated records, written line by line so a tab is seen.
std::string lines(std::initializer_list<std::string_view> rows)
{
    std::string text;
    for (const std::string_view row : rows) {
        text += row;
        text += '\n';
    }
    return text;
}

} // namespace

TEST(OpcodeFieldFile, IsRegisteredAsAnImportableFormatReadingSetupsShotsAndPoints)
{
    const auto format = formatRegistry().find(kId);
    ASSERT_TRUE(format.ok());
    EXPECT_TRUE(format->canImport);
    EXPECT_EQ(format->extensions, std::vector<std::string>{"fld"});
    EXPECT_TRUE(format->reads.points && format->reads.observations && format->reads.stations &&
                format->reads.features && format->reads.coordinateSystem);
    EXPECT_FALSE(format->reads.gnss);
    EXPECT_NE(formatRegistry().reader(kId), nullptr);
}

TEST(OpcodeFieldFile, AFileWithItsVersionLineAndOpcodesIsIdentified)
{
    const std::string bytes = fixture("fld/setup.fld");
    const Detection detection = detectFormat(probeOf(bytes, "setup.fld"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kId);
    EXPECT_NE(detection.candidates().front().evidence.find("{Version"), std::string::npos);
}

TEST(OpcodeFieldFile, AnotherFormatNamedFldIsNotClaimed)
{
    // Comma-separated text with no numeric opcode and tab is some other
    // program's .fld: ruled out, not read.
    const std::string other = "PT,1,1000.0,5000.0\nPT,2,1001.0,5001.0\n";
    for (const auto& probed : formatRegistry().probeAll(probeOf(other, "job.fld"))) {
        if (probed.formatId == kId) {
            EXPECT_EQ(probed.signature.confidence, 0.0) << probed.signature.evidence;
        }
    }
}

TEST(OpcodeFieldFile, EnteredCoordinatesArePositionedPointsEastingFirst)
{
    const ReadResult result = readSetup();
    const survey::SurveyProject& project = result.project;
    const survey::SurveyPoint* cp1 = topcon_test::point(project, "CP1");
    ASSERT_NE(cp1, nullptr);
    EXPECT_DOUBLE_EQ(cp1->easting, 1000.0);
    EXPECT_DOUBLE_EQ(cp1->northing, 5000.0);
    ASSERT_TRUE(cp1->elevation.has_value());
    EXPECT_DOUBLE_EQ(*cp1->elevation, 20.0);
    EXPECT_EQ(cp1->coordinateSource, survey::CoordinateSource::Entered);
    EXPECT_EQ(cp1->source.recordNumber, 8u);

    // Record 9 gives point ID 5 AND point name CP2: the name is the point.
    const survey::SurveyPoint* cp2 = topcon_test::point(project, "CP2");
    ASSERT_NE(cp2, nullptr);
    EXPECT_DOUBLE_EQ(cp2->northing, 5100.0);
    EXPECT_EQ(cp2->metadata.at("point ID"), "5");
    EXPECT_EQ(topcon_test::point(project, "5"), nullptr);
    EXPECT_EQ(project.points.size(), 2u);
}

TEST(OpcodeFieldFile, AStationItsBacksightAndItsShotsBecomeOneSetupsObservations)
{
    const ReadResult result = readSetup();
    const survey::SurveyProject& project = result.project;
    ASSERT_EQ(project.stations.size(), 1u);
    const survey::SurveyStation& station = project.stations[0];
    EXPECT_EQ(station.setup.pointId, "CP1");
    EXPECT_DOUBLE_EQ(station.setup.instrumentHeight, 1.5);
    EXPECT_EQ(station.backsightPointId, "CP2");
    ASSERT_TRUE(station.backsightAzimuth.has_value());
    EXPECT_NEAR(*station.backsightAzimuth, 0.0, kAngleTolerance);
    // Four pointings - backsight, 101, 102, the check on CP2 - of a
    // direction, a zenith and a slope distance each.
    EXPECT_EQ(station.observations.size(), 12u);

    // To CP2: the backsight (record 13) - 0, 90 degrees, 100 m, under target
    // height 1.600 - and the check measurement (record 22) - 0.001 degrees,
    // 100.002 m, under the target height record 14 set, 2.000.
    const auto directions =
        observationsTo<survey::HorizontalDirectionObservation>(station, "CP2");
    ASSERT_EQ(directions.size(), 2u);
    EXPECT_NEAR(directions[0].direction, 0.0, kAngleTolerance);
    EXPECT_NEAR(directions[1].direction, degrees(0.001), kAngleTolerance);
    const auto zeniths = observationsTo<survey::ZenithAngleObservation>(station, "CP2");
    ASSERT_EQ(zeniths.size(), 2u);
    EXPECT_NEAR(zeniths[0].angle, kPi / 2.0, kAngleTolerance);
    EXPECT_DOUBLE_EQ(zeniths[0].targetHeight, 1.6);
    const auto distances = observationsTo<survey::DistanceObservation>(station, "CP2");
    ASSERT_EQ(distances.size(), 2u);
    EXPECT_DOUBLE_EQ(distances[0].distance, 100.0);
    EXPECT_EQ(distances[0].kind, survey::DistanceKind::Slope);
    EXPECT_DOUBLE_EQ(distances[0].instrumentHeight, 1.5);
    EXPECT_DOUBLE_EQ(distances[0].targetHeight, 1.6);
    EXPECT_DOUBLE_EQ(distances[1].distance, 100.002);
    EXPECT_DOUBLE_EQ(distances[1].targetHeight, 2.0);
    EXPECT_EQ(topcon_test::point(project, "CP2")->metadata.at("check measurement"), "record 22");
}

TEST(OpcodeFieldFile, AZenithPastOneEightyIsFaceRightHeldAsItsFaceLeftEquivalent)
{
    const ReadResult result = readSetup();
    const survey::SurveyStation& station = result.project.stations.at(0);
    // 101: 45, 95 degrees - face left, as read.
    const auto direction101 =
        observationsTo<survey::HorizontalDirectionObservation>(station, "101");
    ASSERT_EQ(direction101.size(), 1u);
    EXPECT_NEAR(direction101[0].direction, kPi / 4.0, kAngleTolerance);
    const auto zenith101 = observationsTo<survey::ZenithAngleObservation>(station, "101");
    ASSERT_EQ(zenith101.size(), 1u);
    EXPECT_NEAR(zenith101[0].angle, degrees(95.0), kAngleTolerance);
    EXPECT_EQ(zenith101[0].pointing.face, survey::Face::Left);
    // 102: 275 degrees is face right, the same line of sight as 85.
    const auto zenith102 = observationsTo<survey::ZenithAngleObservation>(station, "102");
    ASSERT_EQ(zenith102.size(), 1u);
    EXPECT_NEAR(zenith102[0].angle, degrees(85.0), kAngleTolerance);
    EXPECT_EQ(zenith102[0].pointing.face, survey::Face::Right);
    const auto distance102 = observationsTo<survey::DistanceObservation>(station, "102");
    ASSERT_EQ(distance102.size(), 1u);
    EXPECT_DOUBLE_EQ(distance102[0].distance, 30.0);
}

TEST(OpcodeFieldFile, ShotsAreUnpositionedPointsStrungIntoTheirFeatureWithTheirAttributes)
{
    const ReadResult result = readSetup();
    const survey::SurveyProject& project = result.project;
    const survey::UnpositionedPoint* p101 = topcon_test::unpositioned(project, "101");
    const survey::UnpositionedPoint* p102 = topcon_test::unpositioned(project, "102");
    ASSERT_NE(p101, nullptr);
    ASSERT_NE(p102, nullptr);
    EXPECT_EQ(p101->code, "EB");
    EXPECT_EQ(p102->description, "kerb");
    EXPECT_EQ(p101->metadata.at("Date"), "2026-08-26");
    EXPECT_EQ(p101->metadata.at("Depth"), "0.6");
    EXPECT_EQ(p101->metadata.at("additional text 18"), "Modified by hand");
    const survey::SurveyFeature* eb = topcon_test::feature(project, "EB");
    ASSERT_NE(eb, nullptr);
    EXPECT_EQ(eb->pointIds, (std::vector<std::string>{"101", "102"}));
}

TEST(OpcodeFieldFile, TheHeaderUnitsAndDeclaredSystemAreKeptAndNothingIsGuessed)
{
    const ReadResult result = readSetup();
    const survey::SurveyProject& project = result.project;
    EXPECT_EQ(project.units.angular, survey::AngularUnit::DecimalDegrees);
    EXPECT_EQ(project.units.linear, survey::LinearUnit::Metres);
    EXPECT_EQ(project.metadata.at("version"), "{Version 6.0}");
    EXPECT_EQ(project.metadata.at("header: Job name"), "fixture");
    EXPECT_EQ(project.metadata.at("notes"), "Hand-built from the format description");
    // Declared by name, as the file spells it; no EPSG code is invented.
    EXPECT_EQ(project.coordinateSystem.name, "Australia/GDA2020, Zone 56");
    EXPECT_EQ(project.coordinateSystem.epsgCode, 0);
}

TEST(OpcodeFieldFile, ARecordShortOfAValueAndAnOpcodeNotReadAreSkippedByName)
{
    const ReadResult result = readSetup();
    // Read: the version line, 100, 29, two 02, 09, 03, two 05, 04, two 07,
    // 73, 72, 41, 06 - sixteen. Skipped: record 20 (a 07 one value short)
    // and record 21 (opcode 42).
    EXPECT_EQ(result.recordsRead, 16u);
    EXPECT_EQ(result.recordsSkipped, 2u);
    EXPECT_TRUE(warned(result, 20, "which value is which cannot be told"));
    EXPECT_TRUE(warned(result, 21, "opcode 42 is not one this reader imports"));
    EXPECT_EQ(topcon_test::unpositioned(result.project, "103"), nullptr);
}

TEST(OpcodeFieldFile, AFileWithoutTheEmptyColumnIsReadByItsCounts)
{
    // [FLD]'s own syntax: the description straight after the opcode. The 07
    // with a feature code decides it; the 02 and 03 with none fit either.
    const std::string bytes = lines({
        "100\tdegrees\tmetres",
        "02\t\t\tA\t\t\t10.0\t20.0\t1.0",
        "03\t\t\tA\t\t\t1.5",
        "07\tTREE\t\t7\t\t\t90.0\t90.0\t12.5",
    });
    auto read = topcon_test::read(kId, bytes, "plain.fld");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const survey::SurveyPoint* a = topcon_test::point(read->project, "A");
    ASSERT_NE(a, nullptr);
    EXPECT_DOUBLE_EQ(a->easting, 10.0);
    EXPECT_DOUBLE_EQ(a->northing, 20.0);
    ASSERT_EQ(read->project.stations.size(), 1u);
    const survey::SurveyStation& station = read->project.stations[0];
    const auto directions = observationsTo<survey::HorizontalDirectionObservation>(station, "7");
    ASSERT_EQ(directions.size(), 1u);
    EXPECT_NEAR(directions[0].direction, kPi / 2.0, kAngleTolerance);
    const auto distances = observationsTo<survey::DistanceObservation>(station, "7");
    ASSERT_EQ(distances.size(), 1u);
    EXPECT_DOUBLE_EQ(distances[0].distance, 12.5);
    EXPECT_EQ(topcon_test::unpositioned(read->project, "7")->code, "TREE");
    EXPECT_EQ(read->recordsSkipped, 0u);
}

TEST(OpcodeFieldFile, AUnitOtherThanDegreesAndMetresIsRefusedNotGuessed)
{
    const std::string bytes = lines({
        "100\t\tgons\tmetres",
        "02\t\t\t\tA\t\t\t10.0\t20.0\t1.0",
    });
    auto read = topcon_test::read(kId, bytes, "gons.fld");
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::FileImportFailure);
    EXPECT_NE(read.error().message.find("'gons'"), std::string::npos) << read.error().message;
    EXPECT_NE(read.error().message.find("record 1 "), std::string::npos) << read.error().message;
}

TEST(OpcodeFieldFile, AShotBeforeAnyStationAndAValueThatIsNotANumberAreWarnedOf)
{
    const std::string bytes = lines({
        "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEB\t01\t2\t\t\t10.0\tnan\t5.0",
    });
    auto read = topcon_test::read(kId, bytes, "early.fld");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(warned(*read, 1, "with no station (opcode 03) before it"));
    EXPECT_TRUE(warned(*read, 3, "vertical circle 'nan' is not a number"));
    // Record 3 still has its direction and distance; it has no zenith.
    const survey::SurveyStation& station = read->project.stations.at(0);
    EXPECT_EQ(observationsTo<survey::HorizontalDirectionObservation>(station, "2").size(), 1u);
    EXPECT_EQ(observationsTo<survey::ZenithAngleObservation>(station, "2").size(), 0u);
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(station, "2").size(), 1u);
}

TEST(OpcodeFieldFile, AFileWithNothingToReadIsAnErrorNotAnEmptySurvey)
{
    for (const std::string& bytes :
         {std::string{}, std::string("{Version 6.0}\r\n"), lines({"29\t\tonly a note"})}) {
        auto read = topcon_test::read(kId, bytes, "empty.fld");
        ASSERT_FALSE(read.ok());
        EXPECT_EQ(read.error().code, ErrorCode::FileImportFailure) << read.error().describe();
    }
}
