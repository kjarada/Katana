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
//                           opcode) with a date and time in the column, each
//                           with its receiver's "GNSS Solution" in a 124 ... 125
//                           attribute group, a keyed-in 02, a bare 20, a mark
//                           measured again, comments; Windows-1252, one 0xB0
//                           degree sign.
//   data/fld/resection.fld  a resection (128 ... 129, with "5" and "7" written
//                           without their zero), its residuals as comments, 16,
//                           42, 43, 71, a coded check, then a 03 setup; UTF-8.
//   data/fld/rtk_setup.fld  two RTK marks, a setup on one backsighting the
//                           other, and two shots with an offset each.
//   data/fld/two_backsights.fld  a setup backsighting an entered mark and
//                           then a point nothing places, and a shot (the
//                           field file cli tests draw it).
// Every expected value below is worked from the record text: circle readings
// are decimal degrees, so 45.00000000 is pi / 4.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <iostream>
#include <map>
#include <numbers>
#include <string>
#include <variant>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/survey/reduction.hpp"
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
// Plan positions through the reduction: its arithmetic on six-figure
// coordinates rounds at 1e-12 m, far inside this.
constexpr double kPositionTolerance = 1e-6;

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

// The GNSS positions a read job gives `id`, in file order.
std::vector<survey::GnssPositionObservation> gnssPositionsOf(const survey::SurveyProject& project,
                                                             std::string_view id)
{
    std::vector<survey::GnssPositionObservation> found;
    for (const survey::Observation& observation : project.observations) {
        const auto* position = std::get_if<survey::GnssPositionObservation>(&observation);
        if (position != nullptr && position->point == id) {
            found.push_back(*position);
        }
    }
    return found;
}

// The reduction of a read job: its defaults, without curvature and
// refraction, so that a position is the plan geometry a test works by hand.
survey::ReductionOutcome reduced(const survey::SurveyProject& project)
{
    survey::ReductionSettings settings;
    settings.curvatureAndRefraction = false;
    auto outcome = survey::reduceAndAdjust(project, settings, survey::ReductionContext{});
    EXPECT_TRUE(outcome.ok()) << (outcome.ok() ? "" : outcome.error().describe());
    return outcome.ok() ? std::move(outcome).value() : survey::ReductionOutcome{};
}

// The positions the reduction gives a read job, by point id.
std::map<std::string, survey::ComputedPoint> reducedPositions(const survey::SurveyProject& project)
{
    std::map<std::string, survey::ComputedPoint> positions;
    for (const survey::ComputedPoint& point : reduced(project).points) {
        positions[point.id] = point;
    }
    return positions;
}

// How many of the reduction's warnings hold `words`.
std::size_t reductionWarningsWith(const survey::ReductionReport& report, std::string_view words)
{
    return static_cast<std::size_t>(
        std::count_if(report.warnings.begin(), report.warnings.end(),
                      [&](const survey::ReportMessage& warning) {
                          return warning.text.find(words) != std::string::npos;
                      }));
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
    // An RTK position written as an 02 with its receiver's solution is a GNSS
    // position.
    EXPECT_TRUE(format->reads.gnss);
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
    // gnss.fld's 34 records: five " 2" (a blank before the opcode), one "02",
    // one bare "20" and the groups and attributes. Each is a record, and the
    // six coordinates are what makes it a survey.
    const std::string bytes = fixture("fld/gnss.fld");
    const Detection detection = detectFormat(probeOf(bytes, "gnss.fld"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kId);
    EXPECT_NE(detection.candidates().front().evidence.find("34 of 34 records"), std::string::npos)
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

TEST(OpcodeFieldFile, ANumberedListOfPointsIsNotTakenForAFieldFile)
{
    // Tab-separated rows numbered 1 to 60, right-aligned: each opens with a
    // number, and rows 2, 3 and 7 with a coordinate's, setup's or shot's
    // opcode - but with five fields, where those records have nine or ten
    // (seven or eight for a setup).
    std::string rows;
    std::string numbers;
    for (int row = 1; row <= 60; ++row) {
        const std::string number = std::to_string(row);
        rows += std::string(4 - number.size(), ' ') + number + "\t" +
                std::to_string(300000 + row) + ".500\t" + std::to_string(6250000 + row) +
                ".250\t50.010\tTREE\r\n";
        numbers += number + "\r\n";
    }
    EXPECT_EQ(fieldFileConfidence(rows, "points.txt"), 0.0);
    EXPECT_EQ(fieldFileConfidence(numbers, "numbers.txt"), 0.0);
}

TEST(OpcodeFieldFile, AFieldFileBehindALongHeaderOfCommentsIsIdentified)
{
    // 250 comment lines, past the 200 lines the probe once looked at, then
    // a job: comments are no records, and the records decide.
    std::string bytes = "{Version 6.0}\r\n";
    for (int line = 0; line < 250; ++line) {
        bytes += "// header line " + std::to_string(line) + "\r\n";
    }
    bytes += "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0\r\n"
             "03\t\t\t\tK\t\t\t1.5\r\n"
             "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0\r\n";
    const Detection detection = detectFormat(probeOf(bytes, "long_header.fld"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kId);
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
    // CP2 was made by record 9: the check of record 22 is that record's.
    EXPECT_EQ(topcon_test::point(project, "CP2")->metadata.at("record 22/check measurement"),
              "from setup CP1");
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
    // The version line and the 34 records of lines 11-37 and 41-47; the
    // blank lines and comments are not records.
    EXPECT_EQ(result.recordsRead, 35u);
    EXPECT_EQ(result.recordsSkipped, 0u);
    // The file is not UTF-8 (record 0), and R001's second GNSS position, of
    // record 41, is 4 mm, 3 mm and 5 mm from where record 11 put it.
    ASSERT_EQ(result.warnings.size(), 2u);
    EXPECT_TRUE(warned(result, 0, "the file is not UTF-8"));
    EXPECT_TRUE(warned(result, 41,
                       "point 'R001' is given a GNSS position here (N 6200020.003, E 500010.004, "
                       "elevation 12.505 m) that differs from the GNSS position of record 11"));
    const survey::SurveyProject& project = result.project;
    EXPECT_TRUE(project.stations.empty());
    // CM1 is keyed in; R001 to R004 are the receiver's, placed by their five
    // GNSS positions, R001's two among them.
    ASSERT_EQ(project.points.size(), 1u);
    EXPECT_EQ(project.points[0].id, "CM1");
    EXPECT_EQ(project.unpositionedPoints.size(), 4u);
    EXPECT_EQ(project.observations.size(), 5u);
    EXPECT_EQ(project.coordinateSystem.name, "Local grid A");
    EXPECT_EQ(project.metadata.at("header: Ellipsoid"), "GRS 1980");
    // Line 9 comes before the first coordinate, so it is the header's; line
    // 39, after it, is a note.
    EXPECT_EQ(project.metadata.at("header"),
              "Field file of an RTK job, hand-built for the tests\n"
              "Antenna Height Change 1.800");
    EXPECT_EQ(project.metadata.at("notes"), "Antenna Height Change 2.000");
}

TEST(OpcodeFieldFile, AnRtkPositionWithItsReceiversSolutionIsAGnssPositionAndAKeyedInOneIsEntered)
{
    const ReadResult result = readFixture("gnss.fld");
    const survey::SurveyProject& project = result.project;
    // R001, record 11: X the easting, Y the northing, Z the mark's height,
    // with the a-priori GNSS precision the read was given (the defaults).
    const auto r001 = gnssPositionsOf(project, "R001");
    ASSERT_EQ(r001.size(), 2u);
    EXPECT_DOUBLE_EQ(r001[0].easting, 500010.0);
    EXPECT_DOUBLE_EQ(r001[0].northing, 6200020.0);
    EXPECT_DOUBLE_EQ(r001[0].elevation, 12.5);
    const survey::ObservationPrecision defaults;
    EXPECT_EQ(r001[0].sigmaNorthing, defaults.gnssHorizontal);
    EXPECT_EQ(r001[0].sigmaEasting, defaults.gnssHorizontal);
    EXPECT_EQ(r001[0].sigmaElevation, defaults.gnssVertical);
    EXPECT_EQ(r001[0].source.recordNumber, 11u);
    // Its measurement of record 41 is a position of its own.
    EXPECT_DOUBLE_EQ(r001[1].easting, 500010.004);
    EXPECT_DOUBLE_EQ(r001[1].northing, 6200020.003);
    EXPECT_EQ(r001[1].source.recordNumber, 41u);
    // Each of R001 to R004 names its solution, R004's in a group of another
    // name: none has coordinates of its own, each a GNSS position.
    for (const char* id : {"R001", "R002", "R003", "R004"}) {
        EXPECT_EQ(topcon_test::point(project, id), nullptr) << id;
        EXPECT_NE(topcon_test::unpositioned(project, id), nullptr) << id;
    }
    EXPECT_EQ(gnssPositionsOf(project, "R004").size(), 1u);
    // CM1 names none: the format's directly entered coordinate.
    const survey::SurveyPoint* cm1 = topcon_test::point(project, "CM1");
    ASSERT_NE(cm1, nullptr);
    EXPECT_EQ(cm1->coordinateSource, survey::CoordinateSource::Entered);
    EXPECT_TRUE(gnssPositionsOf(project, "CM1").empty());
    // The attributes are kept as the receiver wrote them.
    EXPECT_EQ(valueOf(metadataOf(project, "R001"), "GPS Information/GNSS Solution"), "fixed");
    EXPECT_EQ(valueOf(metadataOf(project, "R002"), "GPS Information/GNSS Solution"), "float");
    EXPECT_TRUE(project.controlPoints.empty());
}

TEST(OpcodeFieldFile, TheReductionHoldsNoRtkMarkAsControlAndChecksAMarksSecondPosition)
{
    const survey::ReductionOutcome outcome = reduced(readFixture("gnss.fld").project);
    std::map<std::string, survey::ComputedPoint> points;
    for (const survey::ComputedPoint& point : outcome.points) {
        points[point.id] = point;
    }
    ASSERT_EQ(points.size(), 5u);
    // Where the file puts them: R001 by its first position, by GNSS; CM1,
    // keyed in, as the entered coordinate it is.
    EXPECT_EQ(points.at("R001").method, survey::ComputationMethod::Gnss);
    EXPECT_DOUBLE_EQ(points.at("R001").easting, 500010.0);
    EXPECT_DOUBLE_EQ(points.at("R001").northing, 6200020.0);
    EXPECT_EQ(points.at("R004").method, survey::ComputationMethod::Gnss);
    EXPECT_EQ(points.at("CM1").method, survey::ComputationMethod::Control);
    // R001's second position checks its first: 6200020.003 - 6200020,
    // 500010.004 - 500010 and 12.505 - 12.5, to the double's rounding of
    // seven-figure values (under 1e-9 m; 1e-8 allowed).
    ASSERT_EQ(outcome.report.misclosures.size(), 1u);
    const survey::MisclosureReport& check = outcome.report.misclosures[0];
    EXPECT_NE(check.name.find("R001 by a second GNSS position (record 41)"), std::string::npos)
        << check.name;
    ASSERT_TRUE(check.northing && check.easting && check.height);
    EXPECT_NEAR(*check.northing, 0.003, 1e-8);
    EXPECT_NEAR(*check.easting, 0.004, 1e-8);
    EXPECT_NEAR(*check.height, 0.005, 1e-8);
    // Drawn as calculated - by the receiver - not as entered control.
    const survey::SurveyPoint* r001 = topcon_test::point(outcome.reduced, "R001");
    ASSERT_NE(r001, nullptr);
    EXPECT_EQ(r001->coordinateSource, survey::CoordinateSource::Calculated);
    EXPECT_EQ(valueOf(r001->metadata, "GPS Information/GNSS Solution"), "fixed");
}

TEST(OpcodeFieldFile, OnlyTheReceiversSolutionAmongAnO2sOwnAttributesMakesItAGnssPosition)
{
    const ReadResult result = readLines({
        " 2\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0", // no attributes
        " 2\t\t\t\tB\t\t\t1010.0\t5000.0\t20.0",
        "124\t\t\tGPS Information\t0",
        "73\t\tAntenna Height\t2.000", // a receiver's group, but no solution
        "125\t\t\t\t0",
        " 2\t\t\t\tC\t\t\t1020.0\t5000.0\t20.0",
        "73\t\tgnss solution\tfixed", // in no group, in another case
        " 2\t\t\t\tD\t\t\t1030.0\t5000.0\t20.0",
        "03\t\t\t\tD\t\t\t1.5",
        // After a setup: not among the 02's own records, so D stays entered
        // (the attribute is still the current measurement point's, as [FLD]
        // gives 73 to it).
        "73\t\tGNSS Solution\tfixed",
        " 2\t\t\t\tE\t\t\t1040.0\t5000.0\t", // no Z
        "73\t\tGNSS Solution\tfixed",
    });
    for (const char* id : {"A", "B", "D", "E"}) {
        const survey::SurveyPoint* point = topcon_test::point(result.project, id);
        ASSERT_NE(point, nullptr) << id;
        EXPECT_EQ(point->coordinateSource, survey::CoordinateSource::Entered) << id;
        EXPECT_TRUE(gnssPositionsOf(result.project, id).empty()) << id;
    }
    EXPECT_EQ(topcon_test::point(result.project, "C"), nullptr);
    EXPECT_EQ(gnssPositionsOf(result.project, "C").size(), 1u);
    EXPECT_TRUE(warned(result, 11,
                       "point 'E' is a receiver's solution (its 'GNSS Solution' attribute) with "
                       "no height, which a GNSS position has; it is kept as an entered "
                       "coordinate"));
    EXPECT_EQ(result.recordsSkipped, 0u);
}

TEST(OpcodeFieldFile, AnEnteredCoordinateAfterAGnssPositionOfItsPointKeepsTheGnssPositionBesideIt)
{
    // A mark measured by RTK, then keyed in 50 mm away in plan and height.
    // The entered coordinate is what the point holds, as in the other order;
    // what goes beside it is the position it does not hold, the GNSS one.
    // Before, the metadata called the held coordinates "restated", and the
    // GNSS position's values were nowhere on the point.
    const ReadResult result = readLines({
        " 2\t14/03/26 10:00:00\t\t\tA\t\t\t1000.05\t5000.05\t20.05",
        "73\t\tGNSS Solution\tfixed",
        "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
    });
    const survey::SurveyPoint* a = topcon_test::point(result.project, "A");
    ASSERT_NE(a, nullptr);
    EXPECT_EQ(a->coordinateSource, survey::CoordinateSource::Entered);
    EXPECT_DOUBLE_EQ(a->easting, 1000.0);
    EXPECT_DOUBLE_EQ(a->northing, 5000.0);
    EXPECT_EQ(valueOf(a->metadata, "GNSS position of record 1"),
              "N 5000.05, E 1000.05, elevation 20.05");
    EXPECT_EQ(valueOf(a->metadata, "coordinates restated at record 3"), "(absent)");
    EXPECT_EQ(gnssPositionsOf(result.project, "A").size(), 1u);
    EXPECT_TRUE(warned(result, 3,
                       "point 'A' is given an entered coordinate here (N 5000, E 1000, elevation "
                       "20 m) that differs from the GNSS position of record 1; the reduction holds "
                       "the entered one, and the other is kept beside it"));
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

TEST(OpcodeFieldFile, TheColumnHoldsADateOrTimeInTheFormsControllersWrite)
{
    // A three-point RTK job for each form a controller's clock writes, one
    // with a blank before it as the opcodes have: every point is read, and
    // its time stamp kept as written, less the blank.
    for (const std::string_view stamp :
         {"10:49:06", "03/04/2026", "3 Apr 2026 10:49", "2026-04-03T10:49:06+1000",
          "03/04/2026 10:49:06 PM", " 03/04/26 10:49:06.52", "2026-04-03 10:49:06 UTC",
          "03/04/26 10:49:06.52 AEST", "03/04/26 10:49:06 UTC+10", "20260403T104906Z"}) {
        SCOPED_TRACE(stamp);
        const std::string s(stamp);
        auto read = topcon_test::read(
            kId,
            lines({" 2\t" + s + "\tEP\t1\tG1\t\t\t500000.0\t6200000.0\t10.0",
                   " 2\t" + s + "\tEP\t1\tG2\t\t\t500010.0\t6200000.0\t10.1",
                   " 2\t" + s + "\tEP\t1\tG3\t\t\t500020.0\t6200000.0\t10.2"}),
            "stamps.fld");
        ASSERT_TRUE(read.ok()) << read.error().describe();
        EXPECT_EQ(read->project.points.size(), 3u);
        EXPECT_EQ(read->recordsSkipped, 0u);
        EXPECT_EQ(valueOf(metadataOf(read->project, "G2"), "time stamp"),
                  std::string(katana::core::trimmed(stamp)));
    }
    // A total-station job stamped on a 12-hour clock: the 100 at its head is
    // read with the column, which its date says it has, as are the fixed
    // records after it, whose count fits the column's layout.
    const ReadResult clock = readLines({
        "100\t03/04/2026 10:49:00 PM\tdegrees\tmetres\tmillibars\tcelsius",
        "02\t03/04/2026 10:49:01 PM\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t03/04/2026 10:49:02 PM\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
        "03\t03/04/2026 10:49:03 PM\t\t\tK\t\t\t1.5",
        "04\t03/04/2026 10:49:04 PM\t\t\tB\t\t\t0.0\t90.0\t100.0",
        "07\t03/04/2026 10:49:05 PM\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
    });
    EXPECT_EQ(clock.recordsSkipped, 0u);
    ASSERT_EQ(clock.project.stations.size(), 1u);
    EXPECT_EQ(clock.project.stations[0].backsightPointId, "B");
    EXPECT_EQ(valueOf(metadataOf(clock.project, "1"), "time stamp"), "03/04/2026 10:49:05 PM");
}

TEST(OpcodeFieldFile, AColumnHoldingTextThatIsNotADateIsSkippedNotReadAsAValue)
{
    // "note", numbers alone or two of them joined, and codes with a month's
    // or a time's letters in them are no date or time.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "07\tnote\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
        "07\t12\tEP\t1\t3\t\t\t30.0\t90.0\t5.0",
        "07\tMAR1\tEP\t1\t4\t\t\t40.0\t90.0\t5.0",
        "07\t1-2\tEP\t1\t5\t\t\t50.0\t90.0\t5.0",
        "07\tT1-2\tEP\t1\t6\t\t\t60.0\t90.0\t5.0",
        "07\tKB 10:49\tEP\t1\t7\t\t\t70.0\t90.0\t5.0",
    });
    EXPECT_TRUE(warned(result, 3,
                       "holds 'note' in the column after the opcode, which is neither blank nor a "
                       "date or time"));
    EXPECT_TRUE(warned(result, 4, "holds '12' in the column after the opcode"));
    EXPECT_TRUE(warned(result, 5, "holds 'MAR1' in the column after the opcode"));
    EXPECT_TRUE(warned(result, 6, "holds '1-2' in the column after the opcode"));
    EXPECT_TRUE(warned(result, 7, "holds 'T1-2' in the column after the opcode"));
    // A word before the time is no zone's.
    EXPECT_TRUE(warned(result, 8, "holds 'KB 10:49' in the column after the opcode"));
    EXPECT_EQ(topcon_test::unpositioned(result.project, "2"), nullptr);
    EXPECT_EQ(result.recordsSkipped, 6u);
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
    EXPECT_EQ(valueOf(r001, "record 41/GPS Information/GNSS Solution"), "fixed");
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

TEST(OpcodeFieldFile, AGroupLeftOpenEndsAtTheNextPointOrAtAGroupOfItsLevel)
{
    const ReadResult result = readLines({
        " 2\t14/03/26 09:15:02.40\tKB\t1\tR1\t\t\t500010.0\t6200020.0\t12.5",
        "124\t\t\tGPS Information\t0",
        "73\t\tAntenna Height\t1.800",
        // no 125: record 4 makes another point
        " 2\t14/03/26 09:15:40.10\tKB\t1\tR2\t\t\t500020.0\t6200020.0\t12.6",
        "124\t\t\tSurvey Attributes\t0",
        "73\t\tMaterial\tConcrete",
        // no 125: record 7 opens a group at level 0, where Survey Attributes is
        "124\t\t\tReference Information\t0",
        "73\t\tEasting\t500500.000",
        "125\t\t\t\t0",
    });
    const auto r1 = metadataOf(result.project, "R1");
    const auto r2 = metadataOf(result.project, "R2");
    EXPECT_EQ(valueOf(r1, "GPS Information/Antenna Height"), "1.800");
    EXPECT_EQ(valueOf(r2, "Survey Attributes/Material"), "Concrete");
    EXPECT_EQ(valueOf(r2, "Reference Information/Easting"), "500500.000");
    for (const auto& [key, value] : r2) {
        EXPECT_EQ(key.find("GPS Information"), std::string::npos) << key;
        EXPECT_EQ(key.find("Survey Attributes/Reference"), std::string::npos) << key;
    }
    EXPECT_TRUE(warned(result, 2,
                       "attribute group 'GPS Information' is not ended (opcode 125) before record "
                       "4, which makes another point or a setup; it is ended there"));
    EXPECT_TRUE(warned(result, 5,
                       "attribute group 'Survey Attributes' is not ended (opcode 125) before "
                       "record 7, which opens attribute group 'Reference Information' at its "
                       "level"));
    EXPECT_EQ(result.recordsSkipped, 0u);
    EXPECT_EQ(result.warnings.size(), 2u);
}

TEST(OpcodeFieldFile, GroupsThatAreNeverEndedDoNotGrowTheNamesOfLaterAttributes)
{
    // A job of 400 points, each with three groups and no 125 at all: each
    // group ends at the next one of its level or at the next point, so no
    // name holds more than one group - where nesting every unended group
    // made names, and memory, grow with the square of the file.
    std::string bytes;
    for (int point = 0; point < 400; ++point) {
        bytes += " 2\t14/03/26 09:15:02.40\tKB\t1\tP" + std::to_string(point) + "\t\t\t" +
                 std::to_string(500000 + point) + ".0\t6200000.0\t12.5\n";
        for (const char* group : {"Survey Attributes", "GPS Information", "Reference Information"}) {
            bytes += std::string("124\t\t\t") + group + "\t0\n73\t\tValue\t1\n";
        }
    }
    auto read = topcon_test::read(kId, bytes, "unended.fld");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    std::size_t longest = 0;
    for (const survey::SurveyPoint& point : read->project.points) {
        for (const auto& [key, value] : point.metadata) {
            longest = std::max(longest, key.size());
        }
    }
    EXPECT_EQ(longest, std::string("Reference Information/Value").size());
    EXPECT_EQ(valueOf(metadataOf(read->project, "P399"), "GPS Information/Value"), "1");
}

// ---- The RTK setup fixture -------------------------------------------------------------

TEST(OpcodeFieldFile, ASetupOnAnRtkMarkIsOrientedOnAnotherAndItsOffsetShotsLandWhereTheFileSays)
{
    const ReadResult result = readFixture("rtk_setup.fld");
    EXPECT_EQ(result.recordsSkipped, 0u);
    EXPECT_TRUE(result.warnings.empty()) << result.warnings.front().message;
    // A (1000, 5000) and B (1000, 5100) are GNSS positions, which the
    // reduction places before any setup: the setup on A reads 30 degrees on
    // B, due north, so its orientation is 0 - 30 = -30 degrees and its
    // circle 120 is due east.
    EXPECT_EQ(gnssPositionsOf(result.project, "A").size(), 1u);
    EXPECT_EQ(gnssPositionsOf(result.project, "B").size(), 1u);
    EXPECT_TRUE(result.project.points.empty());
    const auto positions = reducedPositions(result.project);
    ASSERT_TRUE(positions.contains("B"));
    EXPECT_EQ(positions.at("B").method, survey::ComputationMethod::Gnss);
    EXPECT_NEAR(positions.at("B").easting, 1000.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("B").northing, 5100.0, kPositionTolerance);
    // 101: 10 m out, level, and the radial offset of record 20 puts it 0.5 m
    // further from the station: 1010.5, 5000.
    ASSERT_TRUE(positions.contains("101"));
    EXPECT_NEAR(positions.at("101").easting, 1010.5, kPositionTolerance);
    EXPECT_NEAR(positions.at("101").northing, 5000.0, kPositionTolerance);
    // 102: 20 m out, and the tangential offset of record 22, 0.5 m to the
    // right looking east from the station - south: 1020, 4999.5.
    ASSERT_TRUE(positions.contains("102"));
    EXPECT_NEAR(positions.at("102").easting, 1020.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("102").northing, 4999.5, kPositionTolerance);
}

// ---- The resection fixture -------------------------------------------------------------

TEST(OpcodeFieldFile, TheWholeResectionJobIsReadWithNothingSkipped)
{
    const ReadResult result = readFixture("resection.fld");
    // The version line, 100, 09, two 02, 128, 5, 7, 7, 5, 7, 129 (lines 2 and
    // 8-17) and the 15 records of lines 22-36: 27.
    EXPECT_EQ(result.recordsRead, 27u);
    EXPECT_EQ(result.recordsSkipped, 0u);
    // Nothing to warn of: the two offsets are applied.
    EXPECT_TRUE(result.warnings.empty()) << result.warnings.front().message;
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
    // The reduction resects such a setup (docs/survey.md, "The reduction's
    // resection"), so the note must not tell a person it cannot.
    EXPECT_FALSE(topcon_test::anyNotCarriedContains(result, "does not compute a resection"));
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
    // After the 129 the shots are still S1's: 101 at 90 degrees (and 0.25 m
    // to the right, the offset of record 24), 102 at 95.
    const auto to101 = observationsTo<survey::HorizontalDirectionObservation>(s1, "101");
    ASSERT_EQ(to101.size(), 1u);
    EXPECT_NEAR(to101[0].direction, kPi / 2.0 + std::atan2(0.25, 50.0), kAngleTolerance);
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
    // Record 31 checks K2 from S1 and is coded BS 11: no BS string, the code
    // kept. K2 was made by record 10, so what the check says is under its
    // record, with the attribute after it.
    EXPECT_EQ(topcon_test::feature(result.project, "BS"), nullptr);
    const auto k2 = metadataOf(result.project, "K2");
    EXPECT_EQ(valueOf(k2, "record 31/check measurement"),
              "from setup S1, feature code BS, string number 11");
    EXPECT_EQ(valueOf(k2, "record 31/Time"), "10:15:00");
    EXPECT_EQ(k2.count("Time"), 0u);
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

TEST(OpcodeFieldFile, OffsetsMoveTheShotTheyFollowAndKeepWhatWasMeasured)
{
    const ReadResult result = readFixture("resection.fld");
    const survey::SurveyStation& s1 = result.project.stations.at(0);
    // 101: circle 90, zenith 90, 50 m, then record 24's tangential 0.25 m,
    // to the right looking from S1: the circle reading of a point 50 m out
    // and 0.25 m across, 90 + atan2(0.25, 50) degrees, hypot(50, 0.25) m
    // away, still level.
    const auto direction101 = observationsTo<survey::HorizontalDirectionObservation>(s1, "101");
    ASSERT_EQ(direction101.size(), 1u);
    EXPECT_NEAR(direction101[0].direction, kPi / 2.0 + std::atan2(0.25, 50.0), kAngleTolerance);
    const auto distance101 = observationsTo<survey::DistanceObservation>(s1, "101");
    ASSERT_EQ(distance101.size(), 1u);
    EXPECT_NEAR(distance101[0].distance, std::hypot(50.0, 0.25), 1e-12);
    const auto zenith101 = observationsTo<survey::ZenithAngleObservation>(s1, "101");
    ASSERT_EQ(zenith101.size(), 1u);
    EXPECT_NEAR(zenith101[0].angle, kPi / 2.0, kAngleTolerance);
    // 102: circle 95, 50 m, then record 29's radial 0.5 m, away from S1:
    // the same circle, 50.5 m.
    const auto direction102 = observationsTo<survey::HorizontalDirectionObservation>(s1, "102");
    ASSERT_EQ(direction102.size(), 1u);
    EXPECT_NEAR(direction102[0].direction, degrees(95.0), kAngleTolerance);
    const auto distance102 = observationsTo<survey::DistanceObservation>(s1, "102");
    ASSERT_EQ(distance102.size(), 1u);
    EXPECT_NEAR(distance102[0].distance, 50.5, 1e-12);
    // What was measured, and each offset, are kept with the point.
    const auto p101 = metadataOf(result.project, "101");
    EXPECT_EQ(valueOf(p101, "offset record 24"),
              "tangential 0.25 m from setup S1, applied to the shot of record 23");
    EXPECT_EQ(valueOf(p101, "shot record 23 as measured"),
              "horizontal circle 90.00000000, vertical circle 90.00000000, slope distance 50.000");
    EXPECT_EQ(valueOf(metadataOf(result.project, "102"), "offset record 29"),
              "radial 0.5 m from setup S1, applied to the shot of record 28");
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

// ---- A point measured more than once ---------------------------------------------------

TEST(OpcodeFieldFile, EachMeasurementOfAPointKeepsItsAttributesUnderItsRecord)
{
    // A mark shot on both faces and checked: [FLD] makes each a point with
    // its own attributes. The first shot's are the point's; the others'
    // are under their records, none overwriting another.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEB\t01\tM1\t\t\t30.0\t85.0\t40.00",
        "73\t\tTime\t10:15:00",
        "72\t\tTarget height\t1.800",
        "07\t\tEB\t01\tM1\t\t\t210.0\t275.0\t40.01",
        "73\t\tTime\t10:15:20",
        "72\t\tTarget height\t1.800",
        "06\t\t\t\tM1\t\t\t30.0\t85.0\t40.00",
        "73\t\tTime\t10:17:00",
    });
    const auto m1 = metadataOf(result.project, "M1");
    EXPECT_EQ(valueOf(m1, "Time"), "10:15:00");
    EXPECT_EQ(valueOf(m1, "Target height"), "1.800");
    EXPECT_EQ(valueOf(m1, "record 5/Time"), "10:15:20");
    EXPECT_EQ(valueOf(m1, "record 5/Target height"), "1.800");
    EXPECT_EQ(valueOf(m1, "record 8/Time"), "10:17:00");
    EXPECT_EQ(valueOf(m1, "record 8/check measurement"), "from setup S");
    EXPECT_EQ(result.recordsRead, 9u);
    EXPECT_TRUE(result.warnings.empty()) << result.warnings.front().message;
}

TEST(OpcodeFieldFile, ANameGivenTwiceAfterOneRecordKeepsItsSecondValueWithAWarning)
{
    // A shot coded twice (07 then 16) followed by its attribute set twice:
    // what follows the 16 is still the shot's, so the second set's names
    // repeat the first's. A repeat with the same value adds nothing; one
    // with another value is kept under its own record, and said.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tPP\t03\t501\t\t\t160.0\t93.0\t50.0",
        "16\t\tUC\t03\t\t\t",
        "73\t\tDepth\t0.750",
        "73\t\tCondition\tGood",
        "73\t\tDepth\t--",
        "73\t\tCondition\tGood",
    });
    const auto p501 = metadataOf(result.project, "501");
    EXPECT_EQ(valueOf(p501, "Depth"), "0.750");
    EXPECT_EQ(valueOf(p501, "Depth (record 6)"), "--");
    EXPECT_EQ(valueOf(p501, "Condition"), "Good");
    EXPECT_EQ(valueOf(p501, "Condition (record 7)"), "(absent)");
    EXPECT_TRUE(warned(result, 6,
                       "point '501' is given 'Depth' again, as '--' after '0.750'; both are kept, "
                       "this one as 'Depth (record 6)'"));
    EXPECT_EQ(result.warnings.size(), 1u);
    EXPECT_EQ(result.recordsRead, 7u);
    EXPECT_EQ(result.recordsSkipped, 0u);
}

TEST(OpcodeFieldFile, AKeyedInMarkMeasuredAgainKeepsTheNewMeasurementsAttributesApart)
{
    // CM1 keyed in with no attributes, then an RTK measurement of it at the
    // same coordinates: the solution's attributes and time are record 2's,
    // not the keyed-in mark's own.
    const ReadResult result = readLines({
        "02\t\t\t\tCM1\t\t\t500000.0\t6200000.0\t10.0",
        " 2\t14/03/26 09:15:02.40\t\t\tCM1\t\t\t500000.0\t6200000.0\t10.0",
        "124\t\t\tGPS Information\t0",
        "73\t\tGNSS Solution\tfixed",
        "125\t\t\t\t0",
    });
    const auto cm1 = metadataOf(result.project, "CM1");
    EXPECT_EQ(valueOf(cm1, "record 2/time stamp"), "14/03/26 09:15:02.40");
    EXPECT_EQ(valueOf(cm1, "record 2/GPS Information/GNSS Solution"), "fixed");
    EXPECT_EQ(valueOf(cm1, "GPS Information/GNSS Solution"), "(absent)");
    EXPECT_EQ(valueOf(cm1, "time stamp"), "(absent)");
    EXPECT_TRUE(result.warnings.empty()) << result.warnings.front().message;
}

TEST(OpcodeFieldFile, APointIdGivenWithItsNameIsKeptOnceAndADifferentOneUnderItsRecord)
{
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\t\t\t11\tCP\t\t10.0\t90.0\t5.0",
        "07\t\t\t\t12\tCP\t\t190.0\t270.0\t5.0",
        "07\t\t\t\t11\tCP\t\t10.0\t90.0\t5.0",
    });
    const auto cp = metadataOf(result.project, "CP");
    EXPECT_EQ(valueOf(cp, "point ID"), "11");
    EXPECT_EQ(valueOf(cp, "record 3/point ID"), "12");
    EXPECT_EQ(valueOf(cp, "record 4/point ID"), "(absent)");
    EXPECT_EQ(topcon_test::unpositioned(result.project, "11"), nullptr);
}

TEST(OpcodeFieldFile, ALaterMeasurementsCommentIsKeptUnderItsRecord)
{
    // The first shot's comment is the point's description; the second's,
    // which the point's one description cannot hold, is kept under its record.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEB\t01\t101\t\tkerb\t10.0\t90.0\t5.0",
        "07\t\tEB\t01\t101\t\tkerb top\t190.0\t270.0\t5.0",
    });
    const survey::UnpositionedPoint* p101 = topcon_test::unpositioned(result.project, "101");
    ASSERT_NE(p101, nullptr);
    EXPECT_EQ(p101->description, "kerb");
    EXPECT_EQ(valueOf(p101->metadata, "record 3/comment"), "kerb top");
    EXPECT_TRUE(result.warnings.empty()) << result.warnings.front().message;
}

TEST(OpcodeFieldFile, ASetupOrABacksightNamingOnlyThePointIdOfANamedCoordinateFindsIt)
{
    // 44.5: a setup's and a backsight's point is searched for by its name,
    // and by its point ID among the point IDs of the coordinates before. The
    // 03 names only ID 1001, which the 02 gave CP1, and the 04 only ID 1002,
    // BS's: the setup stands on CP1 and backsights BS - where it stood on a
    // new point "1001" with no position, and nothing was radiated.
    const ReadResult result = readLines({
        "02\t\t\t\t1001\tCP1\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\t1002\tBS\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\t1001\t\t\t1.5",
        "04\t\t\t\t1002\t\t\t0.0\t90.0\t100.0",
        "07\t\tEP\t1\t101\t\t\t90.0\t90.0\t10.0",
    });
    ASSERT_EQ(result.project.stations.size(), 1u);
    const survey::SurveyStation& station = result.project.stations[0];
    EXPECT_EQ(station.setup.pointId, "CP1");
    EXPECT_EQ(valueOf(station.metadata, "point ID"), "1001");
    EXPECT_EQ(station.backsightPointId, "BS");
    EXPECT_EQ(topcon_test::unpositioned(result.project, "1001"), nullptr);
    const auto positions = reducedPositions(result.project);
    ASSERT_TRUE(positions.contains("101"));
    EXPECT_NEAR(positions.at("101").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("101").northing, 5000.0, kPositionTolerance);
}

TEST(OpcodeFieldFile, ARecordGivingANameOfItsOwnFindsThePointItsIdNamesIn44Point5sOrder)
{
    // 44.5 searches the coordinates and then the shots, in each the name
    // among the names, the name among the IDs, the ID among the IDs, and
    // stops at the first found. Coordinates 1001 and 1002 have no name; CP3
    // is ID 1003. The 03 names STN1 with ID 1001 and the 04 RO with ID 1002:
    // no coordinate or shot has those names, so each is found by its ID
    // (step 6) - where the setup stood on a new point STN1 with no position
    // and nothing was radiated. The name each gives is kept, not made a point.
    //
    // 1001 at (1000, 5000, 20), HI 1.5 and target height 0, oriented on 1002
    // due north at circle 0: 101 at circle 90, level, 10 m, is at (1010,
    // 5000), height 20 + 1.5 - 0 = 21.5; M5 at circle 270 at (990, 5000);
    // P7 at circle 0 at (1000, 5010).
    const ReadResult result = readLines({
        "02\t\t\t\t1001\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\t1002\t\t\t1000.0\t5100.0\t21.0",
        "02\t\t\t\t1003\tCP3\t\t1000.0\t4990.0\t20.0",
        "03\t\t\t\t1001\tSTN1\t\t1.5",
        "05\t\t0.0",
        "04\t\t\t\t1002\tRO\t\t0.0\t90.0\t100.0",
        "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
        "07\t\tEB\t01\t3001\tM5\t\t270.0\t90.0\t10.0",
        // A shot's name and a coordinate's ID: the coordinates come first
        // (step 6 before step 7), so this checks CP3, not the shot M5.
        "06\t\t\t\t1003\tM5\t\t180.0\t90.0\t10.0",
        "07\t\tEB\t01\t2001\tP7\t\t0.0\t90.0\t10.0",
        // A shot's ID (step 9): the setup stands on P7, the shot that gave it.
        // Oriented on 1001, due south at circle 180 (orientation 0): 102 at
        // circle 90, 5 m, is at (1005, 5010).
        "03\t\t\t\t2001\tSTNX\t\t1.5",
        "04\t\t\t\t1001\t\t\t180.0\t90.0\t10.0",
        "07\t\tEB\t02\t102\t\t\t90.0\t90.0\t5.0",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    const survey::SurveyProject& project = result.project;
    ASSERT_EQ(project.stations.size(), 2u);
    const survey::SurveyStation& first = project.stations[0];
    EXPECT_EQ(first.setup.pointId, "1001");
    EXPECT_EQ(valueOf(first.metadata, "point name"), "STN1");
    EXPECT_EQ(valueOf(first.metadata, "point ID"), "(absent)"); // the point's own
    EXPECT_EQ(first.backsightPointId, "1002");
    EXPECT_EQ(valueOf(metadataOf(project, "1002"), "record 6/point name"), "RO");
    EXPECT_EQ(valueOf(metadataOf(project, "1002"), "record 6/point ID"), "(absent)");
    // The check is on CP3, and what it calls it is kept there. M5 being a
    // point too, the check says which it took.
    const auto cp3 = metadataOf(project, "CP3");
    EXPECT_EQ(valueOf(cp3, "record 9/check measurement"), "from setup 1001");
    EXPECT_EQ(valueOf(cp3, "record 9/point name"), "M5");
    EXPECT_TRUE(warned(result, 9,
                       "the check names point 'M5' with point ID 1003, and the format's search "
                       "for its point (44.5) finds point 'CP3' first: the check checks 'CP3', not "
                       "the point of that name"));
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(first, "CP3").size(), 1u);
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(first, "M5").size(), 1u);
    const survey::SurveyStation& second = project.stations[1];
    EXPECT_EQ(second.setup.pointId, "P7");
    EXPECT_EQ(valueOf(second.metadata, "point name"), "STNX");
    EXPECT_EQ(valueOf(second.metadata, "point ID"), "2001");
    EXPECT_EQ(second.backsightPointId, "1001");
    for (const char* stray : {"STN1", "RO", "STNX"}) {
        EXPECT_EQ(topcon_test::unpositioned(project, stray), nullptr) << stray;
        EXPECT_EQ(topcon_test::point(project, stray), nullptr) << stray;
    }
    const auto positions = reducedPositions(project);
    ASSERT_EQ(positions.count("101") + positions.count("P7") + positions.count("102"), 3u);
    EXPECT_NEAR(positions.at("101").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("101").northing, 5000.0, kPositionTolerance);
    ASSERT_TRUE(positions.at("101").elevation.has_value());
    EXPECT_NEAR(*positions.at("101").elevation, 21.5, kPositionTolerance);
    EXPECT_NEAR(positions.at("P7").easting, 1000.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("P7").northing, 5010.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("102").easting, 1005.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("102").northing, 5010.0, kPositionTolerance);
}

TEST(OpcodeFieldFile, ABacksightNamedByAnotherPointsIdMakesNoPointOfThatName)
{
    // 44.5, step 5: the backsight's name, 1002, is CP2's point ID, so the
    // setup backsights CP2 - due north of A at circle 0, so 101 at circle 90,
    // 10 m, is at (1010, 5000). What the 04 calls it, name 1002 and ID 77, is
    // kept on CP2 under the record, where it made a point 1002 of its own,
    // with no position, that the reduction then reported as not drawn.
    const ReadResult result = readLines({
        "02\t\t\t\t1002\tCP2\t\t1000.0\t5100.0\t21.0",
        "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
        "03\t\t\t\tA\t\t\t1.5",
        "04\t\t\t\t77\t1002\t\t0.0\t90.0\t100.0",
        "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
    });
    ASSERT_EQ(result.project.stations.size(), 1u);
    EXPECT_EQ(result.project.stations[0].backsightPointId, "CP2");
    const auto cp2 = metadataOf(result.project, "CP2");
    EXPECT_EQ(valueOf(cp2, "point ID"), "1002");
    EXPECT_EQ(valueOf(cp2, "record 4/point name"), "1002");
    EXPECT_EQ(valueOf(cp2, "record 4/point ID"), "77");
    ASSERT_EQ(result.project.unpositionedPoints.size(), 1u);
    EXPECT_EQ(result.project.unpositionedPoints[0].id, "101");
    const auto positions = reducedPositions(result.project);
    ASSERT_TRUE(positions.contains("101"));
    EXPECT_NEAR(positions.at("101").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("101").northing, 5000.0, kPositionTolerance);
}

TEST(OpcodeFieldFile, WithinEachOf44Point5sListsANameIsSearchedForBeforeAnId)
{
    // Each record below gives a name and an ID that find two different
    // points in one list; 44.5 takes the name's. Coordinates P1 (ID X1), due
    // north of A, and P2 (ID Y1), due east; shots Q1 (ID S1) and Q2 (ID S2).
    //   record 5: name X1 is P1's ID (step 5), ID Y1 P2's (step 6): P1. The
    //     setup, oriented on P1 at circle 0, puts 101 at circle 90, 10 m, at
    //     (1010, 5000); oriented on P2 it would be at (1000, 4990).
    //   record 7: name P1 is P1's (step 4), ID Y1 P2's (step 6): P1.
    //   record 10: name Q1 is Q1's (step 7), ID S2 Q2's (step 9): Q1.
    //   record 11: name S1 is Q1's ID (step 8), ID S2 Q2's (step 9): Q1.
    const ReadResult result = readLines({
        "02\t\t\t\tX1\tP1\t\t1000.0\t5100.0\t21.0",
        "02\t\t\t\tY1\tP2\t\t1100.0\t5000.0\t21.0",
        "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
        "03\t\t\t\tA\t\t\t1.5",
        "04\t\t\t\tY1\tX1\t\t0.0\t90.0\t100.0",
        "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
        "06\t\t\t\tY1\tP1\t\t0.0\t90.0\t100.0",
        "07\t\tEB\t01\tS1\tQ1\t\t180.0\t90.0\t10.0",
        "07\t\tEB\t01\tS2\tQ2\t\t270.0\t90.0\t10.0",
        "06\t\t\t\tS2\tQ1\t\t180.0\t90.0\t10.0",
        "06\t\t\t\tS2\tS1\t\t180.0\t90.0\t10.0",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    ASSERT_EQ(result.project.stations.size(), 1u);
    EXPECT_EQ(result.project.stations[0].backsightPointId, "P1");
    const auto p1 = metadataOf(result.project, "P1");
    EXPECT_EQ(valueOf(p1, "record 5/point name"), "X1");
    EXPECT_EQ(valueOf(p1, "record 5/point ID"), "Y1");
    EXPECT_EQ(valueOf(p1, "record 7/check measurement"), "from setup A");
    const auto q1 = metadataOf(result.project, "Q1");
    EXPECT_EQ(valueOf(q1, "record 10/check measurement"), "from setup A");
    EXPECT_EQ(valueOf(q1, "record 11/check measurement"), "from setup A");
    for (const char* other : {"P2", "Q2"}) {
        for (const auto& [key, value] : metadataOf(result.project, other)) {
            EXPECT_EQ(key.find("check measurement"), std::string::npos) << other << ": " << key;
        }
    }
    const auto positions = reducedPositions(result.project);
    ASSERT_TRUE(positions.contains("101"));
    EXPECT_NEAR(positions.at("101").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("101").northing, 5000.0, kPositionTolerance);
}

TEST(OpcodeFieldFile, ARecordWhoseNameIsAnotherPointThanTheOne44Point5FindsIsWarnedOf)
{
    // T1 is a point only a check (06) made, so it is in neither of 44.5's
    // lists, the coordinates and the shots. The second setup names T1 with
    // point ID 5, which shot 5 was given: 44.5 finds shot 5 by its ID (step
    // 9), and the setup stands on it, with a warning that its name is another
    // point's. Shot 5 is at (1010, 5000); from it A is on bearing 270, read
    // at circle 0, so 301 at circle 90, 10 m, is on bearing 0: (1010, 5010).
    const ReadResult result = readLines({
        "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tA\t\t\t1.5",
        "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
        "07\t\tEB\t01\t5\t\t\t90.0\t90.0\t10.0",
        "06\t\t\t\t\tT1\t\t180.0\t90.0\t20.0",
        "03\t\t\t\t5\tT1\t\t1.5",
        "04\t\t\t\tA\t\t\t0.0\t90.0\t10.0",
        "07\t\tEB\t02\t301\t\t\t90.0\t90.0\t10.0",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    ASSERT_EQ(result.project.stations.size(), 2u);
    EXPECT_EQ(result.project.stations[1].setup.pointId, "5");
    EXPECT_EQ(valueOf(result.project.stations[1].metadata, "point name"), "T1");
    EXPECT_TRUE(warned(result, 7,
                       "the setup names point 'T1' with point ID 5, and the format's search for "
                       "its point (44.5) finds point '5' first: the setup stands on '5', not the "
                       "point of that name"));
    const auto positions = reducedPositions(result.project);
    ASSERT_TRUE(positions.contains("301"));
    EXPECT_NEAR(positions.at("301").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("301").northing, 5010.0, kPositionTolerance);
}

TEST(OpcodeFieldFile, AHeaderKeyGivenAgainKeepsBothValuesWithAWarning)
{
    const ReadResult result = readLines({
        "// Job name : first",
        "// Job name : second",
        "// Coordinate System: Local grid E",
        "// Coordinate System: Local grid F",
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
    });
    EXPECT_EQ(result.project.metadata.at("header: Job name"), "first");
    EXPECT_EQ(result.project.metadata.at("header: Job name (record 2)"), "second");
    EXPECT_TRUE(warned(result, 2,
                       "the header gives 'Job name' again, as 'second' after 'first'; both are "
                       "kept, this one as 'header: Job name (record 2)'"));
    // The system declared is the first.
    EXPECT_EQ(result.project.coordinateSystem.name, "Local grid E");
    EXPECT_EQ(result.project.metadata.at("header: Coordinate System (record 4)"), "Local grid F");
}

// ---- The backsight -----------------------------------------------------------------------

TEST(OpcodeFieldFile, ABacksightsStatedAzimuthIsKeptApartFromItsCircleAndOrientsASetupNothingElseCan)
{
    // [FLD] 44.8, 04: "The azimuth_value ... may be specified when no
    // coordinate for the backsight point exists." CP1 at (1000, 5000); BSX
    // has no coordinates, reads 30 degrees and is given azimuth 45; the shot
    // reads 45, zenith 95, 50 m.
    const ReadResult result = readLines({
        "02\t\t\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
        "03\t\t\t\tCP1\t\t\t1.5",
        "04\t\t\t\tBSX\t\t\t30.0\t90.0\t50.0\t45.0",
        "07\t\tEB\t01\t101\t\t\t45.0\t95.0\t50.0",
    });
    ASSERT_EQ(result.project.stations.size(), 1u);
    const survey::SurveyStation& station = result.project.stations[0];
    // The circle read on the backsight, and the azimuth the file states, each
    // in its own field.
    ASSERT_TRUE(station.backsightAzimuth.has_value());
    EXPECT_NEAR(*station.backsightAzimuth, degrees(30.0), kAngleTolerance);
    ASSERT_TRUE(station.statedBacksightAzimuth.has_value());
    EXPECT_NEAR(*station.statedBacksightAzimuth, degrees(45.0), kAngleTolerance);
    // The orientation is 45 - 30 = 15 degrees, so the shot's bearing is 60:
    // 50 sin 95 m out, (1000 + that sin 60, 5000 + that cos 60) - where the
    // circle taken as the azimuth would put it on bearing 45, 13 m away. The
    // report says the bearings are the file's, not a circle taken for them.
    const survey::ReductionOutcome outcome = reduced(result.project);
    const double plan = 50.0 * std::sin(degrees(95.0));
    const auto shot = std::find_if(outcome.points.begin(), outcome.points.end(),
                                   [](const survey::ComputedPoint& p) { return p.id == "101"; });
    ASSERT_NE(shot, outcome.points.end());
    EXPECT_NEAR(shot->easting, 1000.0 + plan * std::sin(degrees(60.0)), kPositionTolerance);
    EXPECT_NEAR(shot->northing, 5000.0 + plan * std::cos(degrees(60.0)), kPositionTolerance);
    EXPECT_EQ(reductionWarningsWith(outcome.report, "oriented on the azimuth the file states"), 1u);
    EXPECT_EQ(reductionWarningsWith(outcome.report, "taken as a grid azimuth"), 0u);

    // With no azimuth the circle alone is recorded, and nothing is stated.
    const ReadResult circle = readLines({
        "02\t\t\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
        "03\t\t\t\tCP1\t\t\t1.5",
        "04\t\t\t\tBSX\t\t\t30.0\t90.0\t50.0",
    });
    ASSERT_EQ(circle.project.stations.size(), 1u);
    ASSERT_TRUE(circle.project.stations[0].backsightAzimuth.has_value());
    EXPECT_NEAR(*circle.project.stations[0].backsightAzimuth, degrees(30.0), kAngleTolerance);
    EXPECT_FALSE(circle.project.stations[0].statedBacksightAzimuth.has_value());
}

TEST(OpcodeFieldFile, AStatedAzimuthIsNoCircleReadingAndDoesNotOrientASetupWhoseBacksightHasCoordinates)
{
    // BSX has coordinates, due north of CP1; the backsight gives no
    // horizontal circle, and an azimuth of 45. Coordinates orient a setup
    // through its reading on the backsight, and there is none: the setup is
    // not oriented, and says why - rather than the azimuth taken for the
    // circle set on BSX, which turned the shot at 90 to bearing 45.
    const ReadResult result = readLines({
        "02\t\t\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tBSX\t\t\t1000.0\t5050.0\t20.0",
        "03\t\t\t\tCP1\t\t\t1.5",
        "04\t\t\t\tBSX\t\t\t\t90.0\t50.0\t45.0",
        "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t50.0",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    const survey::SurveyStation& station = result.project.stations.at(0);
    EXPECT_FALSE(station.backsightAzimuth.has_value());
    ASSERT_TRUE(station.statedBacksightAzimuth.has_value());
    EXPECT_NEAR(*station.statedBacksightAzimuth, degrees(45.0), kAngleTolerance);
    const survey::ReductionOutcome outcome = reduced(result.project);
    for (const survey::ComputedPoint& point : outcome.points) {
        EXPECT_NE(point.id, "101");
    }
    EXPECT_EQ(reductionWarningsWith(outcome.report,
                                    "Setup CP1 cannot be oriented on its backsight BSX"),
              1u);
}

TEST(OpcodeFieldFile, ALaterBacksightChangesNeitherTheCircleNorTheStatedAzimuth)
{
    // Face left with the azimuth, then face right without it: the setup
    // keeps the face-left circle, 30, and the azimuth, 45, so the shot at 45
    // is on bearing 60, as with the first alone - where the face-right
    // reading taken for the circle set on BSX turned it to bearing 225.
    const ReadResult result = readLines({
        "02\t\t\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
        "03\t\t\t\tCP1\t\t\t1.5",
        "04\t\t\t\tBSX\t\t\t30.0\t90.0\t50.0\t45.0",
        "04\t\t\t\tBSX\t\t\t210.0\t270.0\t50.0",
        "07\t\tEB\t01\t101\t\t\t45.0\t95.0\t50.0",
        // A setup whose first backsight is face right: its circle is 210 - 180.
        // Its second gives another azimuth, kept apart, and reads 30.01, so
        // a circle taken from the last backsight rather than the first would
        // be 30.01 (the first setup's two readings, 180 apart, cannot tell).
        "03\t\t\t\tCP1\t\t\t1.5",
        "04\t\t\t\tBSX\t\t\t210.0\t270.0\t50.0\t45.0",
        "04\t\t\t\tBSX\t\t\t30.01\t90.0\t50.0\t46.0",
    });
    ASSERT_EQ(result.project.stations.size(), 2u);
    for (const survey::SurveyStation& station : result.project.stations) {
        SCOPED_TRACE(station.setup.id);
        ASSERT_TRUE(station.backsightAzimuth && station.statedBacksightAzimuth);
        EXPECT_NEAR(*station.backsightAzimuth, degrees(30.0), kAngleTolerance);
        EXPECT_NEAR(*station.statedBacksightAzimuth, degrees(45.0), kAngleTolerance);
    }
    EXPECT_EQ(valueOf(result.project.stations[1].metadata, "backsight azimuth record 8"),
              "46.0 degrees");
    EXPECT_TRUE(warned(result, 8,
                       "the backsight gives the azimuth 46.0 degrees, where an earlier backsight "
                       "of setup CP1 (2) gave another"));
    const double plan = 50.0 * std::sin(degrees(95.0));
    const auto positions = reducedPositions(result.project);
    ASSERT_TRUE(positions.contains("101"));
    EXPECT_NEAR(positions.at("101").easting, 1000.0 + plan * std::sin(degrees(60.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("101").northing, 5000.0 + plan * std::cos(degrees(60.0)),
                kPositionTolerance);
}

TEST(OpcodeFieldFile, ABacksightToAnotherPointThanTheOneThatOrientsTheSetupIsAMeasurementOfIt)
{
    // Two backsights to different points in one setup, each of these three
    // with the first able to orient it. The first names the backsight, and
    // the circle and the azimuth are that point's; the second is read as a
    // measurement of its own point. Before, the setup took its backsight
    // point from the last 04 and its circle and azimuth from the first, so
    // it was oriented on one point's circle less the reading on another's,
    // and every direction turned by the difference.
    //
    // K at (1000, 5000). R1, which nothing places, reads 0 and is given
    // azimuth 45: orientation 45 - 0 = 45 degrees. R2, placed by nothing
    // either, reads 90 and is given azimuth 135 (the same orientation, 135 -
    // 90). Shot 501 reads 90, level, 10 m: bearing 90 + 45 = 135, at
    // (1000 + 10 sin 135, 5000 + 10 cos 135) = (1007.0711, 4992.9289); R2 is
    // radiated at bearing 135, 100 m out. The mixed setup read 45 less the
    // reading on R2, 90: orientation -45, and 501 on bearing 45.
    const ReadResult stated = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tR1\t\t\t0.0\t90.0\t100.0\t45.0",
        "04\t\t\t\tR2\t\t\t90.0\t90.0\t100.0\t135.0",
        "07\t\tEB\t01\t501\t\t\t90.0\t90.0\t10.0",
    });
    EXPECT_EQ(stated.recordsSkipped, 0u);
    ASSERT_EQ(stated.project.stations.size(), 1u);
    const survey::SurveyStation& k = stated.project.stations[0];
    EXPECT_EQ(k.backsightPointId, "R1");
    ASSERT_TRUE(k.backsightAzimuth && k.statedBacksightAzimuth);
    EXPECT_NEAR(*k.backsightAzimuth, 0.0, kAngleTolerance);
    EXPECT_NEAR(*k.statedBacksightAzimuth, degrees(45.0), kAngleTolerance);
    EXPECT_EQ(valueOf(k.metadata, "backsight azimuth record 4"), "135.0 degrees");
    EXPECT_TRUE(warned(stated, 4,
                       "a backsight to 'R2', where setup K backsights 'R1' (record 3), the first "
                       "that can orient it, with a horizontal circle and a stated azimuth: this "
                       "one is read as a measurement of its point; its azimuth, 135.0 degrees, is "
                       "kept in the setup's metadata"));
    const auto positions = reducedPositions(stated.project);
    ASSERT_TRUE(positions.contains("501") && positions.contains("R2"));
    EXPECT_NEAR(positions.at("501").easting, 1000.0 + 10.0 * std::sin(degrees(135.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("501").northing, 5000.0 + 10.0 * std::cos(degrees(135.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("R2").easting, 1000.0 + 100.0 * std::sin(degrees(135.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("R2").northing, 5000.0 + 100.0 * std::cos(degrees(135.0)),
                kPositionTolerance);

    // The first backsight has coordinates: R3, due north of K, reads 30, so
    // the orientation is 0 - 30 = -30 degrees. R4, which nothing places,
    // reads 120, 50 m out: bearing 90, at (1050, 5000); shot 502 at circle
    // 120, 10 m: (1010, 5000). The mixed setup, backsighting R4 with no
    // position, took R3's circle for a grid azimuth less the reading on R4:
    // 30 - 120 = -90, and 502 on bearing 30, with no reader warning.
    const ReadResult known = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tR3\t\t\t30.0\t90.0\t100.0",
        "04\t\t\t\tR4\t\t\t120.0\t90.0\t50.0",
        "07\t\tEB\t01\t502\t\t\t120.0\t90.0\t10.0",
    });
    ASSERT_EQ(known.project.stations.size(), 1u);
    EXPECT_EQ(known.project.stations[0].backsightPointId, "R3");
    EXPECT_TRUE(warned(known, 5,
                       "a backsight to 'R4', where setup K backsights 'R3' (record 4), the first "
                       "that can orient it, with a horizontal circle, and coordinates the file "
                       "gives: this one is read as a measurement of its point"));
    const survey::ReductionOutcome outcome = reduced(known.project);
    std::map<std::string, survey::ComputedPoint> placed;
    for (const survey::ComputedPoint& point : outcome.points) {
        placed[point.id] = point;
    }
    ASSERT_TRUE(placed.contains("502") && placed.contains("R4"));
    EXPECT_NEAR(placed.at("R4").easting, 1050.0, kPositionTolerance);
    EXPECT_NEAR(placed.at("R4").northing, 5000.0, kPositionTolerance);
    EXPECT_NEAR(placed.at("502").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(placed.at("502").northing, 5000.0, kPositionTolerance);
    EXPECT_EQ(reductionWarningsWith(outcome.report, "taken as a grid azimuth"), 0u);

    // Where the two disagree - R5, the first, given azimuth 45 at circle 30
    // (orientation 15); R6, due east of K, reading 120 (orientation 90 - 120
    // = -30) - the first can orient the setup, so it does, and the second,
    // which has coordinates, is a check on it: radiated at bearing 120 + 15 =
    // 135, 50 m out, it misses R6's (1050, 5000) by 50 cos 135 = -35.355 m
    // north and 1000 + 50 sin 135 - 1050 = -14.645 m east. Nothing is turned
    // silently.
    const ReadResult disagree = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tR6\t\t\t1050.0\t5000.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tR5\t\t\t30.0\t90.0\t100.0\t45.0",
        "04\t\t\t\tR6\t\t\t120.0\t90.0\t50.0",
    });
    EXPECT_TRUE(warned(disagree, 5, "a backsight to 'R6', where setup K backsights 'R5' (record 4)"));
    const survey::ReductionOutcome checked = reduced(disagree.project);
    ASSERT_EQ(checked.report.misclosures.size(), 1u);
    const survey::MisclosureReport& check = checked.report.misclosures[0];
    EXPECT_EQ(check.name, "R6 from setup K");
    ASSERT_TRUE(check.northing && check.easting);
    EXPECT_NEAR(*check.northing, 50.0 * std::cos(degrees(135.0)), kPositionTolerance);
    EXPECT_NEAR(*check.easting, 1000.0 + 50.0 * std::sin(degrees(135.0)) - 1050.0,
                kPositionTolerance);
}

TEST(OpcodeFieldFile, ABacksightThatCannotOrientItsSetupGivesWayToALaterOneThatCan)
{
    // A 04 orients its setup where it gives a horizontal circle to a point
    // with a position (coordinates the file gives, or a measurement from an
    // earlier setup) or states an azimuth. In each case below the first 04
    // cannot and the second can; the second names the backsight, and the
    // first is read as a measurement. With the first naming it, the setup
    // was oriented on the first's circle taken as a grid azimuth, or not at
    // all, although the second could orient it.
    //
    // K at (1000, 5000), R3 entered due north of it. R4, which nothing
    // places, reads 120, 50 m out; R3 reads 30: the orientation is 0 - 30 =
    // -30 degrees, so R4 is on bearing 90, at (1050, 5000), and 502 at
    // circle 120, 10 m, at (1010, 5000). R4 has no coordinates to check, and
    // R3 is the backsight: nothing misses. With R4 naming the backsight the
    // circle 120 was taken for its grid azimuth, and 502 drawn on bearing
    // 120 and R3 reported 51.8 m out.
    const ReadResult control = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tR4\t\t\t120.0\t90.0\t50.0",
        "04\t\t\t\tR3\t\t\t30.0\t90.0\t100.0",
        "07\t\tEB\t01\t502\t\t\t120.0\t90.0\t10.0",
    });
    EXPECT_EQ(control.recordsSkipped, 0u);
    ASSERT_EQ(control.project.stations.size(), 1u);
    const survey::SurveyStation& k = control.project.stations[0];
    EXPECT_EQ(k.backsightPointId, "R3");
    ASSERT_TRUE(k.backsightAzimuth.has_value());
    EXPECT_NEAR(*k.backsightAzimuth, degrees(30.0), kAngleTolerance);
    EXPECT_TRUE(warned(control, 4,
                       "a backsight to 'R4', where setup K backsights 'R3' (record 5), the first "
                       "that can orient it, with a horizontal circle, and coordinates the file "
                       "gives: this one is read as a measurement of its point"));
    const survey::ReductionOutcome outcome = reduced(control.project);
    EXPECT_TRUE(outcome.report.misclosures.empty());
    EXPECT_EQ(reductionWarningsWith(outcome.report, "taken as a grid azimuth"), 0u);
    std::map<std::string, survey::ComputedPoint> placed;
    for (const survey::ComputedPoint& point : outcome.points) {
        placed[point.id] = point;
    }
    ASSERT_TRUE(placed.contains("502") && placed.contains("R4"));
    EXPECT_NEAR(placed.at("R4").easting, 1050.0, kPositionTolerance);
    EXPECT_NEAR(placed.at("R4").northing, 5000.0, kPositionTolerance);
    EXPECT_NEAR(placed.at("502").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(placed.at("502").northing, 5000.0, kPositionTolerance);

    // The same with R3's coordinates after the setup, and after the next one
    // begins: the reduction places every 02 before any setup, so they still
    // orient it. Settled as each 04 is read, or as the setup ends, neither
    // backsight could have, and R4's circle would.
    const ReadResult later = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tR4\t\t\t120.0\t90.0\t50.0",
        "04\t\t\t\tR3\t\t\t30.0\t90.0\t100.0",
        "07\t\tEB\t01\t502\t\t\t120.0\t90.0\t10.0",
        "03\t\t\t\tK\t\t\t1.5",
        "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
    });
    ASSERT_EQ(later.project.stations.size(), 2u);
    EXPECT_EQ(later.project.stations[0].backsightPointId, "R3");
    const auto afterward = reducedPositions(later.project);
    ASSERT_TRUE(afterward.contains("502"));
    EXPECT_NEAR(afterward.at("502").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(afterward.at("502").northing, 5000.0, kPositionTolerance);

    // R7, which nothing places, reads 0 with no azimuth; R8, placed by
    // nothing either, reads 30 and is given azimuth 45: orientation 45 - 30 =
    // 15 degrees. 503 at circle 120 is on bearing 135, (1000 + 10 sin 135,
    // 5000 + 10 cos 135); R7 on bearing 15, 50 m out. With R7 naming the
    // backsight R8's azimuth was only kept in the metadata and the circle 0
    // taken for R7's grid azimuth: 503 on bearing 120.
    const ReadResult stated = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tR7\t\t\t0.0\t90.0\t50.0",
        "04\t\t\t\tR8\t\t\t30.0\t90.0\t100.0\t45.0",
        "07\t\tEB\t01\t503\t\t\t120.0\t90.0\t10.0",
    });
    ASSERT_EQ(stated.project.stations.size(), 1u);
    const survey::SurveyStation& byAzimuth = stated.project.stations[0];
    EXPECT_EQ(byAzimuth.backsightPointId, "R8");
    ASSERT_TRUE(byAzimuth.backsightAzimuth && byAzimuth.statedBacksightAzimuth);
    EXPECT_NEAR(*byAzimuth.backsightAzimuth, degrees(30.0), kAngleTolerance);
    EXPECT_NEAR(*byAzimuth.statedBacksightAzimuth, degrees(45.0), kAngleTolerance);
    EXPECT_EQ(valueOf(byAzimuth.metadata, "backsight azimuth record 4"), "(absent)");
    EXPECT_TRUE(warned(stated, 3,
                       "a backsight to 'R7', where setup K backsights 'R8' (record 4), the first "
                       "that can orient it, with a horizontal circle and a stated azimuth"));
    const auto byStated = reducedPositions(stated.project);
    ASSERT_TRUE(byStated.contains("503") && byStated.contains("R7"));
    EXPECT_NEAR(byStated.at("503").easting, 1000.0 + 10.0 * std::sin(degrees(135.0)),
                kPositionTolerance);
    EXPECT_NEAR(byStated.at("503").northing, 5000.0 + 10.0 * std::cos(degrees(135.0)),
                kPositionTolerance);
    EXPECT_NEAR(byStated.at("R7").easting, 1000.0 + 50.0 * std::sin(degrees(15.0)),
                kPositionTolerance);
    EXPECT_NEAR(byStated.at("R7").northing, 5000.0 + 50.0 * std::cos(degrees(15.0)),
                kPositionTolerance);

    // R3 entered due north, with no horizontal circle read on it; R6 entered
    // due east, reading 120: orientation 90 - 120 = -30, so 504 at circle
    // 120 is at (1010, 5000). With R3 naming the backsight the setup had no
    // direction to orient on, and 504 was not drawn.
    const ReadResult noCircle = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
        "02\t\t\t\tR6\t\t\t1050.0\t5000.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tR3\t\t\t\t90.0\t100.0",
        "04\t\t\t\tR6\t\t\t120.0\t90.0\t50.0",
        "07\t\tEB\t01\t504\t\t\t120.0\t90.0\t10.0",
    });
    EXPECT_EQ(noCircle.recordsSkipped, 0u);
    ASSERT_EQ(noCircle.project.stations.size(), 1u);
    EXPECT_EQ(noCircle.project.stations[0].backsightPointId, "R6");
    const auto byR6 = reducedPositions(noCircle.project);
    ASSERT_TRUE(byR6.contains("504"));
    EXPECT_NEAR(byR6.at("504").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(byR6.at("504").northing, 5000.0, kPositionTolerance);

    // A traverse: K, oriented on R3 at circle 0, shoots T1 at circle 90 and
    // T2 at circle 180, 10 m each: (1010, 5000) and (1000, 4990). The setup
    // on T1 first reads X, which nothing places, at 50, then T2, which K
    // measured, at 135: T2 is on bearing 225 from T1, so the orientation is
    // 225 - 135 = 90 degrees. 505 at circle 0, 10 m, is on bearing 90 from T1,
    // at (1020, 5000); X at circle 50, 20 m, on bearing 140. With X naming the
    // backsight its circle was taken for its grid azimuth: 505 on bearing 0.
    const ReadResult traverse = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tR3\t\t\t0.0\t90.0\t100.0",
        "07\t\tEB\t01\tT1\t\t\t90.0\t90.0\t10.0",
        "07\t\tEB\t01\tT2\t\t\t180.0\t90.0\t10.0",
        "03\t\t\t\tT1\t\t\t1.5",
        "04\t\t\t\tX\t\t\t50.0\t90.0\t20.0",
        "04\t\t\t\tT2\t\t\t135.0\t90.0\t14.142135623731",
        "07\t\tEB\t02\t505\t\t\t0.0\t90.0\t10.0",
    });
    EXPECT_EQ(traverse.recordsSkipped, 0u);
    ASSERT_EQ(traverse.project.stations.size(), 2u);
    EXPECT_EQ(traverse.project.stations[1].setup.pointId, "T1");
    EXPECT_EQ(traverse.project.stations[1].backsightPointId, "T2");
    EXPECT_TRUE(warned(traverse, 8,
                       "a backsight to 'X', where setup T1 backsights 'T2' (record 9), the first "
                       "that can orient it, with a horizontal circle, and measured from setup K"));
    const auto byTraverse = reducedPositions(traverse.project);
    ASSERT_TRUE(byTraverse.contains("505") && byTraverse.contains("X"));
    EXPECT_NEAR(byTraverse.at("505").easting, 1020.0, kPositionTolerance);
    EXPECT_NEAR(byTraverse.at("505").northing, 5000.0, kPositionTolerance);
    EXPECT_NEAR(byTraverse.at("X").easting, 1010.0 + 20.0 * std::sin(degrees(140.0)),
                kPositionTolerance);
    EXPECT_NEAR(byTraverse.at("X").northing, 5000.0 + 20.0 * std::cos(degrees(140.0)),
                kPositionTolerance);
}

TEST(OpcodeFieldFile, WhereNoBacksightCanOrientItsSetupTheFirstWithACircleNamesIt)
{
    // R3 is entered but read with no horizontal circle; R9, which nothing
    // places, reads 40 with no azimuth. Neither can orient the setup, so the
    // first with a circle names the backsight and the reduction takes that
    // circle as set to the grid, saying so: orientation 40 - 40 = 0, and 507
    // at circle 130, 10 m, is on bearing 130. The first 04 naming it left
    // the setup with no direction to orient on, and nothing was drawn.
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tR3\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tR3\t\t\t\t90.0\t100.0",
        "04\t\t\t\tR9\t\t\t40.0\t90.0\t50.0",
        "07\t\tEB\t01\t507\t\t\t130.0\t90.0\t10.0",
    });
    ASSERT_EQ(result.project.stations.size(), 1u);
    EXPECT_EQ(result.project.stations[0].backsightPointId, "R9");
    EXPECT_TRUE(warned(result, 4,
                       "a backsight to 'R3', where setup K backsights 'R9' (record 5), the first "
                       "with a horizontal circle, as none with one is to a point with a position "
                       "or states an azimuth: this one is read as a measurement of its point"));
    const survey::ReductionOutcome outcome = reduced(result.project);
    EXPECT_EQ(reductionWarningsWith(outcome.report, "taken as a grid azimuth"), 1u);
    const auto shot = std::find_if(outcome.points.begin(), outcome.points.end(),
                                   [](const survey::ComputedPoint& p) { return p.id == "507"; });
    ASSERT_NE(shot, outcome.points.end());
    EXPECT_NEAR(shot->easting, 1000.0 + 10.0 * std::sin(degrees(130.0)), kPositionTolerance);
    EXPECT_NEAR(shot->northing, 5000.0 + 10.0 * std::cos(degrees(130.0)), kPositionTolerance);
}

// ---- Layouts -----------------------------------------------------------------------------

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

TEST(OpcodeFieldFile, ABacksightOfTenFieldsFitsEitherLayoutAndDecidesNothing)
{
    // [FLD]'s syntax with an uncoded backsight that gives its azimuth: ten
    // fields, the count of a backsight in the column's layout without one.
    // It decides nothing; the coded 07 decides, and every record is read as
    // [FLD] writes it.
    const ReadResult result = readLines({
        "100\tdegrees\tmetres",
        "02\t\t\tA\t\t\t10.0\t20.0\t1.0",
        "02\t\t\tB\t\t\t10.0\t120.0\t1.0",
        "03\t\t\tA\t\t\t1.5",
        "04\t\t\tB\t\t\t0.0\t90.0\t100.0\t0.0",
        "07\tTREE\t\t7\t\t\t90.0\t90.0\t12.5",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    EXPECT_TRUE(result.warnings.empty()) << result.warnings.front().message;
    ASSERT_EQ(result.project.stations.size(), 1u);
    const survey::SurveyStation& station = result.project.stations[0];
    EXPECT_EQ(station.backsightPointId, "B");
    ASSERT_TRUE(station.backsightAzimuth.has_value());
    EXPECT_NEAR(*station.backsightAzimuth, 0.0, kAngleTolerance);
    ASSERT_TRUE(station.statedBacksightAzimuth.has_value());
    EXPECT_NEAR(*station.statedBacksightAzimuth, 0.0, kAngleTolerance);
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(station, "7").at(0).distance, 12.5);

    // With no coded record at all nothing decides, and the file is read as
    // [FLD] writes it.
    const ReadResult uncoded = readLines({
        "02\t\t\tA\t\t\t10.0\t20.0\t1.0",
        "02\t\t\tB\t\t\t10.0\t120.0\t1.0",
        "03\t\t\tA\t\t\t1.5",
        "04\t\t\tB\t\t\t0.0\t90.0\t100.0\t0.0",
        "07\t\t\t8\t\t\t90.0\t90.0\t12.5",
    });
    EXPECT_EQ(uncoded.recordsSkipped, 0u);
    ASSERT_EQ(uncoded.project.stations.size(), 1u);
    EXPECT_EQ(uncoded.project.stations[0].backsightPointId, "B");
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(uncoded.project.stations[0], "8").size(),
              1u);
}

TEST(OpcodeFieldFile, InAFileOfBothLayoutsABlankFirstFieldTakesTheLayoutMostRecordsHave)
{
    // Coded records of both layouts - records 1 to 3 without the column,
    // record 4 with it - then a backsight of ten fields with an empty first:
    // the column and no azimuth, or no column and the azimuth.
    const ReadResult result = readLines({
        "02\tK\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
        "02\tK\t\tCP2\t\t\t1000.0\t5100.0\t20.0",
        "03\tK\t\tCP1\t\t\t1.5",
        "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
        "04\t\t\t\tCP2\t\t\t0.0\t90.0\t100.0",
    });
    EXPECT_TRUE(warned(result, 0,
                       "1 fixed record(s) have the column after the opcode (blank or a date or "
                       "time) and 3 do not: each is read in the layout its fields show, and one "
                       "whose first field is blank in the layout most have, without it"));
    // Record 4's first field is blank, and it does not fit the layout most
    // records have.
    EXPECT_TRUE(warned(result, 4, "opcode 07 has 9 values where the format gives 8"));
    // Record 5 read without the column has no horizontal circle; with it, a
    // whole backsight: which wrote it cannot be told.
    EXPECT_TRUE(warned(result, 5,
                       "opcode 04 has 9 values, which fit the format's layout both with the "
                       "column after the opcode and without it: read without it, as this file's "
                       "records are, the horizontal circle is blank, and read with it, it is not"));
    ASSERT_EQ(result.project.stations.size(), 1u);
    EXPECT_TRUE(result.project.stations[0].backsightPointId.empty());
}

TEST(OpcodeFieldFile, ABacksightWrittenWithTheColumnInAFileWithoutItIsSkippedNotReadShifted)
{
    // [FLD]'s layout, decided by its coded records, and one backsight of ten
    // fields written with the column: read without it, its horizontal circle
    // is blank and 0, 90 and 100 become its zenith, slope distance and
    // azimuth - an azimuth of 100 that turned the setup's shots, with no
    // warning. Read with it, it is whole. It is skipped, saying so.
    const ReadResult result = readLines({
        "02\tK\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
        "02\tK\t\tCP2\t\t\t1000.0\t5100.0\t20.0",
        "03\tK\t\tCP1\t\t\t1.5",
        "04\t\t\t\tCP2\t\t\t0.00000000\t90.00000000\t100.000",
        "07\tEB\t01\t101\t\t\t45.0\t95.0\t50.0",
    });
    EXPECT_TRUE(warned(result, 4,
                       "read without it, as this file's records are, the horizontal circle is "
                       "blank, and read with it, it is not; which value is which cannot be told"));
    EXPECT_EQ(result.recordsSkipped, 1u);
    ASSERT_EQ(result.project.stations.size(), 1u);
    EXPECT_TRUE(result.project.stations[0].backsightPointId.empty());
    EXPECT_FALSE(result.project.stations[0].statedBacksightAzimuth.has_value());
    EXPECT_EQ(reducedPositions(result.project).count("101"), 0u);
}

TEST(OpcodeFieldFile, ARecordWhoseOtherLayoutReadsNoWholeRecordIsReadInTheFilesLayout)
{
    // A job with the column after the opcode, and records that fit both
    // layouts with their first measured value blank in it. Read without the
    // column, each would be no record: the setup on A has "nail in kerb"
    // where its instrument height would be; the setup on B (its name A's
    // place, its comment a number) names no point; the backsight's comment
    // is where its horizontal circle would be. That is no sign which layout
    // wrote them, and each is read as the file's records are - where any
    // value there had them skipped, and every measurement after a setup.
    //
    // A (1000, 5000) oriented on B, due north at circle 0: 101 at circle 90,
    // level, 10 m, is at (1010, 5000), 102 at circle 180 at (1000, 4990). B
    // oriented on A, due south at circle 180 (orientation 0): 103 at circle
    // 90, 10 m, at (1010, 5100). Neither setup states a height: 0 is used.
    const ReadResult result = readLines({
        "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tB\t\t\t1000.0\t5100.0\t21.0",
        "03\t\t\t\tA\t\tnail in kerb\t",
        "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
        "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
        "07\t\tEB\t01\t102\t\t\t180.0\t90.0\t10.0",
        "03\t\t\t\t\tB\t1.5\t",
        "04\t\t\t\tA\t\t\t180.0\t90.0\t100.0",
        "04\t\t\t\tA\t\tbs nail\t\t90.0\t100.0",
        "07\t\tEB\t02\t103\t\t\t90.0\t90.0\t10.0",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    EXPECT_TRUE(warned(result, 3, "the setup states no instrument height; 0 is used"));
    EXPECT_TRUE(warned(result, 7, "the setup states no instrument height; 0 is used"));
    ASSERT_EQ(result.project.stations.size(), 2u);
    EXPECT_EQ(result.project.stations[0].setup.pointId, "A");
    EXPECT_EQ(result.project.stations[1].setup.pointId, "B");
    // The comments describe the points the setups stand on.
    EXPECT_EQ(topcon_test::point(result.project, "A")->description, "nail in kerb");
    EXPECT_EQ(topcon_test::point(result.project, "B")->description, "1.5");
    // The backsight of record 9 has no horizontal circle, and a zenith and a
    // distance to A.
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(result.project.stations[1], "A").size(),
              2u);
    const auto positions = reducedPositions(result.project);
    ASSERT_EQ(positions.count("101") + positions.count("102") + positions.count("103"), 3u);
    EXPECT_NEAR(positions.at("101").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("101").northing, 5000.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("102").easting, 1000.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("102").northing, 4990.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("103").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("103").northing, 5100.0, kPositionTolerance);
}

TEST(OpcodeFieldFile, ARecordWrittenWithoutTheColumnInAJobWithItIsReadByItsFields)
{
    // A job with the column, its backsights of ten fields, and one shot
    // written without it: its feature code first says so, and it is read.
    // Before, the one dissenting record made every record that fits both
    // layouts - both backsights - be skipped, and no setup was oriented.
    const ReadResult result = readLines({
        "02\t\t\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tCP2\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tCP1\t\t\t1.5",
        "04\t\t\t\tCP2\t\t\t0.0\t90.0\t100.0",
        "07\t\tEB\t01\t101\t\t\t45.0\t90.0\t50.0",
        "07\tEB\t01\t103\t\t\t47.0\t90.0\t50.0",
        "03\t\t\t\tCP2\t\t\t1.5",
        "04\t\t\t\tCP1\t\t\t180.0\t90.0\t100.0",
        "07\t\tEB\t01\t102\t\t\t135.0\t90.0\t50.0",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    EXPECT_TRUE(warned(result, 0,
                       "6 fixed record(s) have the column after the opcode (blank or a date or "
                       "time) and 1 do not"));
    ASSERT_EQ(result.project.stations.size(), 2u);
    EXPECT_EQ(result.project.stations[0].backsightPointId, "CP2");
    EXPECT_EQ(result.project.stations[1].backsightPointId, "CP1");
    // CP1 reads 0 on CP2, due north: orientation 0; CP2 reads 180 on CP1,
    // due south: orientation 0. Zeniths of 90, so each shot is 50 m out.
    const auto positions = reducedPositions(result.project);
    ASSERT_EQ(positions.count("101") + positions.count("102") + positions.count("103"), 3u);
    EXPECT_NEAR(positions.at("101").easting, 1000.0 + 50.0 * std::sin(degrees(45.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("101").northing, 5000.0 + 50.0 * std::cos(degrees(45.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("103").easting, 1000.0 + 50.0 * std::sin(degrees(47.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("103").northing, 5000.0 + 50.0 * std::cos(degrees(47.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("102").easting, 1000.0 + 50.0 * std::sin(degrees(135.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("102").northing, 5100.0 + 50.0 * std::cos(degrees(135.0)),
                kPositionTolerance);
}

TEST(OpcodeFieldFile, InAFileNoRecordDecidesFreeRecordsAreReadAsTheFormatWritesThemAsTheFixedOnesAre)
{
    // Uncoded records in [FLD]'s layout: none decides (a blank first field is
    // an empty feature code, or the column), so the file is read as [FLD]
    // writes it - its unnamed attributes, and its offset naming a point by
    // its ID, too. The column taken for them had made attributes named by
    // their values and an offset one value short.
    const ReadResult result = readLines({
        "100\tdegrees\tmetres",
        "02\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\tK\t\t\t1.5",
        "04\t\t\tB\t\t\t0.0\t90.0\t100.0",
        "07\t\t\tP5\t\t\t90.0\t90.0\t10.0",
        "73\t\tfence type A",
        "72\t\t0.450",
        "42\t\t\tP5\t\t\t0.500",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    const auto p5 = metadataOf(result.project, "P5");
    EXPECT_EQ(valueOf(p5, "unnamed attribute (record 7)"), "fence type A");
    EXPECT_EQ(valueOf(p5, "unnamed attribute (record 8)"), "0.450");
    EXPECT_EQ(p5.count("fence type A"), 0u);
    // K oriented on B, due north at circle 0; P5 10 m east, then 0.5 m further out.
    const auto positions = reducedPositions(result.project);
    ASSERT_TRUE(positions.contains("P5"));
    EXPECT_NEAR(positions.at("P5").easting, 1010.5, kPositionTolerance);
    EXPECT_NEAR(positions.at("P5").northing, 5000.0, kPositionTolerance);
}

TEST(OpcodeFieldFile, AFeatureCodeShapedLikeANumberRangeIsACodeNotADate)
{
    // "1-2" is no date: a backsight of [FLD]'s layout coded so is read as
    // one, where it was taken to have the column and skipped for naming no
    // point.
    const ReadResult result = readLines({
        "02\tK\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
        "03\tK\t\tCP1\t\t\t1.5",
        "04\t1-2\t\tBSX\t\t\t30.0\t90.0\t100.000\t45.0",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    const survey::SurveyStation& station = result.project.stations.at(0);
    EXPECT_EQ(station.backsightPointId, "BSX");
    ASSERT_TRUE(station.backsightAzimuth && station.statedBacksightAzimuth);
    EXPECT_NEAR(*station.backsightAzimuth, degrees(30.0), kAngleTolerance);
    EXPECT_NEAR(*station.statedBacksightAzimuth, degrees(45.0), kAngleTolerance);
    EXPECT_EQ(valueOf(station.metadata, "backsight record 3"), "feature code 1-2");
}

TEST(OpcodeFieldFile, AFixedRecordThatFitsNeitherLayoutInAFileNoRecordDecidedSaysHowItWasRead)
{
    const ReadResult result = readLines({
        "03\t\t\tS\t\t\t1.5",
        "07\t\t\t1\t\t\t10.0\t90.0", // uncoded, and a value short of either layout
    });
    EXPECT_TRUE(warned(result, 2,
                       "opcode 07 has 7 values where the format gives 8 (no record shows the "
                       "column after the opcode as blank or a date or time, so the file is read "
                       "without it)"));
    EXPECT_EQ(result.recordsSkipped, 1u);
}

TEST(OpcodeFieldFile, AFileWithoutTheColumnReadsARecordOfFixedFormThatHasIt)
{
    // [FLD]'s layout, decided by its coded records, and a target height, a
    // scale factor and the units written with the column: an empty value
    // before a value those records never have, so it is the column.
    const ReadResult result = readLines({
        "100\t\tdegrees\tmetres\tmillibars\tcelsius",
        "02\tK\t\tCP1\t\t\t1000.0\t5000.0\t20.0",
        "03\tK\t\tCP1\t\t\t1.5",
        "05\t\t2.500",
        "09\t\t0.5",
        "07\tEB\t01\t701\t\t\t10.0\t95.0\t50.0",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    ASSERT_EQ(result.project.stations.size(), 1u);
    const survey::SurveyStation& station = result.project.stations[0];
    const auto zeniths = observationsTo<survey::ZenithAngleObservation>(station, "701");
    ASSERT_EQ(zeniths.size(), 1u);
    EXPECT_DOUBLE_EQ(zeniths[0].targetHeight, 2.5);
    const auto distances = observationsTo<survey::DistanceObservation>(station, "701");
    ASSERT_EQ(distances.size(), 1u);
    EXPECT_DOUBLE_EQ(distances[0].distance, 25.0); // 50 x 0.5
}

TEST(OpcodeFieldFile, ATrailingTabIsNotAValue)
{
    // Every fixed record ends with a tab, as the files' 128 does: one blank
    // field past the count, which the records are read without.
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0\t",
        "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0\t",
        "03\t\t\t\tK\t\t\t1.5\t",
        "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0\t",
        "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0\t",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    EXPECT_EQ(result.project.points.size(), 2u);
    ASSERT_EQ(result.project.stations.size(), 1u);
    EXPECT_EQ(result.project.stations[0].backsightPointId, "B");
    EXPECT_DOUBLE_EQ(result.project.stations[0].setup.instrumentHeight, 1.5);
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(result.project.stations[0], "1").at(0)
                  .distance,
              5.0);
}

// ---- Offsets ------------------------------------------------------------------------------

TEST(OpcodeFieldFile, OffsetsPutTheirPointWhereTheFormatSays)
{
    // S at (1000, 5000, 20), instrument and target 1.5 m, oriented on B due
    // north at circle 0: circle 90 is east, 0 north. [FLD] 44.8: a radial
    // offset along the line from the station, positive away; a tangential
    // one at right angles, negative to the left looking from the station; a
    // height offset added to the height.
    const ReadResult result = readLines({
        "02\t\t\t\tS\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tS\t\t\t1.5",
        "05\t\t1.5",
        "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
        "07\t\tEP\t1\tP1\t\t\t90.0\t90.0\t10.0",
        "42\t\t0.5",
        "07\t\tEP\t1\tP2\t\t\t90.0\t90.0\t20.0",
        "43\t\t0.5",
        "07\t\tEP\t1\tP3\t\t\t0.0\t90.0\t30.0",
        "43\t\t-0.5",
        "42\t\t-1.0",
        "07\t\tEP\t1\tP4\t\t\t45.0\t90.0\t10.0",
        "44\t\t0.25",
    });
    EXPECT_TRUE(result.warnings.empty()) << result.warnings.front().message;
    const auto positions = reducedPositions(result.project);
    ASSERT_EQ(positions.count("P1") + positions.count("P2") + positions.count("P3") +
                  positions.count("P4"),
              4u);
    // P1: 10 m east, then 0.5 m further out.
    EXPECT_NEAR(positions.at("P1").easting, 1010.5, kPositionTolerance);
    EXPECT_NEAR(positions.at("P1").northing, 5000.0, kPositionTolerance);
    // P2: 20 m east, then 0.5 m to the right looking east: south.
    EXPECT_NEAR(positions.at("P2").easting, 1020.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("P2").northing, 4999.5, kPositionTolerance);
    // P3: 30 m north, 0.5 m to the left looking north (west) and 1 m back
    // toward the station - each from the measured position, in either order.
    EXPECT_NEAR(positions.at("P3").easting, 999.5, kPositionTolerance);
    EXPECT_NEAR(positions.at("P3").northing, 5029.0, kPositionTolerance);
    // P4: 10 m on bearing 45, its plan position unmoved, 0.25 m higher than
    // the station's 20 + 1.5 - 1.5.
    EXPECT_NEAR(positions.at("P4").easting, 1000.0 + 10.0 * std::sin(kPi / 4.0),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("P4").northing, 5000.0 + 10.0 * std::cos(kPi / 4.0),
                kPositionTolerance);
    ASSERT_TRUE(positions.at("P4").elevation.has_value());
    EXPECT_NEAR(*positions.at("P4").elevation, 20.25, kPositionTolerance);
}

TEST(OpcodeFieldFile, AnOffsetMovesThePointItNamesAndOneThatCannotBeReadIsSkippedSayingWhy)
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
        "42\t\tXX\t9\t\t\t\t0.1",    // a string that is not open
        "42\t\t\t\tZZ\t\t\t0.1",     // a point no record made
    });
    const auto p1 = metadataOf(result.project, "1");
    const auto p2 = metadataOf(result.project, "2");
    EXPECT_EQ(valueOf(p2, "offset record 4"), "height 0.1 m from setup S, applied to the shot of "
                                              "record 3");
    EXPECT_EQ(valueOf(p1, "offset record 5"), "tangential -0.3 m from setup S, applied to the "
                                              "shot of record 2");
    EXPECT_EQ(valueOf(p2, "offset record 6"), "radial 0.2 m from setup S, applied to the shot of "
                                              "record 3");
    EXPECT_TRUE(warned(result, 7, "with 3 value(s) where the format gives the offset"));
    EXPECT_TRUE(warned(result, 8, "radial offset (opcode 42) with no offset"));
    EXPECT_TRUE(warned(result, 9, "radial offset (opcode 42): there is no open string XX 9"));
    EXPECT_TRUE(warned(result, 10, "names no string and no point a record before it made"));
    EXPECT_EQ(result.recordsSkipped, 4u);
    // Point 2 (circle 20, zenith 90, 5 m): 0.2 m further out and 0.1 m up,
    // so 5.2 m out on the same circle, rising 0.1 m. Point 1 (circle 10, 5 m):
    // 0.3 m to the left, the circle less atan2(0.3, 5).
    const survey::SurveyStation& station = result.project.stations.at(0);
    const auto distance2 = observationsTo<survey::DistanceObservation>(station, "2");
    ASSERT_EQ(distance2.size(), 1u);
    EXPECT_NEAR(distance2[0].distance, std::hypot(5.2, 0.1), 1e-12);
    const auto zenith2 = observationsTo<survey::ZenithAngleObservation>(station, "2");
    ASSERT_EQ(zenith2.size(), 1u);
    EXPECT_NEAR(zenith2[0].angle, std::atan2(5.2, 0.1), kAngleTolerance);
    const auto direction1 = observationsTo<survey::HorizontalDirectionObservation>(station, "1");
    ASSERT_EQ(direction1.size(), 1u);
    EXPECT_NEAR(direction1[0].direction, degrees(10.0) - std::atan2(0.3, 5.0), kAngleTolerance);
}

TEST(OpcodeFieldFile, AnOffsetOfAPointMeasuredTwiceOrNotShotFromTheSetupIsKeptNotApplied)
{
    const ReadResult result = readLines({
        "02\t\t\t\tC\t\t\t1000.0\t5000.0\t20.0",
        "42\t\t0.5", // an entered coordinate
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\tA\t\t\t10.0\t90.0\t5.0",
        "07\t\tEP\t1\tA\t\t\t190.0\t270.0\t5.0",
        "43\t\t0.5", // A is shot twice from S: [FLD] moves the second, a point of its own
        "07\t\tEP\t1\tD\t\t\t30.0\t90.0\t5.0",
        "03\t\t\t\tT\t\t\t1.5",
        "43\t\t\t\tD\t\t\t0.5", // D was shot from S, not from T
        "07\t\tEP\t1\tE\t\t\t90.0\t90.0\t1.0",
        "42\t\t-1.0", // 1 m back toward T: T's own mark, which no shot can see
    });
    EXPECT_TRUE(warned(result, 2,
                       "is kept in its metadata and not applied: point 'C' is the coordinate of "
                       "record 1, which is kept as the file states it"));
    EXPECT_TRUE(warned(result, 6,
                       "is kept in its metadata and not applied: point 'A' was measured 2 times "
                       "from setup S: the format moves only the measurement an offset follows, a "
                       "point of its own there, and Katana, which makes one point of them, would "
                       "put it between the moved and the unmoved"));
    EXPECT_TRUE(warned(result, 9,
                       "is kept in its metadata and not applied: point 'D' was not measured "
                       "(opcode 07) from the current setup"));
    EXPECT_EQ(valueOf(metadataOf(result.project, "A"), "offset record 6"),
              "tangential 0.5 m from setup S, not applied");
    EXPECT_EQ(valueOf(metadataOf(result.project, "D"), "offset record 9"),
              "tangential 0.5 m, not applied");
    EXPECT_TRUE(warned(result, 11,
                       "is kept in its metadata and not applied: with the other offsets of its "
                       "shot it puts point 'E' on the station"));
    EXPECT_EQ(valueOf(metadataOf(result.project, "E"), "offset record 11"),
              "radial -1 m from setup T, not applied");
    // A's and E's shots are as measured.
    const auto toA =
        observationsTo<survey::HorizontalDirectionObservation>(result.project.stations.at(0), "A");
    ASSERT_EQ(toA.size(), 2u);
    EXPECT_NEAR(toA[0].direction, degrees(10.0), kAngleTolerance);
    const auto toE =
        observationsTo<survey::DistanceObservation>(result.project.stations.at(1), "E");
    ASSERT_EQ(toE.size(), 1u);
    EXPECT_DOUBLE_EQ(toE[0].distance, 1.0);
    EXPECT_EQ(metadataOf(result.project, "E").count("shot record 10 as measured"), 0u);
    EXPECT_EQ(result.recordsSkipped, 0u);
}

TEST(OpcodeFieldFile, TwoOffsetsOfOneKindOnAShotAreKeptAndNoneOfItsOffsetsIsApplied)
{
    // Whether the second radial offset adds to the first or replaces it,
    // each being "from the specified points original position", the format
    // does not say: the shot is left as measured, its tangential offset too.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\tP1\t\t\t90.0\t90.0\t10.0",
        "42\t\t0.5",
        "42\t\t0.3",
        "43\t\t0.1",
    });
    for (const std::size_t record : {3u, 4u, 5u}) {
        EXPECT_TRUE(warned(result, record,
                           "the shot of record 2 has two offsets of one kind, and whether the "
                           "second adds to the first or replaces it the format does not say"))
            << record;
    }
    const survey::SurveyStation& station = result.project.stations.at(0);
    EXPECT_DOUBLE_EQ(observationsTo<survey::DistanceObservation>(station, "P1").at(0).distance,
                     10.0);
    EXPECT_NEAR(observationsTo<survey::HorizontalDirectionObservation>(station, "P1")
                    .at(0)
                    .direction,
                kPi / 2.0, kAngleTolerance);
    EXPECT_EQ(result.recordsSkipped, 0u);
}

TEST(OpcodeFieldFile, AnOffsetOfAShotWithoutASlopeDistanceOrAZenithIsKeptNotApplied)
{
    // Shot 1 has no slope distance (its last field blank), shot 2 no zenith:
    // neither has a plan distance or a height to offset.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t",
        "43\t\t0.2",
        "07\t\tEP\t1\t2\t\t\t20.0\t\t5.0",
        "42\t\t0.3",
    });
    EXPECT_TRUE(warned(result, 3,
                       "is kept in its metadata and not applied: the shot of record 2 has no "
                       "slope distance or no zenith, so the point has no plan distance or height "
                       "to offset"));
    EXPECT_TRUE(warned(result, 5, "the shot of record 4 has no slope distance or no zenith"));
    EXPECT_EQ(valueOf(metadataOf(result.project, "1"), "offset record 3"),
              "tangential 0.2 m from setup S, not applied");
    const survey::SurveyStation& station = result.project.stations.at(0);
    EXPECT_NEAR(observationsTo<survey::HorizontalDirectionObservation>(station, "1").at(0).direction,
                degrees(10.0), kAngleTolerance);
    EXPECT_DOUBLE_EQ(observationsTo<survey::DistanceObservation>(station, "2").at(0).distance, 5.0);
    EXPECT_EQ(result.recordsSkipped, 0u);
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

TEST(OpcodeFieldFile, WhatActsOnTheMeasurementAfterOneThatWasNotReadIsSkippedNamingIt)
{
    // A multiple coding, an offset and a bare close after a shot a value
    // short: each would act on point 1, the measurement before.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "07\t\tEP\t1\t2\t\t\t20.0\t90.0", // a value short
        "16\t\tNS\t01\t\t\t",
        "42\t\t0.5",
        "20",
    });
    EXPECT_TRUE(warned(result, 4,
                       "multiple coding (opcode 16) of the measurement of record 3, which was not "
                       "read"));
    EXPECT_TRUE(warned(result, 5,
                       "radial offset (opcode 42) of the measurement of record 3, which was not "
                       "read"));
    EXPECT_TRUE(warned(result, 6,
                       "close string (opcode 20) of the measurement of record 3, which was not "
                       "read"));
    EXPECT_EQ(topcon_test::feature(result.project, "NS"), nullptr);
    ASSERT_EQ(featuresOf(result.project, "EP").size(), 1u);
    EXPECT_FALSE(featuresOf(result.project, "EP")[0]->closed);
    EXPECT_DOUBLE_EQ(observationsTo<survey::DistanceObservation>(result.project.stations.at(0), "1")
                         .at(0)
                         .distance,
                     5.0);
    EXPECT_EQ(result.recordsSkipped, 4u);
}

TEST(OpcodeFieldFile, ALineThatIsNoRecordLeavesNothingCurrentSoWhatFollowsIsNotGivenToThePointBefore)
{
    // What a line that is no record made is not known: what follows it and
    // would describe that is skipped, naming the line, not given to P1 or P3
    // - its date, an offset that moved P1's shot, a second string, a close.
    // A setup written with a plus leaves no setup for P4's shot.
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
        "07\t\tEP\t1\tP1\t\t\t90.0\t90.0\t10.0",
        "0007\t\tEP\t1\tP2\t\t\t180.0\t90.0\t10.0", // 6: four digits
        "73\t\tDate\td2",
        "42\t\t0.5",
        "16\t\tNS\t01\t\t\t",
        "20",
        "07\t\tEP\t1\tP3\t\t\t0.0\t90.0\t10.0", // 11
        "O7 written with a letter",            // 12: no number at all
        "72\t\tDepth\t9.9",
        "+3\t\t\t\tB\t\t\t1.5", // 14: a setup, with a plus
        "07\t\tEP\t1\tP4\t\t\t270.0\t90.0\t10.0",
    });
    EXPECT_TRUE(warned(result, 6, "'0007' is not an opcode"));
    EXPECT_TRUE(warned(result, 7, "an attribute of the point of record 6, which was not read"));
    EXPECT_TRUE(warned(result, 8,
                       "radial offset (opcode 42) of the measurement of record 6, which was not "
                       "read"));
    EXPECT_TRUE(warned(result, 9, "of the measurement of record 6, which was not read"));
    EXPECT_TRUE(warned(result, 10, "of the measurement of record 6, which was not read"));
    EXPECT_TRUE(warned(result, 12, "is not an opcode: a record opens with a number"));
    EXPECT_TRUE(warned(result, 13, "an attribute of the point of record 12, which was not read"));
    EXPECT_TRUE(warned(result, 14,
                       "it is taken for the setup (opcode 03) it would be; the measurements after "
                       "it are not read until the next setup"));
    EXPECT_TRUE(warned(result, 15, "measurement from the setup of record 14, which was not read"));
    EXPECT_EQ(metadataOf(result.project, "P1").count("Date"), 0u);
    EXPECT_EQ(metadataOf(result.project, "P3").count("Depth"), 0u);
    EXPECT_EQ(topcon_test::feature(result.project, "NS"), nullptr);
    EXPECT_FALSE(featuresOf(result.project, "EP").at(0)->closed);
    ASSERT_EQ(result.project.stations.size(), 1u);
    const survey::SurveyStation& station = result.project.stations[0];
    EXPECT_DOUBLE_EQ(observationsTo<survey::DistanceObservation>(station, "P1").at(0).distance,
                     10.0);
    EXPECT_TRUE(observationsTo<survey::DistanceObservation>(station, "P4").empty());
    EXPECT_EQ(result.recordsSkipped, 9u);
}

TEST(OpcodeFieldFile, ACheckCoordinateOrAnOpcodeTheFormatDoesNotDefineEndsTheCurrentPoint)
{
    // 14 checks a named coordinate: the note after it is the check's. What an
    // undefined opcode made is not known: the attribute after it is not
    // given to the shot before either.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t101\t\t\t10.0\t90.0\t5.0",
        "14\t\t\t\t\tCP2\t\t1000.010\t5100.020\t21.510",
        "73\t\tNote\tcheck of CP2",
        "07\t\tEP\t1\t102\t\t\t20.0\t90.0\t5.0",
        "150\t\tx",
        "73\t\tAfter\t1",
    });
    EXPECT_TRUE(warned(result, 3, "opcode 14 (check coordinate) is not one this reader imports"));
    EXPECT_TRUE(warned(result, 4, "an attribute of the point of record 3, which was not read"));
    EXPECT_TRUE(warned(result, 6, "opcode 150 is not one the format defines"));
    EXPECT_TRUE(warned(result, 7, "an attribute of the point of record 6, which was not read"));
    EXPECT_EQ(metadataOf(result.project, "101").count("Note"), 0u);
    EXPECT_EQ(metadataOf(result.project, "102").count("After"), 0u);
    EXPECT_EQ(result.recordsSkipped, 4u);
}

TEST(OpcodeFieldFile, ATargetHeightThatIsNoNumberIsSkippedAndTheOneBeforeStillApplies)
{
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "05\t\t1.600",
        "05\t\tabc",
        "05\t\t",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
    });
    EXPECT_TRUE(warned(result, 3, "target height 'abc' is not a number"));
    EXPECT_TRUE(warned(result, 3, "target height record without a height the reader can read"));
    EXPECT_TRUE(warned(result, 4, "target height record without a height the reader can read"));
    EXPECT_DOUBLE_EQ(observationsTo<survey::DistanceObservation>(result.project.stations.at(0), "1")
                         .at(0)
                         .targetHeight,
                     1.6);
    EXPECT_EQ(result.recordsSkipped, 2u);
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

TEST(OpcodeFieldFile, AResectionAfterASetupStartsASetupOfItsOwn)
{
    // The resection job's 128 blocks after a 03: before the reader read 128,
    // their shots were filed under that 03's setup and oriented by it.
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
        "128\t\t\t\t\tRS1\t\t1.600\t",
        "7\t\t\t\tK\tK\t\t180.0\t90.0\t100.0",
        "129",
        "07\t\tEB\t01\t2\t\t\t20.0\t90.0\t5.0",
    });
    ASSERT_EQ(result.project.stations.size(), 2u);
    const survey::SurveyStation& k = result.project.stations[0];
    const survey::SurveyStation& resected = result.project.stations[1];
    EXPECT_EQ(k.setup.pointId, "K");
    EXPECT_EQ(k.observations.size(), 3u); // point 1 only
    EXPECT_EQ(resected.setup.pointId, "RS1");
    EXPECT_DOUBLE_EQ(resected.setup.instrumentHeight, 1.6);
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(resected, "K").size(), 1u);
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(resected, "2").size(), 1u);
    EXPECT_EQ(result.recordsSkipped, 0u);
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

TEST(OpcodeFieldFile, AResectionEndThatDoesNotMatchItsStartIsWarnedOf)
{
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "128\t\t1.500",
        "07\t\tEB\t01\t1\t\t\t10.0\t90.0\t5.0",
        "139", // a Helmert end for a least squares resection
        "138\t\t1.500",
        "07\t\tEB\t01\t2\t\t\t20.0\t90.0\t5.0",
        "139\t\tstray",
    });
    EXPECT_TRUE(warned(result, 4,
                       "end of a resection (opcode 139) ends the resection of record 2, which is "
                       "opcode 128"));
    EXPECT_TRUE(warned(result, 7,
                       "end of a resection (opcode 139) carries values the format does not give "
                       "it; they are not kept"));
    ASSERT_EQ(result.project.stations.size(), 2u);
    EXPECT_EQ(valueOf(result.project.stations[1].metadata, "resection"), "Helmert, record 5");
    EXPECT_EQ(result.recordsSkipped, 0u);
}

TEST(OpcodeFieldFile, ACodedSetupStringsNothingAndKeepsItsCodeWithTheSetup)
{
    // [FLD] 44.4 strings a measurement point; a setup makes none.
    const ReadResult result = readLines({
        "02\t\t\t\tS1\t\t\t1000.0\t5000.0\t20.0",
        "03\t\tKB\t1\tS1\t\tnail\t1.500",
        "07\t\tKB\t1\t101\t\t\t10.0\t90.0\t5.0",
        "128\t\tKB\t1\t\tR1\t\t0.000\t",
        "07\t\tKB\t1\t102\t\t\t20.0\t90.0\t5.0",
    });
    const survey::SurveyFeature* kb = topcon_test::feature(result.project, "KB");
    ASSERT_NE(kb, nullptr);
    EXPECT_EQ(kb->pointIds, (std::vector<std::string>{"101", "102"}));
    ASSERT_EQ(result.project.stations.size(), 2u);
    EXPECT_EQ(valueOf(result.project.stations[0].metadata, "setup coding"),
              "feature code KB, string number 1, comment nail");
    EXPECT_EQ(valueOf(result.project.stations[1].metadata, "setup coding"),
              "feature code KB, string number 1");
    // The comment still describes the point the setup stands on.
    EXPECT_EQ(topcon_test::point(result.project, "S1")->description, "nail");
    EXPECT_EQ(topcon_test::point(result.project, "S1")->code, "");
}

TEST(OpcodeFieldFile, AResectionGivenAPointIdWithItsNameKeepsTheId)
{
    // As a 03 does: the setup is on the name, and the ID is kept with it.
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "128\t\t\t\t77\tRS1\t\t1.550\t",
        "129",
    });
    ASSERT_EQ(result.project.stations.size(), 1u);
    EXPECT_EQ(result.project.stations[0].setup.pointId, "RS1");
    EXPECT_EQ(valueOf(result.project.stations[0].metadata, "point ID"), "77");
    EXPECT_EQ(result.recordsSkipped, 0u);
}

TEST(OpcodeFieldFile, AStringIsClosedByItsCodeOrItsPointAndAClosingThatFindsNoneIsSkipped)
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
        "20\t\t\t\t\t\tnote", // a description naming neither a string nor a point
        "20\t\tEP\t1",         // two values
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
    EXPECT_TRUE(warned(result, 13, "whose point description names neither a string nor a point"));
    EXPECT_TRUE(warned(result, 14,
                       "with 2 value(s) where the format gives none, or a point description of 5"));
    EXPECT_EQ(result.recordsRead, 10u);
    EXPECT_EQ(result.recordsSkipped, 4u);
}

TEST(OpcodeFieldFile, ABareCloseAfterMultipleCodingClosesTheSecondString)
{
    // 44.4: the current string is the current measurement point's, and a 16
    // makes a new measurement point in its own string - so the bare 20 after
    // it closes TB 7, not the shot's HB 2.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tHB\t2\t301\t\t\t10.0\t90.0\t5.0",
        "07\t\tHB\t2\t302\t\t\t20.0\t90.0\t5.0",
        "07\t\tTB\t7\t401\t\t\t30.0\t90.0\t5.0",
        "07\t\tTB\t7\t402\t\t\t40.0\t90.0\t5.0",
        "07\t\tHB\t2\t303\t\t\t50.0\t90.0\t5.0",
        "16\t\tTB\t7\t\t\t",
        "20",
    });
    const auto tb = featuresOf(result.project, "TB");
    ASSERT_EQ(tb.size(), 1u);
    EXPECT_TRUE(tb[0]->closed);
    EXPECT_EQ(tb[0]->pointIds, (std::vector<std::string>{"401", "402", "303"}));
    const auto hb = featuresOf(result.project, "HB");
    ASSERT_EQ(hb.size(), 1u);
    EXPECT_FALSE(hb[0]->closed);
    EXPECT_EQ(hb[0]->pointIds, (std::vector<std::string>{"301", "302", "303"}));
    EXPECT_EQ(result.recordsSkipped, 0u);
}

TEST(OpcodeFieldFile, AMultipleCodingNotReadLeavesTheCurrentStringUnknownSoABareCloseIsSkipped)
{
    // A 16 makes its own string the current one. One that is not read - this
    // one has a sixth value, and the next no feature code - leaves which
    // string is current unknown: the bare 20 after it is skipped, naming it,
    // where it closed EB 01, the string of the shot before. A measurement
    // read after it makes its own string current, and so does a 16 read
    // after it: the last 20 closes TB 4, 301's second string, not HB 3.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEB\t01\t101\t\t\t10.0\t90.0\t5.0",
        "07\t\tEB\t01\t102\t\t\t20.0\t90.0\t5.0",
        "16\t\tNS\t01\t\t\t\tx",
        "20",
        "07\t\tFE\t2\t201\t\t\t30.0\t90.0\t5.0",
        "16\t\t\t\t\t\t",
        "20",
        "07\t\tFE\t2\t202\t\t\t40.0\t90.0\t5.0",
        "20",
        "07\t\tHB\t3\t301\t\t\t50.0\t90.0\t5.0",
        "16\t\t\t\t\t\t",
        "16\t\tTB\t4\t\t\t",
        "20",
    });
    EXPECT_TRUE(warned(result, 14, "the string TB 4 is closed with 1 point(s)"));
    const auto tb = featuresOf(result.project, "TB");
    ASSERT_EQ(tb.size(), 1u);
    EXPECT_TRUE(tb[0]->closed);
    EXPECT_EQ(tb[0]->pointIds, (std::vector<std::string>{"301"}));
    const auto hb = featuresOf(result.project, "HB");
    ASSERT_EQ(hb.size(), 1u);
    EXPECT_FALSE(hb[0]->closed);
    EXPECT_TRUE(warned(result, 12, "multiple coding (opcode 16) with no feature code"));
    EXPECT_TRUE(warned(result, 4,
                       "multiple coding (opcode 16) with 6 value(s) where the format gives a point "
                       "description of 5"));
    EXPECT_TRUE(warned(result, 5,
                       "close string (opcode 20): the current string is the one the multiple "
                       "coding of record 4 made current, which was not read"));
    EXPECT_TRUE(warned(result, 7, "multiple coding (opcode 16) with no feature code"));
    EXPECT_TRUE(warned(result, 8, "the multiple coding of record 7 made current, which was not read"));
    const auto eb = featuresOf(result.project, "EB");
    ASSERT_EQ(eb.size(), 1u);
    EXPECT_FALSE(eb[0]->closed);
    const auto fe = featuresOf(result.project, "FE");
    ASSERT_EQ(fe.size(), 1u);
    EXPECT_TRUE(fe[0]->closed);
    EXPECT_EQ(fe[0]->pointIds, (std::vector<std::string>{"201", "202"}));
    EXPECT_TRUE(warned(result, 10, "the string FE 2 is closed with 2 point(s)"));
    // Records 4, 5, 7, 8 and 12.
    EXPECT_EQ(result.recordsSkipped, 5u);
}

TEST(OpcodeFieldFile, AMultipleCodingACloseAndAnOffsetWithATrailingTabAreRead)
{
    // One blank value past a record's count is a trailing tab, as a fixed
    // record's is: each of these was skipped for its count. K (1000, 5000)
    // oriented on B, due north at circle 0. The 16 strings 102 into NS 01 as
    // well; the 42 moves 102, the current measurement, 0.5 m further out, to
    // 10.5 m on bearing 100; the 20 closes NS 01; the 43 moves 101, 10 m due
    // east, 0.3 m to the left looking from K: north, to (1010, 5000.3).
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
        "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
        "07\t\tEB\t01\t102\t\t\t100.0\t90.0\t10.0",
        "16\t\tNS\t01\t\t\t\t",
        "42\t\t0.5\t",
        "20\t\tNS\t01\t\t\t\t",
        "43\t\t\t\t101\t\t\t-0.3\t",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    const auto ns = featuresOf(result.project, "NS");
    ASSERT_EQ(ns.size(), 1u);
    EXPECT_TRUE(ns[0]->closed);
    EXPECT_EQ(ns[0]->pointIds, (std::vector<std::string>{"102"}));
    const auto positions = reducedPositions(result.project);
    ASSERT_TRUE(positions.contains("101") && positions.contains("102"));
    EXPECT_NEAR(positions.at("102").easting, 1000.0 + 10.5 * std::sin(degrees(100.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("102").northing, 5000.0 + 10.5 * std::cos(degrees(100.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("101").easting, 1010.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("101").northing, 5000.3, kPositionTolerance);
}

TEST(OpcodeFieldFile, ACloseOrAnOffsetFindsANamedShotByItsPointId)
{
    // [FLD] 44.8: an offset "adjusts the point with that point ID" and a 20
    // closes "the string containing that point ID". The shots are named P1,
    // P2 and P3, with IDs 11, 12 and 13; the offset names P2 and the close P1
    // by the ID alone. Before, the ID was looked for among the names, and
    // both were skipped, saying no record had made the point. Two more shots
    // are NAMED 11 and 12, in EQ 1: the ID is looked for before the name, so
    // neither is moved nor has its string closed.
    //
    // K (1000, 5000) oriented on B, due north at circle 0: P2 at circle 80,
    // level, 10 m, moved 0.5 m further out is 10.5 m out on bearing 80; the
    // shot named 12, at circle 50, stays 10 m out.
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
        "07\t\tEP\t1\t11\tP1\t\t90.0\t90.0\t10.0",
        "07\t\tEP\t1\t12\tP2\t\t80.0\t90.0\t10.0",
        "07\t\tEP\t1\t13\tP3\t\t70.0\t90.0\t10.0",
        "07\t\tEQ\t1\t98\t11\t\t60.0\t90.0\t10.0",
        "07\t\tEQ\t1\t99\t12\t\t50.0\t90.0\t10.0",
        "42\t\t\t\t12\t\t\t0.5",
        "20\t\t\t\t11\t\t",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    EXPECT_TRUE(result.warnings.empty()) << result.warnings.front().message;
    const auto ep = featuresOf(result.project, "EP");
    ASSERT_EQ(ep.size(), 1u);
    EXPECT_TRUE(ep[0]->closed);
    EXPECT_EQ(ep[0]->pointIds, (std::vector<std::string>{"P1", "P2", "P3"}));
    const auto eq = featuresOf(result.project, "EQ");
    ASSERT_EQ(eq.size(), 1u);
    EXPECT_FALSE(eq[0]->closed);
    EXPECT_EQ(valueOf(metadataOf(result.project, "P2"), "offset record 10"),
              "radial 0.5 m from setup K, applied to the shot of record 6");
    EXPECT_EQ(valueOf(metadataOf(result.project, "12"), "offset record 10"), "(absent)");
    const auto positions = reducedPositions(result.project);
    ASSERT_TRUE(positions.contains("P2") && positions.contains("12"));
    EXPECT_NEAR(positions.at("P2").easting, 1000.0 + 10.5 * std::sin(degrees(80.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("P2").northing, 5000.0 + 10.5 * std::cos(degrees(80.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("12").easting, 1000.0 + 10.0 * std::sin(degrees(50.0)),
                kPositionTolerance);
    EXPECT_NEAR(positions.at("12").northing, 5000.0 + 10.0 * std::cos(degrees(50.0)),
                kPositionTolerance);
}

TEST(OpcodeFieldFile, AnOffsetOrACloseByAPointIdAShotAndACoordinateShareActsOnTheShot)
{
    // Coordinate CP5 has point ID 5, and so has shot 5, which has no name.
    // [FLD] calls a point ID "normally unique", the ID a measurement is
    // given; an offset moves a shot, and a 20 closes "the string containing
    // that point ID". Both take the shot, saying so. Before, the coordinate
    // was taken, which no offset moves and no string holds: the offset was
    // kept unapplied and the 20 skipped. Coordinate CP9, in string KB 1, and
    // shot 9, in none, have point ID 9: only the coordinate's string is open,
    // so the 20 naming 9 closes it.
    //
    // A (1000, 5000) oriented on B, due north at circle 0: shot 5 at circle
    // 0, level, 50 m, moved 0.5 m further out is at (1000, 5050.5).
    const ReadResult result = readLines({
        "02\t\t\t\t5\tCP5\t\t1000.0\t5050.0\t20.0",
        "02\t\t\t\tA\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tA\t\t\t1.5",
        "04\t\t\t\tB\t\t\t0.0\t90.0\t100.0",
        "07\t\tEB\t01\t5\t\t\t0.0\t90.0\t50.0",
        "07\t\tEB\t01\t6\t\t\t90.0\t90.0\t10.0",
        "42\t\t\t\t5\t\t\t0.5",
        "20\t\t\t\t5\t\t",
        "02\t\tKB\t1\t8\tCP8\t\t1020.0\t5000.0\t20.0",
        "02\t\tKB\t1\t9\tCP9\t\t1030.0\t5000.0\t20.0",
        "07\t\t\t\t9\t\t\t180.0\t90.0\t10.0",
        "20\t\t\t\t9\t\t",
    });
    EXPECT_EQ(result.recordsSkipped, 0u);
    EXPECT_TRUE(warned(result, 8,
                       "radial offset (opcode 42) names point ID 5, which a shot gave point '5' "
                       "and a coordinate point 'CP5': it is taken for point '5', the shot's"));
    EXPECT_TRUE(warned(result, 9,
                       "close string (opcode 20) names point ID 5, which a shot gave point '5' "
                       "and a coordinate point 'CP5': it is taken for point '5', the shot's"));
    EXPECT_TRUE(warned(result, 9, "the string of point '5' is closed with 2 point(s)"));
    EXPECT_TRUE(warned(result, 13,
                       "close string (opcode 20) names point ID 9, which a shot gave point '9' "
                       "and a coordinate point 'CP9': it is taken for point 'CP9', the one on "
                       "an open string"));
    const auto kb = featuresOf(result.project, "KB");
    ASSERT_EQ(kb.size(), 1u);
    EXPECT_TRUE(kb[0]->closed);
    EXPECT_EQ(kb[0]->pointIds, (std::vector<std::string>{"CP8", "CP9"}));
    const auto eb = featuresOf(result.project, "EB");
    ASSERT_EQ(eb.size(), 1u);
    EXPECT_TRUE(eb[0]->closed);
    EXPECT_EQ(valueOf(metadataOf(result.project, "5"), "offset record 8"),
              "radial 0.5 m from setup A, applied to the shot of record 6");
    EXPECT_EQ(valueOf(metadataOf(result.project, "CP5"), "offset record 8"), "(absent)");
    const auto positions = reducedPositions(result.project);
    ASSERT_TRUE(positions.contains("5"));
    EXPECT_NEAR(positions.at("5").easting, 1000.0, kPositionTolerance);
    EXPECT_NEAR(positions.at("5").northing, 5050.5, kPositionTolerance);
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

TEST(OpcodeFieldFile, RecordsAfterTheFileEndDoNotDecideTheLayout)
{
    // Four records with the column after the opcode, the end of the file
    // (99), then five written without it. What follows the 99 is not read,
    // so it does not vote on the layout either: its five votes against four
    // made the file one without the column, and every record before the 99
    // was refused - the file "holds no station, measurement or coordinate".
    const ReadResult result = readLines({
        "02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "02\t\t\t\tB\t\t\t1000.0\t5100.0\t20.0",
        "03\t\t\t\tK\t\t\t1.5",
        "07\t\tEB\t01\t101\t\t\t90.0\t90.0\t10.0",
        "99",
        "07\tEB\t01\t201\t\t\t90.0\t90.0\t10.0",
        "07\tEB\t01\t202\t\t\t91.0\t90.0\t10.0",
        "07\tEB\t01\t203\t\t\t92.0\t90.0\t10.0",
        "07\tEB\t01\t204\t\t\t93.0\t90.0\t10.0",
        "07\tEB\t01\t205\t\t\t94.0\t90.0\t10.0",
    });
    EXPECT_EQ(result.recordsRead, 5u);
    EXPECT_EQ(result.recordsSkipped, 5u);
    EXPECT_TRUE(warned(result, 5, "5 record(s) after the end of the file (opcode 99) are not read"));
    ASSERT_EQ(result.warnings.size(), 1u);
    EXPECT_EQ(result.project.points.size(), 2u);
    ASSERT_EQ(result.project.stations.size(), 1u);
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(result.project.stations[0], "101").size(),
              1u);
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
        "50\t\t\t\tS\t\t\t0.5",
        "51\t\tkerbs\tfor",
        "48",
        "47",
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
    // 50 is a bearing datum difference for what follows; 51 starts a field
    // template, from which what follows may take its codes.
    EXPECT_TRUE(warned(result, 7,
                       "opcode 50 (backsight bearing) is not one this reader imports; the "
                       "measurements after it are read without it"));
    EXPECT_TRUE(warned(result, 8,
                       "opcode 51 (template start) is not one this reader imports; the "
                       "measurements after it keep the codes they are written with"));
    // 48 ends the current string: the points after it would start another.
    EXPECT_TRUE(warned(result, 9,
                       "opcode 48 (end string) is not one this reader imports; the points after "
                       "it are strung as if the string had not ended"));
    // 47 ends it before the current point, which "becomes the first point of
    // a new string": that point stays where it is, too.
    EXPECT_TRUE(warned(result, 10,
                       "opcode 47 (new string) is not one this reader imports; the current point "
                       "is not moved into a new string of its own, and the points after it are "
                       "strung as if the string had not ended"));
    EXPECT_EQ(result.recordsSkipped, 8u);
}

TEST(OpcodeFieldFile, LinesThatAreNoRecordSayWhyAndIndentedRecordsAndCommentsAreRead)
{
    const ReadResult result = readLines({
        "  {Version 6.0}",
        "   // Coordinate System: Local grid D",
        "\t// Job name : indented by a tab",
        "03\t\t\t\tS\t\t\t1.5",
        "\t07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0", // a tab before the opcode, as the probe reads it
        "0007\t\tEP\t1\t2\t\t\t20.0\t90.0\t5.0",
        "+7\t\tEP\t1\t3\t\t\t30.0\t90.0\t5.0",
        "{Version 7.0}", // not the first record
        "\x1A",          // a DOS end-of-file byte
    });
    EXPECT_EQ(result.project.metadata.at("version"), "{Version 6.0}");
    EXPECT_EQ(result.project.coordinateSystem.name, "Local grid D");
    EXPECT_EQ(result.project.metadata.at("header: Job name"), "indented by a tab");
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(result.project.stations.at(0), "1").size(),
              1u);
    EXPECT_TRUE(warned(result, 6,
                       "'0007' is not an opcode: an opcode is a number of one to three digits"));
    EXPECT_TRUE(warned(result, 7, "'+7' is not an opcode: an opcode is a number"));
    EXPECT_TRUE(warned(result, 8, "'{Version 7.0}' is not an opcode: a record opens with a number"));
    EXPECT_EQ(result.recordsRead, 3u);
    EXPECT_EQ(result.recordsSkipped, 3u);

    // A file indented throughout, which the probe identifies, is read.
    const ReadResult indented = readLines({
        "\t{Version 6.0}",
        "\t02\t\t\t\tK\t\t\t1000.0\t5000.0\t20.0",
        "\t03\t\t\t\tK\t\t\t1.5",
        "\t07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
    });
    EXPECT_EQ(indented.recordsSkipped, 0u);
    EXPECT_EQ(indented.recordsRead, 4u);
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

TEST(OpcodeFieldFile, AnAttributeWithExtraValuesOrWithNothingIsWarnedOf)
{
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t5.0",
        "72\t\tDepth\t0.6\t0.7",
        "71\t\tCount\t3\t4",
        "73\t\t",
        "73",
    });
    const auto p1 = metadataOf(result.project, "1");
    EXPECT_EQ(valueOf(p1, "Depth"), "0.6");
    EXPECT_EQ(valueOf(p1, "Count"), "3");
    EXPECT_TRUE(warned(result, 3, "attribute 'Depth' has values after its value; they are not "
                                  "kept"));
    EXPECT_TRUE(warned(result, 4, "attribute 'Count' has values after its value"));
    EXPECT_TRUE(warned(result, 5, "attribute record (opcode 73) with neither a name nor a value"));
    EXPECT_TRUE(warned(result, 6, "attribute record (opcode 73) with neither a name nor a value"));
    EXPECT_EQ(result.recordsSkipped, 2u);
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

TEST(OpcodeFieldFile, ASetupsSettingsStateTheScaleFactorEveryOneOfItsDistancesWasMultipliedBy)
{
    // S: a 09 after the 03, before any distance: the setup's every distance
    // (100 m x 0.5) is scaled by it. T: the factor changes after its first
    // shot, so its settings state none, and say why. U: a factor set between
    // setups is U's, and says nothing of T.
    const ReadResult result = readLines({
        "03\t\t\t\tS\t\t\t1.5",
        "09\t\t0.5",
        "07\t\tEP\t1\t1\t\t\t10.0\t90.0\t100.000",
        "03\t\t\t\tT\t\t\t1.5",
        "07\t\tEP\t1\t2\t\t\t20.0\t90.0\t100.000",
        "09\t\t1.0",
        "07\t\tEP\t1\t3\t\t\t30.0\t90.0\t100.000",
        "09\t\t2.0",
        "03\t\t\t\tU\t\t\t1.5",
        "07\t\tEP\t1\t4\t\t\t40.0\t90.0\t100.000",
    });
    ASSERT_EQ(result.project.stations.size(), 3u);
    const survey::SurveyStation& s = result.project.stations[0];
    ASSERT_TRUE(s.instrument.scaleFactor.has_value());
    EXPECT_DOUBLE_EQ(*s.instrument.scaleFactor, 0.5);
    EXPECT_EQ(s.instrument.scaleFactorState, survey::CorrectionState::Applied);
    EXPECT_DOUBLE_EQ(observationsTo<survey::DistanceObservation>(s, "1").at(0).distance, 50.0);
    const survey::SurveyStation& t = result.project.stations[1];
    EXPECT_FALSE(t.instrument.scaleFactor.has_value());
    EXPECT_EQ(t.instrument.scaleFactorState, survey::CorrectionState::Unknown);
    EXPECT_DOUBLE_EQ(observationsTo<survey::DistanceObservation>(t, "2").at(0).distance, 50.0);
    EXPECT_DOUBLE_EQ(observationsTo<survey::DistanceObservation>(t, "3").at(0).distance, 100.0);
    EXPECT_TRUE(warned(result, 7, "the scale factor changed within setup T"));
    const survey::SurveyStation& u = result.project.stations[2];
    ASSERT_TRUE(u.instrument.scaleFactor.has_value());
    EXPECT_DOUBLE_EQ(*u.instrument.scaleFactor, 2.0);
    EXPECT_EQ(u.instrument.scaleFactorState, survey::CorrectionState::Applied);
    EXPECT_FALSE(warned(result, 8, "changed within"));
    EXPECT_FALSE(warned(result, 9, "changed within"));
    EXPECT_EQ(result.recordsSkipped, 0u);
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
}

TEST(OpcodeFieldFile, AGroupRecordThatDoesNotFitIsWarnedOfOrSkipped)
{
    const ReadResult result = readLines({
        "02\t\t\t\tP\t\t\t1000.0\t5000.0\t10.0",
        "124\t\ta\tb\tc\td",     // four values: which is the name?
        "124\t\tx\tNamed\t0",    // text before the name
        "73\t\tA\t1",
        "125\t\t\t\t5",          // not the open group's level
        "124\t\t\t\t0",          // no name
        "73\t\tB\t2",
        "125\t\t\tOther\t0",     // names another group
    });
    EXPECT_TRUE(warned(result, 2, "attribute group (opcode 124) with 4 value(s); which is its name "
                                  "cannot be told"));
    EXPECT_TRUE(warned(result, 3, "attribute group 'Named' has 'x' before its name"));
    EXPECT_TRUE(warned(result, 5,
                       "the end of attribute group 'Named' gives the level '5', which is not the "
                       "group's"));
    EXPECT_TRUE(warned(result, 6, "attribute group with no name: its attributes are kept without "
                                  "one"));
    EXPECT_TRUE(warned(result, 8, "the end of attribute group '' names 'Other'; the open group is "
                                  "ended"));
    const auto p = metadataOf(result.project, "P");
    EXPECT_EQ(valueOf(p, "Named/A"), "1");
    EXPECT_EQ(valueOf(p, "B"), "2");
    EXPECT_EQ(result.recordsSkipped, 1u);
}

TEST(OpcodeFieldFile, AGroupWrittenWithoutItsLevelKeepsItsName)
{
    // The files' layout less the level: an empty value and the name, where
    // two values had been read as a name (the empty one) and a level.
    const ReadResult result = readLines({
        " 2\t\t\t\tP\t\t\t1000.0\t5000.0\t10.0",
        "124\t\t\tGPS Information",
        "73\t\tSolution\tfixed",
        "125\t\t\t\t0",
        "124\t\t\tReference",
        "73\t\tSolution\tnetwork",
        "125\t\t\t\t0",
    });
    const auto p = metadataOf(result.project, "P");
    EXPECT_EQ(valueOf(p, "GPS Information/Solution"), "fixed");
    EXPECT_EQ(valueOf(p, "Reference/Solution"), "network");
    EXPECT_TRUE(result.warnings.empty()) << result.warnings.front().message;
}

TEST(OpcodeFieldFile, GroupsNestedPastTheBoundAreReadButNotNamedSoNamesGrowNoFasterThanTheFile)
{
    // One point and 2000 groups, each opened inside the last at the next
    // level, an attribute in each: named, the last attribute's name held 2000
    // group names, and the names - and memory - grew with the square of the
    // file. The first 16 groups are named, the rest read and warned of; every
    // attribute is kept.
    std::string bytes = " 2\t\t\t\tP\t\t\t1000.0\t5000.0\t10.0\n";
    for (int g = 1; g <= 2000; ++g) {
        bytes += "124\t\t\tG" + std::to_string(g) + "\t" + std::to_string(g - 1) + "\n73\t\tA" +
                 std::to_string(g) + "\tv\n";
    }
    auto read = topcon_test::read(kId, bytes, "deep.fld");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const auto p = metadataOf(read->project, "P");
    std::string sixteen;
    for (int g = 1; g <= 16; ++g) {
        sixteen += "G" + std::to_string(g) + "/";
    }
    EXPECT_EQ(valueOf(p, sixteen + "A16"), "v");
    EXPECT_EQ(valueOf(p, sixteen + "A17"), "v");
    EXPECT_EQ(valueOf(p, sixteen + "A2000"), "v");
    std::size_t longest = 0;
    std::size_t kept = 0;
    for (const auto& [key, value] : p) {
        longest = std::max(longest, key.size());
        kept += key.find("/A") != std::string::npos ? 1 : 0;
    }
    EXPECT_EQ(kept, 2000u);
    EXPECT_EQ(longest, (sixteen + "A2000").size());
    // Group 17 opens at record 34: the point, then two records a group.
    EXPECT_TRUE(warned(*read, 34, "attribute group 'G17' opens inside 16 named group(s)"));

    // Long names meet the length bound first: five of 100 characters and
    // their separators fit in 512, the sixth does not.
    std::string wide = " 2\t\t\t\tQ\t\t\t1000.0\t5000.0\t10.0\n";
    for (int g = 1; g <= 6; ++g) {
        wide += "124\t\t\t" + std::string(99, 'W') + std::to_string(g) + "\t" +
                std::to_string(g - 1) + "\n";
    }
    wide += "73\t\tLast\tv\n";
    auto broad = topcon_test::read(kId, wide, "wide.fld");
    ASSERT_TRUE(broad.ok()) << broad.error().describe();
    std::string five;
    for (int g = 1; g <= 5; ++g) {
        five += std::string(99, 'W') + std::to_string(g) + "/";
    }
    EXPECT_EQ(valueOf(metadataOf(broad->project, "Q"), five + "Last"), "v");
    EXPECT_TRUE(warned(*broad, 7, "past the 16 levels or 512 characters"));
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
    for (const char* name :
         {"setup.fld", "gnss.fld", "resection.fld", "rtk_setup.fld", "two_backsights.fld"}) {
        reads += topcon_test::readEveryTruncationAndNoise(
            kId, fixture(std::filesystem::path("fld") / name), name);
    }
    EXPECT_GT(reads, 4000u);
}
