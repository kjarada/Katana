// Leica GSI-8 / GSI-16 (src/katana_surveyio/leica_gsi.cpp).
//
// The fixtures in data/leica/ were built by hand from Leica's "GSI ONLINE for
// Leica TPS and DNA" (November 2003): every expected value below is worked
// from the word as written in the fixture and the unit digit's rule (pages
// 5-6), in the comment beside it - never from what the reader returned.

#include <gtest/gtest.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <numbers>
#include <random>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/leica.hpp"
#include "katana/surveyio/reader.hpp"

using namespace katana::surveyio;
namespace survey = katana::survey;
using katana::core::ErrorCode;
using katana::core::Result;

namespace {

constexpr double kPi = std::numbers::pi;
constexpr double kGon = kPi / 200.0;
constexpr double kDegree = kPi / 180.0;
// The fixtures' millimetres come through a division by 1000, so they are the
// nearest double to the decimal - EXPECT_DOUBLE_EQ is the right comparison.
// Angles pass through a product with pi, a few units in the last place.
constexpr double kAngleTolerance = 1e-14;

std::filesystem::path dataFolder()
{
    return std::filesystem::path(__FILE__).parent_path() / "data";
}

std::filesystem::path leicaFolder()
{
    return dataFolder() / "leica";
}

std::string slurp(const std::filesystem::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

Result<ReadResult> readText(std::string_view bytes, std::string_view name = "job.gsi",
                            const ReadOptions& options = {})
{
    return readSurvey(formatRegistry(), kLeicaGsiFormatId, bytes, name, options);
}

ReadResult readFixture(const std::string& name)
{
    const std::string bytes = slurp(leicaFolder() / name);
    EXPECT_FALSE(bytes.empty()) << "fixture " << name << " is missing";
    Result<ReadResult> read = readText(bytes, name);
    EXPECT_TRUE(read.ok()) << (read.ok() ? "" : read.error().message);
    return read.ok() ? std::move(read).value() : ReadResult{};
}

const survey::SurveyPoint* point(const survey::SurveyProject& project, std::string_view id)
{
    for (const survey::SurveyPoint& p : project.points) {
        if (p.id == id) {
            return &p;
        }
    }
    return nullptr;
}

const survey::UnpositionedPoint* unpositioned(const survey::SurveyProject& project,
                                              std::string_view id)
{
    for (const survey::UnpositionedPoint& p : project.unpositionedPoints) {
        if (p.id == id) {
            return &p;
        }
    }
    return nullptr;
}

template <class T>
std::vector<const T*> all(const survey::SurveyStation& station)
{
    std::vector<const T*> found;
    for (const survey::Observation& observation : station.observations) {
        if (const T* typed = std::get_if<T>(&observation)) {
            found.push_back(typed);
        }
    }
    return found;
}

template <class T>
std::vector<const T*> toTarget(const survey::SurveyStation& station, std::string_view target)
{
    std::vector<const T*> found;
    for (const T* observation : all<T>(station)) {
        if (observation->to == target) {
            found.push_back(observation);
        }
    }
    return found;
}

// The warning about `record` whose message contains `text`, or nullptr.
const ReadWarning* warningAbout(const ReadResult& result, std::size_t record,
                                std::string_view text)
{
    for (const ReadWarning& warning : result.warnings) {
        if (warning.record == record && warning.message.find(text) != std::string::npos) {
            return &warning;
        }
    }
    return nullptr;
}

bool says(const std::vector<std::string>& lines, std::string_view text)
{
    return std::ranges::any_of(
        lines, [text](const std::string& line) { return line.find(text) != std::string::npos; });
}

std::string allWarnings(const ReadResult& result)
{
    std::string text;
    for (const ReadWarning& warning : result.warnings) {
        text += describe(warning) + "\n";
    }
    return text;
}

} // namespace

// ---- The format ------------------------------------------------------------------

TEST(LeicaGsi, TheFormatIsRegisteredWithAReaderAndSaysWhatItCanRead)
{
    Result<FormatDescriptor> format = formatRegistry().find(kLeicaGsiFormatId);
    ASSERT_TRUE(format.ok());
    EXPECT_EQ(format->manufacturer, Manufacturer::Leica);
    EXPECT_EQ(format->parserVersion, "1.0");
    EXPECT_EQ(format->extensions, std::vector<std::string>{"gsi"});
    EXPECT_TRUE(format->reads.points);
    EXPECT_TRUE(format->reads.observations);
    EXPECT_TRUE(format->reads.stations);
    EXPECT_TRUE(format->reads.features);
    EXPECT_TRUE(format->reads.instrumentSettings);
    EXPECT_FALSE(format->reads.coordinateSystem); // GSI declares none
    EXPECT_FALSE(format->reads.gnss);
    EXPECT_NE(formatRegistry().reader(kLeicaGsiFormatId), nullptr);
}

// ---- A GSI-8 job with two setups (data/leica/tps_gsi8.gsi) ------------------------

TEST(LeicaGsi, AStationBlockFollowedByMeasurementsIsOneSetupWithItsInstrumentHeight)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    const survey::SurveyProject& project = read.project;
    ASSERT_EQ(project.stations.size(), 2U);
    // Record 1: "88..10+00001550" - unit 0, last digit millimetres: 1.550 m.
    EXPECT_EQ(project.stations[0].setup.id, "STN1");
    EXPECT_EQ(project.stations[0].setup.pointId, "STN1");
    EXPECT_DOUBLE_EQ(project.stations[0].setup.instrumentHeight, 1.550);
    EXPECT_EQ(project.stations[0].source.recordNumber, 1U);
    // Record 9: "88..10+00001600" = 1.600 m.
    EXPECT_EQ(project.stations[1].setup.pointId, "STN2");
    EXPECT_DOUBLE_EQ(project.stations[1].setup.instrumentHeight, 1.600);
    // GSI marks no backsight. Setup 1's first shot (record 2) is to BS, which
    // the file has given no coordinates: no backsight is named. Setup 2's
    // first shot (record 10) is to STN1, positioned by record 1: that is its
    // backsight. No circle setting is recorded for either.
    EXPECT_TRUE(project.stations[0].backsightPointId.empty());
    EXPECT_EQ(project.stations[1].backsightPointId, "STN1");
    EXPECT_FALSE(project.stations[0].backsightAzimuth.has_value());
    EXPECT_FALSE(project.stations[1].backsightAzimuth.has_value());
    EXPECT_TRUE(says(read.notCarried, "1 of 2 setups took the point of their first shot as the "
                                      "backsight"));
    EXPECT_EQ(read.recordsRead, 12U);
    EXPECT_EQ(read.recordsSkipped, 0U);
}

TEST(LeicaGsi, AStationsCoordinatesAreKeyedInWhenItsWordsSaySo)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    const survey::SurveyPoint* station = point(read.project, "STN1");
    ASSERT_NE(station, nullptr);
    // "84..10+00100000": input mode 1 (keyed in), unit 0: 100000 mm = 100.000 m.
    EXPECT_DOUBLE_EQ(station->easting, 100.0);
    EXPECT_DOUBLE_EQ(station->northing, 200.0);
    ASSERT_TRUE(station->elevation.has_value());
    EXPECT_DOUBLE_EQ(*station->elevation, 50.0);
    EXPECT_EQ(station->coordinateSource, survey::CoordinateSource::Entered);
    EXPECT_EQ(read.project.units.linear, survey::LinearUnit::Metres);
    EXPECT_EQ(read.project.units.angular, survey::AngularUnit::Gons);
}

TEST(LeicaGsi, BothFacesOfOneTargetAreTwoPointingsAndFaceRightZenithIsTheFaceLeftEquivalent)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    const survey::SurveyStation& setup = read.project.stations[0];
    const auto directions = toTarget<survey::HorizontalDirectionObservation>(setup, "BS");
    const auto zeniths = toTarget<survey::ZenithAngleObservation>(setup, "BS");
    const auto distances = toTarget<survey::DistanceObservation>(setup, "BS");
    ASSERT_EQ(directions.size(), 2U);
    ASSERT_EQ(zeniths.size(), 2U);
    ASSERT_EQ(distances.size(), 2U);

    // Record 2: Hz "21.102+00000000" = 0 gon; V "22.102+10000000" = 100.00000
    // gon, under 200 gon: face left, zenith 100 gon = pi/2.
    EXPECT_EQ(directions[0]->pointing.face, survey::Face::Left);
    EXPECT_NEAR(directions[0]->direction, 0.0, kAngleTolerance);
    EXPECT_NEAR(zeniths[0]->angle, 100.0 * kGon, kAngleTolerance);
    // Record 3: Hz 200 gon = pi, the raw circle reading; V 300 gon is over
    // 200 gon: face right, and the zenith angle is 400 - 300 = 100 gon.
    EXPECT_EQ(directions[1]->pointing.face, survey::Face::Right);
    EXPECT_NEAR(directions[1]->direction, 200.0 * kGon, kAngleTolerance);
    EXPECT_NEAR(zeniths[1]->angle, 100.0 * kGon, kAngleTolerance);
    EXPECT_EQ(zeniths[1]->pointing.face, survey::Face::Right);
    // The three values of one block share one pointing; the two blocks differ.
    EXPECT_EQ(directions[0]->pointing, zeniths[0]->pointing);
    EXPECT_EQ(directions[0]->pointing, distances[0]->pointing);
    EXPECT_EQ(directions[1]->pointing, distances[1]->pointing);
    EXPECT_NE(directions[0]->pointing.index, 0U);
    EXPECT_NE(directions[0]->pointing.index, directions[1]->pointing.index);
    // "31..00+00100000" = 100.000 m; "31..00+00100002" = 100.002 m.
    EXPECT_EQ(distances[0]->kind, survey::DistanceKind::Slope);
    EXPECT_DOUBLE_EQ(distances[0]->distance, 100.000);
    EXPECT_DOUBLE_EQ(distances[1]->distance, 100.002);
}

TEST(LeicaGsi, AnObservationWithNoStatedPrecisionTakesTheDefaultOnes)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    const survey::SurveyStation& setup = read.project.stations[0];
    const survey::ObservationPrecision defaults{};
    const auto directions = toTarget<survey::HorizontalDirectionObservation>(setup, "BS");
    const auto distances = toTarget<survey::DistanceObservation>(setup, "BS");
    ASSERT_FALSE(directions.empty());
    ASSERT_FALSE(distances.empty());
    EXPECT_DOUBLE_EQ(directions[0]->sigma, defaults.direction);
    // 2 mm + 2 ppm over 100 m: sqrt(0.002^2 + (2e-6 * 100)^2) = 0.00201 m.
    EXPECT_NEAR(distances[0]->sigma, std::sqrt(0.002 * 0.002 + 0.0002 * 0.0002), 1e-15);
}

TEST(LeicaGsi, HeightsGoOnEveryShotAndAReflectorHeightHoldsUntilTheNextOne)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    const survey::SurveyStation& first = read.project.stations[0];
    const survey::SurveyStation& second = read.project.stations[1];
    // Record 2 sets hr "87..10+00001700" = 1.700 m; record 5 (point 101)
    // states none, so it is still 1.700.
    const auto to101 = toTarget<survey::DistanceObservation>(first, "101");
    ASSERT_EQ(to101.size(), 1U);
    EXPECT_DOUBLE_EQ(to101[0]->targetHeight, 1.700);
    EXPECT_DOUBLE_EQ(to101[0]->instrumentHeight, 1.550);
    const auto zenith101 = toTarget<survey::ZenithAngleObservation>(first, "101");
    ASSERT_EQ(zenith101.size(), 1U);
    EXPECT_DOUBLE_EQ(zenith101[0]->targetHeight, 1.700);
    EXPECT_DOUBLE_EQ(zenith101[0]->instrumentHeight, 1.550);
    // Record 10 sets hr 1.500 at the second setup (hi 1.600).
    const auto toStn1 = toTarget<survey::DistanceObservation>(second, "STN1");
    ASSERT_EQ(toStn1.size(), 1U);
    EXPECT_DOUBLE_EQ(toStn1[0]->targetHeight, 1.500);
    EXPECT_DOUBLE_EQ(toStn1[0]->instrumentHeight, 1.600);
    // "31..00+00070711" = 70.711 m.
    EXPECT_DOUBLE_EQ(toStn1[0]->distance, 70.711);
}

TEST(LeicaGsi, Word51sPpmAndPrismConstantAreTheSetupsSettingsAndWereApplied)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    const survey::InstrumentSettings& instrument = read.project.stations[0].instrument;
    // Record 2: "51..1.+0012-034" - each value signed: +12 ppm, -34 mm.
    ASSERT_TRUE(instrument.atmosphericPpm.has_value());
    EXPECT_DOUBLE_EQ(*instrument.atmosphericPpm, 12.0);
    EXPECT_EQ(instrument.atmosphericPpmState, survey::CorrectionState::Applied);
    ASSERT_TRUE(instrument.prismConstant.has_value());
    EXPECT_DOUBLE_EQ(*instrument.prismConstant, -0.034);
    EXPECT_EQ(instrument.prismConstantState, survey::CorrectionState::Applied);
    // The shot records the prism constant it was measured with.
    const auto toBs = toTarget<survey::DistanceObservation>(read.project.stations[0], "BS");
    ASSERT_FALSE(toBs.empty());
    ASSERT_TRUE(toBs[0]->target.prismConstant.has_value());
    EXPECT_DOUBLE_EQ(*toBs[0]->target.prismConstant, -0.034);
    EXPECT_EQ(toBs[0]->target.prismConstantState, survey::CorrectionState::Applied);
}

TEST(LeicaGsi, ASetupsOwnPpmAndPrismWordsReplaceThoseCarriedFromTheSetupBefore)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    const survey::InstrumentSettings& instrument = read.project.stations[1].instrument;
    // Record 10: "59..16+00100000" - unit 6, four decimals: 100000 / 10^4 = 10
    // ppm (GET 59's own example is "59..16+02200000" = 220 ppm);
    // "58..16+00000000" = 0 mm. Record 2's 12 ppm / -34 mm carried until then.
    ASSERT_TRUE(instrument.atmosphericPpm.has_value());
    EXPECT_DOUBLE_EQ(*instrument.atmosphericPpm, 10.0);
    ASSERT_TRUE(instrument.prismConstant.has_value());
    EXPECT_DOUBLE_EQ(*instrument.prismConstant, 0.0);
    // A replacement, not a change within the setup: no warning about it.
    EXPECT_EQ(warningAbout(read, 10, "ppm changes"), nullptr) << allWarnings(read);
}

TEST(LeicaGsi, ACodeBlockCodesThePointAfterItAndWord71CodesItsOwnBlocksPoint)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    // Record 4: "410004+0000KERB 42....+00000001" before record 5 (point 101).
    const survey::UnpositionedPoint* p101 = unpositioned(read.project, "101");
    ASSERT_NE(p101, nullptr);
    EXPECT_EQ(p101->code, "KERB");
    ASSERT_TRUE(p101->metadata.contains("code block information 1"));
    EXPECT_EQ(p101->metadata.at("code block information 1"), "1");
    // Record 6: "71....+0000KERB 72....+000000A2" - remark 1 is the code.
    const survey::UnpositionedPoint* p102 = unpositioned(read.project, "102");
    ASSERT_NE(p102, nullptr);
    EXPECT_EQ(p102->code, "KERB");
    ASSERT_TRUE(p102->metadata.contains("remark 2"));
    EXPECT_EQ(p102->metadata.at("remark 2"), "A2");
    EXPECT_EQ(read.project.metadata.at("code blocks belong to"), "the point after them");
}

TEST(LeicaGsi, ConsecutivePointsWithOneCodeAreOneFeatureAndAnUncodedPointEndsIt)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    // 101 and 102 are KERB, 103 (record 7) has no code.
    ASSERT_EQ(read.project.features.size(), 1U);
    const survey::SurveyFeature& kerb = read.project.features[0];
    EXPECT_EQ(kerb.code, "KERB");
    EXPECT_EQ(kerb.pointIds, (std::vector<std::string>{"101", "102"}));
    EXPECT_EQ(kerb.source.recordNumber, 5U);
}

TEST(LeicaGsi, AHorizontalDistanceAndHeightDifferenceWithNoAnglesGiveADistanceAndALevelDifference)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    const survey::SurveyStation& setup = read.project.stations[0];
    // Record 7: "32..00+00027000" = 27.000 m, "33..00-00000250" = -0.250 m,
    // and "87..10+00000000" sets the reflector height to 0.
    const auto distances = toTarget<survey::DistanceObservation>(setup, "103");
    ASSERT_EQ(distances.size(), 1U);
    EXPECT_EQ(distances[0]->kind, survey::DistanceKind::Horizontal);
    EXPECT_DOUBLE_EQ(distances[0]->distance, 27.0);
    EXPECT_DOUBLE_EQ(distances[0]->targetHeight, 0.0);
    const auto levels = toTarget<survey::LevelDifferenceObservation>(setup, "103");
    ASSERT_EQ(levels.size(), 1U);
    EXPECT_EQ(levels[0]->from, "STN1");
    EXPECT_DOUBLE_EQ(levels[0]->heightDifference, -0.250);
    EXPECT_DOUBLE_EQ(levels[0]->length, 27.0);
    EXPECT_GT(levels[0]->sigma, 0.0);
    // Hz "21.102+07000000" = 70 gon.
    const auto directions = toTarget<survey::HorizontalDirectionObservation>(setup, "103");
    ASSERT_EQ(directions.size(), 1U);
    EXPECT_NEAR(directions[0]->direction, 70.0 * kGon, kAngleTolerance);
    EXPECT_EQ(directions[0]->pointing.face, survey::Face::Unknown); // no V, no face
}

TEST(LeicaGsi, ATargetsCoordinateWordsAreMeasuredCoordinates)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    // Record 8: "81..00+00100500" - input mode 0 (measured), unit 0: 100.500 m.
    const survey::SurveyPoint* p104 = point(read.project, "104");
    ASSERT_NE(p104, nullptr);
    EXPECT_DOUBLE_EQ(p104->easting, 100.5);
    EXPECT_DOUBLE_EQ(p104->northing, 200.5);
    ASSERT_TRUE(p104->elevation.has_value());
    EXPECT_DOUBLE_EQ(*p104->elevation, 50.1);
    EXPECT_EQ(p104->coordinateSource, survey::CoordinateSource::FieldObserved);
    EXPECT_EQ(p104->source.recordNumber, 8U);
    EXPECT_EQ(p104->source.formatVersion, "GSI-8");
    EXPECT_EQ(p104->source.fileName, "tps_gsi8.gsi");
    // Targets named with no coordinates are unpositioned, never placed at 0,0.
    EXPECT_NE(unpositioned(read.project, "BS"), nullptr);
    EXPECT_EQ(point(read.project, "BS"), nullptr);
}

TEST(LeicaGsi, AZeroDistanceIsNoDistanceAndAnUnreadWordIsSaidOnceForTheFile)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    // Record 11: "31..00+00000000" - Leica's "no distance".
    EXPECT_TRUE(toTarget<survey::DistanceObservation>(read.project.stations[1], "105").empty());
    EXPECT_EQ(toTarget<survey::ZenithAngleObservation>(read.project.stations[1], "105").size(), 1U);
    EXPECT_NE(warningAbout(read, 11, "distance of zero"), nullptr) << allWarnings(read);
    // Record 12: word 25 (horizontal circle difference) is not read, and says so.
    EXPECT_NE(warningAbout(read, 12, "word 25"), nullptr) << allWarnings(read);
}

TEST(LeicaGsi, TheImportSaysWhatTheFileDoesNotCarry)
{
    const ReadResult read = readFixture("tps_gsi8.gsi");
    EXPECT_TRUE(says(read.notCarried, "no coordinate system"));
    EXPECT_TRUE(says(read.notCarried, "backsight"));
    EXPECT_TRUE(says(read.notCarried, "zenith angle"));
    // Both setups have an instrument height and a ppm: nothing said about them.
    EXPECT_FALSE(says(read.notCarried, "no instrument height"));
    EXPECT_FALSE(says(read.notCarried, "no atmospheric ppm"));
}

// ---- GSI-16 (data/leica/tps_gsi16.gsi) -----------------------------------------------

TEST(LeicaGsi, Gsi16ReadsTenthsAndHundredthsOfAMillimetre)
{
    const ReadResult read = readFixture("tps_gsi16.gsi");
    const survey::SurveyPoint* station = point(read.project, "STN9");
    ASSERT_NE(station, nullptr);
    // "84..16+0000000050000000": unit 6, last digit 1/10 mm: 50000000 / 10^4 =
    // 5000.0000 m; "86..16+0000000001234560" = 123.4560 m.
    EXPECT_DOUBLE_EQ(station->easting, 5000.0);
    EXPECT_DOUBLE_EQ(station->northing, 6000.0);
    EXPECT_DOUBLE_EQ(*station->elevation, 123.456);
    ASSERT_EQ(read.project.stations.size(), 1U);
    EXPECT_DOUBLE_EQ(read.project.stations[0].setup.instrumentHeight, 1.5);
    EXPECT_EQ(read.project.source.formatVersion, "GSI-16");
    const auto p200 = toTarget<survey::DistanceObservation>(read.project.stations[0], "P200");
    ASSERT_EQ(p200.size(), 1U);
    // "31..16+0000000000123456" = 12.3456 m; "87..16+...20000" = 2.0000 m.
    EXPECT_DOUBLE_EQ(p200[0]->distance, 12.3456);
    EXPECT_DOUBLE_EQ(p200[0]->targetHeight, 2.0);
    // "31..18+0000000001234567": unit 8, last digit 1/100 mm: 12.34567 m.
    const auto p201 = toTarget<survey::DistanceObservation>(read.project.stations[0], "P201");
    ASSERT_EQ(p201.size(), 1U);
    EXPECT_DOUBLE_EQ(p201[0]->distance, 12.34567);
}

TEST(LeicaGsi, Gsi16ReadsDecimalDegreesAndSexagesimalAngles)
{
    const ReadResult read = readFixture("tps_gsi16.gsi");
    const survey::SurveyStation& setup = read.project.stations[0];
    // P200: "21.103+0000000009000000" - unit 3, five decimals: 90.00000 deg.
    // "22.103+0000000027000000" = 270 deg: face right, zenith 360 - 270 = 90.
    const auto hz200 = toTarget<survey::HorizontalDirectionObservation>(setup, "P200");
    const auto v200 = toTarget<survey::ZenithAngleObservation>(setup, "P200");
    ASSERT_EQ(hz200.size(), 1U);
    ASSERT_EQ(v200.size(), 1U);
    EXPECT_NEAR(hz200[0]->direction, 90.0 * kDegree, kAngleTolerance);
    EXPECT_NEAR(v200[0]->angle, 90.0 * kDegree, kAngleTolerance);
    EXPECT_EQ(v200[0]->pointing.face, survey::Face::Right);
    // P201: "21.104+0000000004530150" - unit 4, DDD.MMSSs: 45 deg 30' 15.0" =
    // 45 + 30/60 + 15/3600 = 45.504166... deg.
    const auto hz201 = toTarget<survey::HorizontalDirectionObservation>(setup, "P201");
    ASSERT_EQ(hz201.size(), 1U);
    EXPECT_NEAR(hz201[0]->direction, (45.0 + 30.0 / 60.0 + 15.0 / 3600.0) * kDegree,
                kAngleTolerance);
    // The file mixes decimal degrees and sexagesimal, and says so.
    EXPECT_EQ(read.project.units.angular, survey::AngularUnit::DecimalDegrees);
    EXPECT_TRUE(says(std::vector<std::string>{allWarnings(read)}, "mixes angle units"));
}

TEST(LeicaGsi, TheStationBlocksSerialTypeAndTimeFillTheInstrumentSettings)
{
    const ReadResult read = readFixture("tps_gsi16.gsi");
    const survey::InstrumentSettings& instrument = read.project.stations[0].instrument;
    EXPECT_EQ(instrument.serialNumber, "123456"); // "12....+0000000000123456"
    EXPECT_EQ(instrument.model, "TS16");          // "13....+000000000000TS16"
    // Word 19 "03051015" is MMDDhhmm: 5 March, 10:15. Word 18 "2430500" is
    // YYSSmmm: 2024 (POSIX %y: 00-68 are 20xx), 30 s, 500 ms.
    ASSERT_TRUE(instrument.time.known());
    EXPECT_EQ(instrument.time.year, 2024);
    EXPECT_EQ(instrument.time.month, 3);
    EXPECT_EQ(instrument.time.day, 5);
    EXPECT_EQ(instrument.time.hour, 10);
    EXPECT_EQ(instrument.time.minute, 15);
    EXPECT_DOUBLE_EQ(instrument.time.second, 30.5);
    // No ppm or prism word anywhere: stated as unknown, not assumed.
    EXPECT_FALSE(instrument.atmosphericPpm.has_value());
    EXPECT_EQ(instrument.atmosphericPpmState, survey::CorrectionState::Unknown);
    EXPECT_TRUE(says(read.notCarried, "no atmospheric ppm"));
}

// ---- Feet and mils (data/leica/feet_mil_gsi8.gsi) ------------------------------------

TEST(LeicaGsi, FeetAreReadAsUsSurveyFeetAndSaySoAndMilsHaveFourDecimals)
{
    const ReadResult read = readFixture("feet_mil_gsi8.gsi");
    const survey::SurveyPoint* station = point(read.project, "STN5");
    ASSERT_NE(station, nullptr);
    // "84..11+00328084": unit 1, last digit 1/1000 ft: 328.084 ft, and a US
    // survey foot is 1200/3937 m: 328.084 * 1200 / 3937 m.
    EXPECT_NEAR(station->easting, 328.084 * 1200.0 / 3937.0, 1e-12);
    EXPECT_NEAR(station->northing, 656.168 * 1200.0 / 3937.0, 1e-12);
    EXPECT_EQ(read.project.units.linear, survey::LinearUnit::UsSurveyFeet);
    EXPECT_TRUE(says(std::vector<std::string>{allWarnings(read)}, "US survey feet"));
    const survey::SurveyStation& setup = read.project.stations.at(0);
    EXPECT_NEAR(setup.setup.instrumentHeight, 5.0 * 1200.0 / 3937.0, 1e-12);
    // "21.105+16000000": unit 5, 6400 mil to the circle, four decimals:
    // 1600.0000 mil = a quarter turn = pi/2.
    const auto hz = toTarget<survey::HorizontalDirectionObservation>(setup, "301");
    ASSERT_EQ(hz.size(), 1U);
    EXPECT_NEAR(hz[0]->direction, kPi / 2.0, kAngleTolerance);
    EXPECT_EQ(read.project.units.angular, survey::AngularUnit::Mils);
    const auto sd = toTarget<survey::DistanceObservation>(setup, "301");
    ASSERT_EQ(sd.size(), 1U);
    EXPECT_NEAR(sd[0]->distance, 32.808 * 1200.0 / 3937.0, 1e-12);
}

// ---- A coordinate list (data/leica/coordinates_gsi8.gsi) -----------------------------

TEST(LeicaGsi, StationWordsWithNoMeasurementAfterThemAreACoordinateListNotSetups)
{
    const ReadResult read = readFixture("coordinates_gsi8.gsi");
    EXPECT_TRUE(read.project.stations.empty());
    const survey::SurveyPoint* p1 = point(read.project, "1");
    ASSERT_NE(p1, nullptr);
    // "84..00+1234.567": the file writes its own decimal point; read as written.
    EXPECT_DOUBLE_EQ(p1->easting, 1234.567);
    EXPECT_DOUBLE_EQ(p1->northing, 2345.678);
    EXPECT_DOUBLE_EQ(*p1->elevation, 12.345);
    // "86..00+00012.35" - two decimals where the others have three.
    EXPECT_DOUBLE_EQ(*point(read.project, "2")->elevation, 12.35);
    // Station words, input mode 0: "measured" says nothing about a station.
    EXPECT_EQ(p1->coordinateSource, survey::CoordinateSource::Unknown);
    EXPECT_NE(warningAbout(read, 1, "decimal point"), nullptr) << allWarnings(read);
    EXPECT_NE(warningAbout(read, 1, "coordinates of the point they name"), nullptr)
        << allWarnings(read);
}

TEST(LeicaGsi, TextInAHeightWordIsThePointsCodeAndTheImportSaysSo)
{
    const ReadResult read = readFixture("coordinates_gsi8.gsi");
    // "88..00+00000STR": word 88 is the instrument height, and this is text.
    EXPECT_EQ(point(read.project, "1")->code, "STR");
    EXPECT_EQ(point(read.project, "3")->code, "FL");
    EXPECT_NE(warningAbout(read, 1, "read as the point's code"), nullptr) << allWarnings(read);
    // Word 71 in its own block: "71....+00000TRE"; "81..00+01240000" = 1240.000.
    const survey::SurveyPoint* a7 = point(read.project, "A7");
    ASSERT_NE(a7, nullptr);
    EXPECT_EQ(a7->code, "TRE");
    EXPECT_DOUBLE_EQ(a7->easting, 1240.0);
    ASSERT_EQ(read.project.features.size(), 3U);
    EXPECT_EQ(read.project.features[0].pointIds, (std::vector<std::string>{"1", "2"}));
    EXPECT_EQ(read.project.features[1].code, "FL");
    EXPECT_EQ(read.project.features[2].code, "TRE");
}

TEST(LeicaGsi, AHeightWithNoPositionIsKeptInMetadataNotPlacedAtTheOrigin)
{
    const ReadResult read = readFixture("coordinates_gsi8.gsi");
    // Record 5: "83..00+00014000" = 14.000 m and nothing else.
    EXPECT_EQ(point(read.project, "5"), nullptr);
    const survey::UnpositionedPoint* p5 = unpositioned(read.project, "5");
    ASSERT_NE(p5, nullptr);
    ASSERT_TRUE(p5->metadata.contains("height without a position"));
    EXPECT_EQ(p5->metadata.at("height without a position"), "14.0");
    EXPECT_NE(warningAbout(read, 5, "height and no easting"), nullptr) << allWarnings(read);
}

// ---- Damage (data/leica/damaged_gsi8.gsi) --------------------------------------------

TEST(LeicaGsi, DamagedWordsAndLinesAreWarningsNamingTheirRecordAndTheRestIsRead)
{
    const ReadResult read = readFixture("damaged_gsi8.gsi");
    // Record 4: Hz "1000000X" is not a number - that value only is lost.
    EXPECT_NE(warningAbout(read, 4, "not a number"), nullptr) << allWarnings(read);
    const survey::SurveyStation& setup = read.project.stations.at(0);
    EXPECT_TRUE(toTarget<survey::HorizontalDirectionObservation>(setup, "202").empty());
    EXPECT_EQ(toTarget<survey::ZenithAngleObservation>(setup, "202").size(), 1U);
    // Record 5 is not GSI at all.
    EXPECT_NE(warningAbout(read, 5, "not read"), nullptr) << allWarnings(read);
    EXPECT_EQ(read.recordsSkipped, 1U);
    EXPECT_EQ(read.recordsRead, 6U);
    // Record 6: word 99 is not a word this parser reads.
    EXPECT_NE(warningAbout(read, 6, "word 99"), nullptr) << allWarnings(read);
}

TEST(LeicaGsi, APointGivenTwoPositionsKeepsTheFirstAndTheDifferenceIsReported)
{
    const ReadResult read = readFixture("damaged_gsi8.gsi");
    // Records 2 and 3: "82..00+00010000" then "82..00+00010003" - 3 mm apart.
    const survey::SurveyPoint* p201 = point(read.project, "201");
    ASSERT_NE(p201, nullptr);
    EXPECT_DOUBLE_EQ(p201->northing, 10.0);
    const ReadWarning* warning = warningAbout(read, 3, "given coordinates again");
    ASSERT_NE(warning, nullptr) << allWarnings(read);
    EXPECT_NE(warning->message.find("0.003 m"), std::string::npos) << warning->message;
}

TEST(LeicaGsi, ACodeBlockWithNoPointAfterItBelongsToThePointBefore)
{
    const ReadResult read = readFixture("damaged_gsi8.gsi");
    const survey::UnpositionedPoint* p203 = unpositioned(read.project, "203");
    ASSERT_NE(p203, nullptr);
    EXPECT_EQ(p203->code, "TR");
    EXPECT_NE(warningAbout(read, 7, "point before it"), nullptr) << allWarnings(read);
}

// ---- Inline cases --------------------------------------------------------------------

TEST(LeicaGsi, MeasurementsBeforeAnyStationBlockAreKeptUnderASetupWithNoPosition)
{
    const std::string text = "110001+00000001 21.102+00000000 22.102+10000000 31..00+00010000 \r\n"
                             "110002+00000002 21.102+10000000 22.102+10000000 31..00+00020000 \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    ASSERT_EQ(read->project.stations.size(), 1U);
    const std::string& at = read->project.stations[0].setup.pointId;
    EXPECT_EQ(at, "GSI station");
    EXPECT_NE(unpositioned(read->project, at), nullptr);
    EXPECT_EQ(read->project.stations[0].observations.size(), 6U);
    EXPECT_NE(warningAbout(*read, 1, "before any station block"), nullptr);
    EXPECT_TRUE(says(read->notCarried, "no instrument height on 1 of 1 setup"));
    EXPECT_TRUE(says(read->notCarried, "no reflector height before 2 shots"));
}

TEST(LeicaGsi, AShotAtTheOccupiedPointIsAWarningNotAnInvalidObservation)
{
    const std::string text = "110001+0000STN1 88..10+00001500 \n"
                             "110002+0000STN1 21.102+00000000 22.102+10000000 31..00+00010000 \n"
                             "110003+00000002 21.102+10000000 22.102+10000000 31..00+00020000 \n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    EXPECT_NE(warningAbout(*read, 2, "stands on"), nullptr);
    EXPECT_EQ(read->project.stations.at(0).observations.size(), 3U);
    EXPECT_EQ(read->recordsSkipped, 1U);
}

TEST(LeicaGsi, ADigitalLevelsBlocksAreCountedNotReadAsTotalStationShots)
{
    // Page 40's example: "32...8+02505387 330.08+00125972" - a staff distance
    // and a staff reading, three-digit word 330.
    const std::string text = "110001+0000A110 32...8+02505387 330.08+00125972 \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    EXPECT_TRUE(read->project.stations.empty());
    EXPECT_NE(unpositioned(read->project, "A110"), nullptr);
    EXPECT_NE(warningAbout(*read, 1, "levelling"), nullptr) << allWarnings(*read);
}

TEST(LeicaGsi, CarriageReturnOnlyAndLineFeedOnlyFilesReadAlike)
{
    const std::string lf = "110001+00000001 81..00+00001000 82..00+00002000 \n"
                           "110002+00000002 81..00+00003000 82..00+00004000 \n";
    std::string cr = lf;
    std::ranges::replace(cr, '\n', '\r');
    Result<ReadResult> a = readText(lf);
    Result<ReadResult> b = readText(cr);
    ASSERT_TRUE(a.ok());
    ASSERT_TRUE(b.ok());
    EXPECT_EQ(a->project.points, b->project.points);
    EXPECT_EQ(a->project.points.size(), 2U);
}

TEST(LeicaGsi, AFileWithNoGsiBlockIsAnErrorNotAnEmptyProject)
{
    for (const std::string_view text : {std::string_view{}, std::string_view{"\r\n\r\n"},
                                        std::string_view{"P,N,E,Z\n1,2,3,4\n"}}) {
        Result<ReadResult> read = readText(text);
        ASSERT_FALSE(read.ok());
        EXPECT_EQ(read.error().code, ErrorCode::ParseFailure);
        EXPECT_NE(read.error().message.find("word 11"), std::string::npos);
    }
}

TEST(LeicaGsi, AZeroDefaultPrecisionIsRefusedBecauseEveryObservationNeedsASigma)
{
    ReadOptions options;
    options.precision.direction = 0.0;
    Result<ReadResult> read = readText("110001+00000001 81..00+00001000 82..00+00002000 \n",
                                       "job.gsi", options);
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::InvalidArgument);
}

// ---- Detection ------------------------------------------------------------------------

namespace {

double gsiConfidence(std::string_view bytes, std::string_view name)
{
    for (const FormatRegistry::ProbeResult& result :
         formatRegistry().probeAll(probeOf(bytes, name))) {
        if (result.formatId == kLeicaGsiFormatId) {
            return result.signature.confidence;
        }
    }
    return -1.0;
}

// Small hand-written samples of the other formats Katana reads, so that the
// GSI probe is tested against them even before their own fixtures exist on
// this branch.
struct Sample {
    const char* name;
    const char* text;
};

constexpr Sample kOtherFormats[] = {
    {"job.jxl", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<JOBFile jobName=\"J\" "
                "product=\"Trimble Access\" version=\"5.9\">\n<FieldBook>\n<PointRecord "
                "ID=\"00000001\">\n<Name>1</Name>\n</PointRecord>\n</FieldBook>\n</JOBFile>\n"},
    {"job.dc", "10NMJOB1            071118\r\n13TSTrimble Survey Controller\r\n"
               "08KI1       5000.000     5000.000      100.000CP\r\n"},
    {"job.rw5", "JB,NMJOB1,DT05-06-2024,TM10:15:30\nMO,AD0,UN0,SF1.00000000,EC1,EO0.0,AU0\n"
                "OC,OP1,N 5000.000,E 5000.000,EL100.000,--STN\nBK,OP1,BP2,BS0.0000,BC0.0000\n"
                "SS,OP1,FP3,AR90.0000,ZE90.0000,SD100.000,--TOPO\n"},
    {"job.gt7", "JOB,MYJOB\nINST,GTS-700\nUNITS,M,D\nSTN,1,1.500,STN\nXYZ,5000,5000,100\n"
                "BKB,2,0.0000,0.0000\nSS,3,1.600,TOPO\nSD,90.0000,90.0000,100.000\n"},
    {"site0010.24o", "     3.04           OBSERVATION DATA    M                   RINEX VERSION "
                     "/ TYPE\nsbf2rin-13.4.3                          20240305 101530 UTC PGM / "
                     "RUN BY / DATE\n                                                            "
                     "END OF HEADER\n"},
    {"job.xml", "<?xml version=\"1.0\"?>\n<LandXML xmlns=\"http://www.landxml.org/schema/"
                "LandXML-1.2\" version=\"1.2\">\n<CgPoints><CgPoint name=\"1\">5000 5000 "
                "100</CgPoint></CgPoints>\n</LandXML>\n"},
    {"points.csv", "P,N,E,Z,D\n1,6000.000,5000.000,100.000,IP\n2,6010.000,5010.000,101.000,IP\n"},
    {"points.txt", "1 6000.000 5000.000 100.000 IP\n2 6010.000 5010.000 101.000 IP\n"},
};

} // namespace

TEST(LeicaGsiDetection, EveryGsiFixtureIsIdentifiedAsGsiWithItsEvidence)
{
    for (const char* name : {"tps_gsi8.gsi", "tps_gsi16.gsi", "feet_mil_gsi8.gsi",
                             "coordinates_gsi8.gsi"}) {
        const std::string bytes = slurp(leicaFolder() / name);
        const Detection detection = detectFormat(probeOf(bytes, name));
        ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified)
            << name << ": " << detection.summary();
        EXPECT_EQ(detection.format()->id, kLeicaGsiFormatId) << name;
        EXPECT_NE(detection.candidates().front().evidence.find("word 11 or 41"),
                  std::string::npos);
    }
    // Without its extension a GSI file is still GSI.
    const std::string bytes = slurp(leicaFolder() / "tps_gsi8.gsi");
    const Detection renamed = detectFormat(probeOf(bytes, "export.txt"));
    ASSERT_EQ(renamed.outcome(), DetectionOutcome::Identified) << renamed.summary();
    EXPECT_EQ(renamed.format()->id, kLeicaGsiFormatId);
}

TEST(LeicaGsiDetection, TheGsiProbeRulesOutEveryOtherFormatsSamplesAndFixtures)
{
    for (const Sample& sample : kOtherFormats) {
        EXPECT_EQ(gsiConfidence(sample.text, sample.name), 0.0) << sample.name;
    }
    // Every fixture of every other format in this tree, whoever wrote it.
    std::size_t others = 0;
    for (const auto& entry : std::filesystem::recursive_directory_iterator(dataFolder())) {
        if (!entry.is_regular_file() ||
            entry.path().parent_path() == leicaFolder()) { // our own GSI fixtures
            continue;
        }
        const std::string bytes = slurp(entry.path());
        const std::string name = entry.path().filename().string();
        EXPECT_EQ(gsiConfidence(std::string_view(bytes).substr(0, kProbeBytes), name), 0.0)
            << entry.path().string();
        ++others;
    }
    EXPECT_GT(others, 0U);
}

// ---- Robustness ------------------------------------------------------------------------

namespace {

// Reads `bytes` and checks the one thing that must always hold: an answer -
// a project that passed validation, or an error with a message - and no crash.
void readsOrFailsCleanly(std::string_view bytes, const std::string& what)
{
    Result<ReadResult> read = readText(bytes);
    if (!read.ok()) {
        EXPECT_FALSE(read.error().message.empty()) << what;
        EXPECT_NE(read.error().code, ErrorCode::Internal) << what << ": " << read.error().message;
    }
}

} // namespace

TEST(LeicaGsiRobustness, EveryTruncationOfEveryFixtureReadsOrFailsWithAMessage)
{
    for (const char* name : {"tps_gsi8.gsi", "tps_gsi16.gsi", "feet_mil_gsi8.gsi",
                             "coordinates_gsi8.gsi", "damaged_gsi8.gsi"}) {
        const std::string bytes = slurp(leicaFolder() / name);
        ASSERT_FALSE(bytes.empty());
        for (std::size_t length = 0; length <= bytes.size(); ++length) {
            readsOrFailsCleanly(std::string_view(bytes).substr(0, length),
                                std::string(name) + " cut at " + std::to_string(length));
        }
    }
}

TEST(LeicaGsiRobustness, RandomBytesAndRandomlyDamagedFixturesReadOrFailWithAMessage)
{
    std::mt19937 random(20260924); // fixed: a failure must be repeatable
    std::uniform_int_distribution<int> byte(0, 255);
    const std::string base = slurp(leicaFolder() / "tps_gsi8.gsi");
    // Characters that matter to the grammar, so damage lands where it hurts.
    constexpr std::string_view kGrammar = "0123456789+-.* \r\n\x1a";
    for (int round = 0; round < 400; ++round) {
        std::string noise(static_cast<std::size_t>(round * 7 % 900), '\0');
        for (char& c : noise) {
            c = static_cast<char>(byte(random));
        }
        readsOrFailsCleanly(noise, "noise round " + std::to_string(round));

        std::string damaged = base;
        for (int hit = 0; hit < 1 + round % 12; ++hit) {
            const std::size_t at = static_cast<std::size_t>(byte(random)) * damaged.size() / 256;
            damaged[at] = round % 2 == 0 ? static_cast<char>(byte(random))
                                         : kGrammar[static_cast<std::size_t>(byte(random)) %
                                                    kGrammar.size()];
        }
        readsOrFailsCleanly(damaged, "damaged round " + std::to_string(round));
    }
}

// ---- Throughput ------------------------------------------------------------------------

namespace {

std::string field16(std::uint64_t value)
{
    std::string text = std::to_string(value);
    return std::string(16 - std::min<std::size_t>(16, text.size()), '0') + text;
}

// A GSI-16 job of `bytes` or a little more: a setup every 100 shots, each shot
// Hz, V, slope distance and reflector height - the common Leica recording mask.
std::string syntheticJob(std::size_t bytes)
{
    std::string text;
    text.reserve(bytes + 512);
    std::uint64_t block = 0;
    std::uint64_t shot = 0;
    while (text.size() < bytes) {
        if (shot % 100 == 0) {
            text += "*11" + field16(++block).substr(12) + "+" + field16(1000000 + shot / 100) +
                    " 84..16+" + field16(50000000 + shot) + " 85..16+" +
                    field16(60000000 + shot) + " 86..16+" + field16(1000000) + " 88..16+" +
                    field16(15500) + " \r\n";
        }
        ++shot;
        text += "*11" + field16(++block).substr(12) + "+" + field16(shot) + " 21.102+" +
                field16((shot * 7919) % 40000000) + " 22.102+" +
                field16(9000000 + (shot * 31) % 2000000) + " 31..16+" +
                field16(100000 + (shot * 131) % 5000000) + " 87..16+" + field16(17000) + " \r\n";
    }
    return text;
}

} // namespace

TEST(LeicaGsiThroughput, ASyntheticJobIsReadAndItsSpeedReported)
{
    // KATANA_GSI_THROUGHPUT_MB sets the size (50 for the measurement the
    // parser's performance is judged by); the default keeps ctest quick.
    std::size_t megabytes = 4;
    if (const char* setting = std::getenv("KATANA_GSI_THROUGHPUT_MB")) {
        megabytes = static_cast<std::size_t>(std::max(1, std::atoi(setting)));
    }
    const std::string job = syntheticJob(megabytes * 1000 * 1000);
    const FormatReader reader = formatRegistry().reader(kLeicaGsiFormatId);
    ASSERT_NE(reader, nullptr);

    const auto start = std::chrono::steady_clock::now();
    Result<ReadResult> parsed = reader(job, "synthetic.gsi", ReadOptions{});
    const auto parsedAt = std::chrono::steady_clock::now();
    ASSERT_TRUE(parsed.ok()) << parsed.error().message;
    Result<ReadResult> checked = readText(job, "synthetic.gsi");
    const auto checkedAt = std::chrono::steady_clock::now();
    ASSERT_TRUE(checked.ok()) << checked.error().message;

    const double seconds = std::chrono::duration<double>(parsedAt - start).count();
    const double withCheck = std::chrono::duration<double>(checkedAt - parsedAt).count();
    const double size = static_cast<double>(job.size()) / 1e6;
    std::size_t observations = 0;
    for (const survey::SurveyStation& station : parsed->project.stations) {
        observations += station.observations.size();
    }
    EXPECT_EQ(parsed->warnings.size(), 0U) << allWarnings(*parsed);
    EXPECT_GT(observations, 0U);
    std::cout << "[ throughput ] leica-gsi: " << size << " MB, " << parsed->recordsRead
              << " blocks, " << observations << " observations; reader " << seconds << " s = "
              << size / seconds << " MB/s; readSurvey (with validateProject) " << withCheck
              << " s = " << size / withCheck << " MB/s\n";
    RecordProperty("megabytes_per_second", std::to_string(size / seconds));
}
