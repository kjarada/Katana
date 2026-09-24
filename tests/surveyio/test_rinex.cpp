// The RINEX observation reader (src/katana_surveyio/rinex.cpp).
//
// The fixtures in data/rinex/ are synthetic and small, built by hand from the
// specifications' tables (RINEX 2.11 tables A1/A2, 3.05 tables A2/A3, 4.00
// section 5). Every expected value below is worked out from how the fixture
// was designed - counted epochs, satellites, lines - with the working in a
// comment; none is copied from the reader's output.

#include <gtest/gtest.h>

#include <algorithm>
#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <random>
#include <sstream>
#include <string>
#include <vector>

#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/reader.hpp"
#include "katana/surveyio/rinex.hpp"

using namespace katana::surveyio;
namespace survey = katana::survey;
using katana::core::ErrorCode;
using katana::core::Result;

namespace {

// The fixtures sit beside this file; __FILE__ is the absolute path the build
// compiled it from, so the suite finds them from any working directory.
std::filesystem::path fixtureFolder()
{
    return std::filesystem::path(__FILE__).parent_path() / "data" / "rinex";
}

std::string fixture(const std::string& name)
{
    std::ifstream stream(fixtureFolder() / name, std::ios::binary);
    EXPECT_TRUE(stream.good()) << "missing fixture " << name;
    std::ostringstream text;
    text << stream.rdbuf();
    return text.str();
}

std::size_t lineCount(const std::string& text)
{
    return static_cast<std::size_t>(std::count(text.begin(), text.end(), '\n'));
}

Result<ReadResult> readRinex(std::string_view bytes, std::string_view name,
                             std::vector<SiblingFile> siblings = {})
{
    ReadOptions options;
    options.siblings = siblingsInMemory(std::move(siblings));
    return readSurvey(formatRegistry(), kRinexObservationFormatId, bytes, name, options);
}

Result<ReadResult> readFixture(const std::string& name, std::vector<SiblingFile> siblings = {})
{
    const std::string bytes = fixture(name);
    return readRinex(bytes, name, std::move(siblings));
}

bool anyWarningContains(const ReadResult& result, std::string_view text)
{
    return std::any_of(result.warnings.begin(), result.warnings.end(), [&](const ReadWarning& w) {
        return w.message.find(text) != std::string::npos;
    });
}

bool anyNotCarriedContains(const ReadResult& result, std::string_view text)
{
    return std::any_of(result.notCarried.begin(), result.notCarried.end(),
                       [&](const std::string& n) { return n.find(text) != std::string::npos; });
}

std::string describeWarnings(const ReadResult& result)
{
    std::string text;
    for (const ReadWarning& warning : result.warnings) {
        text += describe(warning) + "\n";
    }
    return text;
}

const survey::GnssGlobalPositionObservation& positionAt(const survey::SurveyProject& project,
                                                        std::size_t index)
{
    return std::get<survey::GnssGlobalPositionObservation>(project.observations.at(index));
}

double probeConfidence(std::string_view bytes, std::string_view name)
{
    const ProbeInput input = probeOf(bytes.substr(0, std::min(bytes.size(), kProbeBytes)), name,
                                     bytes.size() > kProbeBytes);
    for (const FormatRegistry::ProbeResult& probed : formatRegistry().probeAll(input)) {
        if (probed.formatId == kRinexObservationFormatId) {
            return probed.signature.confidence;
        }
    }
    return -1.0;
}

// A header record: content in columns 1-60, label in 61-80.
std::string header(std::string content, const std::string& label)
{
    content.resize(60, ' ');
    return content + label + "\n";
}

// A minimal RINEX 3.04 GPS header with four observation types, for the
// hand-built damaged files below. 11 lines.
std::string
minimalV3Header(const std::string& approx = " -4646000.0000  2553000.0000 -3534000.0000")
{
    return header("     3.04           OBSERVATION DATA    G", "RINEX VERSION / TYPE") +
           header("katana test", "PGM / RUN BY / DATE") + header("M1", "MARKER NAME") +
           header("", "OBSERVER / AGENCY") + header("", "REC # / TYPE / VERS") +
           header("", "ANT # / TYPE") + header(approx, "APPROX POSITION XYZ") +
           header("        1.0000        0.0000        0.0000", "ANTENNA: DELTA H/E/N") +
           header("G    4 C1C L1C D1C S1C", "SYS / # / OBS TYPES") +
           header("  2024     9    12    10     0    0.0000000     GPS", "TIME OF FIRST OBS") +
           header("", "END OF HEADER");
}

std::string epochV3(int minute, double second, int flag, int count)
{
    char buffer[64];
    std::snprintf(buffer, sizeof buffer, "> 2024 09 12 10 %02d%11.7f  %d%3d\n", minute, second,
                  flag, count);
    return buffer;
}

std::string satelliteV3(const std::string& satellite)
{
    return satellite + "  21000000.000   110000001.50008     -2000.250          44.500\n";
}

} // namespace

// ---- Registration and detection ----------------------------------------------------

TEST(RinexFormat, IsRegisteredWithAReaderAndSaysItReadsGnssSessionsOnly)
{
    const Result<FormatDescriptor> descriptor = formatRegistry().find(kRinexObservationFormatId);
    ASSERT_TRUE(descriptor.ok());
    EXPECT_EQ(descriptor->humanName, "RINEX observation");
    EXPECT_EQ(descriptor->manufacturer, Manufacturer::OpenStandard);
    EXPECT_EQ(descriptor->parserVersion, "1.0");
    EXPECT_TRUE(descriptor->canImport);
    EXPECT_FALSE(descriptor->canExport);
    // It reads GNSS sessions and an approximate position - no surveyed
    // points, no total-station observations or setups, no coded strings.
    EXPECT_EQ(descriptor->reads, (FormatContent{.gnss = true}));
    EXPECT_NE(formatRegistry().reader(kRinexObservationFormatId), nullptr);
    EXPECT_NE(std::find(descriptor->extensions.begin(), descriptor->extensions.end(), "rnx"),
              descriptor->extensions.end());
}

TEST(RinexFormat, EveryObservationFixtureIsIdentifiedAsRinexObservation)
{
    for (const std::string name :
         {"test2560.24o", "TEST00AUS_R_20242561000_01H_30S_MO.rnx",
          "ROVR00AUS_R_20242561100_01H_01S_GO.rnx", "BASE00AUS_R_20242561200_01H_30S_MO.rnx"}) {
        const std::string bytes = fixture(name);
        const Detection detection = detectFormat(probeOf(bytes, name));
        ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified)
            << name << ": " << detection.summary();
        EXPECT_EQ(detection.format()->id, kRinexObservationFormatId) << name;
        EXPECT_NE(detection.candidates().front().evidence.find("RINEX VERSION / TYPE"),
                  std::string::npos)
            << "the evidence says what was seen: " << detection.candidates().front().evidence;
    }
}

TEST(RinexFormat, TheProbeRulesOutEverySurveyFixtureThatIsNotRinex)
{
    // Every other format's fixtures in tests/surveyio/data - whatever parsers
    // have added theirs - plus hand-written openings of the other instrument
    // formats. None of them may look like RINEX.
    std::size_t looked = 0;
    const std::filesystem::path data = fixtureFolder().parent_path();
    for (const auto& entry : std::filesystem::recursive_directory_iterator(data)) {
        if (!entry.is_regular_file() || entry.path().parent_path().filename() == "rinex") {
            continue;
        }
        std::ifstream stream(entry.path(), std::ios::binary);
        std::ostringstream text;
        text << stream.rdbuf();
        const std::string name = entry.path().filename().string();
        EXPECT_EQ(probeConfidence(text.str(), name), 0.0) << name;
        ++looked;
    }
    EXPECT_GE(looked, 4U) << "the delimited-points fixtures at least";

    const std::vector<std::pair<std::string, std::string>> others{
        {"job.gsi", "*110001+0000000000000A01 21.324+0000000000000000 22.324+0000000009000000\n"},
        {"job.gsi", "110001+000000A1 21.324+00000000 22.324+09000000 31..00+00000000\n"},
        {"job.jxl", "<?xml version=\"1.0\"?>\n<JOBFile jobName=\"x\" version=\"5.6\">\n"},
        {"job.rw5", "JB,NMJOB,DT09-12-2024,TM10:00:00\nMO,AD0,UN2,SF1.00000000,EC1,EO0.0\n"},
        {"job.gts", "JOB     test\nSTN     1,1.500,\nBKB     2,0.0000,0.0000\n"},
        {"job.raw", "10NMJOB    121111\n13OOUNITS  \n"},
        {"points.csv", "P1,1000.000,2000.000,50.000,IP\nP2,1010.000,2000.000,50.100,IP\n"},
        {"job.xml", "<?xml version=\"1.0\"?>\n<LandXML "
                    "xmlns=\"http://www.landxml.org/schema/LandXML-1.2\">\n"},
        {"empty.rnx", ""},
        {"photo.gz", std::string("\x1f\x8b\x08\x00\x00\x00\x00\x00", 8)},
    };
    for (const auto& [name, bytes] : others) {
        EXPECT_EQ(probeConfidence(bytes, name), 0.0) << name;
    }
}

TEST(RinexFormat, PackedFilesAreRecognisedAndRefusedWithTheStepThatUnpacksThem)
{
    // Hatanaka: its own header record first; the probe knows it for RINEX, the
    // reader says to run CRX2RNX.
    const std::string hatanaka = fixture("test2560.24d");
    EXPECT_DOUBLE_EQ(probeConfidence(hatanaka, "test2560.24d"), 0.95);
    const Result<ReadResult> crx = readRinex(hatanaka, "test2560.24d");
    ASSERT_FALSE(crx.ok());
    EXPECT_EQ(crx.error().code, ErrorCode::Unsupported);
    EXPECT_NE(crx.error().message.find("Hatanaka"), std::string::npos);
    EXPECT_NE(crx.error().message.find("CRX2RNX"), std::string::npos);

    // gzip and Unix compress: only the name can say RINEX is inside.
    const std::string gzip("\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x03", 10);
    EXPECT_DOUBLE_EQ(probeConfidence(gzip, "TEST00AUS_R_20242561000_01H_30S_MO.rnx.gz"), 0.8);
    const Result<ReadResult> gz = readRinex(gzip, "TEST00AUS_R_20242561000_01H_30S_MO.rnx.gz");
    ASSERT_FALSE(gz.ok());
    EXPECT_EQ(gz.error().code, ErrorCode::Unsupported);
    EXPECT_NE(gz.error().message.find("gzip"), std::string::npos);
    EXPECT_NE(gz.error().message.find("Decompress"), std::string::npos);

    const std::string compress("\x1f\x9d\x90\x00", 4);
    EXPECT_DOUBLE_EQ(probeConfidence(compress, "test2560.24d.Z"), 0.8);
    const Result<ReadResult> z = readRinex(compress, "test2560.24d.Z");
    ASSERT_FALSE(z.ok());
    EXPECT_NE(z.error().message.find("Unix-compressed"), std::string::npos);
}

TEST(RinexFormat, ANavigationFileIsRecognisedAsRinexButRefusedAsAnOccupation)
{
    const std::string navigation = fixture("test2560.24n");
    EXPECT_DOUBLE_EQ(probeConfidence(navigation, "test2560.24n"), 0.75);
    const Result<ReadResult> read = readRinex(navigation, "test2560.24n");
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::FileImportFailure);
    EXPECT_NE(read.error().message.find("navigation file"), std::string::npos);
    EXPECT_NE(read.error().message.find("Import the observation file"), std::string::npos);
}

// ---- RINEX 2.11 ----------------------------------------------------------------------

TEST(Rinex2, TheSessionCarriesTheHeadersMarkerReceiverAndAntenna)
{
    const Result<ReadResult> read = readFixture("test2560.24o");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const survey::SurveyProject& project = read->project;
    ASSERT_EQ(project.gnssSessions.size(), 1U);
    const survey::GnssSession& session = project.gnssSessions.front();
    EXPECT_EQ(session.markerName, "TEST");
    EXPECT_EQ(session.markerNumber, "12345M001");
    EXPECT_EQ(session.observer, "A SURVEYOR");
    EXPECT_EQ(session.agency, "KATANA TESTS");
    EXPECT_EQ(session.receiverSerial, "5001234");
    EXPECT_EQ(session.receiverType, "LEICA GR50");
    EXPECT_EQ(session.receiverFirmware, "4.50/7.000");
    // The IGS name keeps the blanks between model and radome.
    EXPECT_EQ(session.antenna.type, "LEIAR20         LEIM");
    EXPECT_EQ(session.antenna.serialNumber, "7654321");
    // ANTENNA: DELTA H/E/N 1.5230 0.0010 -0.0020: a vertical height to the ARP.
    EXPECT_DOUBLE_EQ(session.antenna.height, 1.523);
    EXPECT_DOUBLE_EQ(session.antenna.eastOffset, 0.001);
    EXPECT_DOUBLE_EQ(session.antenna.northOffset, -0.002);
    EXPECT_EQ(session.antenna.method, survey::AntennaHeightMethod::Vertical);
    EXPECT_EQ(session.formatVersion, "2.11");
    EXPECT_EQ(session.source.fileName, "test2560.24o");
    EXPECT_EQ(session.source.recordNumber, 4U) << "MARKER NAME is line 4";
    EXPECT_EQ(project.name, "TEST");
}

TEST(Rinex2, EpochsAreCountedWithTheirTimesIntervalAndSatellitesPerSystem)
{
    const Result<ReadResult> read = readFixture("test2560.24o");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const survey::GnssSession& session = read->project.gnssSessions.front();
    // Observation epochs (flags 0 and 1) at 10:00:00, 10:00:30, 10:01:00,
    // 10:01:30 and 10:02:00; the flag 4 and flag 5 events are not epochs.
    EXPECT_EQ(session.epochCount, 5U);
    EXPECT_EQ(toString(session.firstEpoch), "2024-09-12T10:00:00 GPS");
    EXPECT_EQ(toString(session.lastEpoch), "2024-09-12T10:02:00 GPS");
    ASSERT_TRUE(session.intervalSeconds.has_value());
    EXPECT_DOUBLE_EQ(*session.intervalSeconds, 30.0);
    // GPS: G01-G09 in the first epoch, G10 in the fourth -> 10.
    // GLONASS: R01-R05 in the first, R06 in the fourth -> 6.
    // Galileo: E11 in the last -> 1.
    const std::map<std::string, std::size_t> expected{{"GPS", 10}, {"GLONASS", 6}, {"Galileo", 1}};
    EXPECT_EQ(session.satellitesPerSystem, expected);
    // 14 + 5 + 4 + 2 + 4 satellite records in the five epochs.
    EXPECT_EQ(read->project.metadata.at("satellite records"), "29");
    EXPECT_EQ(read->project.metadata.at("observation types"), "C1 L1 L2 P2 D1 S1 S2");
}

TEST(Rinex2, EveryLineIsARecordReadAndTheOnlyWarningIsThePowerFailure)
{
    const std::string bytes = fixture("test2560.24o");
    const Result<ReadResult> read = readRinex(bytes, "test2560.24o");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    // Header 17 lines; epoch 1: its line, one continuation (14 satellites >
    // 12) and 14 x 2 observation lines (7 types = 5 + 2) = 30; epoch 2: 1 +
    // 5 x 2 = 11; the flag 4 event and its comment = 2; epoch 4: 1 + 4 x 2 =
    // 9; the flag 5 event = 1; epoch 6: 1 + 2 x 2 = 5; epoch 7: 1 + 4 x 2 = 9.
    // 17 + 30 + 11 + 2 + 9 + 1 + 5 + 9 = 84.
    EXPECT_EQ(lineCount(bytes), 84U);
    EXPECT_EQ(read->recordsRead, 84U);
    EXPECT_EQ(read->recordsSkipped, 0U);
    ASSERT_EQ(read->warnings.size(), 1U) << describeWarnings(*read);
    // Epoch 2 is line 17 + 30 + 1 = 48, and carries event flag 1.
    EXPECT_EQ(read->warnings.front().record, 48U);
    EXPECT_NE(read->warnings.front().message.find("lost power"), std::string::npos);
    EXPECT_EQ(read->project.metadata.at("power failures (event flag 1)"), "1");
    EXPECT_EQ(read->project.metadata.at("external events (event flag 5)"),
              "1: 2024-09-12T10:01:15 GPS");
    EXPECT_EQ(read->project.metadata.at("comments"),
              "synthetic, from RINEX 2.11 tables A1 and A2\na comment after an event flag");
    EXPECT_EQ(read->project.metadata.at("leap seconds"), "18");
    EXPECT_EQ(read->project.metadata.at("wavelength factors"), "1     1");
}

TEST(Rinex2, TheApproximatePositionIsAnAutonomousGeocentricPositionWithAMetresSigma)
{
    const Result<ReadResult> read = readFixture("test2560.24o");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const survey::SurveyProject& project = read->project;
    // The marker is named, not positioned: no grid coordinate is invented.
    EXPECT_TRUE(project.points.empty());
    ASSERT_EQ(project.unpositionedPoints.size(), 1U);
    EXPECT_EQ(project.unpositionedPoints.front().id, "TEST");
    EXPECT_EQ(project.unpositionedPoints.front().metadata.at("position"),
              "approximate only: the RINEX header's APPROX POSITION XYZ");
    ASSERT_EQ(project.observations.size(), 1U);
    const survey::GnssGlobalPositionObservation& position = positionAt(project, 0);
    EXPECT_EQ(position.point, "TEST");
    ASSERT_TRUE(position.geocentric.has_value());
    EXPECT_FALSE(position.geodetic.has_value());
    EXPECT_DOUBLE_EQ(position.geocentric->x, -4646000.1234);
    EXPECT_DOUBLE_EQ(position.geocentric->y, 2553000.5678);
    EXPECT_DOUBLE_EQ(position.geocentric->z, -3534000.9012);
    // 10 m per axis: 100 m^2 on the diagonal, nothing off it.
    EXPECT_DOUBLE_EQ(position.covariance.xx, 100.0);
    EXPECT_DOUBLE_EQ(position.covariance.yy, 100.0);
    EXPECT_DOUBLE_EQ(position.covariance.zz, 100.0);
    EXPECT_DOUBLE_EQ(position.covariance.xy, 0.0);
    EXPECT_EQ(position.solution, survey::GnssSolution::Autonomous);
    EXPECT_TRUE(position.referenceFrame.empty()) << "the file declares none";
    EXPECT_DOUBLE_EQ(position.antenna.height, 1.523);
    EXPECT_EQ(position.source.recordNumber, 9U) << "APPROX POSITION XYZ is line 9";
    EXPECT_TRUE(anyNotCarriedContains(*read, "survey-grade position"));
    EXPECT_TRUE(anyNotCarriedContains(*read, "reference frame"));
    EXPECT_TRUE(anyNotCarriedContains(*read, "29 satellite records"));
}

TEST(Rinex2, TheNavigationFileBesideItIsReadAndSummarised)
{
    const Result<ReadResult> read =
        readFixture("test2560.24o", {SiblingFile{"test2560.24n", fixture("test2560.24n")}});
    ASSERT_TRUE(read.ok()) << read.error().describe();
    // Two GPS ephemerides of eight lines each.
    EXPECT_EQ(read->project.metadata.at("navigation files"), "test2560.24n (RINEX 2.11)");
    EXPECT_EQ(read->project.metadata.at("broadcast ephemerides GPS"), "2");
    ASSERT_EQ(read->siblingsRead.size(), 1U);
    EXPECT_EQ(read->siblingsRead.front().name, "test2560.24n");
    EXPECT_EQ(read->warnings.size(), 1U)
        << "still only the power failure: " << describeWarnings(*read);
}

TEST(Rinex2, NewObservationTypesAfterAnEventChangeHowManyLinesEachSatelliteTakes)
{
    // Two types (one line a satellite) until the flag 4 event at line 9
    // redefines them as seven (two lines a satellite: five and two).
    const std::string oneLine = "  21000000.123   110000001.50008\n";
    const std::string twoLines = "  21000000.123   110000001.50008  85000005.50007"
                                 "  21000004.123       -2000.250\n"
                                 "        44.500          40.250\n";
    const std::string bytes =
        header("     2.11           OBSERVATION DATA    G (GPS)", "RINEX VERSION / TYPE") +
        header("M2", "MARKER NAME") + header("     2    C1    L1", "# / TYPES OF OBSERV") +
        header("  2024     9    12    10     0    0.0000000     GPS", "TIME OF FIRST OBS") +
        header("", "END OF HEADER") +                          // lines 1-5
        " 24  9 12 10  0  0.0000000  0  2G01G02\n" + oneLine + // 6, 7
        oneLine +                                              // 8
        std::string(26, ' ') + "  4  1\n" +                    // 9
        header("     7    C1    L1    L2    P2    D1    S1    S2", "# / TYPES OF OBSERV") + // 10
        " 24  9 12 10  0 30.0000000  0  2G01G03\n" + twoLines + twoLines + // 11, 12-15
        " 24  9 12 10  1  0.0000000  0  1G04\n" + twoLines;                // 16, 17-18
    const Result<ReadResult> read = readRinex(bytes, "m2.24o");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const survey::GnssSession& session = read->project.gnssSessions.at(0);
    EXPECT_EQ(session.epochCount, 3U);
    EXPECT_EQ(session.satellitesPerSystem.at("GPS"), 4U) << "G01, G02, G03 and G04";
    EXPECT_EQ(read->recordsRead, 18U);
    EXPECT_EQ(read->recordsSkipped, 0U);
    ASSERT_EQ(read->warnings.size(), 1U) << describeWarnings(*read);
    EXPECT_EQ(read->warnings.front().record, 10U);
    EXPECT_NE(read->warnings.front().message.find("change here from 2 to 7"), std::string::npos);
    EXPECT_EQ(read->project.metadata.at("observation types"), "C1 L1 L2 P2 D1 S1 S2");
}

// ---- RINEX 3.04 ----------------------------------------------------------------------

TEST(Rinex3, AMixedFileCountsItsEpochsAndSatellitesPerSystemAndSkipsCycleSlips)
{
    const std::string name = "TEST00AUS_R_20242561000_01H_30S_MO.rnx";
    const std::string bytes = fixture(name);
    const Result<ReadResult> read = readRinex(bytes, name);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->project.gnssSessions.size(), 1U);
    const survey::GnssSession& session = read->project.gnssSessions.front();
    EXPECT_EQ(session.markerName, "PILLAR 7");
    EXPECT_EQ(session.receiverType, "TRIMBLE ALLOY");
    EXPECT_EQ(session.antenna.type, "TRM59800.00     NONE");
    EXPECT_DOUBLE_EQ(session.antenna.height, 0.0);
    // Four observation epochs at 10:00:00, 10:00:30, 10:01:00, 10:01:30; the
    // flag 6 cycle-slip record at 10:00:30 is not an epoch.
    EXPECT_EQ(session.epochCount, 4U);
    EXPECT_EQ(toString(session.firstEpoch), "2024-09-12T10:00:00 GPS");
    EXPECT_EQ(toString(session.lastEpoch), "2024-09-12T10:01:30 GPS");
    EXPECT_DOUBLE_EQ(session.intervalSeconds.value_or(0.0), 30.0);
    // GPS G05 G12 G15; GLONASS R01 R08; Galileo E11 E30; BeiDou C20.
    const std::map<std::string, std::size_t> expected{
        {"GPS", 3}, {"GLONASS", 2}, {"Galileo", 2}, {"BeiDou", 1}};
    EXPECT_EQ(session.satellitesPerSystem, expected);
    EXPECT_EQ(session.formatVersion, "3.04");
    // 6 + 5 + 4 + 3 satellite records; one cycle-slip record.
    EXPECT_EQ(read->project.metadata.at("satellite records"), "18");
    EXPECT_EQ(read->project.metadata.at("cycle slip records (event flag 6)"), "1");
    EXPECT_EQ(read->project.unpositionedPoints.front().metadata.at("marker type"), "GEODETIC");
    EXPECT_EQ(read->project.unpositionedPoints.front().metadata.at("marker number"), "P7");
}

TEST(Rinex3, ObservationTypesWithContinuationLinesAndHeaderRecordsReachTheMetadata)
{
    const std::string name = "TEST00AUS_R_20242561000_01H_30S_MO.rnx";
    const std::string bytes = fixture(name);
    const Result<ReadResult> read = readRinex(bytes, name);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const std::map<std::string, std::string>& metadata = read->project.metadata;
    // Sixteen GPS types: thirteen on the record, three on its continuation.
    EXPECT_EQ(metadata.at("observation types GPS"),
              "C1C L1C D1C S1C C2W L2W D2W S2W C5Q L5Q D5Q S5Q C1L L1L D1L S1L");
    EXPECT_EQ(metadata.at("observation types BeiDou"), "C2I L2I D2I S2I");
    EXPECT_EQ(metadata.at("GLONASS slots and frequency numbers"), "3 R01  1 R02 -4 R08  6");
    EXPECT_EQ(metadata.at("signal strength unit"), "DBHZ");
    EXPECT_EQ(metadata.at("RINEX version"), "3.04");
    EXPECT_EQ(metadata.at("time system"), "GPS");
    // Header 23 lines; epochs 1 + 6, 1 + 5, 1 + 1 (cycle slip), 1 + 4, 1 + 3.
    // 23 + 7 + 6 + 2 + 5 + 4 = 47.
    EXPECT_EQ(lineCount(bytes), 47U);
    EXPECT_EQ(read->recordsRead, 47U);
    EXPECT_EQ(read->recordsSkipped, 0U);
    // The one record RINEX does not define is kept and said to be.
    ASSERT_EQ(read->warnings.size(), 1U) << describeWarnings(*read);
    EXPECT_EQ(read->warnings.front().record, 22U);
    EXPECT_NE(read->warnings.front().message.find("MY OWN RECORD"), std::string::npos);
    EXPECT_EQ(metadata.at("header record MY OWN RECORD"), "something a receiver invented");
}

// ---- RINEX 3.05: stop and go ---------------------------------------------------------

TEST(Rinex3, EventFlagsSplitAStopAndGoFileIntoOneSessionPerOccupation)
{
    const std::string name = "ROVR00AUS_R_20242561100_01H_01S_GO.rnx";
    const std::string bytes = fixture(name);
    const Result<ReadResult> read = readRinex(bytes, name);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const survey::SurveyProject& project = read->project;
    // START (3 epochs), flag 2 -> moving (2 epochs), flag 3 -> CP1 (3 epochs),
    // flag 4 changes the antenna height -> CP1 again (2 epochs), flag 2 ->
    // moving (1 epoch), flag 3 -> CP2 (1 epoch).
    ASSERT_EQ(project.gnssSessions.size(), 4U);
    const std::vector<std::string> markers{"START", "CP1", "CP1", "CP2"};
    const std::vector<std::size_t> epochs{3, 3, 2, 1};
    const std::vector<double> heights{2.0, 2.0, 1.8, 1.8};
    const std::vector<std::size_t> gps{4, 5, 4, 4};
    for (std::size_t i = 0; i < 4; ++i) {
        const survey::GnssSession& session = project.gnssSessions[i];
        EXPECT_EQ(session.markerName, markers[i]) << i;
        EXPECT_EQ(session.epochCount, epochs[i]) << i;
        EXPECT_DOUBLE_EQ(session.antenna.height, heights[i]) << i;
        EXPECT_EQ(session.satellitesPerSystem.at("GPS"), gps[i]) << i;
        EXPECT_EQ(session.receiverType, "SEPT MOSAIC-X5") << "the equipment moves with the rover";
    }
    EXPECT_EQ(toString(project.gnssSessions[1].firstEpoch), "2024-09-12T11:00:06 GPS");
    EXPECT_EQ(toString(project.gnssSessions[1].lastEpoch), "2024-09-12T11:00:08 GPS");
    EXPECT_EQ(toString(project.gnssSessions[3].firstEpoch), "2024-09-12T11:00:14 GPS");
    EXPECT_EQ(project.metadata.at("kinematic epochs"), "3");

    // Three markers, each once; positions for START and both CP1 sessions,
    // none for CP2, whose occupation states no APPROX POSITION XYZ.
    ASSERT_EQ(project.unpositionedPoints.size(), 3U);
    ASSERT_EQ(project.observations.size(), 3U);
    EXPECT_EQ(positionAt(project, 0).point, "START");
    EXPECT_EQ(positionAt(project, 1).point, "CP1");
    EXPECT_DOUBLE_EQ(positionAt(project, 1).geocentric->x, -4646110.0);
    EXPECT_DOUBLE_EQ(positionAt(project, 2).antenna.height, 1.8);
    EXPECT_TRUE(anyNotCarriedContains(*read, "a position for CP2"));
    EXPECT_TRUE(anyNotCarriedContains(*read, "3 epochs recorded while the antenna was moving"));
}

TEST(Rinex3, EachOccupationChangeIsWarnedAtTheEventThatCausedIt)
{
    const std::string name = "ROVR00AUS_R_20242561100_01H_01S_GO.rnx";
    const std::string bytes = fixture(name);
    const Result<ReadResult> read = readRinex(bytes, name);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    // Header 12 lines; three static epochs of 1 + 4 lines (13-27); the flag 2
    // event 28 and its comment 29; two kinematic epochs of 1 + 3 (30-37); the
    // flag 3 event 38 and its three records (39-41); three epochs of 1 + 5
    // (42-59); the flag 4 event 60 and its record 61; two epochs of 1 + 4
    // (62-71); flag 2 at 72; one epoch of 1 + 1 (73-74); flag 3 at 75 and its
    // record 76; one epoch of 1 + 4 (77-81).
    EXPECT_EQ(lineCount(bytes), 81U);
    EXPECT_EQ(read->recordsRead, 81U);
    ASSERT_EQ(read->warnings.size(), 3U) << describeWarnings(*read);
    EXPECT_EQ(read->warnings[0].record, 60U);
    EXPECT_NE(read->warnings[0].message.find("a new session starts here"), std::string::npos);
    EXPECT_EQ(read->warnings[1].record, 75U);
    EXPECT_NE(read->warnings[1].message.find("1.8000 m"), std::string::npos);
    EXPECT_EQ(read->warnings[2].record, 28U) << "the kinematic epochs, from the first flag 2";
    EXPECT_NE(read->warnings[2].message.find("3 epochs were recorded with the antenna moving"),
              std::string::npos);
}

// ---- RINEX 4.01 ----------------------------------------------------------------------

TEST(Rinex4, AVersionFourFileIsReadWithItsNewHeaderRecordsAndLongNamedNavigationFile)
{
    const std::string name = "BASE00AUS_R_20242561200_01H_30S_MO.rnx";
    const std::string navigation = "BASE00AUS_R_20242561200_01H_MN.rnx";
    const Result<ReadResult> read =
        readFixture(name, {SiblingFile{navigation, fixture(navigation)}});
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(read->warnings.empty()) << describeWarnings(*read);
    const survey::GnssSession& session = read->project.gnssSessions.at(0);
    EXPECT_EQ(session.markerName, "BASE");
    EXPECT_EQ(session.formatVersion, "4.01");
    EXPECT_EQ(session.epochCount, 2U);
    // No INTERVAL record: the epochs, 12:00:00 and 12:00:30, give 30 s.
    EXPECT_DOUBLE_EQ(session.intervalSeconds.value_or(0.0), 30.0);
    EXPECT_DOUBLE_EQ(session.antenna.height, 0.305);
    const std::map<std::string, std::size_t> expected{{"GPS", 2}, {"Galileo", 1}};
    EXPECT_EQ(session.satellitesPerSystem, expected);
    const std::map<std::string, std::string>& metadata = read->project.metadata;
    EXPECT_EQ(metadata.at("DOI"), "https://doi.org/10.0000/katana-fixture");
    EXPECT_EQ(metadata.at("licence of use"), "CC BY 4.0");
    EXPECT_EQ(metadata.at("station information"), "https://example.org/station/BASE");
    // The long observation name ..._01H_30S_MO.rnx has its mixed navigation
    // file at ..._01H_MN.rnx: G05, G07 (LNAV) and E11 (INAV); the ION record
    // is not an ephemeris.
    EXPECT_EQ(metadata.at("navigation files"), navigation + " (RINEX 4.01)");
    EXPECT_EQ(metadata.at("broadcast ephemerides GPS"), "2");
    EXPECT_EQ(metadata.at("broadcast ephemerides Galileo"), "1");
    const survey::GnssGlobalPositionObservation& position = positionAt(read->project, 0);
    EXPECT_DOUBLE_EQ(position.geocentric->y, 2553200.25);
}

TEST(RinexNavigation, AMissingNavigationFileIsSaidAndAnUnreadableOneIsAWarning)
{
    // None beside it: the metadata names what was looked for.
    const Result<ReadResult> alone = readFixture("test2560.24o");
    ASSERT_TRUE(alone.ok());
    EXPECT_EQ(alone->project.metadata.at("navigation files"),
              "none beside the observation file (looked for test2560.24n, test2560.24g, "
              "test2560.24l, test2560.24h, test2560.24p)");

    // A file with the right name that is not a navigation file.
    const Result<ReadResult> wrong =
        readFixture("test2560.24o", {SiblingFile{"test2560.24g", "not RINEX at all\n"}});
    ASSERT_TRUE(wrong.ok());
    EXPECT_TRUE(anyWarningContains(*wrong, "the navigation file test2560.24g is not used"));

    // A lookup that fails for another reason than absence.
    ReadOptions options;
    options.siblings = [](std::string_view) -> Result<std::string> {
        return katana::core::makeError(ErrorCode::FileImportFailure, "the disk is unreadable");
    };
    const std::string bytes = fixture("test2560.24o");
    const Result<ReadResult> failing =
        readSurvey(formatRegistry(), kRinexObservationFormatId, bytes, "test2560.24o", options);
    ASSERT_TRUE(failing.ok());
    EXPECT_TRUE(anyWarningContains(*failing, "could not be read: the disk is unreadable"));

    // Any other name: stem.nav, as converters write it.
    const std::string navigation = fixture("test2560.24n");
    const Result<ReadResult> other =
        readRinex(bytes, "site.obs", {SiblingFile{"site.nav", navigation}});
    ASSERT_TRUE(other.ok());
    EXPECT_EQ(other->project.metadata.at("navigation files"), "site.nav (RINEX 2.11)");
}

// ---- Damaged and unusual files ---------------------------------------------------------

TEST(RinexDamage, AnEpochPromisingMoreSatellitesThanFollowIsWarnedAndTheNextEpochStillCounts)
{
    // Header 11 lines; epoch at line 12 promises 3 satellites, 2 follow
    // (13, 14); the next epoch at 15 has 1 (16).
    const std::string bytes = minimalV3Header() + epochV3(0, 0.0, 0, 3) + satelliteV3("G01") +
                              satelliteV3("G02") + epochV3(0, 30.0, 0, 1) + satelliteV3("G03");
    const Result<ReadResult> read = readRinex(bytes, "damaged.rnx");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->project.gnssSessions.at(0).epochCount, 2U);
    EXPECT_EQ(read->project.gnssSessions.at(0).satellitesPerSystem.at("GPS"), 3U);
    ASSERT_EQ(read->warnings.size(), 1U) << describeWarnings(*read);
    EXPECT_EQ(read->warnings.front().record, 12U);
    EXPECT_NE(read->warnings.front().message.find("lists 3 satellites but only 2"),
              std::string::npos);
}

TEST(RinexDamage, UnreadableRecordsAreWarnedByLineAndCountedAsSkipped)
{
    // Line 12 epoch; 13 a good G01; 14 a G02 with letters in it; 15 an E05
    // for which the header declares no types; 16 an epoch with month 13 and
    // one satellite (17); 18 a line that is no record at all, 19 more of the
    // same; 20 a good epoch with 21 its satellite.
    const std::string bytes =
        minimalV3Header() + epochV3(0, 0.0, 0, 3) + satelliteV3("G01") + "G02  2100000X.000\n" +
        satelliteV3("E05") + "> 2024 13 12 10 00 30.0000000  0  1\n" + satelliteV3("G04") +
        "garbage\n" + "more garbage\n" + epochV3(1, 0.0, 0, 1) + satelliteV3("G05");
    const Result<ReadResult> read = readRinex(bytes, "damaged.rnx");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const survey::GnssSession& session = read->project.gnssSessions.at(0);
    EXPECT_EQ(session.epochCount, 2U) << "10:00:00 and 10:01:00; the month-13 epoch is not one";
    EXPECT_EQ(session.satellitesPerSystem.at("GPS"), 2U) << "G01 and G05 only";
    EXPECT_FALSE(session.satellitesPerSystem.contains("Galileo"));
    std::vector<std::size_t> lines;
    for (const ReadWarning& warning : read->warnings) {
        lines.push_back(warning.record);
    }
    EXPECT_EQ(lines, (std::vector<std::size_t>{14, 15, 16, 18})) << describeWarnings(*read);
    EXPECT_TRUE(anyWarningContains(*read, "holds characters an observation field"));
    EXPECT_TRUE(anyWarningContains(*read, "declares no observation types for Galileo"));
    EXPECT_TRUE(anyWarningContains(*read, "month 13 does not exist"));
    EXPECT_TRUE(anyWarningContains(*read, "skipped it and the next line"));
    // Skipped: 14, 15, the bad epoch 16 and its record 17, 18 and 19.
    EXPECT_EQ(read->recordsSkipped, 6U);
}

TEST(RinexDamage, ARinexTwoObservationLineWithForeignCharactersIsWarnedAndItsSatelliteNotCounted)
{
    std::string bytes = fixture("test2560.24o");
    // Line 20 is G01's first observation line in the first epoch (header 17,
    // epoch line 18, continuation 19). Put a letter in it.
    std::vector<std::string> lines;
    std::istringstream stream(bytes);
    for (std::string line; std::getline(stream, line);) {
        lines.push_back(line);
    }
    ASSERT_GE(lines.size(), 20U);
    lines[19][5] = 'Z';
    std::string damaged;
    for (const std::string& line : lines) {
        damaged += line + "\n";
    }
    const Result<ReadResult> read = readRinex(damaged, "test2560.24o");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(std::any_of(read->warnings.begin(), read->warnings.end(), [](const ReadWarning& w) {
        return w.record == 20 && w.message.find("holds characters") != std::string::npos;
    })) << describeWarnings(*read);
    EXPECT_EQ(read->recordsSkipped, 1U);
    // G01 is still seen in later epochs, so the count is unchanged; the
    // satellite records drop by the one that could not be read.
    EXPECT_EQ(read->project.metadata.at("satellite records"), "28");
    EXPECT_EQ(read->project.gnssSessions.at(0).satellitesPerSystem.at("GPS"), 10U);
}

TEST(RinexDamage, AFileCutInsideAnEpochIsWarnedAndWhatWasReadIsKept)
{
    const std::string bytes = minimalV3Header() + epochV3(0, 0.0, 0, 1) + satelliteV3("G01") +
                              epochV3(0, 30.0, 0, 4) + satelliteV3("G02");
    const Result<ReadResult> read = readRinex(bytes, "cut.rnx");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->project.gnssSessions.at(0).epochCount, 2U);
    ASSERT_EQ(read->warnings.size(), 1U) << describeWarnings(*read);
    EXPECT_EQ(read->warnings.front().record, 14U);
    EXPECT_NE(read->warnings.front().message.find("3 of its 4 satellite records are missing"),
              std::string::npos);
}

TEST(RinexDamage, EpochsOutOfOrderAndAWrongHeaderIntervalAreWarned)
{
    std::string head = minimalV3Header();
    head.insert(head.find(std::string(60, ' ') + "END OF HEADER"),
                header("     5.000", "INTERVAL"));
    // Header now 12 lines: epochs at 13 (10:00:30), 15 (10:00:00, earlier
    // than the one before), 17 (10:01:00), 19 (10:01:30) and 21 (10:02:00).
    // Gaps after the backward step: 60, 30, 30 - mostly 30 s, not the 5 s the
    // header says.
    const std::string bytes = head + epochV3(0, 30.0, 0, 1) + satelliteV3("G01") +
                              epochV3(0, 0.0, 0, 1) + satelliteV3("G01") + epochV3(1, 0.0, 0, 1) +
                              satelliteV3("G01") + epochV3(1, 30.0, 0, 1) + satelliteV3("G01") +
                              epochV3(2, 0.0, 0, 1) + satelliteV3("G01");
    const Result<ReadResult> read = readRinex(bytes, "order.rnx");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const survey::GnssSession& session = read->project.gnssSessions.at(0);
    EXPECT_EQ(toString(session.firstEpoch), "2024-09-12T10:00:00 GPS");
    EXPECT_EQ(toString(session.lastEpoch), "2024-09-12T10:02:00 GPS");
    EXPECT_EQ(session.epochCount, 5U);
    EXPECT_DOUBLE_EQ(session.intervalSeconds.value_or(0.0), 5.0) << "the header's, as stated";
    EXPECT_TRUE(std::any_of(read->warnings.begin(), read->warnings.end(), [](const ReadWarning& w) {
        return w.record == 15 &&
               w.message.find("earlier than the epoch before it") != std::string::npos;
    })) << describeWarnings(*read);
    EXPECT_TRUE(anyWarningContains(*read, "interval of 5.000 s, but the epochs of M1 are mostly "
                                          "30.000 s apart"))
        << describeWarnings(*read);
    // TIME OF FIRST OBS says 10:00:00 and the earliest epoch is 10:00:00.
    EXPECT_FALSE(anyWarningContains(*read, "TIME OF FIRST OBS"));
}

TEST(RinexDamage, AZeroOrImplausibleApproximatePositionGivesNoPosition)
{
    const std::string zero = minimalV3Header("        0.0000        0.0000        0.0000") +
                             epochV3(0, 0.0, 0, 1) + satelliteV3("G01");
    const Result<ReadResult> none = readRinex(zero, "zero.rnx");
    ASSERT_TRUE(none.ok()) << none.error().describe();
    EXPECT_TRUE(none->project.observations.empty());
    EXPECT_TRUE(anyNotCarriedContains(*none, "a position for M1 (its header position is 0, 0, 0)"));
    EXPECT_FALSE(anyNotCarriedContains(*none, "survey-grade"));

    // Kilometres where metres belong: 6 371 m from the centre of the earth.
    const std::string kilometres = minimalV3Header("    -4646.0000     2553.0000    -3534.0000") +
                                   epochV3(0, 0.0, 0, 1) + satelliteV3("G01");
    const Result<ReadResult> wrong = readRinex(kilometres, "km.rnx");
    ASSERT_TRUE(wrong.ok()) << wrong.error().describe();
    EXPECT_TRUE(wrong->project.observations.empty());
    ASSERT_EQ(wrong->warnings.size(), 1U) << describeWarnings(*wrong);
    EXPECT_EQ(wrong->warnings.front().record, 7U);
    // sqrt(4646^2 + 2553^2 + 3534^2) = 6371.2 m = 6.4 km.
    EXPECT_NE(wrong->warnings.front().message.find("6.4 km from the earth's centre"),
              std::string::npos);
}

TEST(RinexDamage, AHeaderWithoutAMarkerNameNamesTheMarkerAfterTheFile)
{
    std::string head = minimalV3Header();
    const std::string marker = header("M1", "MARKER NAME");
    head.erase(head.find(marker), marker.size());
    const std::string bytes = head + epochV3(0, 0.0, 0, 1) + satelliteV3("G01");
    const Result<ReadResult> read = readRinex(bytes, "SITE00AUS_R_20242561000_01H_30S_GO.rnx");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->project.gnssSessions.at(0).markerName, "SITE00AUS");
    EXPECT_EQ(read->project.unpositionedPoints.at(0).id, "SITE00AUS");
    EXPECT_TRUE(anyWarningContains(*read, "no MARKER NAME; the marker is named 'SITE00AUS'"));
}

TEST(RinexDamage, FilesThatCannotBeReadAtAllAreErrorsASurveyorCanActOn)
{
    const auto errorOf = [](const std::string& bytes, const std::string& name) {
        const Result<ReadResult> read = readRinex(bytes, name);
        EXPECT_FALSE(read.ok()) << name;
        return read.ok() ? katana::core::Error{} : read.error();
    };
    EXPECT_NE(errorOf("", "empty.rnx").message.find("is empty"), std::string::npos);
    EXPECT_NE(errorOf("P1,1,2,3\n", "points.rnx").message.find("not a RINEX file"),
              std::string::npos);

    std::string noEnd = minimalV3Header();
    noEnd.resize(noEnd.find(std::string(60, ' ') + "END OF HEADER"));
    EXPECT_NE(errorOf(noEnd, "noend.rnx").message.find("no END OF HEADER"), std::string::npos);

    std::string noTypes = minimalV3Header();
    const std::string types = header("G    4 C1C L1C D1C S1C", "SYS / # / OBS TYPES");
    noTypes.erase(noTypes.find(types), types.size());
    EXPECT_NE(errorOf(noTypes, "notypes.rnx").message.find("declares no observation types"),
              std::string::npos);

    std::string versionOne = minimalV3Header();
    versionOne.replace(0, 9, "     1.00");
    const katana::core::Error old = errorOf(versionOne, "old.rnx");
    EXPECT_EQ(old.code, ErrorCode::Unsupported);
    EXPECT_NE(old.message.find("versions 2, 3 and 4"), std::string::npos);

    std::string meteorological = minimalV3Header();
    meteorological[20] = 'M';
    EXPECT_NE(errorOf(meteorological, "met.rnx").message.find("meteorological"), std::string::npos);
}

TEST(RinexDamage, CrLfLineEndsReadExactlyAsLfDo)
{
    // The fixture as checked out may have either; make both from it.
    const std::string name = "TEST00AUS_R_20242561000_01H_30S_MO.rnx";
    std::string lf;
    for (const char c : fixture(name)) {
        if (c != '\r') {
            lf += c;
        }
    }
    std::string crlf;
    for (const char c : lf) {
        if (c == '\n') {
            crlf += '\r';
        }
        crlf += c;
    }
    const Result<ReadResult> a = readRinex(lf, name);
    const Result<ReadResult> b = readRinex(crlf, name);
    ASSERT_TRUE(a.ok());
    ASSERT_TRUE(b.ok());
    EXPECT_EQ(a->project, b->project);
    EXPECT_EQ(a->recordsRead, b->recordsRead);
    EXPECT_EQ(a->warnings, b->warnings);
}

// ---- Robustness -----------------------------------------------------------------------

namespace {

// Reading must end in a value or an error - and an error must never be the
// reader throwing (readSurvey turns that into Internal).
void expectHandled(const std::string& bytes, const std::string& name, const std::string& what)
{
    const Result<ReadResult> read = readRinex(bytes, name);
    if (!read.ok()) {
        EXPECT_NE(read.error().code, ErrorCode::Internal)
            << what << ": " << read.error().describe();
    }
}

const std::vector<std::string>& allFixtures()
{
    static const std::vector<std::string> names{"test2560.24o",
                                                "TEST00AUS_R_20242561000_01H_30S_MO.rnx",
                                                "ROVR00AUS_R_20242561100_01H_01S_GO.rnx",
                                                "BASE00AUS_R_20242561200_01H_30S_MO.rnx",
                                                "test2560.24d",
                                                "test2560.24n",
                                                "BASE00AUS_R_20242561200_01H_MN.rnx"};
    return names;
}

} // namespace

TEST(RinexRobustness, EveryTruncationOfEveryFixtureIsReadOrRefusedWithoutACrash)
{
    for (const std::string& name : allFixtures()) {
        const std::string bytes = fixture(name);
        std::size_t whole = 0;
        const Result<ReadResult> full = readRinex(bytes, name);
        if (full.ok()) {
            for (const survey::GnssSession& session : full->project.gnssSessions) {
                whole += session.epochCount;
            }
        }
        for (std::size_t length = 0; length <= bytes.size(); ++length) {
            const std::string cut = bytes.substr(0, length);
            const Result<ReadResult> read = readRinex(cut, name);
            if (!read.ok()) {
                EXPECT_NE(read.error().code, ErrorCode::Internal)
                    << name << " cut at " << length << ": " << read.error().describe();
                continue;
            }
            // A part of a file never holds more epochs than the whole.
            std::size_t epochs = 0;
            for (const survey::GnssSession& session : read->project.gnssSessions) {
                epochs += session.epochCount;
            }
            EXPECT_LE(epochs, whole) << name << " cut at " << length;
        }
    }
}

TEST(RinexRobustness, RandomBytesAndRandomDamageAreReadOrRefusedWithoutACrash)
{
    std::mt19937 random(20240912U); // fixed: a failure must be repeatable
    std::uniform_int_distribution<int> byte(0, 255);
    for (int i = 0; i < 300; ++i) {
        std::string bytes(static_cast<std::size_t>(random() % 3000), '\0');
        for (char& c : bytes) {
            c = static_cast<char>(byte(random));
        }
        expectHandled(bytes, "random.rnx", "random bytes " + std::to_string(i));
        // The same bytes behind a real RINEX first line, so they reach the
        // header and body readers instead of stopping at the version check.
        expectHandled(minimalV3Header() + bytes, "random.rnx", "random body " + std::to_string(i));
    }
    // Damage: a few bytes of each fixture replaced with digits, blanks, signs,
    // '>' and line ends - the characters that change what a record means.
    const std::string alphabet = "0123456789 .->\n\rGREC*";
    std::uniform_int_distribution<std::size_t> pick(0, alphabet.size() - 1);
    for (const std::string& name : allFixtures()) {
        const std::string original = fixture(name);
        for (int i = 0; i < 200; ++i) {
            std::string bytes = original;
            const int changes = 1 + static_cast<int>(random() % 8);
            for (int c = 0; c < changes; ++c) {
                bytes[random() % bytes.size()] = alphabet[pick(random)];
            }
            expectHandled(bytes, name, name + " damaged " + std::to_string(i));
        }
    }
}

// ---- Throughput -------------------------------------------------------------------------

namespace {

// A synthetic RINEX 3.04 file at 1 Hz: 30 satellites (12 GPS, 8 GLONASS, 10
// Galileo), eight observation types each - 131-character satellite records,
// about 4 kB an epoch.
std::string syntheticV3(std::size_t epochs)
{
    std::string text =
        header("     3.04           OBSERVATION DATA    M", "RINEX VERSION / TYPE") +
        header("katana test", "PGM / RUN BY / DATE") + header("BIG", "MARKER NAME") +
        header(" -4646000.0000  2553000.0000 -3534000.0000", "APPROX POSITION XYZ") +
        header("        1.0000        0.0000        0.0000", "ANTENNA: DELTA H/E/N") +
        header("G    8 C1C L1C D1C S1C C2W L2W D2W S2W", "SYS / # / OBS TYPES") +
        header("R    8 C1C L1C D1C S1C C2C L2C D2C S2C", "SYS / # / OBS TYPES") +
        header("E    8 C1X L1X D1X S1X C5X L5X D5X S5X", "SYS / # / OBS TYPES") +
        header("     1.000", "INTERVAL") +
        header("  2024     9    12     0     0    0.0000000     GPS", "TIME OF FIRST OBS") +
        header("", "END OF HEADER");
    std::vector<std::string> satellites;
    for (int n = 1; n <= 12; ++n) {
        satellites.push_back((n < 10 ? "G0" : "G") + std::to_string(n));
    }
    for (int n = 1; n <= 8; ++n) {
        satellites.push_back("R0" + std::to_string(n));
    }
    for (int n = 1; n <= 10; ++n) {
        satellites.push_back((n < 10 ? "E0" : "E") + std::to_string(n));
    }
    // Eight F14.3,I1,I1 fields of 16 characters: 128.
    const std::string values = std::string("  21000000.123  ") + " 110000001.50008" +
                               "     -2000.250  " + "        44.500  " + "  21000004.123  " +
                               "  85000005.50007" + "     -1500.250  " + "        40.250  ";
    text.reserve(epochs * (57 + 30 * (3 + values.size() + 1)) + text.size());
    char line[80];
    for (std::size_t e = 0; e < epochs; ++e) {
        const std::size_t day = e / 86400;
        const std::size_t hour = (e / 3600) % 24;
        const std::size_t minute = (e / 60) % 60;
        const std::size_t second = e % 60;
        std::snprintf(line, sizeof line, "> 2024 09 %02zu %02zu %02zu%11.7f  0 30\n", 12 + day,
                      hour, minute, static_cast<double>(second));
        text += line;
        for (const std::string& satellite : satellites) {
            text += satellite;
            text += values;
            text += '\n';
        }
    }
    return text;
}

// The same in RINEX 2.11: GPS and GLONASS, 30 satellites, seven observation
// types - two lines a satellite, about 3.5 kB an epoch.
std::string syntheticV2(std::size_t epochs)
{
    std::string text =
        header("     2.11           OBSERVATION DATA    M (MIXED)", "RINEX VERSION / TYPE") +
        header("katana test", "PGM / RUN BY / DATE") + header("BIG", "MARKER NAME") +
        header(" -4646000.0000  2553000.0000 -3534000.0000", "APPROX POSITION XYZ") +
        header("        1.0000        0.0000        0.0000", "ANTENNA: DELTA H/E/N") +
        header("     7    C1    L1    L2    P2    D1    S1    S2", "# / TYPES OF OBSERV") +
        header("     1.000", "INTERVAL") +
        header("  2024     9    12     0     0    0.0000000     GPS", "TIME OF FIRST OBS") +
        header("", "END OF HEADER");
    std::string list;
    for (int n = 1; n <= 18; ++n) {
        list += (n < 10 ? "G0" : "G") + std::to_string(n);
    }
    for (int n = 1; n <= 12; ++n) {
        list += (n < 10 ? "R0" : "R") + std::to_string(n);
    }
    // Seven F14.3,I1,I1 fields: five on the first line, two on the second.
    const std::string first = std::string("  21000000.123  ") + " 110000001.50008" +
                              "  85000005.50007" + "  21000004.123  " + "     -2000.250  ";
    const std::string second = std::string("        44.500  ") + "        40.250";
    char line[80];
    for (std::size_t e = 0; e < epochs; ++e) {
        const std::size_t hour = (e / 3600) % 24;
        const std::size_t minute = (e / 60) % 60;
        const std::size_t second_ = e % 60;
        std::snprintf(line, sizeof line, " 24  9 12 %2zu %2zu%11.7f  0 30", hour, minute,
                      static_cast<double>(second_));
        text += line;
        text += list.substr(0, 36);
        text += '\n';
        for (std::size_t at = 36; at < list.size(); at += 36) {
            text += std::string(32, ' ') + list.substr(at, 36) + '\n';
        }
        for (int s = 0; s < 30; ++s) {
            text += first;
            text += '\n';
            text += second;
            text += '\n';
        }
    }
    return text;
}

double megabytesPerSecond(std::size_t bytes, std::chrono::steady_clock::duration elapsed)
{
    const double seconds = std::chrono::duration<double>(elapsed).count();
    return static_cast<double>(bytes) / 1e6 / std::max(seconds, 1e-9);
}

} // namespace

TEST(RinexThroughput, A50MegabyteOneHertzFileIsCountedNotStored)
{
    // ~4 kB an epoch: 12 500 epochs is about 50 MB. KATANA_RINEX_THROUGHPUT_MB
    // scales it for a measurement by hand (200 for a day-sized file).
    std::size_t megabytes = 50;
    if (const char* wanted = std::getenv("KATANA_RINEX_THROUGHPUT_MB")) {
        megabytes = static_cast<std::size_t>(std::max(1, std::atoi(wanted)));
    }
    const std::size_t epochs = megabytes * 250;
    const std::string bytes = syntheticV3(epochs);
    const auto start = std::chrono::steady_clock::now();
    const Result<ReadResult> read = readRinex(bytes, "BIG00AUS_R_20242560000_01D_01S_MO.rnx");
    const auto elapsed = std::chrono::steady_clock::now() - start;
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(read->warnings.empty()) << describeWarnings(*read);
    const survey::GnssSession& session = read->project.gnssSessions.at(0);
    EXPECT_EQ(session.epochCount, epochs);
    const std::map<std::string, std::size_t> expected{{"GPS", 12}, {"GLONASS", 8}, {"Galileo", 10}};
    EXPECT_EQ(session.satellitesPerSystem, expected);
    EXPECT_EQ(read->project.metadata.at("satellite records"), std::to_string(epochs * 30));
    const double rate = megabytesPerSecond(bytes.size(), elapsed);
    std::cout << "[ RINEX    ] read " << static_cast<double>(bytes.size()) / 1e6 << " MB, "
              << epochs << " epochs in " << std::chrono::duration<double>(elapsed).count()
              << " s: " << rate << " MB/s\n";
    RecordProperty("megabytes_per_second", std::to_string(rate));
}

TEST(RinexThroughput, A50MegabyteRinexTwoFileIsCountedNotStored)
{
    std::size_t megabytes = 50;
    if (const char* wanted = std::getenv("KATANA_RINEX_THROUGHPUT_MB")) {
        megabytes = static_cast<std::size_t>(std::max(1, std::atoi(wanted)));
    }
    // ~3.5 kB an epoch; a day at 1 Hz is 86 400 epochs, so the time of day wraps
    // only past 300 MB.
    const std::size_t epochs = std::min<std::size_t>(megabytes * 282, 86'400);
    const std::string bytes = syntheticV2(epochs);
    const auto start = std::chrono::steady_clock::now();
    const Result<ReadResult> read = readRinex(bytes, "big02560.24o");
    const auto elapsed = std::chrono::steady_clock::now() - start;
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(read->warnings.empty()) << describeWarnings(*read);
    const survey::GnssSession& session = read->project.gnssSessions.at(0);
    EXPECT_EQ(session.epochCount, epochs);
    const std::map<std::string, std::size_t> expected{{"GPS", 18}, {"GLONASS", 12}};
    EXPECT_EQ(session.satellitesPerSystem, expected);
    EXPECT_EQ(read->project.metadata.at("satellite records"), std::to_string(epochs * 30));
    const double rate = megabytesPerSecond(bytes.size(), elapsed);
    std::cout << "[ RINEX 2  ] read " << static_cast<double>(bytes.size()) / 1e6 << " MB, "
              << epochs << " epochs in " << std::chrono::duration<double>(elapsed).count()
              << " s: " << rate << " MB/s\n";
    RecordProperty("megabytes_per_second", std::to_string(rate));
}
