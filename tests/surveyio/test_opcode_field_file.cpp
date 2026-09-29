// The opcode field file reader (src/katana_surveyio/opcode_field_file.cpp).
//
// The fixtures are hand-built from the format's published description
// ("Field File Format", sections 44.2 "Structure of the .fld File", 44.3
// "Point Description" and 44.8, one entry per opcode), in the layout of the
// field files the reader was written against: a column after every opcode,
// CRLF line ends. None holds a real job.
//   data/fld/setup.fld      a total-station setup: 02, 03, 04, 05, 06, 07,
//                           09, 29, 41, 72, 73, 100, one record a value short
//                           and one opcode the reader does not read (18).
//   data/fld/gnss.fld       an RTK job: coordinates as " 2" (a blank before the
//                           opcode) with a date and time in the column, 124 and
//                           125 attribute groups, a bare 20, a remeasured mark,
//                           comments; Windows-1252, one 0xB0 degree sign.
//   data/fld/resection.fld  a resection (128 ... 129, with "5" and "7" written
//                           without their zero), its residuals as comments, 16,
//                           42, 43, 71, a coded check, then a 03 setup; UTF-8.
// Every expected value below is worked from the record text: circle readings
// are decimal degrees, so 45.00000000 is pi / 4.

#include <gtest/gtest.h>

#include <algorithm>
#include <filesystem>
#include <iostream>
#include <map>
#include <numbers>
#include <string>
#include <variant>
#include <vector>

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

ReadResult readFixture(std::string_view name)
{
    const std::string bytes = fixture(std::filesystem::path("fld") / name);
    auto result = topcon_test::read(kId, bytes, name);
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(result).value() : ReadResult{};
}

ReadResult readSetup() { return readFixture("setup.fld"); }

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

ReadResult readLines(std::initializer_list<std::string_view> rows)
{
    auto result = topcon_test::read(kId, lines(rows), "inline.fld");
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(result).value() : ReadResult{};
}

// The metadata of a point, positioned or not; empty when there is no such point.
std::map<std::string, std::string> metadataOf(const survey::SurveyProject& project,
                                              std::string_view id)
{
    if (const survey::SurveyPoint* point = topcon_test::point(project, id)) {
        return point->metadata;
    }
    if (const survey::UnpositionedPoint* point = topcon_test::unpositioned(project, id)) {
        return point->metadata;
    }
    return {};
}

std::string valueOf(const std::map<std::string, std::string>& metadata, const std::string& key)
{
    const auto found = metadata.find(key);
    return found == metadata.end() ? std::string("(absent)") : found->second;
}

// The features of `code`, in file order.
std::vector<const survey::SurveyFeature*> featuresOf(const survey::SurveyProject& project,
                                                     std::string_view code)
{
    std::vector<const survey::SurveyFeature*> found;
    for (const survey::SurveyFeature& feature : project.features) {
        if (feature.code == code) {
            found.push_back(&feature);
        }
    }
    return found;
}

double fieldFileConfidence(std::string_view bytes, std::string_view name)
{
    for (const auto& probed : formatRegistry().probeAll(probeOf(bytes, name))) {
        if (probed.formatId == kId) {
            return probed.signature.confidence;
        }
    }
    return -1.0;
}

} // namespace

// ---- Registration and detection --------------------------------------------------------

TEST(OpcodeFieldFile, IsRegisteredAsAnImportableFormatReadingSetupsShotsAndPoints)
{
    const auto format = formatRegistry().find(kId);
    ASSERT_TRUE(format.ok());
    EXPECT_TRUE(format->canImport);
    EXPECT_EQ(format->extensions, std::vector<std::string>{"fld"});
    EXPECT_TRUE(format->reads.points && format->reads.observations && format->reads.stations &&
                format->reads.features && format->reads.coordinateSystem);
    // A GNSS position written as an 02 is a grid coordinate, not a GNSS
    // observation: the format carries none of those.
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

TEST(OpcodeFieldFile, AGnssJobOfBlankPaddedOpcodesAndBareRecordsIsIdentified)
{
    // gnss.fld's 31 records: five " 2" (a blank before the opcode), one "02",
    // one bare "20" and the groups and attributes. Each is a record, and the
    // six coordinates are what makes it a survey.
    const std::string bytes = fixture("fld/gnss.fld");
    const Detection detection = detectFormat(probeOf(bytes, "gnss.fld"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kId);
    EXPECT_NE(detection.candidates().front().evidence.find("31 of 31 records"), std::string::npos)
        << detection.candidates().front().evidence;
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

// Every file under tests/surveyio/data, whichever format wrote it: this
// probe rules out each that is not a .fld, and every .fld is identified as
// this format. The table printed ("detect <file> ...") is what a change to a
// probe is compared by, before and after.
TEST(OpcodeFieldFile, TheProbeClaimsNoOtherFormatsFixtureOrSample)
{
    std::vector<std::filesystem::path> files;
    for (const auto& entry :
         std::filesystem::recursive_directory_iterator(topcon_test::dataDirectory())) {
        if (entry.is_regular_file()) {
            files.push_back(entry.path());
        }
    }
    std::sort(files.begin(), files.end());
    std::size_t others = 0;
    for (const std::filesystem::path& path : files) {
        const std::string bytes =
            fixture(std::filesystem::relative(path, topcon_test::dataDirectory()));
        const std::string name = path.filename().string();
        const bool truncated = bytes.size() > kProbeBytes;
        const std::string_view probed =
            truncated ? std::string_view(bytes).substr(0, kProbeBytes) : std::string_view(bytes);
        const Detection detection = detectFormat(probeOf(probed, name, truncated));
        std::string candidates;
        for (const FormatCandidate& candidate : detection.candidates()) {
            candidates += (candidates.empty() ? "" : ",") + candidate.formatId + ":" +
                          std::to_string(static_cast<int>(candidate.confidence * 100.0 + 0.5));
        }
        std::cout << "detect "
                  << std::filesystem::relative(path, topcon_test::dataDirectory())
                         .generic_string()
                  << " outcome=" << toString(detection.outcome())
                  << " format=" << (detection.format() ? detection.format()->id : "-")
                  << " candidates=" << (candidates.empty() ? "-" : candidates) << "\n";
        if (path.extension() == ".fld") {
            EXPECT_EQ(detection.outcome(), DetectionOutcome::Identified)
                << name << ": " << detection.summary();
            if (detection.format()) {
                EXPECT_EQ(detection.format()->id, kId) << name;
            }
        } else {
            EXPECT_EQ(fieldFileConfidence(probed, name), 0.0) << path.string();
            ++others;
        }
    }
    EXPECT_GE(others, 20u) << "every other format's fixtures";
    for (const topcon_test::ForeignSample& sample : topcon_test::kForeignSamples) {
        EXPECT_EQ(fieldFileConfidence(sample.bytes, sample.name), 0.0) << sample.name;
    }
}

// ---- The total-station fixture ---------------------------------------------------------

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
    // "// Field File Version 6" is no "Key : value": it is kept as written.
    EXPECT_EQ(project.metadata.at("header"), "Field File Version 6");
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
    // and record 21 (opcode 18, a circle feature, which the reader does not
    // import).
    EXPECT_EQ(result.recordsRead, 16u);
    EXPECT_EQ(result.recordsSkipped, 2u);
    EXPECT_TRUE(warned(result, 20, "which value is which cannot be told"));
    EXPECT_TRUE(warned(result, 21, "opcode 18 (circle feature) is not one this reader imports"));
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
    // A setup is a 03 or, since the reader reads resections, a 128.
    EXPECT_TRUE(warned(*read, 1, "with no setup (opcode 03 or 128) before it"));
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

// ---- The RTK fixture -------------------------------------------------------------------

TEST(OpcodeFieldFile, TheWholeGnssJobIsReadWithNothingSkipped)
{
    const ReadResult result = readFixture("gnss.fld");
    // The version line and the 31 records of lines 11-37 and 41-44; the
    // blank lines and comments are not records.
    EXPECT_EQ(result.recordsRead, 32u);
    EXPECT_EQ(result.recordsSkipped, 0u);
    // The file is not UTF-8 (record 0), and R001 is measured again at
    // record 41 4 mm, 3 mm and 5 mm from where record 11 put it.
    ASSERT_EQ(result.warnings.size(), 2u);
    EXPECT_TRUE(warned(result, 0, "the file is not UTF-8"));
    EXPECT_TRUE(warned(result, 41, "point 'R001' is given different coordinates here"));
    const survey::SurveyProject& project = result.project;
    EXPECT_TRUE(project.stations.empty());
    EXPECT_EQ(project.points.size(), 5u); // R001 to R004 and CM1
    EXPECT_TRUE(project.unpositionedPoints.empty());
    EXPECT_EQ(project.coordinateSystem.name, "Local grid A");
    EXPECT_EQ(project.metadata.at("header: Ellipsoid"), "GRS 1980");
    // Line 9 comes before the first coordinate, so it is the header's; line
    // 39, after it, is a note.
    EXPECT_EQ(project.metadata.at("header"),
              "Field file of an RTK job, hand-built for the tests\n"
              "Antenna Height Change 1.800");
    EXPECT_EQ(project.metadata.at("notes"), "Antenna Height Change 2.000");
}

TEST(OpcodeFieldFile, AnRtkPositionWrittenAs02IsCalculatedAndAKeyedInOneStaysEntered)
{
    const ReadResult result = readFixture("gnss.fld");
    const survey::SurveyProject& project = result.project;
    const survey::SurveyPoint* r001 = topcon_test::point(project, "R001");
    ASSERT_NE(r001, nullptr);
    // X is the easting; the first measurement's coordinates are kept.
    EXPECT_DOUBLE_EQ(r001->easting, 500010.0);
    EXPECT_DOUBLE_EQ(r001->northing, 6200020.0);
    ASSERT_TRUE(r001->elevation.has_value());
    EXPECT_DOUBLE_EQ(*r001->elevation, 12.5);
    // R001 to R003 open a "GPS Information" group; R004's group is
    // "Solution", but it holds a "GNSS Solution" attribute.
    for (const char* id : {"R001", "R002", "R003", "R004"}) {
        const survey::SurveyPoint* point = topcon_test::point(project, id);
        ASSERT_NE(point, nullptr) << id;
        EXPECT_EQ(point->coordinateSource, survey::CoordinateSource::Calculated) << id;
    }
    // CM1 has no GNSS attribute: keyed in.
    const survey::SurveyPoint* cm1 = topcon_test::point(project, "CM1");
    ASSERT_NE(cm1, nullptr);
    EXPECT_EQ(cm1->coordinateSource, survey::CoordinateSource::Entered);
    EXPECT_TRUE(project.controlPoints.empty());
}

TEST(OpcodeFieldFile, ATimeStampInTheColumnAfterTheOpcodeIsKeptOnThePointItDates)
{
    const ReadResult result = readFixture("gnss.fld");
    const auto r002 = metadataOf(result.project, "R002");
    EXPECT_EQ(valueOf(r002, "time stamp"), "14/03/26 09:15:40.10");
    // CM1's column is empty: no time stamp.
    EXPECT_EQ(valueOf(metadataOf(result.project, "CM1"), "time stamp"), "(absent)");

    // [FLD] 44.6's time_text in a total-station file dates the setup and the
    // shot; on an attribute it is not kept, which is said once.
    const ReadResult w3c = readLines({
        "03\t2015-09-28T06:40:00Z\t\t\tS\t\t\t1.5",
        "07\t2015-09-28T06:42:45Z\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "73\t2015-09-28T06:42:46Z\tDate\t2015-09-28",
        "73\t2015-09-28T06:42:47Z\tTime\t06:42:45",
    });
    ASSERT_EQ(w3c.project.stations.size(), 1u);
    EXPECT_EQ(valueOf(w3c.project.stations[0].metadata, "time stamp"), "2015-09-28T06:40:00Z");
    const auto shot = metadataOf(w3c.project, "1");
    EXPECT_EQ(valueOf(shot, "time stamp"), "2015-09-28T06:42:45Z");
    EXPECT_EQ(valueOf(shot, "Date"), "2015-09-28");
    EXPECT_EQ(valueOf(shot, "Time"), "06:42:45");
    EXPECT_TRUE(warned(w3c, 3, "is not kept"));
    EXPECT_FALSE(warned(w3c, 4, "is not kept"));
    EXPECT_EQ(w3c.recordsSkipped, 0u);
}

TEST(OpcodeFieldFile, AColumnHoldingTextThatIsNotADateIsSkippedNotReadAsAValue)
{
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "07\tnote\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
    });
    EXPECT_TRUE(warned(result, 3,
                       "holds 'note' in the column after the opcode, which is neither empty nor a "
                       "date and time"));
    EXPECT_EQ(topcon_test::unpositioned(result.project, "2"), nullptr);
    EXPECT_EQ(result.recordsSkipped, 1u);
}

TEST(OpcodeFieldFile, AttributesInsideAGroupKeepTheGroupAndARemeasuredMarkKeepsBothMeasurements)
{
    const ReadResult result = readFixture("gnss.fld");
    const auto r001 = metadataOf(result.project, "R001");
    EXPECT_EQ(valueOf(r001, "Survey Attributes/Kerb height"), "0.150");
    EXPECT_EQ(valueOf(r001, "GPS Information/Antenna Height"), "1.800");
    EXPECT_EQ(valueOf(r001, "GPS Information/GNSS Solution"), "fixed");
    // 0xB0 in a Windows-1252 file is the degree sign, U+00B0.
    EXPECT_EQ(valueOf(r001, "GPS Information/Tilt"),
              "0\xC2\xB0"
              "10'00\"");
    // The reference station's coordinates, not R001's.
    EXPECT_EQ(valueOf(r001, "Reference Information/Easting"), "500500.000");
    EXPECT_EQ(valueOf(r001, "Reference Information/Northing"), "6200500.000");
    EXPECT_EQ(valueOf(r001, "time stamp"), "14/03/26 09:15:02.40");
    // Record 41 measures R001 again: its coordinates, time and attributes are
    // the second measurement's, kept beside the first's.
    EXPECT_EQ(valueOf(r001, "coordinates restated at record 41"),
              "N 6200020.003, E 500010.004, elevation 12.505");
    EXPECT_EQ(valueOf(r001, "record 41/time stamp"), "14/03/26 09:30:00.00");
    EXPECT_EQ(valueOf(r001, "record 41/GPS Information/Antenna Height"), "2.000");
    EXPECT_EQ(r001.count("Antenna Height"), 0u);
}

TEST(OpcodeFieldFile, ACloseStringRecordClosesTheCurrentStringAndALaterPointStartsANewOne)
{
    const ReadResult result = readFixture("gnss.fld");
    // Record 32, a bare 20, closes KB 1 - the string of R003, the current
    // measurement point. R004, coded KB 1 after it, starts a new string.
    const auto kb = featuresOf(result.project, "KB");
    ASSERT_EQ(kb.size(), 2u);
    EXPECT_EQ(kb[0]->name, "1");
    EXPECT_TRUE(kb[0]->closed);
    EXPECT_EQ(kb[0]->pointIds, (std::vector<std::string>{"R001", "R002", "R003"}));
    EXPECT_EQ(kb[1]->name, "1");
    EXPECT_FALSE(kb[1]->closed);
    EXPECT_EQ(kb[1]->pointIds, (std::vector<std::string>{"R004"}));
    EXPECT_EQ(result.project.features.size(), 2u);
}

// ---- The resection fixture -------------------------------------------------------------

TEST(OpcodeFieldFile, TheWholeResectionJobIsReadWithNothingSkipped)
{
    const ReadResult result = readFixture("resection.fld");
    // The version line, 100, 09, two 02, 128, 5, 7, 7, 5, 7, 129 (lines 2 and
    // 8-17) and the 15 records of lines 22-36: 27.
    EXPECT_EQ(result.recordsRead, 27u);
    EXPECT_EQ(result.recordsSkipped, 0u);
    // Only the two offsets, which are not applied.
    ASSERT_EQ(result.warnings.size(), 2u);
    const survey::SurveyProject& project = result.project;
    ASSERT_EQ(project.stations.size(), 2u);
    EXPECT_EQ(project.points.size(), 2u); // K1 and K2
    // S1, 101, 102 and 103, in the order first named.
    ASSERT_EQ(project.unpositionedPoints.size(), 4u);
    EXPECT_EQ(project.unpositionedPoints[0].id, "S1");
    EXPECT_EQ(project.unpositionedPoints[3].id, "103");
    EXPECT_EQ(project.coordinateSystem.name, "Local grid B");
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(
        result, "no coordinates for the 1 setup(s) made by resection"));
}

TEST(OpcodeFieldFile, AResectionStartsASetupOnItsNamedPointAndItsShotsAreItsOwn)
{
    const ReadResult result = readFixture("resection.fld");
    ASSERT_EQ(result.project.stations.size(), 2u);
    const survey::SurveyStation& s1 = result.project.stations[0];
    EXPECT_EQ(s1.setup.pointId, "S1");
    EXPECT_DOUBLE_EQ(s1.setup.instrumentHeight, 1.55);
    EXPECT_TRUE(s1.backsightPointId.empty());
    EXPECT_EQ(valueOf(s1.metadata, "resection"), "least squares, record 11");
    EXPECT_EQ(valueOf(s1.metadata, "resection end"), "record 17");
    // Inside the block, "7" is opcode 07: K1 on both faces - 180 and 0
    // degrees, zenith 90 on face left and 270 on face right - and K2 at 135
    // degrees under the target height "5" set to 1.700.
    const auto toK1 = observationsTo<survey::HorizontalDirectionObservation>(s1, "K1");
    ASSERT_EQ(toK1.size(), 2u);
    EXPECT_NEAR(toK1[0].direction, kPi, kAngleTolerance);
    EXPECT_NEAR(toK1[1].direction, 0.0, kAngleTolerance);
    const auto zenithsK1 = observationsTo<survey::ZenithAngleObservation>(s1, "K1");
    ASSERT_EQ(zenithsK1.size(), 2u);
    EXPECT_EQ(zenithsK1[0].pointing.face, survey::Face::Left);
    EXPECT_EQ(zenithsK1[1].pointing.face, survey::Face::Right);
    EXPECT_NEAR(zenithsK1[1].angle, kPi / 2.0, kAngleTolerance);
    EXPECT_DOUBLE_EQ(zenithsK1[0].targetHeight, 1.6);
    const auto toK2 = observationsTo<survey::DistanceObservation>(s1, "K2");
    ASSERT_EQ(toK2.size(), 2u); // the resection's, and the check of record 31
    EXPECT_DOUBLE_EQ(toK2[0].distance, 141.421);
    EXPECT_DOUBLE_EQ(toK2[0].targetHeight, 1.7);
    EXPECT_DOUBLE_EQ(toK2[1].distance, 141.42);
    // After the 129 the shots are still S1's: 101 at 90 degrees, 102 at 95.
    const auto to101 = observationsTo<survey::HorizontalDirectionObservation>(s1, "101");
    ASSERT_EQ(to101.size(), 1u);
    EXPECT_NEAR(to101[0].direction, kPi / 2.0, kAngleTolerance);
    EXPECT_EQ(observationsTo<survey::HorizontalDirectionObservation>(s1, "102").size(), 1u);
    // Six pointings of three observations each.
    EXPECT_EQ(s1.observations.size(), 18u);
    // The 03 at line 33 is the next setup, with its backsight and its shot.
    const survey::SurveyStation& k1 = result.project.stations[1];
    EXPECT_EQ(k1.setup.pointId, "K1");
    EXPECT_EQ(k1.backsightPointId, "K2");
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(k1, "103").size(), 1u);
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(k1, "101").size(), 0u);
}

TEST(OpcodeFieldFile, TheResectionResidualCommentsAreNotesOnTheResectedSetup)
{
    const ReadResult result = readFixture("resection.fld");
    const survey::SurveyStation& s1 = result.project.stations.at(0);
    // Verbatim, the delta and degree signs as UTF-8; the rules are dropped.
    EXPECT_EQ(valueOf(s1.metadata, "notes"),
              "Station Setup - Residuals\n"
              "ID: K1 \xCE\x94"
              "E:0.001 \xCE\x94"
              "N:-0.002 \xCE\x94"
              "HA:0\xC2\xB0"
              "00'01\"");
    // Nothing of the residual lines became header metadata.
    EXPECT_EQ(result.project.metadata.count("header: ID"), 0u);
}

TEST(OpcodeFieldFile, MultipleCodingStringsTheCurrentPointIntoASecondFeature)
{
    const ReadResult result = readFixture("resection.fld");
    const survey::SurveyProject& project = result.project;
    // 101 (EB 01) is coded NS 01 by record 25, and 102 PF (no string number)
    // by record 30; each keeps its own code.
    const survey::SurveyFeature* ns = topcon_test::feature(project, "NS");
    ASSERT_NE(ns, nullptr);
    EXPECT_EQ(ns->name, "01");
    EXPECT_EQ(ns->pointIds, (std::vector<std::string>{"101"}));
    const survey::SurveyFeature* pf = topcon_test::feature(project, "PF");
    ASSERT_NE(pf, nullptr);
    EXPECT_EQ(pf->name, "");
    EXPECT_EQ(pf->pointIds, (std::vector<std::string>{"102"}));
    const survey::SurveyFeature* eb = topcon_test::feature(project, "EB");
    ASSERT_NE(eb, nullptr);
    EXPECT_EQ(eb->pointIds, (std::vector<std::string>{"101", "102", "103"}));
    EXPECT_EQ(topcon_test::unpositioned(project, "101")->code, "EB");
    EXPECT_EQ(project.features.size(), 3u);
}

TEST(OpcodeFieldFile, ACheckMeasurementKeepsItsCodeWithoutStringingItsTarget)
{
    const ReadResult result = readFixture("resection.fld");
    // Record 31 checks K2 and is coded BS 11: no BS string, the code kept.
    EXPECT_EQ(topcon_test::feature(result.project, "BS"), nullptr);
    const auto k2 = metadataOf(result.project, "K2");
    EXPECT_EQ(valueOf(k2, "check measurement"), "record 31, feature code BS, string number 11");
    // The attribute after the check describes its target.
    EXPECT_EQ(valueOf(k2, "Time"), "10:15:00");
    EXPECT_EQ(topcon_test::point(result.project, "K2")->code, "");
}

TEST(OpcodeFieldFile, ABacksightsCodeIsKeptWithItsSetupAndStringsNothing)
{
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\tBS\t02\tB\t\tbacksight mark\t0.0\t90.0\t100.0",
    });
    ASSERT_EQ(result.project.stations.size(), 1u);
    EXPECT_EQ(valueOf(result.project.stations[0].metadata, "backsight record 4"),
              "feature code BS, string number 02, comment backsight mark");
    EXPECT_TRUE(result.project.features.empty());
    const survey::SurveyPoint* b = topcon_test::point(result.project, "B");
    ASSERT_NE(b, nullptr);
    EXPECT_EQ(b->code, "");
    EXPECT_EQ(b->description, "");
    EXPECT_EQ(b->metadata.count("backsight"), 0u);
}

TEST(OpcodeFieldFile, OffsetsAreKeptWithTheirSetupAndWarnedOfNotApplied)
{
    const ReadResult result = readFixture("resection.fld");
    EXPECT_EQ(valueOf(metadataOf(result.project, "101"), "offset record 24"),
              "tangential 0.25 m from setup S1");
    EXPECT_EQ(valueOf(metadataOf(result.project, "102"), "offset record 29"),
              "radial 0.5 m from setup S1");
    EXPECT_TRUE(warned(result, 24, "the tangential offset (opcode 43) of 0.25 m to point '101'"));
    EXPECT_TRUE(warned(result, 29, "the radial offset (opcode 42) of 0.5 m to point '102'"));
    EXPECT_TRUE(warned(result, 29, "not applied"));
}

TEST(OpcodeFieldFile, AnIntegerAttributeIsKeptLikeTheOthers)
{
    const ReadResult result = readFixture("resection.fld");
    const auto p101 = metadataOf(result.project, "101");
    EXPECT_EQ(valueOf(p101, "Tree Spread"), "5");
    EXPECT_EQ(valueOf(p101, "Date"), "2026-03-14");

    const ReadResult notWhole = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tPT\t1\t1\t\t\t10.0\t90.0\t5.0",
        "71\t\tTree Spread\t4.5",
    });
    EXPECT_TRUE(warned(notWhole, 3, "which is not a whole number; it is kept as text"));
    EXPECT_EQ(valueOf(metadataOf(notWhole.project, "1"), "Tree Spread"), "4.5");
}

// ---- Records that are not read, and what follows them -----------------------------------

TEST(OpcodeFieldFile, AttributesAfterAShotThatWasNotReadAreNotGivenToThePointBefore)
{
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
        "73\t\tDate\td1",
        "07\t\tEB\t01\t2\t\t\t20.0\t90.0", // a value short
        "73\t\tDate\td2",
        "72\t\tDepth\t0.6",
    });
    const auto p1 = metadataOf(result.project, "1");
    EXPECT_EQ(valueOf(p1, "Date"), "d1");
    EXPECT_EQ(p1.count("Depth"), 0u);
    EXPECT_TRUE(warned(result, 5, "of the point of record 4, which was not read"));
    EXPECT_TRUE(warned(result, 6, "of the point of record 4, which was not read"));
    EXPECT_EQ(result.recordsSkipped, 3u);
}

TEST(OpcodeFieldFile, ASetupThatCannotBeReadLeavesItsShotsUnreadRatherThanFiledUnderTheOneBefore)
{
    const ReadResult result = readLines({
        "03\t\t\t\tA\t\t\t1.5",
        "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
        "03\t\t\t\tB\t\t\t1.5\t9", // a value too many
        "07\t\tEB\t01\t2\t\t\t20.0\t90.0\t5.0",
        "128\t\tS\t1.5", // neither the height alone nor a description and it
        "07\t\tEB\t01\t3\t\t\t30.0\t90.0\t5.0",
    });
    ASSERT_EQ(result.project.stations.size(), 1u);
    const survey::SurveyStation& a = result.project.stations[0];
    EXPECT_EQ(a.observations.size(), 3u); // point 1 only
    EXPECT_TRUE(observationsTo<survey::DistanceObservation>(a, "2").empty());
    EXPECT_TRUE(observationsTo<survey::DistanceObservation>(a, "3").empty());
    EXPECT_TRUE(warned(result, 3, "the measurements after it are not read until the next setup"));
    EXPECT_TRUE(warned(result, 4, "measurement from the setup of record 3, which was not read"));
    EXPECT_TRUE(warned(result, 5, "resection (opcode 128) with 2 value(s)"));
    EXPECT_TRUE(warned(result, 6, "measurement from the setup of record 5, which was not read"));
    EXPECT_EQ(result.recordsSkipped, 4u);
}

TEST(OpcodeFieldFile, AResectionOfTheDescriptionsOwnFormNamesNoPointAndStillOwnsItsShots)
{
    // [FLD]'s .fld syntax: "128 instrument_height_value", no description.
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "128\t\t1.500",
        "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
        "129",
    });
    ASSERT_EQ(result.project.stations.size(), 1u);
    const survey::SurveyStation& setup = result.project.stations[0];
    EXPECT_EQ(setup.setup.pointId, "resection at record 2");
    EXPECT_DOUBLE_EQ(setup.setup.instrumentHeight, 1.5);
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(setup, "1").size(), 1u);
    EXPECT_EQ(valueOf(setup.metadata, "resection end"), "record 4");
    EXPECT_EQ(result.recordsSkipped, 0u);
    // A 129 with no resection open says so.
    const ReadResult stray = readLines({"03\t\t\t\tS\t\t\t1.5", "129"});
    EXPECT_TRUE(warned(stray, 2, "end of a resection (opcode 129) with no resection open"));
}

TEST(OpcodeFieldFile, CloseStringByItsCodeOrItsPointAndWhenThereIsNoneToClose)
{
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "07\t\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
        "07\t\tEP\t1\t3\t\t\t30.0\t90.0\t5.0",
        "20\t\tEP\t1\t\t\t", // closes EP 1 by its code and number
        "20",                // the current string, EP 1, is closed already
        "07\t\tFE\t\t4\t\t\t40.0\t90.0\t5.0",
        "07\t\tFE\t\t5\t\t\t50.0\t90.0\t5.0",
        "20\t\t\t\t4\t\t", // closes the string of point 4: FE, two points
        "07\t\t\t\t6\t\t\t60.0\t90.0\t5.0",
        "20", // point 6 has no feature code: no current string
        "07\t\tEP\t1\t7\t\t\t70.0\t90.0\t5.0",
    });
    const auto ep = featuresOf(result.project, "EP");
    ASSERT_EQ(ep.size(), 2u);
    EXPECT_TRUE(ep[0]->closed);
    EXPECT_EQ(ep[0]->pointIds, (std::vector<std::string>{"1", "2", "3"}));
    EXPECT_FALSE(ep[1]->closed);
    EXPECT_EQ(ep[1]->pointIds, (std::vector<std::string>{"7"}));
    const auto fe = featuresOf(result.project, "FE");
    ASSERT_EQ(fe.size(), 1u);
    EXPECT_TRUE(fe[0]->closed);
    EXPECT_TRUE(warned(result, 6, "there is no open string EP 1"));
    EXPECT_TRUE(warned(result, 9, "is closed with 2 point(s)"));
    EXPECT_TRUE(warned(result, 11, "the point of record 10 has no feature code"));
    EXPECT_EQ(result.recordsRead, 10u);
    EXPECT_EQ(result.recordsSkipped, 2u);
}

TEST(OpcodeFieldFile, OffsetsNamingAPointOrAStringAHeightOffsetAndOffsetsThatCannotBeRead)
{
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "07\t\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
        "44\t\t0.1",                // height offset of the current point, 2
        "43\t\t\t\t1\t\t\t-0.3",     // tangential offset of point 1, by its ID
        "42\t\tEP\t1\t\t\t\t0.2",    // radial offset of EP 1's last point, 2
        "42\t\t0.5\t0.0\t0.0",       // three values: neither of the two forms
        "42\t\tabc",                 // not a number
    });
    const auto p1 = metadataOf(result.project, "1");
    const auto p2 = metadataOf(result.project, "2");
    EXPECT_EQ(valueOf(p2, "offset record 4"), "height 0.1 m");
    EXPECT_EQ(valueOf(p1, "offset record 5"), "tangential -0.3 m from setup S");
    EXPECT_EQ(valueOf(p2, "offset record 6"), "radial 0.2 m from setup S");
    EXPECT_TRUE(warned(result, 7, "with 3 value(s) where the format gives the offset"));
    EXPECT_TRUE(warned(result, 8, "radial offset (opcode 42) with no offset"));
    EXPECT_EQ(result.recordsSkipped, 2u);
}

TEST(OpcodeFieldFile, TheFileEndRecordStopsTheReadAndCountsWhatFollows)
{
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "99",
        "07\t\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
        "// a comment after the end",
        "73\t\tDate\td",
    });
    EXPECT_EQ(result.recordsRead, 3u);
    EXPECT_EQ(result.recordsSkipped, 2u);
    EXPECT_TRUE(warned(result, 3, "2 record(s) after the end of the file (opcode 99)"));
    EXPECT_EQ(topcon_test::unpositioned(result.project, "2"), nullptr);
    EXPECT_EQ(metadataOf(result.project, "1").count("Date"), 0u);
}

TEST(OpcodeFieldFile, AnOpcodeOnlyTheXmlFormHasAndOneTheFormatDoesNotDefineAreSkippedSayingSo)
{
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "140\t\tEP\t1\t9\t\t\t1.0\t2.0\t3.0",
        "73\t\tDate\td",
        "8\t\tx",
        "15\t\t0.001",
    });
    EXPECT_TRUE(warned(result, 3,
                       "opcode 140 (GNSS coordinate) is defined only for the format's XML form"));
    // 140 makes a point in [FLD]: what follows it is not given to point 1.
    EXPECT_TRUE(warned(result, 4, "of the point of record 3, which was not read"));
    EXPECT_EQ(metadataOf(result.project, "1").count("Date"), 0u);
    EXPECT_TRUE(warned(result, 5, "opcode 08 is not one the format defines"));
    EXPECT_TRUE(warned(result, 6,
                       "opcode 15 (vertical circle correction) is not one this reader imports; "
                       "the measurements after it are read without it"));
    EXPECT_EQ(result.recordsSkipped, 4u);
}

TEST(OpcodeFieldFile, AnUnnamedAttributeInAFileWithoutTheColumnIsKeptUnderItsRecord)
{
    // "If there is no name for the attribute (name is just spaces or a tab),
    // then the attribute is unnamed": without the column, "72<tab><tab>0.5"
    // is a blank name and 0.5.
    const ReadResult result = readLines({
        "100\tdegrees\tmetres",
        "03\t\t\tA\t\t\t1.5",
        "07\tTREE\t\t7\t\t\t90.0\t90.0\t12.5",
        "72\t\t0.5",
        "72\tDepth\t1.5",
    });
    const auto p7 = metadataOf(result.project, "7");
    EXPECT_EQ(valueOf(p7, "unnamed attribute (record 4)"), "0.5");
    EXPECT_EQ(valueOf(p7, "Depth"), "1.5");
    EXPECT_EQ(p7.count("0.5"), 0u);
}

TEST(OpcodeFieldFile, AdditionalTextKeepsItsSpaces)
{
    // "any spaces from column four onwards will be part of the text"
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "41\t\t  indented text  ",
    });
    EXPECT_EQ(valueOf(metadataOf(result.project, "1"), "additional text 3"), "  indented text  ");
}

TEST(OpcodeFieldFile, AScaleFactorIsInTheDistancesAndTheSetupSaysItIsApplied)
{
    const ReadResult result = readLines({
        "09\t\t1.0002",
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t100.000",
    });
    ASSERT_EQ(result.project.stations.size(), 1u);
    const survey::SurveyStation& station = result.project.stations[0];
    ASSERT_TRUE(station.instrument.scaleFactor.has_value());
    EXPECT_DOUBLE_EQ(*station.instrument.scaleFactor, 1.0002);
    // The reader multiplied the distance (100 m x 1.0002 = 100.02 m), so a
    // reduction must not apply the factor again.
    EXPECT_EQ(station.instrument.scaleFactorState, survey::CorrectionState::Applied);
    const auto distances = observationsTo<survey::DistanceObservation>(station, "1");
    ASSERT_EQ(distances.size(), 1u);
    EXPECT_NEAR(distances[0].distance, 100.02, 1e-9);
}

TEST(OpcodeFieldFile, AttributeGroupsNestAndAGroupEndedTwiceOrLeftOpenIsWarnedOf)
{
    const ReadResult result = readLines({
        "02\t\t\t\tP\t\t\t1000.0\t5000.0\t10.0",
        "124\t\t\tOuter\t0",
        "124\t\t\tInner\t1",
        "73\t\tA\t1",
        "125\t\t\t\t1",
        "73\t\tB\t2",
        "125\t\t\t\t0",
        "125\t\t\t\t0", // no group open
        "124\t\t\tLeft open\t3",
        "73\t\tC\t3",
    });
    const auto p = metadataOf(result.project, "P");
    EXPECT_EQ(valueOf(p, "Outer/Inner/A"), "1");
    EXPECT_EQ(valueOf(p, "Outer/B"), "2");
    EXPECT_EQ(valueOf(p, "Left open/C"), "3");
    EXPECT_TRUE(warned(result, 8, "end of an attribute group (opcode 125) with no group open"));
    EXPECT_TRUE(warned(result, 9, "gives the level '3' and opens inside 0 group(s)"));
    EXPECT_TRUE(warned(result, 9, "is not ended (opcode 125) before the file ends"));
    EXPECT_EQ(result.recordsSkipped, 1u);
    // No group names GPS or GNSS, and no attribute is a GNSS solution: P
    // stays a keyed-in coordinate.
    EXPECT_EQ(topcon_test::point(result.project, "P")->coordinateSource,
              survey::CoordinateSource::Entered);
}

TEST(OpcodeFieldFile, MultipleCodingThatNamesItsOwnPointOrHasNoCodeIsWarnedOf)
{
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "16\t\tNS\t01\t\t\t", // no measurement yet
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "16\t\tNS\t01\tX9\t\tpole",
        "16\t\t\t\t\t\t", // no feature code
        "16\t\tNS",      // one value
    });
    EXPECT_TRUE(warned(result, 2, "with no measurement (opcode 02 or 07) before it"));
    const survey::SurveyFeature* ns = topcon_test::feature(result.project, "NS");
    ASSERT_NE(ns, nullptr);
    EXPECT_EQ(ns->pointIds, (std::vector<std::string>{"1"}));
    EXPECT_EQ(valueOf(metadataOf(result.project, "1"), "multiple coding record 4"),
              "NS 01, point ID X9, comment pole");
    EXPECT_TRUE(warned(result, 4, "names point 'X9'"));
    EXPECT_EQ(topcon_test::unpositioned(result.project, "X9"), nullptr);
    EXPECT_TRUE(warned(result, 5, "with no feature code"));
    EXPECT_TRUE(warned(result, 6, "with 1 value(s) where the format gives a point description"));
    EXPECT_EQ(result.recordsSkipped, 3u);
}

TEST(OpcodeFieldFile, ReadsEveryTruncationAndNoiseOfItsFixturesWithoutCrashing)
{
    std::size_t reads = 0;
    for (const char* name : {"setup.fld", "gnss.fld", "resection.fld"}) {
        reads += topcon_test::readEveryTruncationAndNoise(
            kId, fixture(std::filesystem::path("fld") / name), name);
    }
    EXPECT_GT(reads, 3000u);
}
