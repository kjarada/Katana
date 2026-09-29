// The Sokkia SDR reader (src/katana_surveyio/sokkia_sdr.cpp).
//
// data/sdr/traverse.sdr is written by hand from the SDR33 record layouts of
// "Interfacing with the SOKKIA SDR Electronic Field Book" (October 1999),
// section 3.6.2: a header in degrees, metres, millibars and Celsius with
// the coordinates east first; two keyed control marks; a setup on CP1 with
// two rounds of face 1 and face 2 to its backsight CP2 and to T1; a block
// of deleted ("DD") records; a setup on T1, its circle zeroed on its
// backsight CP1 (azimuth 270), observing CP1 and T2. Point ids are
// right-justified in their 16 columns, as some writers pad them. Every
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
#include <set>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/survey/reduction.hpp"
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
constexpr std::size_t kNone = std::string::npos;

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

std::size_t warningsWith(const ReadResult& result, std::string_view words)
{
    std::size_t count = 0;
    for (const ReadWarning& warning : result.warnings) {
        count += warning.message.find(words) != std::string::npos ? 1 : 0;
    }
    return count;
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

std::string f10(std::string_view text)
{
    return field(text, 10);
}

// An SDR33 header: version, serial number, date and time, then the six
// options ([SDR] 3.6.2) - by default degrees, metres, mbar, Celsius, north
// first, angles right.
std::string header33(std::string_view options = "113111")
{
    return "00NM" + f16("SDR33 V04-03.00") + "0001" + f16("28-Sep-26 08:00") +
           std::string(options);
}

// A header written as the Nikon and Spectra manuals give theirs: no blank
// between "SDR33" and the version.
std::string headerNikon(std::string_view options)
{
    return "00NM" + f16("SDR33V04-01") + "0000" + f16("28-Sep-26 08:00") + std::string(options);
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

// An SDR33 instrument record: a manual EDM, theodolite "HAND" serial 1,
// mounting 3, the vertical angle option given, no offsets, prism constant 0.
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
    static const survey::SurveyStation kNoSetup{};
    if (i >= project.stations.size()) {
        ADD_FAILURE() << "no setup " << i << ": " << project.stations.size() << " were read";
        return kNoSetup;
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

// The reduction with every correction off, so a position worked by hand
// from the record text is what it gives.
Result<survey::ReductionOutcome> reduced(const survey::SurveyProject& project)
{
    survey::ReductionSettings settings;
    settings.atmospheric = survey::AtmosphericCorrection::None;
    settings.prismConstantPolicy = survey::PrismConstantPolicy::None;
    settings.curvatureAndRefraction = false;
    return survey::reduceAndAdjust(project, settings, {});
}

const survey::ComputedPoint* computed(const survey::ReductionOutcome& outcome, std::string_view id)
{
    for (const survey::ComputedPoint& point : outcome.points) {
        if (point.id == id) {
            return &point;
        }
    }
    return nullptr;
}

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

TEST(SokkiaSdr, AHeaderWithSomeOddLinesStillIdentifiesTheFileAndSaysHowFewAreRecords)
{
    // No other format writes an SDR header, so 3 odd lines in 18 - a damaged
    // stretch the reader skips line by line - do not leave the file
    // unrecognised. A clean file is surer than this one.
    std::vector<std::string> lines = {header33(), setupOnP1()};
    for (int i = 0; i < 13; ++i) {
        lines.push_back("03NM" + f16("1.5"));
    }
    lines.insert(lines.end(), {"garbled line one", "garbled line two", "garbled line three"});
    std::string text;
    for (const std::string& line : lines) {
        text += line + "\n";
    }
    const double odd = confidenceOf(text, "job.txt", kId);
    EXPECT_GE(odd, kIdentifiedConfidence);
    EXPECT_LT(odd, confidenceOf(file({header33(), setupOnP1()}), "job.txt", kId));
    const Detection detection = detectFormat(probeOf(text, "job.txt"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_NE(detection.candidates().front().evidence.find("but only 15 of 18 lines"),
              std::string::npos)
        << detection.candidates().front().evidence;
}

TEST(SokkiaSdr, AnStxGluedToTheHeaderIsFramingAndTheHeaderIsRead)
{
    // [SDR] chapter 3 opens a transmission with STX CR LF; a writer that
    // leaves out the line end glues the header to it.
    const std::string text = "\x02" + file({header33(), setupOnP1()});
    const Detection detection = detectFormat(probeOf(text, "job.sdr"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kId);
    const auto result = readText(text);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsRead, 2u);
    EXPECT_EQ(result->recordsSkipped, 0u);
    EXPECT_EQ(result->project.source.formatVersion, "SDR33 V04-03.00");
}

// ---- The header -------------------------------------------------------------------

TEST(SokkiaSdr, AShortOrStrayHeaderRecordIsAnAnswerAndNotACrash)
{
    // A second "00" record two characters long once its blanks are gone,
    // three, or naming no version: the header reader must not cut past its
    // end. Each is skipped, naming the file's own header, and the file
    // reads on - one line must not cost the rest.
    for (const char* stray : {"00  ", "00A ", "00NM"}) {
        SCOPED_TRACE(stray);
        const auto result =
            readText(file({header33(), stray, setupOnP1(), "03NM" + f16("1.5")}));
        ASSERT_TRUE(result.ok()) << result.error().describe();
        EXPECT_EQ(result->recordsSkipped, 1u);
        EXPECT_EQ(result->recordsRead, 3u);
        EXPECT_TRUE(warned(*result, 2, "follows the file's own (record 1)"));
        EXPECT_EQ(result->project.stations.size(), 1u);
    }
    // As the first line, read as SDR because it was asked to be: refused in
    // a sentence.
    for (const char* first : {"00  ", "00A ", "00NM"}) {
        SCOPED_TRACE(first);
        const auto result = readText(file({first, setupOnP1()}));
        ASSERT_FALSE(result.ok());
        EXPECT_EQ(result.error().code, ErrorCode::FileImportFailure);
        EXPECT_NE(result.error().message.find("names no version"), kNone)
            << result.error().message;
    }
}

TEST(SokkiaSdr, AHeaderEndingInWhiteSpaceReadsItsOptions)
{
    for (const char* padding : {"\t", "  ", " \t "}) {
        SCOPED_TRACE(padding);
        const auto result = readText(file({header33("123111") + padding,
                                           "02NM" + f16("P1") + f16("") + f16("") + f16("") +
                                               f16("10.0")}));
        ASSERT_TRUE(result.ok()) << result.error().describe();
        EXPECT_EQ(result->warnings.size(), 0u);
        // Option 2 of 42: feet, so 10 is 3.048 m.
        EXPECT_NEAR(setupAt(result->project, 0).setup.instrumentHeight, 3.048, kLengthTolerance);
    }
}

TEST(SokkiaSdr, TheVersionIsReadOneWayForTheMetadataAndTheProvenance)
{
    // Blanks between the name and the version, as some writers pad them:
    // one reading, "SDR33 V04-04", in both places.
    const auto result = readText(
        file({"00NM" + f16("SDR33    V04-04") + "0042" + f16("01-Feb-26 07:15") + "113111",
              setupOnP1()}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->project.metadata.at("header: version"), "SDR33 V04-04");
    EXPECT_EQ(result->project.source.formatVersion, "SDR33 V04-04");
    EXPECT_EQ(result->project.metadata.at("header: serial number"), "0042");
}

TEST(SokkiaSdr, AHeaderVersionOtherThanSdr33OrSdr2xIsRefused)
{
    const auto result = readText(file(
        {"00NM" + f16("SDR44 V01-00") + "0001" + f16("28-Sep-26 08:00") + "113111", setupOnP1()}));
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::FileImportFailure);
    EXPECT_NE(result.error().message.find("names the version 'SDR44 V01-00'"), kNone)
        << result.error().message;
}

TEST(SokkiaSdr, ASecondHeaderWithTheSameUnitsReadsOnAndOneWithOthersIsRefused)
{
    // [SDR] chapter 3: each transmission opens with a header, so two joined
    // files hold two. The same units: read on. Others: every number after
    // the second would mean something else.
    const auto same = readText(file({header33(), setupOnP1(), header33(),
                                     "02NM" + f16("P2") + f16("") + f16("") + f16("") +
                                         f16("1.6")}));
    ASSERT_TRUE(same.ok()) << same.error().describe();
    EXPECT_EQ(same->recordsRead, 4u);
    EXPECT_EQ(same->project.stations.size(), 2u);
    const auto other = readText(file({header33("113111"), setupOnP1(), header33("123111")}));
    ASSERT_FALSE(other.ok());
    EXPECT_NE(other.error().message.find("record 3 "), kNone) << other.error().message;
    EXPECT_NE(other.error().message.find("a second header record states other units"), kNone);
}

TEST(SokkiaSdr, AHeaderOfAnotherLengthIsReadFromItsLastSixCharactersAndSaysSo)
{
    // 44 characters: the date takes 21-38, the options 39-44 ("123111":
    // degrees and feet).
    const std::string header =
        "00NM" + f16("SDR33 V04-03.00") + field("28-Sep-26 08:00", 18) + "123111";
    ASSERT_EQ(header.size(), 44u);
    const auto result = readText(file({header, "02NM" + f16("P1") + f16("") + f16("") + f16("") +
                                                   f16("10.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 1, "the header record is 44 characters where the format's is 46"));
    EXPECT_NEAR(setupAt(result->project, 0).setup.instrumentHeight, 3.048, kLengthTolerance);
}

TEST(SokkiaSdr, AngleOptionFourIsMilsInTheNikonHeaderFormAndDegreesOtherwiseEachSaid)
{
    // [SDR] 3.5 defines 1 to 3. The Nikon and Spectra manuals give 4 as mils
    // in headers written "SDR33V04-01"; Trimble's writer gives 4 as quadrant
    // bearings, which Sokkia's manual says are degrees underneath. A shot at
    // 90 on the circle: pi/2 in degrees, 90 * pi / 3200 in mils.
    const struct {
        std::string header;
        double circle;
        survey::AngularUnit unit;
        const char* words;
    } cases[] = {{header33("413111"), kPi / 2.0, survey::AngularUnit::DecimalDegrees,
                  "so the angles are read in degrees"},
                 {headerNikon("413111"), 90.0 * kPi / 3200.0, survey::AngularUnit::Mils,
                  "so the angles are read in mils"}};
    for (const auto& c : cases) {
        SCOPED_TRACE(c.header);
        const auto result = readText(file({c.header, instrument33(), setupOnP1(),
                                           "03NM" + f16("1.5"), shot("F1", "P2", "90.0")}));
        ASSERT_TRUE(result.ok()) << result.error().describe();
        EXPECT_TRUE(warned(*result, 1, "angle unit option is 4")) << c.header;
        EXPECT_TRUE(warned(*result, 1, c.words));
        EXPECT_EQ(result->project.units.angular, c.unit);
        const auto direction = firstOf(observationsTo<survey::HorizontalDirectionObservation>(
            setupAt(result->project, 0), "P2"));
        EXPECT_NEAR(direction.direction, c.circle, kAngleTolerance);
    }
}

TEST(SokkiaSdr, CoordinatesReadEastFirstUnderOptionTwoSayWhichReadingWasTaken)
{
    // Option 2 (and Trimble's 3): the first ordinate is the easting, as
    // Trimble's writer puts it; a Sokkia field book may put the northing
    // there, so the first record with coordinates says so - once.
    const auto point = [](std::string_view id, std::string_view first, std::string_view second) {
        return "08KI" + f16(id) + f16(first) + f16(second) + f16("") + f16("");
    };
    for (const char* options : {"113121", "113131"}) {
        SCOPED_TRACE(options);
        const auto east = readText(file({header33(options),
                                         point("A", "500000.000", "5000000.000"),
                                         point("B", "500100.000", "5000000.000")}));
        ASSERT_TRUE(east.ok()) << east.error().describe();
        const survey::SurveyPoint* a = topcon_test::point(east->project, "A");
        ASSERT_NE(a, nullptr);
        EXPECT_DOUBLE_EQ(a->easting, 500000.0);
        EXPECT_DOUBLE_EQ(a->northing, 5000000.0);
        EXPECT_TRUE(warned(*east, 2, "is read as the easting"));
        EXPECT_TRUE(warned(*east, 2, "may hold the northing first"));
        EXPECT_EQ(east->warnings.size(), 1u);
    }
    // Option 1 is the format's own order and needs no word; nor does a file
    // under option 2 that states no coordinates.
    const auto north = readText(file({header33("113111"), point("A", "5000000.000", "500000.000")}));
    ASSERT_TRUE(north.ok()) << north.error().describe();
    EXPECT_EQ(north->warnings.size(), 0u);
    const auto none = readText(file({header33("113121"), setupOnP1()}));
    ASSERT_TRUE(none.ok()) << none.error().describe();
    EXPECT_EQ(warningsWith(*none, "coordinate order"), 0u);
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
    // 51 lines: 4 blank, 7 deleted (33 to 39), 40 records read. The eighth
    // warning is the header's coordinate order, option 2, at the first
    // record that states coordinates (8).
    const ReadResult result = readTraverse();
    EXPECT_EQ(result.recordsRead, 40u);
    EXPECT_EQ(result.recordsSkipped, 7u);
    EXPECT_EQ(result.warnings.size(), 8u);
    for (std::size_t record = 33; record <= 39; ++record) {
        EXPECT_TRUE(warned(result, record, "deleted record")) << record;
    }
    EXPECT_TRUE(warned(result, 35, "02NM (station)"));
    EXPECT_TRUE(warned(result, 39, "'DDDD'"));
    EXPECT_TRUE(warned(result, 8, "coordinate order option is 2"));
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

TEST(SokkiaSdr, TheBacksightRecordsAzimuthAndCircleReadingEachHaveTheirField)
{
    const ReadResult result = readTraverse();
    // Records 14 and 23: azimuth 0, circle 0 on CP2, in both rounds.
    const survey::SurveyStation& first = setupAt(result.project, 0);
    EXPECT_EQ(first.backsightPointId, "CP2");
    EXPECT_NEAR(valueOf(first.backsightAzimuth), 0.0, kAngleTolerance);
    EXPECT_NEAR(valueOf(first.statedBacksightAzimuth), 0.0, kAngleTolerance);
    EXPECT_EQ(first.metadata.at("backsight circle readings (radians)"), "0, 0");
    // Record 45: T1's circle zeroed on CP1, whose azimuth is 270 - the
    // circle is the model's backsightAzimuth, the azimuth its own field.
    const survey::SurveyStation& second = setupAt(result.project, 1);
    EXPECT_EQ(second.backsightPointId, "CP1");
    EXPECT_NEAR(valueOf(second.backsightAzimuth), 0.0, kAngleTolerance);
    EXPECT_NEAR(valueOf(second.statedBacksightAzimuth), degrees(270.0), kAngleTolerance);
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
    // Record 13: 11TP CP1 to CP2, azimuth 0, no distances. The setup's 07
    // orients it, so the keyed azimuth is kept, not used.
    const ReadResult result = readTraverse();
    const survey::SurveyStation& first = setupAt(result.project, 0);
    EXPECT_EQ(first.metadata.at("azimuth to CP2 (radians)"), "0");
    EXPECT_EQ(first.metadata.at("azimuth to CP2, description"), "BS AZ");
    EXPECT_FALSE(first.metadata.contains("oriented by"));
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

TEST(SokkiaSdr, TheTraverseReducesToThePositionsWorkedByHand)
{
    // From CP1 (500000 E, 5000000 N) oriented on CP2, due north at circle
    // 0, T1 at circle 90 and zenith 87, its four distances meaning
    // 150.0015: 150.0015 sin 87 = 149.795935 east. From T1 oriented on CP1
    // (azimuth 270 from the coordinates, circle 0 on it), T2 at circle 90 is
    // azimuth 360, due north, at zenith 91 and 80.001: 80.001 sin 91 =
    // 79.988815 north. With no curvature and refraction, those exactly.
    const ReadResult result = readTraverse();
    const auto outcome = reduced(result.project);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const survey::ComputedPoint* t1 = computed(*outcome, "T1");
    ASSERT_NE(t1, nullptr);
    EXPECT_NEAR(t1->easting, 500000.0 + 150.0015 * std::sin(degrees(87.0)), 1e-6);
    EXPECT_NEAR(t1->northing, 5000000.0, 1e-6);
    const survey::ComputedPoint* t2 = computed(*outcome, "T2");
    ASSERT_NE(t2, nullptr);
    EXPECT_NEAR(t2->easting, t1->easting, 1e-6);
    EXPECT_NEAR(t2->northing, 5000000.0 + 80.001 * std::sin(degrees(91.0)), 1e-6);
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
        "01NM1" + field("", 16) + "000000" + f16("DT2") + "000031" + "3" + "1" + f10("") + f10("") +
            f10("0.000"),
        "02KI0001" + f10("5100.000") + f10("200.000") + f10("50.000") + f10("1.000") +
            f16("DEAD DOG"),
        "07TP00010002" + f10("14.00000") + f10("0.00000"),
        "03NM" + f10("1.200"),
        "09F100010002" + f10("42.500") + f10("91.50000") + f10("0.00000") + f16("BS"),
        "09F200010002" + f10("42.502") + f10("268.50000") + f10("180.00000") + f16("BS"),
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
    // [SDR] chapter 4's sample: the azimuth 14, the circle zeroed on 0002.
    EXPECT_NEAR(valueOf(setup.statedBacksightAzimuth), degrees(14.0), kAngleTolerance);
    EXPECT_NEAR(valueOf(setup.backsightAzimuth), 0.0, kAngleTolerance);
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
    // gons, 6400 mils to the circle).
    for (const auto& [options, circle] :
         {std::pair{"213111", "100.0"}, std::pair{"313111", "1600.0"}}) {
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
    // A note with no code, or one the format does not define, says nothing
    // the header has not: skipped, and the header's feet hold.
    for (const char* note : {"", "9:Parsecs:"}) {
        SCOPED_TRACE(note);
        const auto odd = readShot("123111", note[0] == '\0' ? " " : note);
        ASSERT_TRUE(odd.ok()) << odd.error().describe();
        EXPECT_EQ(odd->recordsSkipped, 1u);
        EXPECT_TRUE(warned(*odd, 2, "the header's unit, feet, holds"));
        EXPECT_NEAR(setupAt(odd->project, 0).setup.instrumentHeight, 1.524, kLengthTolerance);
    }
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
    // An undefined pressure or temperature unit: said once, and that value
    // not read.
    const auto noPressure = weather("119111", "1000.0", "20.0");
    ASSERT_TRUE(noPressure.ok()) << noPressure.error().describe();
    EXPECT_TRUE(warned(*noPressure, 1, "pressure unit option is '9'"));
    EXPECT_FALSE(setupAt(noPressure->project, 0).instrument.pressureHectopascals.has_value());
    EXPECT_DOUBLE_EQ(valueOf(setupAt(noPressure->project, 0).instrument.temperatureCelsius),
                     20.0);
    const auto noTemperature = weather("113711", "1000.0", "20.0");
    ASSERT_TRUE(noTemperature.ok()) << noTemperature.error().describe();
    EXPECT_TRUE(warned(*noTemperature, 1, "temperature unit option is '7'"));
    EXPECT_FALSE(setupAt(noTemperature->project, 0).instrument.temperatureCelsius.has_value());
    EXPECT_DOUBLE_EQ(valueOf(setupAt(noTemperature->project, 0).instrument.pressureHectopascals),
                     1000.0);
}

TEST(SokkiaSdr, ADistanceUnitNoteThatContradictsTheHeaderIsRefused)
{
    const auto result = readText(file({header33("113111"), "13DU3:US Feet:", "10NM" + f16("J")}));
    ASSERT_FALSE(result.ok());
    EXPECT_NE(result.error().message.find("record 2 "), std::string::npos);
}

// ---- The job and the instrument ---------------------------------------------------------

TEST(SokkiaSdr, ASecondJobIsReadIntoTheSameSurveyAndSaysSo)
{
    const auto result = readText(file({header33(), "10NM" + f16("FIRST") + "121111", setupOnP1(),
                                       "10NM" + f16("SECOND") + "121111"}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->project.name, "FIRST");
    EXPECT_EQ(result->project.metadata.at("job at record 4"), "SECOND");
    EXPECT_TRUE(warned(*result, 4, "a second job ('SECOND') begins here"));
    EXPECT_EQ(result->recordsRead, 4u);
}

TEST(SokkiaSdr, WithRecordElevNoAPointIsGivenNoElevation)
{
    // [SDR] 3.6.2's second job flag, "Include elevation": 1 No, 2 Yes.
    const auto point = "08KI" + f16("A") + f16("5000.0") + f16("200.0") + f16("50.0") + f16("");
    const auto without = readText(file({header33(), "10NM" + f16("J") + "111111", point}));
    ASSERT_TRUE(without.ok()) << without.error().describe();
    const survey::SurveyPoint* a = topcon_test::point(without->project, "A");
    ASSERT_NE(a, nullptr);
    EXPECT_FALSE(a->elevation.has_value());
    EXPECT_EQ(without->project.metadata.at("job: elevations"), "not recorded");
    const auto with = readText(file({header33(), "10NM" + f16("J") + "121111", point}));
    ASSERT_TRUE(with.ok()) << with.error().describe();
    EXPECT_DOUBLE_EQ(valueOf(topcon_test::point(with->project, "A")->elevation), 50.0);
}

TEST(SokkiaSdr, TheNikonHeaderFormReadsTheRefractionOptionAsNikonsConstant)
{
    // Job flags 122211: curvature and refraction on, refraction option 1 -
    // 0.14 to Sokkia ([SDR] 3.5), 0.132 to Nikon (its manual, page 172).
    const auto job = "10NM" + f16("J") + "122211";
    const auto sokkia = readText(file({header33(), job, setupOnP1()}));
    ASSERT_TRUE(sokkia.ok()) << sokkia.error().describe();
    EXPECT_DOUBLE_EQ(valueOf(setupAt(sokkia->project, 0).instrument.refractionCoefficient), 0.14);
    const auto nikon = readText(file({headerNikon("113111"), job, setupOnP1()}));
    ASSERT_TRUE(nikon.ok()) << nikon.error().describe();
    EXPECT_DOUBLE_EQ(valueOf(setupAt(nikon->project, 0).instrument.refractionCoefficient),
                     0.132);
}

TEST(SokkiaSdr, CorrectionNotesOfAnSdr2xJobGiveItsSwitches)
{
    // SDR2x has no job flags; its corrections are 13CP notes ([SDR] chapter
    // 4: "Atmos crn: N").
    const auto result = readText(
        file({"00NM" + f16("SDR20 V03-05") + "0000" + f16("18-Jan-80 20:34") + "113111",
              "10NM" + f16("J"), "13CPAtmos crn: Y", "13CPC and R crn: N", "13CPSea level crn: N",
              "02NM0001" + f10("") + f10("") + f10("") + f10("1.5")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const auto& metadata = result->project.metadata;
    EXPECT_EQ(metadata.at("job: atmospheric correction"), "on");
    EXPECT_EQ(metadata.at("job: curvature and refraction correction"), "off");
    EXPECT_EQ(metadata.at("job: sea level correction"), "off");
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(*result, "(this job turns it on)"));
}

TEST(SokkiaSdr, AnUndefinedVerticalAngleOptionDropsTheZenithsAfterIt)
{
    const auto result = readText(file({header33(), instrument33('7'), setupOnP1(),
                                       "03NM" + f16("1.5"), backsight("P2", "0.0", "0.0"),
                                       shot("F1", "P2", "0.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 2, "vertical angle option is '7'"));
    const survey::SurveyStation& setup = setupAt(result->project, 0);
    EXPECT_TRUE(observationsTo<survey::ZenithAngleObservation>(setup, "P2").empty());
    EXPECT_EQ(observationsTo<survey::HorizontalDirectionObservation>(setup, "P2").size(), 1u);
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(setup, "P2").size(), 1u);
}

TEST(SokkiaSdr, EdmAndReflectorOffsetsAreWarnedAboutAndKept)
{
    const auto result = readText(file({header33(),
                                       "01NM:" + field("", 16) + "000000" + f16("HAND") +
                                           "000001" + "3" + "1" + f16("0.050") + f16("-0.020") +
                                           f16("0"),
                                       setupOnP1()}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 2, "EDM offset is 0.05 m, which is not applied"));
    EXPECT_TRUE(warned(*result, 2, "reflector offset is -0.02 m, which is not applied"));
    EXPECT_EQ(result->project.metadata.at("instrument: EDM offset (m)"), "0.05");
    EXPECT_EQ(result->project.metadata.at("instrument: reflector offset (m)"), "-0.02");
}

TEST(SokkiaSdr, NoInstrumentOrWeatherRecordIsSaidOnceAndInWhatTheFileDidNotCarry)
{
    const auto result =
        readText(file({header33(), setupOnP1(), "03NM" + f16("1.5"), backsight("P2", "0.0", "0.0"),
                       shot("F1", "P2", "0.0"), shot("F1", "P3", "10.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 5, "no instrument record comes before the first observation"));
    EXPECT_EQ(warningsWith(*result, "no instrument record"), 1u);
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(*result, "no instrument record"));
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(*result, "no pressure and temperature"));
}

// ---- Records that say nothing, or come too early -----------------------------------------

TEST(SokkiaSdr, EmptyWeatherScaleCollimationAndTargetRecordsAreSkipped)
{
    const auto result = readText(file({header33(), setupOnP1(), "05NM", "06NM",
                                       "06NM" + f16("0.0"), "04CL", "03NM"}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsSkipped, 5u);
    EXPECT_TRUE(warned(*result, 3, "neither a pressure nor a temperature"));
    EXPECT_TRUE(warned(*result, 4, "without a positive factor"));
    EXPECT_TRUE(warned(*result, 5, "without a positive factor"));
    EXPECT_TRUE(warned(*result, 6, "collimation record with no value"));
    EXPECT_TRUE(warned(*result, 7, "target height record with no height"));
}

TEST(SokkiaSdr, ACollimationRecordIsKeptWithItsSetup)
{
    // [SDR] 3.6.2 COL: vertical then horizontal collimation, angles; 10"
    // is 0.00277778 degrees. Kept, not applied: a face pair cancels it.
    const auto result = readText(file({header33(), setupOnP1(),
                                       "04CL" + f16("0.00277778") + f16("-0.00138889")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const auto& metadata = setupAt(result->project, 0).metadata;
    EXPECT_NEAR(valueOf(katana::core::parseFiniteDouble(
                    metadata.at("collimation, vertical (radians)"))),
                0.00277778 * kPi / 180.0, 1e-15);
    EXPECT_NEAR(valueOf(katana::core::parseFiniteDouble(
                    metadata.at("collimation, horizontal (radians)"))),
                -0.00138889 * kPi / 180.0, 1e-15);
}

TEST(SokkiaSdr, RecordsThatNameNoPointOrComeBeforeAnyStationAreSkipped)
{
    const auto result = readText(file({
        header33(), instrument33(),
        "07NM" + f16("P1") + f16("P2") + f16("0.0") + f16("0.0"),                    // 3
        "09F1" + f16("P1") + f16("P2") + f16("50.0") + f16("90.0") + f16("0.0"),     // 4
        "02NM" + f16("") + f16("") + f16("") + f16("") + f16("1.5"),                 // 5
        "08KI" + f16("") + f16("1.0") + f16("2.0"),                                  // 6
        "11KI" + f16("P1") + f16("") + f16("45.0"),                                  // 7
        setupOnP1(),                                                                 // 8
    }));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsSkipped, 5u);
    EXPECT_TRUE(warned(*result, 3, "backsight record with no station record before it"));
    EXPECT_TRUE(warned(*result, 4, "observation with no station record before it"));
    EXPECT_TRUE(warned(*result, 5, "station record names no point"));
    EXPECT_TRUE(warned(*result, 6, "coordinate record names no point"));
    EXPECT_TRUE(warned(*result, 7, "reduced observation record names no point"));
    EXPECT_EQ(result->project.stations.size(), 1u);
}

TEST(SokkiaSdr, ASetupWithNoInstrumentHeightUsesZeroAndSaysSo)
{
    const auto result = readText(file({header33(), "02NM" + f16("P1")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 2, "states no instrument height; 0 is used"));
    EXPECT_DOUBLE_EQ(setupAt(result->project, 0).setup.instrumentHeight, 0.0);
}

TEST(SokkiaSdr, CoordinatesAreEnteredObservedCalculatedOrUnknownByTheirDerivationCode)
{
    // [SDR] 3.4: KI keyed in, TP a topographical shot stored as a position,
    // AJ the traverse adjustment, TV the traverse, RS the resection.
    const auto point = [](std::string_view code, std::string_view id) {
        return "08" + std::string(code) + f16(id) + f16("5000.0") + f16("200.0") + f16("") +
               f16("");
    };
    const auto result = readText(file({header33(), point("KI", "A"), point("TP", "B"),
                                       point("AJ", "C"), point("TV", "D"), point("RS", "E"),
                                       point("NM", "F")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const struct {
        const char* id;
        survey::CoordinateSource source;
    } expected[] = {{"A", survey::CoordinateSource::Entered},
                    {"B", survey::CoordinateSource::FieldObserved},
                    {"C", survey::CoordinateSource::Calculated},
                    {"D", survey::CoordinateSource::Calculated},
                    {"E", survey::CoordinateSource::Calculated},
                    {"F", survey::CoordinateSource::Unknown}};
    for (const auto& e : expected) {
        const survey::SurveyPoint* p = topcon_test::point(result->project, e.id);
        ASSERT_NE(p, nullptr) << e.id;
        EXPECT_EQ(p->coordinateSource, e.source) << e.id;
    }
}

TEST(SokkiaSdr, APointIdHoldingAControlByteIsSkippedNotMergedWithAnother)
{
    // Line noise in an id field: "\x01" and "\x02" blanked would both be one
    // point named with a blank, and two shots 110 degrees apart would place
    // it at neither. Each record is skipped, naming the byte.
    const auto result = readText(file({
        header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
        backsight("P2", "0.0", "0.0"), shot("F1", "P2", "0.0"),
        "09F1" + f16("P1") + f16("\x01") + f16("10.0") + f16("90.0") + f16("10.0"),   // 7
        "09F1" + f16("P1") + f16("\x02") + f16("20.0") + f16("90.0") + f16("120.0"),  // 8
        "08KI" + f16("Q\x7f") + f16("1.0") + f16("2.0") + f16("") + f16(""),          // 9
        "02NM" + f16("\x01") + f16("") + f16("") + f16("") + f16("1.5"),              // 10
    }));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsSkipped, 4u);
    EXPECT_TRUE(warned(*result, 7, "control byte \\x01"));
    EXPECT_TRUE(warned(*result, 8, "control byte \\x02"));
    EXPECT_TRUE(warned(*result, 9, "control byte \\x7F"));
    EXPECT_TRUE(warned(*result, 10, "control byte \\x01"));
    EXPECT_EQ(result->project.stations.size(), 1u);
    EXPECT_TRUE(result->project.points.empty());
    // The setup's point and its backsight, and no point named with noise.
    std::set<std::string> named;
    for (const survey::UnpositionedPoint& p : result->project.unpositionedPoints) {
        named.insert(p.id);
    }
    EXPECT_EQ(named, (std::set<std::string>{"P1", "P2"}));
}

// ---- Observations ----------------------------------------------------------------------

TEST(SokkiaSdr, AZeroOrNegativeSlopeDistanceIsNoDistance)
{
    for (const auto& [distance, words] : {std::pair{"0.000", "slope distance of 0 read as no"},
                                          std::pair{"-5.0", "slope distance of -5 read as no"}}) {
        SCOPED_TRACE(distance);
        const auto result = readText(file({header33(), instrument33(), setupOnP1(),
                                           "03NM" + f16("1.5"), backsight("P2", "0.0", "0.0"),
                                           "09F1" + f16("P1") + f16("P2") + f16(distance) +
                                               f16("90.0") + f16("0.0")}));
        ASSERT_TRUE(result.ok()) << result.error().describe();
        EXPECT_TRUE(warned(*result, 6, words));
        const survey::SurveyStation& setup = setupAt(result->project, 0);
        EXPECT_TRUE(observationsTo<survey::DistanceObservation>(setup, "P2").empty());
        EXPECT_EQ(observationsTo<survey::ZenithAngleObservation>(setup, "P2").size(), 1u);
        EXPECT_EQ(observationsTo<survey::HorizontalDirectionObservation>(setup, "P2").size(), 1u);
    }
}

TEST(SokkiaSdr, AVerticalReadingOutsideAFullCircleIsNotImported)
{
    for (const char* reading : {"400.0", "-10.0"}) {
        SCOPED_TRACE(reading);
        const auto result = readText(file({header33(), instrument33(), setupOnP1(),
                                           "03NM" + f16("1.5"), backsight("P2", "0.0", "0.0"),
                                           shot("F1", "P2", "0.0", reading)}));
        ASSERT_TRUE(result.ok()) << result.error().describe();
        EXPECT_TRUE(warned(*result, 6, "vertical reading outside a full circle"));
        const survey::SurveyStation& setup = setupAt(result->project, 0);
        EXPECT_TRUE(observationsTo<survey::ZenithAngleObservation>(setup, "P2").empty());
        EXPECT_EQ(observationsTo<survey::DistanceObservation>(setup, "P2").size(), 1u);
    }
}

TEST(SokkiaSdr, AnObservationOrAReducedOneWithNoValueIsSkippedAndNamesNothing)
{
    const auto result = readText(file({header33(), instrument33(), setupOnP1(),
                                       "03NM" + f16("1.5"), "09F1" + f16("P1") + f16("P2"),
                                       "11KI" + f16("P1") + f16("P3")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsSkipped, 2u);
    EXPECT_TRUE(warned(*result, 5, "observation with no readable value"));
    EXPECT_TRUE(warned(*result, 6, "reduced observation record with no value"));
    // A record that is skipped names no point.
    EXPECT_EQ(topcon_test::unpositioned(result->project, "P2"), nullptr);
    EXPECT_EQ(topcon_test::unpositioned(result->project, "P3"), nullptr);
}

TEST(SokkiaSdr, ADerivedViewInASetupWithNoRawObservationSaysTheShotIsLost)
{
    // An SDR2x set collection from V04-02 on stores only MC records ([SDR]
    // chapter 2); writers disagree about what an MC holds, so it is not
    // imported - and the read says the shots are lost, not that they were
    // counted already. After a raw observation of the same setup an MC is
    // the raw one again.
    const auto result = readText(file({
        "00NM" + f16("SDR20 V03-05") + "0000" + f16("18-Jan-80 20:34") + "113111",
        "01NM1" + field("", 16) + "000000" + f16("DT2") + "000031" + "3" + "1" + f10("") + f10("") +
            f10("0.000"),
        "02KI0001" + f10("5100.000") + f10("200.000") + f10("50.000") + f10("1.000"),
        "12SC0001002",
        "09MC00010002" + f10("42.500") + f10("91.50000") + f10("14.00000"),        // 5
        "11TP00010003" + f10("14.00000") + f10("42.480") + f10("-1.112"),          // 6
        "09F100010002" + f10("42.500") + f10("91.50000") + f10("0.00000"),         // 7
        "09MC00010002" + f10("42.500") + f10("91.50000") + f10("14.00000"),        // 8
        "11TP00010003" + f10("14.00000") + f10("42.480") + f10("-1.112"),          // 9
    }));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsSkipped, 4u);
    EXPECT_TRUE(warned(*result, 5, "in a setup with no raw observation is not imported"));
    EXPECT_TRUE(warned(*result, 5, "so this shot is lost"));
    EXPECT_TRUE(warned(*result, 6, "in a setup with no raw observation is not imported"));
    EXPECT_TRUE(warned(*result, 8, "since it would count them twice"));
    EXPECT_TRUE(warned(*result, 9, "since it would count them twice"));
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(
        *result, "1 corrected (MC) observation(s) in setups with no raw observation"));
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(
        *result, "1 reduced (RED) observation(s) with distances in setups with no raw"));
}

// ---- Setups, rounds, sets and backsights ---------------------------------------------------

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
    // the same instrument height: azimuth 45, circle 10 on P3.
    EXPECT_EQ(setupAt(project, 1).setup.id, "P1 (2)");
    EXPECT_EQ(setupAt(project, 1).setup.pointId, "P1");
    EXPECT_DOUBLE_EQ(setupAt(project, 1).setup.instrumentHeight, 1.5);
    EXPECT_EQ(setupAt(project, 1).backsightPointId, "P3");
    EXPECT_NEAR(valueOf(setupAt(project, 1).statedBacksightAzimuth), degrees(45.0),
                kAngleTolerance);
    EXPECT_NEAR(valueOf(setupAt(project, 1).backsightAzimuth), degrees(10.0), kAngleTolerance);
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
    EXPECT_NEAR(valueOf(setupAt(result->project, 0).statedBacksightAzimuth), degrees(90.0),
                kAngleTolerance);
    EXPECT_NEAR(valueOf(setupAt(result->project, 0).backsightAzimuth), 0.0, kAngleTolerance);
    EXPECT_TRUE(warned(*result, 6, "before any observation on the first (record 5) replaces it"));
}

TEST(SokkiaSdr, ABacksightRecordClosingItsSetOrientsThatSet)
{
    // Sokkia's Set Collection writes each set's back-bearing (07 SC) after
    // the set ([SDR] chapter 2). Set 2 is observed with the circle moved 90
    // degrees; its own 07 says so, and its observations go with it into a
    // setup of their own. From CP1 (N 5000000, E 500000) oriented on CP2
    // due north, T1 is 50 m due east in both sets: at circle 90 in set 1
    // (CP2 at 0) and at 180 in set 2 (CP2 at 90).
    const auto obs = [](std::string_view face, std::string_view to, std::string_view distance,
                        std::string_view zenith, std::string_view circle) {
        return "09" + std::string(face) + f16("CP1") + f16(to) + f16(distance) + f16(zenith) +
               f16(circle);
    };
    const auto set = [](std::string_view number) {
        return "12SC" + f16("CP1") + "  4" + std::string(number) + "111";
    };
    const auto closing = [](std::string_view circle) {
        return "07SC" + f16("CP1") + f16("CP2") + f16("0.0") + f16(circle);
    };
    const auto result = readText(file({
        header33(), instrument33(),
        "08KI" + f16("CP2") + f16("5000100.000") + f16("500000.000") + f16("10.000") + f16(""),
        "02KI" + f16("CP1") + f16("5000000.000") + f16("500000.000") + f16("10.000") + f16("1.500"),
        "03NM" + f16("1.500"),                                                         // 5
        set("  1"),                                                                    // 6
        obs("F1", "CP2", "100.000", "90.0", "0.0"), obs("F1", "T1", "50.000", "90.0", "90.0"),
        obs("F2", "T1", "50.000", "270.0", "270.0"), obs("F2", "CP2", "100.000", "270.0", "180.0"),
        closing("0.0"),                                                                // 11
        set("  2"),                                                                    // 12
        obs("F1", "CP2", "100.000", "90.0", "90.0"), obs("F1", "T1", "50.000", "90.0", "180.0"),
        obs("F2", "T1", "50.000", "270.0", "0.0"), obs("F2", "CP2", "100.000", "270.0", "270.0"),
        closing("90.0"),                                                               // 17
    }));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyProject& project = result->project;
    ASSERT_EQ(project.stations.size(), 2u);
    EXPECT_EQ(setupAt(project, 0).observations.size(), 12u);
    const survey::SurveyStation& second = setupAt(project, 1);
    EXPECT_EQ(second.setup.id, "CP1 (2)");
    EXPECT_EQ(second.source.recordNumber, 17u);
    EXPECT_EQ(second.observations.size(), 12u);
    EXPECT_NEAR(valueOf(second.backsightAzimuth), degrees(90.0), kAngleTolerance);
    EXPECT_EQ(second.metadata.at("set at record 12"), "number 2, 4 observation(s)");
    // Its four records are its pointings 1 to 4, in order.
    std::set<std::size_t> pointings;
    for (const auto& direction :
         observationsTo<survey::HorizontalDirectionObservation>(second, "T1")) {
        pointings.insert(direction.pointing.index);
    }
    for (const auto& direction :
         observationsTo<survey::HorizontalDirectionObservation>(second, "CP2")) {
        pointings.insert(direction.pointing.index);
    }
    EXPECT_EQ(pointings, (std::set<std::size_t>{1, 2, 3, 4}));
    EXPECT_EQ(warningsWith(*result, "has no observation of its backsight"), 0u);
    const auto outcome = reduced(project);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const survey::ComputedPoint* t1 = computed(*outcome, "T1");
    ASSERT_NE(t1, nullptr);
    EXPECT_NEAR(t1->northing, 5000000.0, 1e-6);
    EXPECT_NEAR(t1->easting, 500050.0, 1e-6);
}

TEST(SokkiaSdr, ABadSetEndsAfterItsCountOfObservations)
{
    // [SDR] 3.6.2 SET: source point, count, set number, bad marker (2 is
    // "Bad set", 3.5), return sight, prompt order. Its two observations are
    // not used; the shot after them is.
    const auto withCount = readText(file({header33(), instrument33(), setupOnP1(),
                                          "03NM" + f16("1.5"), backsight("P2", "0.0", "0.0"),
                                          "12SC" + f16("P1") + "002" + "001" + "2" + "1" + "1",
                                          shot("F1", "P2", "0.0"), shot("F2", "P2", "180.0"),
                                          shot("F1", "P3", "10.0")}));
    ASSERT_TRUE(withCount.ok()) << withCount.error().describe();
    EXPECT_EQ(withCount->recordsSkipped, 2u);
    EXPECT_TRUE(warned(*withCount, 6, "set 001 at 'P1' is marked bad; its 002 observation(s)"));
    EXPECT_EQ(observationsTo<survey::DistanceObservation>(setupAt(withCount->project, 0), "P3")
                  .size(),
              1u);
    // With no count, the set runs to the next set or setup.
    const auto withoutCount = readText(file({header33(), instrument33(), setupOnP1(),
                                             "03NM" + f16("1.5"), backsight("P2", "0.0", "0.0"),
                                             "12SC" + f16("P1") + "   " + "001" + "2" + "1" + "1",
                                             shot("F1", "P2", "0.0"), shot("F2", "P2", "180.0"),
                                             shot("F1", "P3", "10.0")}));
    ASSERT_TRUE(withoutCount.ok()) << withoutCount.error().describe();
    EXPECT_EQ(withoutCount->recordsSkipped, 3u);
    EXPECT_TRUE(warned(*withoutCount, 6, "up to the next set or setup"));
    // A bad set that counts none takes none of the shots after it.
    const auto ofNone = readText(file({header33(), instrument33(), setupOnP1(),
                                       "03NM" + f16("1.5"), backsight("P2", "0.0", "0.0"),
                                       "12SC" + f16("P1") + "000" + "001" + "2" + "1" + "1",
                                       shot("F1", "P2", "0.0"), shot("F2", "P2", "180.0")}));
    ASSERT_TRUE(ofNone.ok()) << ofNone.error().describe();
    EXPECT_EQ(ofNone->recordsSkipped, 0u);
    EXPECT_TRUE(warned(*ofNone, 6, "is marked bad; its 000 observation(s)"));
}

TEST(SokkiaSdr, TheSdr2xSetLayoutHasNoSetNumberAndNoBadMarker)
{
    // [SDR] 3.6.1 SET: 5-8 the point, 9-11 the count, and nothing more.
    const auto result = readText(file({
        "00NM" + f16("SDR20 V03-05") + "0000" + f16("18-Jan-80 20:34") + "113111",
        "02NM0001" + f10("") + f10("") + f10("") + f10("1.5"), "12SC0001004"}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(setupAt(result->project, 0).metadata.at("set at record 3"), "004 observation(s)");
    EXPECT_EQ(warningsWith(*result, "marked bad"), 0u);
}

TEST(SokkiaSdr, ABadSetsObservationsAreSkippedAndAGoodSetsRead)
{
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

TEST(SokkiaSdr, ASetupThatNeverObservesItsBacksightIsOrientedByItsAzimuthLessItsCircle)
{
    // [SETX] 29.2.6: A = H + BKB azimuth - BKB h.obs. The 07 gives RO's
    // azimuth 90 and the circle 0 on it; RO is never observed. T1 at circle
    // 0, 100 m level, is azimuth 90: 100 m due east of CP1.
    const auto result = readText(file({
        header33(), instrument33(),
        "02KI" + f16("P1") + f16("5000000.000") + f16("500000.000") + f16("10.000") + f16("1.500"),
        "03NM" + f16("1.500"), backsight("RO", "90.0", "0.0"),
        "09F1" + f16("P1") + f16("T1") + f16("100.000") + f16("90.0") + f16("0.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 5, "orients it by the backsight record's azimuth less its "
                                   "circle reading"));
    const auto outcome = reduced(result->project);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const survey::ComputedPoint* t1 = computed(*outcome, "T1");
    ASSERT_NE(t1, nullptr);
    EXPECT_NEAR(t1->northing, 5000000.0, 1e-6);
    EXPECT_NEAR(t1->easting, 500100.0, 1e-6);
}

TEST(SokkiaSdr, AStatedAzimuthOrientsASetupWhoseCircleWasZeroedOnItsBacksight)
{
    // 07: azimuth 123 to RO, circle 0 on it; RO observed at 0, T at circle
    // 90, 50 m level. With nothing placing RO, T is at azimuth 123 + 90 - 0
    // = 213: N = 5000000 + 50 cos 213 = 4999958.0664716,
    // E = 500000 + 50 sin 213 = 499972.7680482.
    const auto result = readText(file({
        header33(), instrument33(),
        "02KI" + f16("P1") + f16("5000000.000") + f16("500000.000") + f16("10.000") + f16("1.500"),
        "03NM" + f16("1.500"), backsight("RO", "123.0", "0.0"),
        "09F1" + f16("P1") + f16("RO") + f16("80.000") + f16("90.0") + f16("0.0"),
        "09F1" + f16("P1") + f16("T") + f16("50.000") + f16("90.0") + f16("90.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyStation& setup = setupAt(result->project, 0);
    EXPECT_NEAR(valueOf(setup.statedBacksightAzimuth), degrees(123.0), kAngleTolerance);
    EXPECT_NEAR(valueOf(setup.backsightAzimuth), 0.0, kAngleTolerance);
    const auto outcome = reduced(result->project);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const survey::ComputedPoint* t = computed(*outcome, "T");
    ASSERT_NE(t, nullptr);
    EXPECT_NEAR(t->northing, 4999958.0664716, 1e-6);
    EXPECT_NEAR(t->easting, 499972.7680482, 1e-6);
}

TEST(SokkiaSdr, ASetupWithNoBacksightRecordIsOrientedByItsKeyedAzimuth)
{
    // Sokkia's own example, [SDR] chapter 2, "The same job with observations
    // stored in OBS view", written in the SDR2x records of 3.6.1: the setup
    // has no back-bearing record, only an azimuth keyed to 0101 (23-56'15")
    // and an observation of 0101 at 0-00'00". Its printed positions, the
    // same job in POS view: 1000 at N 745427.696 E 415468.380 Elev 27.702,
    // 1001 (the average of its two faces) at N 745215.777 E 415357.357 Elev
    // 22.656 - feet, printed to 0.001, so each within half of that (1000's
    // northing works out 0.00049 ft from its printed value). Angles are in
    // decimal degrees here as the file holds them: 88-54'28" is 88.9077778.
    const auto result = readText(file({
        "00NM" + f16("SDR20 V03-05") + "0000" + f16("06-Jan-80 00:00") + "122211",
        "10NM" + f16("SIMPLE OBS TOPO"),
        "13CPSea level crn: N",
        "13CPAtmos crn: N",
        "06NM" + f10("1.00000000"),
        "13TS05-Jan-80 23:48",
        "01NM1" + field("", 16) + "000000" + field("", 16) + "000000" + "3" + "1" + f10("") +
            f10("") + f10("0.000"),
        "02TP0100" + f10("745256.356") + f10("415265.958") + f10("23.256") + f10("5.230") +
            f16("OIT II"),
        "11KI01000101" + f10("23.9375000") + f10("") + f10("") + f16("BS AZ"), // 9
        "03NM" + f10("5.840"),
        "09F101000101" + f10("") + f10("") + f10("0.00000000") + f16("BS"),
        "09F101001000" + f10("265.250") + f10("88.9077778") + f10("25.8163889") + f16("TREE"),
        "09F101001001" + f10("100.000") + f10("90.0000000") + f10("90.0000000") +
            f16("BLDG CNR F1"),
        "09F201001001" + f10("100.005") + f10("270.011111") + f10("270.005556") +
            f16("BLDG CNR F2"),
        "09MC01001001" + f10("100.004") + f10("90.3438889") + f10("113.940278") +
            f16("BLDG CNR AVG"),
    }));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyStation& setup = setupAt(result->project, 0);
    EXPECT_EQ(setup.backsightPointId, "0101");
    EXPECT_NEAR(valueOf(setup.statedBacksightAzimuth), degrees(23.9375), kAngleTolerance);
    EXPECT_FALSE(setup.backsightAzimuth.has_value());
    EXPECT_EQ(setup.metadata.at("oriented by"), "the azimuth to '0101' keyed at record 9");
    EXPECT_TRUE(warned(*result, 15, "since it would count them twice"));
    const auto outcome = reduced(result->project);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    constexpr double kFoot = 0.3048;
    constexpr double kHalfPrintedUnit = 0.0005 * kFoot + 1e-9;
    const struct {
        const char* id;
        double northing;
        double easting;
        double elevation;
    } printed[] = {{"1000", 745427.696, 415468.380, 27.702},
                   {"1001", 745215.777, 415357.357, 22.656}};
    for (const auto& p : printed) {
        SCOPED_TRACE(p.id);
        const survey::ComputedPoint* point = computed(*outcome, p.id);
        ASSERT_NE(point, nullptr);
        EXPECT_NEAR(point->northing, p.northing * kFoot, kHalfPrintedUnit);
        EXPECT_NEAR(point->easting, p.easting * kFoot, kHalfPrintedUnit);
        EXPECT_NEAR(valueOf(point->elevation), p.elevation * kFoot, kHalfPrintedUnit);
    }
}

TEST(SokkiaSdr, ASetupWithNeitherABacksightNorAKeyedAzimuthTakesItsReadingsAsAzimuths)
{
    // [SETX] 8.2.1: with the backsight skipped, "Horizontal angles stored
    // are treated as azimuths". T at circle 90, 50 m level: due east.
    const auto result = readText(file({
        header33(), instrument33(),
        "02KI" + f16("P1") + f16("5000000.000") + f16("500000.000") + f16("10.000") + f16("1.500"),
        "03NM" + f16("1.500"),
        "09F1" + f16("P1") + f16("T") + f16("50.000") + f16("90.0") + f16("90.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 3, "its horizontal readings are taken as azimuths"));
    const survey::SurveyStation& setup = setupAt(result->project, 0);
    EXPECT_TRUE(setup.backsightPointId.empty());
    EXPECT_NEAR(valueOf(setup.backsightAzimuth), 0.0, kAngleTolerance);
    const auto outcome = reduced(result->project);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const survey::ComputedPoint* t = computed(*outcome, "T");
    ASSERT_NE(t, nullptr);
    EXPECT_NEAR(t->northing, 5000000.0, 1e-6);
    EXPECT_NEAR(t->easting, 500050.0, 1e-6);
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

TEST(SokkiaSdr, EveryChangeInTheMiddleOfASetupIsReported)
{
    // An instrument record (a new prism, -30 mm) and then the weather, both
    // between two shots of one setup: each is said, not only the last.
    const auto result = readText(file({
        header33(), instrument33(), setupOnP1(), "03NM" + f16("1.5"),
        backsight("P2", "0.0", "0.0"), shot("F1", "P2", "0.0"),
        "01NM:" + field("", 16) + "000000" + f16("HAND") + "000001" + "3" + "1" + f16("") +
            f16("") + f16("-30"),                                                      // 7
        "05NM" + f16("990.0") + f16("15.0"),                                           // 8
        shot("F1", "P3", "10.0")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 7, "the instrument changes in the middle of setup 'P1'"));
    EXPECT_TRUE(warned(*result, 8, "the weather changes in the middle of setup 'P1'"));
    const auto& metadata = setupAt(result->project, 0).metadata;
    EXPECT_EQ(metadata.at("the instrument changed at record 7"), "not applied to this setup");
    EXPECT_EQ(metadata.at("the weather changed at record 8"), "not applied to this setup");
}

// ---- Faces and vertical readings -------------------------------------------------------------

TEST(SokkiaSdr, HorizonVerticalReadingsBecomeZenithAngles)
{
    // Vertical angle option 2, "measured upwards from horizontal" ([SDR]
    // 3.5): +3 degrees is the zenith 87 on face 1; 177 is the same line on
    // face 2 (its zenith reading 360 - 87 = 273, less 90 ... 90 - 273 =
    // -183, which is 177).
    const auto result = readText(file({header33(), instrument33('2'), setupOnP1(),
                                       "03NM" + f16("1.5"), backsight("P2", "0.0", "10.0"),
                                       shot("F1", "P2", "10.0", "3.0"),
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
    EXPECT_TRUE(warned(*result, 9, "'NOT A RECORD' is not a record"));
    EXPECT_EQ(setupAt(result->project, 0).observations.size(), 3u);
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
    const struct {
        std::string text;
        const char* words;
    } cases[] = {
        {std::string{}, "holds no SDR header record"},
        {std::string("\n\n"), "holds no SDR header record"},
        {file({"10NM" + f16("J")}), "does not begin with an SDR header record"},
        {file({header33()}), "holds no setup, observation or point"},
        {file({header33(), "10NM" + f16("J"), "13NMA NOTE"}),
         "holds no setup, observation or point"},
    };
    for (const auto& c : cases) {
        SCOPED_TRACE(c.text);
        const auto result = readText(c.text);
        ASSERT_FALSE(result.ok());
        EXPECT_EQ(result.error().code, ErrorCode::FileImportFailure);
        EXPECT_NE(result.error().message.find(c.words), kNone) << result.error().message;
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

TEST(SokkiaSdr, ADosEndOfFileMarkIsFramingAndOtherControlBytesAreShownInTheirWarning)
{
    // A file copied through MS-DOS ends in Ctrl-Z (0x1A): not a record, and
    // no warning. A line of NULs is not a record either, and its warning
    // shows the bytes rather than blanks nobody can tell from a space.
    const auto ctrlZ = readText(file({header33(), setupOnP1()}) + "\x1a");
    ASSERT_TRUE(ctrlZ.ok()) << ctrlZ.error().describe();
    EXPECT_EQ(ctrlZ->recordsRead, 2u);
    EXPECT_EQ(ctrlZ->recordsSkipped, 0u);
    EXPECT_EQ(ctrlZ->warnings.size(), 0u);
    const auto nul = readText(file({header33(), setupOnP1(), std::string(4, '\0')}));
    ASSERT_TRUE(nul.ok()) << nul.error().describe();
    EXPECT_EQ(nul->recordsSkipped, 1u);
    EXPECT_TRUE(warned(*nul, 3, "'\\x00\\x00\\x00\\x00' is not a record"));
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
                  {"Time Date 12/31/1999 Time 23:59:58", {1999, 12, 31, 23, 59, 58.0, ""}},
                  // Leap days: 2024 is divisible by 4, 2000 by 400.
                  {"Time Date 02/29/2024 Time 10:00:00", {2024, 2, 29, 10, 0, 0.0, ""}},
                  {"29-Feb-00 10:00", {2000, 2, 29, 10, 0, 0.0, ""}}};
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

TEST(SokkiaSdr, ADayThatIsNotInItsMonthDatesNothing)
{
    // February has no 31st, and no 29th in 2023 or 1900 (divisible by 100,
    // not by 400): each is kept as a note and dates no setup.
    for (const char* text : {"Time Date 02/31/2022 Time 10:00:00", "31-Feb-99 10:00",
                             "Feb-31-99 10:00", "Time Date 02/29/2023 Time 10:00:00",
                             "Time Date 02/29/1900 Time 10:00:00", "31-Apr-21 08:00"}) {
        SCOPED_TRACE(text);
        const auto result = readText(file({header33(), setupOnP1(), "13TS" + std::string(text)}));
        ASSERT_TRUE(result.ok()) << result.error().describe();
        EXPECT_TRUE(warned(*result, 3, "is not a date and time"));
        EXPECT_FALSE(setupAt(result->project, 0).instrument.time.known());
    }
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

TEST(SokkiaSdr, AFileThatIsNotUtf8IsReadAsItsGuessedEncodingAndSaysSo)
{
    // 0xE9 alone is not UTF-8: the file is decoded as a single-byte code
    // page, and the read says it guessed.
    const auto result = readText(file({header33(), "08KI" + std::string("P\xe9") +
                                                       std::string(14, ' ') + f16("5000.0") +
                                                       f16("200.0") + f16("")}));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(warned(*result, 0, "the file is not UTF-8"));
    const survey::SurveyPoint* point = topcon_test::point(result->project, "P\xc3\xa9");
    ASSERT_NE(point, nullptr);
    EXPECT_DOUBLE_EQ(point->northing, 5000.0);
}

TEST(SokkiaSdr, ShortRecordsReadTheirFieldsAndNothingPastTheirEnd)
{
    // [SDR] chapter 5: "trailing blanks may be truncated". The 02 below
    // ends after its instrument height, the 03 after three characters.
    const auto result = readText(file({header33(), instrument33(), "02NM" + f16("P1") + f16("") +
                                                                       f16("") + f16("") + "1.5",
                                       "03NM1.6", backsight("P2", "0.0", "0.0"),
                                       shot("F1", "P2", "0.0")}));
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
