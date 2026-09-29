// The Sokkia SDR reader (src/katana_surveyio/sokkia_sdr.cpp).
//
// data/sdr/traverse.sdr is written by hand from the SDR33 record layouts of
// "Interfacing with the SOKKIA SDR Electronic Field Book" (October 1999),
// section 3.6.2: a header in degrees, metres, millibars and Celsius with
// the coordinates east first; two keyed control marks; a setup on CP1 with
// two rounds of face 1 and face 2 to its backsight CP2 and to T1; a block
// of deleted ("DD") records; a setup on T1 observing CP1 and T2. Point ids
// are right-justified in their 16 columns, as some writers pad them. Every
// value below is read off the record text by hand: the fields of an
// observation are slope distance (columns 37-52), vertical reading (53-68)
// and horizontal reading (69-84), in decimal degrees, so 87.00000000 is
// 87 * pi / 180.

#include <gtest/gtest.h>

#include <cmath>
#include <filesystem>
#include <initializer_list>
#include <limits>
#include <numbers>
#include <optional>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "surveyio/topcon_test_support.hpp"

using namespace katana::surveyio;
namespace survey = katana::survey;
using katana::core::ErrorCode;
using katana::core::Result;
using topcon_test::fixture;
using topcon_test::observationsTo;

namespace {

constexpr std::string_view kId = "sokkia-sdr";
constexpr double kPi = std::numbers::pi;
constexpr double kAngleTolerance = 1e-12;
constexpr double kLengthTolerance = 1e-9;

double degrees(double value)
{
    return value * kPi / 180.0;
}

ReadResult readTraverse()
{
    const std::string bytes = fixture("sdr/traverse.sdr");
    auto result = topcon_test::read(kId, bytes, "traverse.sdr");
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(result).value() : ReadResult{};
}

bool warned(const ReadResult& result, std::size_t record, std::string_view words)
{
    return topcon_test::anyWarningContains(result, record, words);
}

// A field of `width` characters, the text left-justified: [SDR] 3.2 pads an
// alpha field on the right, and a real is written from its first column.
std::string field(std::string_view text, std::size_t width)
{
    std::string padded(text);
    padded.resize(width, ' ');
    return padded;
}

std::string f16(std::string_view text)
{
    return field(text, 16);
}

// An SDR33 header: version, serial number, date and time, then the six
// options ([SDR] 3.6.2) - by default degrees, metres, mbar, Celsius, north
// first, angles right.
std::string header33(std::string_view options = "113111")
{
    return "00NM" + f16("SDR33 V04-03.00") + "0001" + f16("28-Sep-26 08:00") +
           std::string(options);
}

// The lines of a file, each ended as `ending` says.
std::string file(std::initializer_list<std::string> lines, std::string_view ending = "\n")
{
    std::string text;
    for (const std::string& line : lines) {
        text += line;
        text += ending;
    }
    return text;
}

// A setup on P1 (instrument height 1.500) oriented on P2 at circle 0, one
// face 1 shot to P3: the smallest file with an observation in it.
std::string instrument33(char verticalOption = '1')
{
    return "01NM:" + field("", 16) + "000000" + f16("HAND") + "000001" + "3" +
           std::string(1, verticalOption) + f16("") + f16("") + f16("0");
}

Result<ReadResult> readText(const std::string& text, std::string_view name = "job.sdr")
{
    return topcon_test::read(kId, text, name);
}

// The i-th setup, or an empty one and a failure when there is none: a read
// that went wrong fails the test rather than indexing past the end.
const survey::SurveyStation& setupAt(const survey::SurveyProject& project, std::size_t i)
{
    static const survey::SurveyStation kNone{};
    if (i >= project.stations.size()) {
        ADD_FAILURE() << "no setup " << i << ": " << project.stations.size() << " were read";
        return kNone;
    }
    return project.stations[i];
}

// The value of `stated`, or NaN and a failure when it is absent.
double valueOf(const std::optional<double>& stated)
{
    if (!stated) {
        ADD_FAILURE() << "a value the file states was not read";
        return std::numeric_limits<double>::quiet_NaN();
    }
    return *stated;
}

// The first of `found`, or a default one and a failure when it is empty.
template <typename T>
T firstOf(const std::vector<T>& found)
{
    if (found.empty()) {
        ADD_FAILURE() << "no observation of the kind asked for";
        return T{};
    }
    return found.front();
}

const survey::SurveyStation* stationNamed(const survey::SurveyProject& project,
                                          std::string_view id)
{
    for (const survey::SurveyStation& station : project.stations) {
        if (station.setup.id == id) {
            return &station;
        }
    }
    return nullptr;
}

double confidenceOf(std::string_view bytes, std::string_view name, std::string_view format)
{
    for (const auto& probed : formatRegistry().probeAll(probeOf(bytes, name))) {
        if (probed.formatId == format) {
            return probed.signature.confidence;
        }
    }
    return -1.0; // not registered
}

} // namespace

// ---- Registration and detection --------------------------------------------------

TEST(SokkiaSdr, IsRegisteredAsAnImportableSokkiaFormatReadingSetupsShotsAndPoints)
{
    const auto format = formatRegistry().find(kId);
    ASSERT_TRUE(format.ok());
    EXPECT_TRUE(format->canImport);
    EXPECT_FALSE(format->canExport);
    EXPECT_EQ(format->manufacturer, Manufacturer::Sokkia);
    EXPECT_EQ(format->extensions, std::vector<std::string>{"sdr"});
    EXPECT_TRUE(format->reads.points && format->reads.observations && format->reads.stations &&
                format->reads.instrumentSettings && format->reads.features);
    EXPECT_FALSE(format->reads.gnss);
    EXPECT_FALSE(format->reads.coordinateSystem);
    EXPECT_NE(formatRegistry().reader(kId), nullptr);
}

TEST(SokkiaSdr, ItsHeaderIdentifiesTheFileWhateverItIsNamed)
{
    const std::string bytes = fixture("sdr/traverse.sdr");
    for (const char* name : {"traverse.sdr", "traverse.txt", "traverse.dc", "traverse.raw",
                             "traverse"}) {
        const Detection detection = detectFormat(probeOf(bytes, name));
        ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified)
            << name << ": " << detection.summary();
        EXPECT_EQ(detection.format()->id, kId) << name;
        EXPECT_NE(detection.candidates().front().evidence.find("header SDR33 V04-03.00"),
                  std::string::npos)
            << detection.candidates().front().evidence;
    }
    // The Survey Controller probe steps aside for an SDR header even in a
    // file named .dc, which a controller can write SDR33 into.
    EXPECT_EQ(confidenceOf(bytes, "traverse.dc", "trimble-dc"), 0.0);
    // No other format claims it at all, whatever it is called.
    for (const char* name : {"traverse.sdr", "traverse.txt", "traverse.dc", "traverse.raw",
                             "traverse.csv", "traverse.gsi", "traverse.rw5", "traverse.fld"}) {
        for (const auto& probed : formatRegistry().probeAll(probeOf(bytes, name))) {
            if (probed.formatId != kId) {
                EXPECT_EQ(probed.signature.confidence, 0.0)
                    << name << ": " << probed.formatId << " " << probed.signature.evidence;
            }
        }
    }
}

TEST(SokkiaSdr, AnEditedHeaderNamedDcIsStillSdrAndNotSurveyController)
{
    // [SDR] 3.6.2 gives the header the derivation code ED as well as NM.
    const std::string bytes =
        file({"00ED" + f16("SDR33 V04-03.00") + "0001" + f16("28-Sep-26 08:00") + "113111",
              "10NM" + f16("EDITED") + "121111"});
    EXPECT_EQ(confidenceOf(bytes, "job.dc", "trimble-dc"), 0.0);
    const Detection detection = detectFormat(probeOf(bytes, "job.dc"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kId);
}

TEST(SokkiaSdr, ItsProbeRulesOutEveryOtherFormatsFixturesAndSamples)
{
    const std::filesystem::path data = topcon_test::dataDirectory();
    std::size_t others = 0;
    const auto check = [&](const std::filesystem::path& folder) {
        for (const auto& entry : std::filesystem::recursive_directory_iterator(folder)) {
            if (!entry.is_regular_file() || entry.path().parent_path() == data / "sdr") {
                continue;
            }
            const std::string bytes = topcon_test::fixture(entry.path());
            EXPECT_EQ(confidenceOf(std::string_view(bytes).substr(0, kProbeBytes),
                                   entry.path().filename().string(), kId),
                      0.0)
                << entry.path().string();
            ++others;
        }
    };
    check(data);
    check(data.parent_path().parent_path() / "qt_widgets" / "survey" / "data");
    EXPECT_GT(others, 30u);
    for (const topcon_test::ForeignSample& sample : topcon_test::kForeignSamples) {
        EXPECT_EQ(confidenceOf(sample.bytes, sample.name, kId), 0.0) << sample.name;
    }
    // Records of the same shape with no SDR header: a Survey Controller job
    // that has lost its header, and another program's raw file.
    EXPECT_EQ(confidenceOf("10NMJOB1            071118\r\n13TSTrimble Survey Controller\r\n"
                           "08KI1       5000.000     5000.000      100.000CP\r\n",
                           "job.dc", kId),
              0.0);
    EXPECT_EQ(confidenceOf("10NMJOB    121111\n13OOUNITS\n", "job.raw", kId), 0.0);
    // Four digits and blanks are a record's shape too: a headerless point
    // list must not be taken for one.
    EXPECT_EQ(confidenceOf("1001 5000.000 2000.000 50.000\n1002 5001.000 2001.000 51.000\n",
                           "points.txt", kId),
              0.0);
}

TEST(SokkiaSdr, AnSdrExtensionWithoutAHeaderIsOnlyASuggestion)
{
    const std::string headerless = file({"10NM" + f16("NOHEADER") + "121111",
                                         "02NM" + f16("P1") + f16("") + f16("") + f16("") +
                                             f16("1.5")});
    EXPECT_GT(confidenceOf(headerless, "job.sdr", kId), 0.0);
    EXPECT_LT(confidenceOf(headerless, "job.sdr", kId), kIdentifiedConfidence);
    EXPECT_EQ(confidenceOf(headerless, "job.txt", kId), 0.0);
    EXPECT_LE(confidenceOf("hello\n", "notes.sdr", kId), 0.1);
    const auto refused = readText(headerless);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("header"), std::string::npos);
}

// ---- The traverse fixture ----------------------------------------------------------

TEST(SokkiaSdr, TheHeaderGivesTheUnitsAndTheJobItsNameAndSwitches)
{
    const ReadResult result = readTraverse();
    const survey::SurveyProject& project = result.project;
    EXPECT_EQ(project.units.linear, survey::LinearUnit::Metres);
    EXPECT_EQ(project.units.angular, survey::AngularUnit::DecimalDegrees);
    EXPECT_EQ(project.name, "FIXTURE TRAV");
    EXPECT_EQ(project.metadata.at("header: version"), "SDR33 V04-03.00");
    EXPECT_EQ(project.metadata.at("header: serial number"), "0001");
    EXPECT_EQ(project.metadata.at("header: date and time"), "28-Sep-26 08:00");
    // Job flags 122211: elevations recorded, atmospheric correction on,
    // curvature and refraction on (0.14), sea level off.
    EXPECT_EQ(project.metadata.at("job: atmospheric correction"), "on");
    EXPECT_EQ(project.metadata.at("job: curvature and refraction correction"), "on");
    EXPECT_EQ(project.metadata.at("job: sea level correction"), "off");
    EXPECT_EQ(project.source.manufacturer, "Sokkia");
    EXPECT_EQ(project.source.format, "SDR33");
    EXPECT_EQ(project.source.formatVersion, "SDR33 V04-03.00");
    EXPECT_FALSE(project.coordinateSystem.epsgCode > 0);
}

TEST(SokkiaSdr, EveryLiveRecordIsReadAndEveryDeletedOneSkipped)
{
    // 51 lines: 4 blank, 7 deleted (33 to 39), 40 records read.
    const ReadResult result = readTraverse();
    EXPECT_EQ(result.recordsRead, 40u);
    EXPECT_EQ(result.recordsSkipped, 7u);
    EXPECT_EQ(result.warnings.size(), 7u);
    for (std::size_t record = 33; record <= 39; ++record) {
        EXPECT_TRUE(warned(result, record, "deleted record")) << record;
    }
    EXPECT_TRUE(warned(result, 35, "02NM (station)"));
    EXPECT_TRUE(warned(result, 39, "'DDDD'"));
}

TEST(SokkiaSdr, KeyedControlIsReadEastingFirstUnderOptionTwo)
{
    const ReadResult result = readTraverse();
    const survey::SurveyPoint* cp1 = topcon_test::point(result.project, "CP1");
    ASSERT_NE(cp1, nullptr);
    // Record 8: "500000.000" in 21-36, "5000000.000" in 37-52, option 2.
    EXPECT_DOUBLE_EQ(cp1->easting, 500000.0);
    EXPECT_DOUBLE_EQ(cp1->northing, 5000000.0);
    ASSERT_TRUE(cp1->elevation.has_value());
    EXPECT_DOUBLE_EQ(*cp1->elevation, 100.0);
    EXPECT_EQ(cp1->coordinateSource, survey::CoordinateSource::Entered);
    EXPECT_EQ(cp1->source.recordNumber, 8u);
    const survey::SurveyPoint* cp2 = topcon_test::point(result.project, "CP2");
    ASSERT_NE(cp2, nullptr);
    EXPECT_DOUBLE_EQ(cp2->northing, 5000100.0);
    EXPECT_DOUBLE_EQ(cp2->easting, 500000.0);
    EXPECT_EQ(result.project.points.size(), 2u);
    // The traverse marks are named, not placed.
    EXPECT_NE(topcon_test::unpositioned(result.project, "T1"), nullptr);
    EXPECT_NE(topcon_test::unpositioned(result.project, "T2"), nullptr);
    EXPECT_EQ(result.project.unpositionedPoints.size(), 2u);
}

TEST(SokkiaSdr, EachStationRecordIsOneSetupWithItsRoundsTogether)
{
    const ReadResult result = readTraverse();
    const survey::SurveyProject& project = result.project;
    ASSERT_EQ(project.stations.size(), 2u);
    const survey::SurveyStation& first = setupAt(project, 0);
    EXPECT_EQ(first.setup.id, "CP1");
    EXPECT_EQ(first.setup.pointId, "CP1");
    EXPECT_DOUBLE_EQ(first.setup.instrumentHeight, 1.5);
    EXPECT_EQ(first.source.recordNumber, 12u);
    // Two rounds of four pointings, each of a direction, a zenith and a
    // slope distance.
    EXPECT_EQ(first.observations.size(), 24u);
    EXPECT_EQ(first.metadata.at("backsight rounds"), "2");
    const survey::SurveyStation& second = setupAt(project, 1);
    EXPECT_EQ(second.setup.id, "T1");
    EXPECT_DOUBLE_EQ(second.setup.instrumentHeight, 1.55);
    EXPECT_EQ(second.observations.size(), 12u);
    EXPECT_EQ(second.metadata.at("backsight rounds"), "1");
}

TEST(SokkiaSdr, TheBacksightRecordsAzimuthOrientsTheSetupAndItsCircleReadingsAreKept)
{
    const ReadResult result = readTraverse();
    const survey::SurveyStation& first = setupAt(result.project, 0);
    EXPECT_EQ(first.backsightPointId, "CP2");
    ASSERT_TRUE(first.backsightAzimuth.has_value());
    EXPECT_NEAR(valueOf(first.backsightAzimuth), 0.0, kAngleTolerance);
    EXPECT_EQ(first.metadata.at("backsight circle readings (radians)"), "0, 0");
    const survey::SurveyStation& second = setupAt(result.project, 1);
    EXPECT_EQ(second.backsightPointId, "CP1");
    ASSERT_TRUE(second.backsightAzimuth.has_value());
    EXPECT_NEAR(valueOf(second.backsightAzimuth), degrees(270.0), kAngleTolerance);
}

TEST(SokkiaSdr, FaceOneAndFaceTwoArePointingsOfOneTargetReadAsDistanceZenithCircle)
{
    const ReadResult result = readTraverse();
    const survey::SurveyStation& first = setupAt(result.project, 0);
    // Records 18, 19, 27, 28: F1 150.000 87 90, F2 150.002 273 270, F1
    // 150.001 87 90, F2 150.003 273 270. The face 2 zenith 273 is the line
    // 360 - 273 = 87 on face 1; its circle reading stays as read.
    const auto directions = observationsTo<survey::HorizontalDirectionObservation>(first, "T1");
    const auto zeniths = observationsTo<survey::ZenithAngleObservation>(first, "T1");
    const auto distances = observationsTo<survey::DistanceObservation>(first, "T1");
    ASSERT_EQ(directions.size(), 4u);
    ASSERT_EQ(zeniths.size(), 4u);
    ASSERT_EQ(distances.size(), 4u);
    const double circle[] = {90.0, 270.0, 90.0, 270.0};
    const double slope[] = {150.000, 150.002, 150.001, 150.003};
    const survey::Face face[] = {survey::Face::Left, survey::Face::Right, survey::Face::Left,
                                 survey::Face::Right};
    const std::size_t record[] = {18, 19, 27, 28};
    for (std::size_t i = 0; i < 4; ++i) {
        SCOPED_TRACE(i);
        EXPECT_NEAR(directions[i].direction, degrees(circle[i]), kAngleTolerance);
        EXPECT_NEAR(zeniths[i].angle, degrees(87.0), kAngleTolerance);
        EXPECT_NEAR(distances[i].distance, slope[i], kLengthTolerance);
        EXPECT_EQ(distances[i].kind, survey::DistanceKind::Slope);
        EXPECT_EQ(directions[i].pointing.face, face[i]);
        // One record is one pointing: its three observations share it.
        EXPECT_EQ(directions[i].pointing, zeniths[i].pointing);
        EXPECT_EQ(directions[i].pointing, distances[i].pointing);
        EXPECT_NE(directions[i].pointing.index, 0u);
        EXPECT_EQ(directions[i].source.recordNumber, record[i]);
        EXPECT_DOUBLE_EQ(distances[i].instrumentHeight, 1.5);
    }
    EXPECT_NE(directions[0].pointing.index, directions[1].pointing.index);
    // Every observation stated a precision the model accepts.
    for (const survey::Observation& observation : first.observations) {
        EXPECT_TRUE(survey::validateObservation(observation).ok());
    }
}

TEST(SokkiaSdr, ATargetHeightHoldsUntilTheNextAndADeletedOneSetsNothing)
{
    const ReadResult result = readTraverse();
    const survey::SurveyStation& first = setupAt(result.project, 0);
    for (const auto& zenith : observationsTo<survey::ZenithAngleObservation>(first, "CP2")) {
        EXPECT_DOUBLE_EQ(zenith.targetHeight, 1.6); // records 15, 20, 24, 29
    }
    for (const auto& distance : observationsTo<survey::DistanceObservation>(first, "T1")) {
        EXPECT_DOUBLE_EQ(distance.targetHeight, 1.65); // records 17, 26
    }
    // Record 46, the first shot of the second setup, has no 03 before it
    // in its setup: the last live one (record 29, 1.600) holds, and the
    // deleted DD03NM of record 37 (9.999) does not.
    const survey::SurveyStation& second = setupAt(result.project, 1);
    const auto toCp1 = observationsTo<survey::DistanceObservation>(second, "CP1");
    ASSERT_EQ(toCp1.size(), 2u);
    EXPECT_EQ(toCp1[0].source.recordNumber, 46u);
    EXPECT_DOUBLE_EQ(toCp1[0].targetHeight, 1.6);
    EXPECT_DOUBLE_EQ(toCp1[0].instrumentHeight, 1.55);
    for (const auto& distance : observationsTo<survey::DistanceObservation>(second, "T2")) {
        EXPECT_DOUBLE_EQ(distance.targetHeight, 1.7); // record 47
    }
}

TEST(SokkiaSdr, DeletedRecordsChangeNothingThatFollows)
{
    const ReadResult result = readTraverse();
    const survey::SurveyProject& project = result.project;
    // The deleted setup on T9 is neither a setup nor a point.
    EXPECT_EQ(stationNamed(project, "T9"), nullptr);
    EXPECT_EQ(topcon_test::point(project, "T9"), nullptr);
    EXPECT_EQ(topcon_test::unpositioned(project, "T9"), nullptr);
    // Its weather (1000 mbar, 35 C) reached no setup: the second has its
    // own, record 42's.
    const survey::SurveyStation& second = setupAt(project, 1);
    ASSERT_TRUE(second.instrument.pressureHectopascals.has_value());
    EXPECT_DOUBLE_EQ(valueOf(second.instrument.pressureHectopascals), 1012.8);
    ASSERT_TRUE(second.instrument.temperatureCelsius.has_value());
    EXPECT_DOUBLE_EQ(valueOf(second.instrument.temperatureCelsius), 21.5);
}

TEST(SokkiaSdr, InstrumentWeatherAndScaleSettleOnTheSetupTheyAreWrittenBefore)
{
    const ReadResult result = readTraverse();
    const survey::SurveyStation& first = setupAt(result.project, 0);
    const survey::InstrumentSettings& instrument = first.instrument;
    EXPECT_EQ(instrument.model, "TEST THEODOLITE");
    EXPECT_EQ(instrument.serialNumber, "004711");
    // Record 7's prism constant, -30 mm ([SDR] 3.3.4), in the distances.
    ASSERT_TRUE(instrument.prismConstant.has_value());
    EXPECT_DOUBLE_EQ(valueOf(instrument.prismConstant), -0.030);
    EXPECT_EQ(instrument.prismConstantState, survey::CorrectionState::Applied);
    // Record 11: 1013.25 mbar, which is hPa, and 20 C.
    ASSERT_TRUE(instrument.pressureHectopascals.has_value());
    EXPECT_DOUBLE_EQ(valueOf(instrument.pressureHectopascals), 1013.25);
    ASSERT_TRUE(instrument.temperatureCelsius.has_value());
    EXPECT_DOUBLE_EQ(valueOf(instrument.temperatureCelsius), 20.0);
    EXPECT_EQ(instrument.atmosphericPpmState, survey::CorrectionState::Unknown);
    ASSERT_TRUE(instrument.scaleFactor.has_value());
    EXPECT_DOUBLE_EQ(valueOf(instrument.scaleFactor), 1.0);
    EXPECT_EQ(instrument.scaleFactorState, survey::CorrectionState::NotApplied);
    ASSERT_TRUE(instrument.refractionCoefficient.has_value());
    EXPECT_DOUBLE_EQ(valueOf(instrument.refractionCoefficient), 0.14);
    EXPECT_EQ(instrument.curvatureRefractionState, survey::CorrectionState::NotApplied);
    // Records 41 and 42 come after the first setup's shots and before the
    // second's 02: they are the second's, and no "changed in the middle"
    // warning is due.
    EXPECT_FALSE(warned(result, 41, "middle"));
    EXPECT_FALSE(warned(result, 42, "middle"));
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(result, "atmospheric correction"));
}

TEST(SokkiaSdr, TimeStampsDateTheirSetupAndNotesAreKept)
{
    const ReadResult result = readTraverse();
    const survey::SurveyProject& project = result.project;
    // Records 4 and 5, before any setup.
    const std::string& notes = project.metadata.at("notes");
    EXPECT_NE(notes.find("TS: Time Date 09/28/2026 Time 08:05:00"), std::string::npos) << notes;
    EXPECT_NE(notes.find("Written by hand from the published SDR33 layouts"), std::string::npos);
    // Record 31 dates the first setup, record 44 the second; the deleted
    // one (record 39) dates nothing.
    const survey::SurveyTimestamp first = setupAt(project, 0).instrument.time;
    EXPECT_EQ(first, (survey::SurveyTimestamp{2026, 9, 28, 8, 31, 10.0, ""}));
    const survey::SurveyTimestamp second = setupAt(project, 1).instrument.time;
    EXPECT_EQ(second, (survey::SurveyTimestamp{2026, 9, 28, 8, 52, 40.0, ""}));
    EXPECT_NE(setupAt(project, 1).metadata.at("notes").find("08:52:40"), std::string::npos);
}

TEST(SokkiaSdr, AKeyedAzimuthIsKeptWithItsSetup)
{
    // Record 13: 11TP CP1 to CP2, azimuth 0, no distances.
    const ReadResult result = readTraverse();
    const survey::SurveyStation& first = setupAt(result.project, 0);
    EXPECT_EQ(first.metadata.at("azimuth to CP2 (radians)"), "0");
    EXPECT_EQ(first.metadata.at("azimuth to CP2, description"), "BS AZ");
}

TEST(SokkiaSdr, CodedPointsAreStrungByTheirCode)
{
    const ReadResult result = readTraverse();
    const survey::SurveyFeature* control = topcon_test::feature(result.project, "CTRL");
    ASSERT_NE(control, nullptr);
    EXPECT_EQ(control->pointIds, (std::vector<std::string>{"CP1", "CP2"}));
    const survey::SurveyFeature* traverse = topcon_test::feature(result.project, "TRAV");
    ASSERT_NE(traverse, nullptr);
    EXPECT_EQ(traverse->pointIds, (std::vector<std::string>{"T1", "T2"}));
    EXPECT_EQ(result.project.features.size(), 2u);
}

// ---- Layouts and units ----------------------------------------------------------------

TEST(SokkiaSdr, TheSdr2xLayoutReadsFourCharacterIdsAndTenCharacterReals)
{
    // [SDR] 3.6.1: the same records with a 4-digit point number and
    // 10-character reals. 02: 5-8 point, 9-18 N, 19-28 E, 29-38 Z, 39-48
    // instrument height; 09: 5-8, 9-12, 13-22 SD, 23-32 VA, 33-42 HA.
    const std::string text = file({
        "00NM" + f16("SDR20 V03-05") + "0000" + f16("18-Jan-80 20:34") + "113111",
        "10NM" + f16("SMALL"),
        "01NM1" + field("", 16) + "000000" + f16("DT2") + "000031" + "3" + "1" + field("", 10) +
            field("", 10) + field("0.000", 10),
        "02KI0001" + field("5100.000", 10) + field("200.000", 10) + field("50.000", 10) +
            field("1.000", 10) + f16("DEAD DOG"),
        "07TP00010002" + field("14.00000", 10) + field("0.00000", 10),
        "03NM" + field("1.200", 10),
        "09F100010002" + field("42.500", 10) + field("91.50000", 10) + field("0.00000", 10) +
            f16("BS"),
        "09F200010002" + field("42.502", 10) + field("268.50000", 10) + field("180.00000", 10) +
            f16("BS"),
    });
    const auto result = readText(text);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->project.source.format, "SDR2x");
    EXPECT_EQ(result->recordsSkipped, 0u);
    const survey::SurveyPoint* station = topcon_test::point(result->project, "0001");
    ASSERT_NE(station, nullptr);
    EXPECT_DOUBLE_EQ(station->northing, 5100.0); // option 1: north first
    EXPECT_DOUBLE_EQ(station->easting, 200.0);
    EXPECT_EQ(station->coordinateSource, survey::CoordinateSource::Entered); // 02KI
    ASSERT_EQ(result->project.stations.size(), 1u);
    const survey::SurveyStation& setup = setupAt(result->project, 0);
    EXPECT_DOUBLE_EQ(setup.setup.instrumentHeight, 1.0);
    EXPECT_EQ(setup.backsightPointId, "0002");
    EXPECT_NEAR(valueOf(setup.backsightAzimuth), degrees(14.0), kAngleTolerance);
    const auto distances = observationsTo<survey::DistanceObservation>(setup, "0002");
    ASSERT_EQ(distances.size(), 2u);
    EXPECT_NEAR(distances[0].distance, 42.500, kLengthTolerance);
    EXPECT_NEAR(distances[1].distance, 42.502, kLengthTolerance);
    EXPECT_DOUBLE_EQ(distances[0].targetHeight, 1.2);
    const auto zeniths = observationsTo<survey::ZenithAngleObservation>(setup, "0002");
    ASSERT_EQ(zeniths.size(), 2u);
    EXPECT_NEAR(zeniths[1].angle, degrees(91.5), kAngleTolerance); // 360 - 268.5
}

TEST(SokkiaSdr, OptionOneReadsTheNorthingFirst)
{
    const auto result = readText(file({header33("113111"),
                                       "08KI" + f16("A") + f16("5000000.000") +
                                           f16("500000.000") + f16("10.000") + f16("")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyPoint* a = topcon_test::point(result->project, "A");
    ASSERT_NE(a, nullptr);
    EXPECT_DOUBLE_EQ(a->northing, 5000000.0);
    EXPECT_DOUBLE_EQ(a->easting, 500000.0);
}

TEST(SokkiaSdr, GonsAndMilsAreTurnedToRadians)
{
    // 100 gons and 1600 mils are each a quarter circle ([SDR] 3.3.1: 400
    // gons, 6400 mils to the circle); 4 is the Nikon manual's code for mils.
    for (const auto& [options, circle] : {std::pair{"213111", "100.0"},
                                          std::pair{"313111", "1600.0"},
                                          std::pair{"413111", "1600.0"}}) {
        SCOPED_TRACE(options);
        const auto result = readText(file({
            header33(options),
            instrument33(),
            "02NM" + f16("P1") + f16("") + f16("") + f16("") + f16("1.5"),
            "03NM" + f16("1.5"),
            "09F1" + f16("P1") + f16("P2") + f16("10.0") + f16(circle) + f16(circle),
        }));
        ASSERT_TRUE(result.ok()) << result.error().describe();
        const auto directions = observationsTo<survey::HorizontalDirectionObservation>(
            setupAt(result->project, 0), "P2");
        ASSERT_EQ(directions.size(), 1u);
        EXPECT_NEAR(directions[0].direction, kPi / 2.0, kAngleTolerance);
        const auto zeniths =
            observationsTo<survey::ZenithAngleObservation>(setupAt(result->project, 0), "P2");
        ASSERT_EQ(zeniths.size(), 1u);
        EXPECT_NEAR(zeniths[0].angle, kPi / 2.0, kAngleTolerance);
        EXPECT_EQ(result->project.units.angular, options[0] == '2'
                                                     ? survey::AngularUnit::Gons
                                                     : survey::AngularUnit::Mils);
    }
}

TEST(SokkiaSdr, FeetAndUsSurveyFeetAreTurnedToMetres)
{
    const auto readShot = [](std::string_view options, std::string_view unitNote) {
        std::vector<std::string> lines = {header33(options)};
        if (!unitNote.empty()) {
            lines.push_back("13DU" + std::string(unitNote));
        }
        lines.push_back(instrument33());
        lines.push_back("02NM" + f16("P1") + f16("") + f16("") + f16("") + f16("5.0"));
        lines.push_back("03NM" + f16("5.0"));
        lines.push_back("09F1" + f16("P1") + f16("P2") + f16("100.0") + f16("90.0") + f16("0.0"));
        std::string text;
        for (const std::string& line : lines) {
            text += line + "\n";
        }
        return readText(text);
    };
    // International foot 0.3048 m exactly; US survey foot 1200/3937 m.
    const auto feet = readShot("123111", "");
    ASSERT_TRUE(feet.ok()) << feet.error().describe();
    EXPECT_EQ(feet->project.units.linear, survey::LinearUnit::Feet);
    const survey::SurveyStation& setup = setupAt(feet->project, 0);
    EXPECT_NEAR(setup.setup.instrumentHeight, 1.524, kLengthTolerance);
    const auto shotInFeet = firstOf(observationsTo<survey::DistanceObservation>(setup, "P2"));
    EXPECT_NEAR(shotInFeet.distance, 30.48, kLengthTolerance);
    EXPECT_NEAR(shotInFeet.targetHeight, 1.524, kLengthTolerance);
    // [SDR] 3.3.2: 13DU3 narrows the header's feet to US survey feet.
    const auto usFeet = readShot("123111", "3:US Feet:");
    ASSERT_TRUE(usFeet.ok()) << usFeet.error().describe();
    EXPECT_EQ(usFeet->project.units.linear, survey::LinearUnit::UsSurveyFeet);
    const auto shotInUsFeet =
        firstOf(observationsTo<survey::DistanceObservation>(setupAt(usFeet->project, 0), "P2"));
    EXPECT_NEAR(shotInUsFeet.distance, 100.0 * 1200.0 / 3937.0, kLengthTolerance);
    // A published writer puts 3 in the header itself.
    const auto three = readShot("133111", "");
    ASSERT_TRUE(three.ok()) << three.error().describe();
    EXPECT_EQ(three->project.units.linear, survey::LinearUnit::UsSurveyFeet);
}

TEST(SokkiaSdr, PressureAndTemperatureUnitsAreConverted)
{
    const auto weather = [](std::string_view options, std::string_view pressure,
                            std::string_view temperature) {
        return readText(file({header33(options), "05NM" + f16(pressure) + f16(temperature),
                              "02NM" + f16("P1") + f16("") + f16("") + f16("") + f16("1.5")}));
    };
    // The conventional millimetre of mercury is 13595.1 kg/m3 x 9.80665
    // m/s2 x 1 mm; an inch of mercury 25.4 of them. 68 F is 20 C exactly.
    // Options 3 and 4 are the pressure and temperature units.
    const auto mmHg = weather("111211", "760.0", "68.0");
    ASSERT_TRUE(mmHg.ok()) << mmHg.error().describe();
    EXPECT_NEAR(valueOf(setupAt(mmHg->project, 0).instrument.pressureHectopascals),
                13595.1 * 9.80665 * 0.760 / 100.0, 1e-9);
    EXPECT_NEAR(valueOf(setupAt(mmHg->project, 0).instrument.temperatureCelsius), 20.0, 1e-12);
    const auto inHg = weather("112111", "29.92", "68.0");
    ASSERT_TRUE(inHg.ok()) << inHg.error().describe();
    EXPECT_NEAR(valueOf(setupAt(inHg->project, 0).instrument.pressureHectopascals),
                13595.1 * 9.80665 * (29.92 * 0.0254) / 100.0, 1e-9);
    EXPECT_NEAR(valueOf(setupAt(inHg->project, 0).instrument.temperatureCelsius), 68.0, 1e-12);
    const auto fahrenheit = weather("113211", "1000.0", "68.0");
    ASSERT_TRUE(fahrenheit.ok()) << fahrenheit.error().describe();
    const survey::InstrumentSettings& inFahrenheit = setupAt(fahrenheit->project, 0).instrument;
    EXPECT_NEAR(valueOf(inFahrenheit.temperatureCelsius), 20.0, 1e-12);
    EXPECT_DOUBLE_EQ(valueOf(inFahrenheit.pressureHectopascals), 1000.0);
    // An undefined pressure unit: said once, and no pressure read.
    const auto unknown = weather("119111", "1000.0", "20.0");
    ASSERT_TRUE(unknown.ok()) << unknown.error().describe();
    EXPECT_TRUE(warned(*unknown, 1, "pressure unit option is '9'"));
    EXPECT_FALSE(setupAt(unknown->project, 0).instrument.pressureHectopascals.has_value());
    EXPECT_DOUBLE_EQ(valueOf(setupAt(unknown->project, 0).instrument.temperatureCelsius), 20.0);
}

TEST(SokkiaSdr, AnUndefinedAngleOrDistanceUnitIsRefusedNamingTheHeader)
{
    for (const char* options : {"513111", "143111", "11311", ""}) {
        SCOPED_TRACE(options);
        const auto result = readText(file({header33(options), "10NM" + f16("J")}));
        ASSERT_FALSE(result.ok());
        EXPECT_EQ(result.error().code, ErrorCode::FileImportFailure);
        EXPECT_NE(result.error().message.find("record 1 "), std::string::npos)
            << result.error().message;
    }
}

TEST(SokkiaSdr, CoordinatesUnderAnUndefinedOrderAreRefusedButAFileWithoutThemReads)
{
    const std::string station = "02NM" + f16("P1") + f16("") + f16("") + f16("") + f16("1.5");
    const auto refused = readText(file({header33("113151"), station,
                                        "08KI" + f16("A") + f16("1.0") + f16("2.0") + f16("") +
                                            f16("")}));
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("record 3 "), std::string::npos);
    EXPECT_NE(refused.error().message.find("coordinate order option is '5'"), std::string::npos);
    const auto reads = readText(file({header33("113151"), station}));
    ASSERT_TRUE(reads.ok()) << reads.error().describe();
    EXPECT_EQ(reads->project.stations.size(), 1u);
}

TEST(SokkiaSdr, AnUndocumentedAnglesLeftOptionIsRefused)
{
    const auto result = readText(file({header33("113112"), "10NM" + f16("J")}));
    ASSERT_FALSE(result.ok());
    EXPECT_NE(result.error().message.find("angles left/right option is '2'"), std::string::npos);
}

TEST(SokkiaSdr, ADistanceUnitNoteThatContradictsTheHeaderIsRefused)
{
    const auto result = readText(file({header33("113111"), "13DU3:US Feet:", "10NM" + f16("J")}));
    ASSERT_FALSE(result.ok());
    EXPECT_NE(result.error().message.find("record 2 "), std::string::npos);
}

TEST(SokkiaSdr, TheFortyTwoCharacterHeaderOfAPublishedWriterReadsItsOptions)
{
    // Trimble's published SDR33 writer: no serial number, so the date is in
    // 21-36 and the options in 37-42; here feet and Fahrenheit.
    const std::string header = "00NM" + f16("SDR33 V04-04.25") + f16("28-Sep-26 08:00") + "123211";
    ASSERT_EQ(header.size(), 42u);
    const auto result = readText(file({header, "02NM" + f16("P1") + f16("") + f16("") + f16("") +
                                                   f16("10.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->project.units.linear, survey::LinearUnit::Feet);
    EXPECT_NEAR(setupAt(result->project, 0).setup.instrumentHeight, 3.048, kLengthTolerance);
    EXPECT_EQ(result->project.metadata.at("header: date and time"), "28-Sep-26 08:00");
    EXPECT_EQ(result->warnings.size(), 0u);
}

// ---- Setups, rounds and backsights ---------------------------------------------------------

namespace {

std::string backsight(std::string_view to, std::string_view azimuth, std::string_view circle)
{
    return "07NM" + f16("P1") + f16(to) + f16(azimuth) + f16(circle);
}

std::string shot(std::string_view face, std::string_view to, std::string_view circle,
                 std::string_view zenith = "90.0")
{
    return "09" + std::string(face) + f16("P1") + f16(to) + f16("50.0") + f16(zenith) +
           f16(circle);
}

std::string setupOnP1()
{
    return "02NM" + f16("P1") + f16("") + f16("") + f16("") + f16("1.5");
}

} // namespace

TEST(SokkiaSdr, ABacksightToAnotherPointAfterShotsBeginsANewSetup)
{
    const auto result = readText(file({header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
                                       backsight("P2", "0.0", "0.0"), shot("F1", "P2", "0.0"),
                                       shot("F1", "P3", "10.0"), backsight("P3", "45.0", "10.0"),
                                       shot("F1", "P3", "10.0"), shot("F1", "P4", "20.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyProject& project = result->project;
    ASSERT_EQ(project.stations.size(), 2u);
    EXPECT_EQ(setupAt(project, 0).backsightPointId, "P2");
    EXPECT_EQ(setupAt(project, 0).observations.size(), 6u);
    // The re-oriented half is a setup of its own on the same point, with
    // the same instrument height.
    EXPECT_EQ(setupAt(project, 1).setup.id, "P1 (2)");
    EXPECT_EQ(setupAt(project, 1).setup.pointId, "P1");
    EXPECT_DOUBLE_EQ(setupAt(project, 1).setup.instrumentHeight, 1.5);
    EXPECT_EQ(setupAt(project, 1).backsightPointId, "P3");
    EXPECT_NEAR(valueOf(setupAt(project, 1).backsightAzimuth), degrees(45.0), kAngleTolerance);
    EXPECT_EQ(setupAt(project, 1).observations.size(), 6u);
    EXPECT_EQ(setupAt(project, 1).source.recordNumber, 8u);
}

TEST(SokkiaSdr, ACircleMovedBetweenRoundsBeginsANewSetupAndOneWithinAMinuteDoesNot)
{
    // Round 2's circle reading on the backsight is 59" from round 1's
    // (0.01638888 degrees): another round. Round 3's is 61" (0.01694444):
    // the circle was moved, so a new setup.
    const auto result = readText(file({header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
                                       backsight("P2", "0.0", "0.0"), shot("F1", "P2", "0.0"),
                                       backsight("P2", "0.0", "0.01638888"),
                                       shot("F1", "P2", "0.01638888"),
                                       backsight("P2", "0.0", "0.01694444"),
                                       shot("F1", "P2", "0.01694444")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyProject& project = result->project;
    ASSERT_EQ(project.stations.size(), 2u);
    EXPECT_EQ(setupAt(project, 0).metadata.at("backsight rounds"), "2");
    EXPECT_EQ(setupAt(project, 0).observations.size(), 6u);
    EXPECT_EQ(setupAt(project, 1).setup.id, "P1 (2)");
    EXPECT_EQ(setupAt(project, 1).observations.size(), 3u);
}

TEST(SokkiaSdr, ASecondBacksightBeforeAnyShotReplacesTheFirst)
{
    const auto result = readText(file({header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
                                       backsight("P2", "0.0", "0.0"),
                                       backsight("P3", "90.0", "0.0"), shot("F1", "P3", "0.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result->project.stations.size(), 1u);
    EXPECT_EQ(setupAt(result->project, 0).backsightPointId, "P3");
    EXPECT_NEAR(valueOf(setupAt(result->project, 0).backsightAzimuth), degrees(90.0),
                kAngleTolerance);
    EXPECT_TRUE(warned(*result, 6, "replaces the first (record 5)"));
}

TEST(SokkiaSdr, ASetupThatNeverObservesItsBacksightSaysSo)
{
    const auto result = readText(file({header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
                                       backsight("P2", "0.0", "0.0"), shot("F1", "P3", "10.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 5, "no observation of its backsight 'P2'"));
}

TEST(SokkiaSdr, AShotFromAnotherPointThanTheSetupIsSkipped)
{
    const auto result = readText(file({header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
                                       "09F1" + f16("P9") + f16("P2") + f16("50.0") + f16("90.0") +
                                           f16("0.0"),
                                       "07NM" + f16("P9") + f16("P2") + f16("0.0") + f16("0.0"),
                                       shot("F1", "P1", "0.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsSkipped, 3u);
    EXPECT_TRUE(warned(*result, 5, "from 'P9' while the setup is on 'P1'"));
    EXPECT_TRUE(warned(*result, 6, "from 'P9' while the setup is on 'P1'"));
    EXPECT_TRUE(warned(*result, 7, "to itself"));
    EXPECT_TRUE(setupAt(result->project, 0).observations.empty());
}

// ---- Faces and vertical readings -------------------------------------------------------------

TEST(SokkiaSdr, HorizonVerticalReadingsBecomeZenithAngles)
{
    // Vertical angle option 2, "measured upwards from horizontal" ([SDR]
    // 3.5): +3 degrees is the zenith 87 on face 1; 177 is the same line on
    // face 2 (its zenith reading 360 - 87 = 273, less 90 ... 90 - 273 =
    // -183, which is 177).
    const auto result = readText(file({header33(), instrument33('2'), setupOnP1(),
                                       "03NM" + f16("1.5"), shot("F1", "P2", "10.0", "3.0"),
                                       shot("F2", "P2", "190.0", "177.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const auto zeniths =
        observationsTo<survey::ZenithAngleObservation>(setupAt(result->project, 0), "P2");
    ASSERT_EQ(zeniths.size(), 2u);
    EXPECT_NEAR(zeniths[0].angle, degrees(87.0), 1e-12);
    EXPECT_EQ(zeniths[0].pointing.face, survey::Face::Left);
    EXPECT_NEAR(zeniths[1].angle, degrees(87.0), 1e-12);
    EXPECT_EQ(zeniths[1].pointing.face, survey::Face::Right);
    EXPECT_EQ(result->warnings.size(), 0u);
}

TEST(SokkiaSdr, AMultipleDistanceObservationTakesItsFaceFromItsVerticalReading)
{
    // [SETX] 29.2.3: 0 to 180 degrees is face 1, 180 to 360 face 2.
    const auto result = readText(file({header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
                                       shot("MD", "P2", "10.0", "88.0"),
                                       shot("MD", "P2", "190.0", "272.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const auto directions =
        observationsTo<survey::HorizontalDirectionObservation>(setupAt(result->project, 0), "P2");
    ASSERT_EQ(directions.size(), 2u);
    EXPECT_EQ(directions[0].pointing.face, survey::Face::Left);
    EXPECT_EQ(directions[1].pointing.face, survey::Face::Right);
}

TEST(SokkiaSdr, AFaceTwoRecordWithAFaceOneReadingWarnsAndKeepsTheRecordsFace)
{
    const auto result = readText(file({header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
                                       shot("F2", "P2", "10.0", "88.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 5, "F2 says face right"));
    const auto zeniths =
        observationsTo<survey::ZenithAngleObservation>(setupAt(result->project, 0), "P2");
    ASSERT_EQ(zeniths.size(), 1u);
    EXPECT_EQ(zeniths[0].pointing.face, survey::Face::Right);
    EXPECT_NEAR(zeniths[0].angle, degrees(88.0), kAngleTolerance);
}

// ---- What is not imported, and why ---------------------------------------------------------

TEST(SokkiaSdr, DerivedViewsAndUnknownRecordsAreSkippedWithTheirReason)
{
    const auto result = readText(file({
        header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
        "09MC" + f16("P1") + f16("P2") + f16("50.0") + f16("90.0") + f16("45.0"),
        "11TP" + f16("P1") + f16("P2") + f16("45.0") + f16("50.0") + f16("0.1"),
        "25RO" + f16("P1"),
        "09TP" + f16("P1") + f16("P2") + f16("50.0") + f16("90.0") + f16("45.0"),
        "NOT A RECORD",
        shot("F1", "P2", "45.0"),
    }));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsSkipped, 5u);
    EXPECT_TRUE(warned(*result, 5, "corrected observation (MC)"));
    EXPECT_TRUE(warned(*result, 6, "reduced observation"));
    EXPECT_TRUE(warned(*result, 7, "25RO (road station)"));
    EXPECT_TRUE(warned(*result, 8, "derivation code 'TP'"));
    EXPECT_TRUE(warned(*result, 9, "is not a record"));
    EXPECT_EQ(setupAt(result->project, 0).observations.size(), 3u);
}

TEST(SokkiaSdr, ABadSetsObservationsAreSkippedAndAGoodSetsRead)
{
    // [SDR] 3.6.2 SET: source point, count, set number, bad marker (2 is
    // "Bad set", 3.5), return sight, prompt order.
    const auto result = readText(file({header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
                                       "12SC" + f16("P1") + "002" + "001" + "2" + "1" + "1",
                                       shot("F1", "P2", "0.0"), shot("F2", "P2", "180.0"),
                                       "12SC" + f16("P1") + "002" + "002" + "1" + "1" + "1",
                                       shot("F1", "P2", "0.0"), shot("F2", "P2", "180.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsSkipped, 2u);
    EXPECT_TRUE(warned(*result, 5, "set 001 at 'P1' is marked bad"));
    EXPECT_TRUE(warned(*result, 6, "marks bad"));
    EXPECT_TRUE(warned(*result, 7, "marks bad"));
    EXPECT_EQ(setupAt(result->project, 0).observations.size(), 6u);
    const auto firstRead =
        firstOf(observationsTo<survey::DistanceObservation>(setupAt(result->project, 0), "P2"));
    EXPECT_EQ(firstRead.source.recordNumber, 9u);
}

TEST(SokkiaSdr, WeatherChangedInTheMiddleOfASetupWarnsAndIsNotApplied)
{
    const auto result = readText(file({header33(), "05NM" + f16("1000.0") + f16("10.0"),
                                       instrument33(), setupOnP1(), "03NM" + f16("1.5"),
                                       shot("F1", "P2", "0.0"),
                                       "05NM" + f16("990.0") + f16("15.0"),
                                       shot("F1", "P3", "10.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 7, "the weather changes in the middle of setup 'P1'"));
    const survey::InstrumentSettings& instrument = setupAt(result->project, 0).instrument;
    EXPECT_DOUBLE_EQ(valueOf(instrument.pressureHectopascals), 1000.0);
    EXPECT_DOUBLE_EQ(valueOf(instrument.temperatureCelsius), 10.0);
}

TEST(SokkiaSdr, NotANumberWarnsWithItsRecordAndIsReadAsNotMeasured)
{
    const auto result = readText(file({header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
                                       "09F1" + f16("P1") + f16("P2") + f16("5O.0") + f16("90.0") +
                                           f16("0.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 5, "slope distance '5O.0' is not a number"));
    const survey::SurveyStation& setup = setupAt(result->project, 0);
    EXPECT_TRUE(observationsTo<survey::DistanceObservation>(setup, "P2").empty());
    EXPECT_EQ(observationsTo<survey::ZenithAngleObservation>(setup, "P2").size(), 1u);
    // [SDR] 3.3 (i) allows a minus, digits and a point: an exponent or a
    // plus sign is not a real of the format, however a C library reads it.
    for (const char* text : {"1e3", "+50.0", "5.0.0", "-"}) {
        const auto odd = readText(file({header33(), instrument33(), setupOnP1(),
                                        "03NM" + f16("1.5"),
                                        "09F1" + f16("P1") + f16("P2") + f16(text) + f16("90.0") +
                                            f16("0.0")}));
        ASSERT_TRUE(odd.ok()) << odd.error().describe();
        EXPECT_TRUE(warned(*odd, 5, "is not a number")) << text;
        EXPECT_TRUE(
            observationsTo<survey::DistanceObservation>(setupAt(odd->project, 0), "P2").empty())
            << text;
    }
}

TEST(SokkiaSdr, AShotBeforeAnyTargetHeightWarnsAndSaysSo)
{
    const auto result =
        readText(file({header33(), instrument33(), setupOnP1(), shot("F1", "P2", "0.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 4, "no target height record"));
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(*result, "no target height"));
    EXPECT_DOUBLE_EQ(
        firstOf(observationsTo<survey::DistanceObservation>(setupAt(result->project, 0), "P2"))
            .targetHeight,
        0.0);
}

TEST(SokkiaSdr, AFileWithoutAHeaderOrWithNothingInItIsAnError)
{
    for (const std::string& text :
         {std::string{}, std::string("\n\n"), file({"10NM" + f16("J")}), file({header33()}),
          file({header33(), "10NM" + f16("J"), "13NMA NOTE"})}) {
        const auto result = readText(text);
        EXPECT_FALSE(result.ok()) << text;
    }
}

// ---- Lines, framing, text ----------------------------------------------------------------------

TEST(SokkiaSdr, LfCrLfAndCrLineEndsReadAlike)
{
    const std::initializer_list<std::string> lines = {
        header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"), shot("F1", "P2", "0.0")};
    const auto lf = readText(file(lines, "\n"));
    const auto crlf = readText(file(lines, "\r\n"));
    const auto cr = readText(file(lines, "\r"));
    ASSERT_TRUE(lf.ok() && crlf.ok() && cr.ok());
    EXPECT_EQ(lf->project, crlf->project);
    EXPECT_EQ(lf->project, cr->project);
    EXPECT_EQ(lf->recordsRead, 5u);
}

TEST(SokkiaSdr, TransmissionFramingIsNotARecordAndItsChecksumIsKept)
{
    // [SDR] chapter 3: STX CR LF first, ETX and a five-digit checksum last.
    const auto result = readText(file({"\x02", header33(), setupOnP1(), "\x03" "12345"}, "\r\n"));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsRead, 2u);
    EXPECT_EQ(result->recordsSkipped, 0u);
    EXPECT_EQ(result->project.metadata.at("transmission checksum"), "12345");
}

TEST(SokkiaSdr, TimeStampsInTheFieldBooksOwnFormsAreRead)
{
    // [SDR] chapter 4 writes "18-Jan-80 20:14", its V04-04.30 example
    // "May-12-97 10:53"; a two-digit year follows POSIX strptime %y.
    const struct {
        const char* text;
        survey::SurveyTimestamp expected;
    } stamps[] = {{"18-Jan-80 20:14", {1980, 1, 18, 20, 14, 0.0, ""}},
                  {"May-12-97 10:53", {1997, 5, 12, 10, 53, 0.0, ""}},
                  {"07-Mar-21 13:36", {2021, 3, 7, 13, 36, 0.0, ""}},
                  {"Time Date 12/31/1999 Time 23:59:58", {1999, 12, 31, 23, 59, 58.0, ""}}};
    for (const auto& stamp : stamps) {
        SCOPED_TRACE(stamp.text);
        const auto result =
            readText(file({header33(), setupOnP1(), "13TS" + std::string(stamp.text)}));
        ASSERT_TRUE(result.ok()) << result.error().describe();
        EXPECT_EQ(setupAt(result->project, 0).instrument.time, stamp.expected);
    }
    const auto odd = readText(file({header33(), setupOnP1(), "13TSsometime tomorrow"}));
    ASSERT_TRUE(odd.ok());
    EXPECT_TRUE(warned(*odd, 3, "time stamp 'sometime tomorrow'"));
    EXPECT_FALSE(setupAt(odd->project, 0).instrument.time.known());
}

TEST(SokkiaSdr, ANameOutsideAsciiDoesNotShiftTheFieldsAfterIt)
{
    // "Pé1" is three characters and, in UTF-8, four bytes: the northing
    // after it still starts in column 21.
    const auto result = readText(file({header33(), "08KI" + std::string("P\xc3\xa9") +
                                                       std::string(13, ' ') + "1" +
                                                       f16("5000.0") + f16("200.0") + f16("")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyPoint* point =
        topcon_test::point(result->project, "P\xc3\xa9             1");
    ASSERT_NE(point, nullptr);
    EXPECT_DOUBLE_EQ(point->northing, 5000.0);
    EXPECT_DOUBLE_EQ(point->easting, 200.0);
    EXPECT_TRUE(warned(*result, 2, "outside the ASCII"));
}

TEST(SokkiaSdr, ShortRecordsReadTheirFieldsAndNothingPastTheirEnd)
{
    // [SDR] chapter 5: "trailing blanks may be truncated". The 02 below
    // ends after its instrument height, the 03 after three characters.
    const auto result = readText(file({header33(), instrument33(), "02NM" + f16("P1") + f16("") +
                                                                       f16("") + f16("") + "1.5",
                                       "03NM1.6", shot("F1", "P2", "0.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_DOUBLE_EQ(setupAt(result->project, 0).setup.instrumentHeight, 1.5);
    EXPECT_DOUBLE_EQ(
        firstOf(observationsTo<survey::DistanceObservation>(setupAt(result->project, 0), "P2"))
            .targetHeight,
        1.6);
    EXPECT_EQ(result->warnings.size(), 0u);
}

TEST(SokkiaSdr, EveryTruncationAndRandomBytesComeBackAsAnAnswer)
{
    const std::string bytes = fixture("sdr/traverse.sdr");
    const std::size_t reads =
        topcon_test::readEveryTruncationAndNoise(kId, bytes, "traverse.sdr");
    EXPECT_GT(reads, bytes.size());
}
