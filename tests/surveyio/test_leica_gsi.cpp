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
#include <set>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/text.hpp"
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
    // 1.1: 60-second angle words, all-zero code words and backsights given
    // coordinates later in the file read differently from 1.0.
    EXPECT_EQ(format->parserVersion, "1.1");
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
    // GSI ONLINE's SET/CONF 171 makes the horizontal circle's direction a
    // setting no word records; the fixture's word 21s were read clockwise.
    EXPECT_TRUE(says(read.notCarried, "direction of the horizontal circle"));
    EXPECT_TRUE(says(read.notCarried, "word 21 was read as increasing clockwise"));
    // Record 4 is a code block and the file ends with a point block (record
    // 12): read Before Point, and the import says it is the instrument's call.
    EXPECT_TRUE(says(read.notCarried, "<Rec Free Code:>"));
    EXPECT_TRUE(says(read.notCarried, "each code block was taken to code the point after it"));
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

// ---- Sixty seconds (data/leica/rounded_seconds_gsi16.gsi) ------------------------------
//
// A sexagesimal angle's minutes and seconds run to 59, and 60.0 seconds is
// exactly the next minute. A writer that rounds the seconds field on its own
// and does not carry the minute writes 59.95" as 60 - Leica's Format Manager
// builds such an angle from separate degree, minute and second fields
// (Reference Guide V1.0, 8.3), and one file seen had 24 words so, each agreeing
// with the other face of its round only when read as the next minute
// (docs/survey.md, "Leica GSI"). In the fixture, CP01 is keyed in 100 m due
// north of STN1; P1 and P2 are measured with such words, P3 with a word no
// rounding writes.

TEST(LeicaGsi, SixtySecondsWithNothingAfterThemAreReadAsTheNextMinute)
{
    const ReadResult read = readFixture("rounded_seconds_gsi16.gsi");
    ASSERT_EQ(read.project.stations.size(), 1U);
    const survey::SurveyStation& setup = read.project.stations[0];
    // Record 4, P1. Hz "21.324+0000000004459600": unit 4, DDD MM SS s, so 044
    // 59 60.0 - which is 045 00 00.0, pi/4 = 0.785398163397448310 rad. V
    // "22.324+0000000008959600" = 089 59 60.0, the carry running on through
    // the minutes into the degrees: 090 00 00.0 = pi/2, under half a turn, so
    // face left.
    const auto hzP1 = toTarget<survey::HorizontalDirectionObservation>(setup, "P1");
    const auto vP1 = toTarget<survey::ZenithAngleObservation>(setup, "P1");
    ASSERT_EQ(hzP1.size(), 1U);
    ASSERT_EQ(vP1.size(), 1U);
    EXPECT_NEAR(hzP1[0]->direction, 0.785398163397448310, kAngleTolerance);
    EXPECT_NEAR(vP1[0]->angle, 1.570796326794896619, kAngleTolerance);
    EXPECT_EQ(vP1[0]->pointing.face, survey::Face::Left);
    // Record 5, P2. V "22.324+0000000008401600" = 084 01 60.0 = 084 02 00.0 =
    // 84 + 2/60 = 84.0333... deg, which times pi/180 - worked in exact
    // rationals with a 50-digit pi, not by this reader - is
    // 1.466658348092568288 rad. Hz "21.324+0000000013500000" = 135 00 00.0 =
    // 3 pi / 4 = 2.356194490192344929 rad.
    const auto vP2 = toTarget<survey::ZenithAngleObservation>(setup, "P2");
    const auto hzP2 = toTarget<survey::HorizontalDirectionObservation>(setup, "P2");
    ASSERT_EQ(vP2.size(), 1U);
    ASSERT_EQ(hzP2.size(), 1U);
    EXPECT_NEAR(vP2[0]->angle, 1.466658348092568288, kAngleTolerance);
    EXPECT_NEAR(hzP2[0]->direction, 2.356194490192344929, kAngleTolerance);
    // Each slope distance keeps the zenith angle of its pointing, so each of
    // the two shots is its three observations.
    EXPECT_EQ(toTarget<survey::DistanceObservation>(setup, "P1").size(), 1U);
    EXPECT_EQ(toTarget<survey::DistanceObservation>(setup, "P2").size(), 1U);
    EXPECT_EQ(read.recordsRead, 6U);
    EXPECT_EQ(read.recordsSkipped, 0U);
}

TEST(LeicaGsi, TheImportSaysOnceHowManyAngleWordsWroteSixtySecondsAndWhereTheFirstIs)
{
    const ReadResult read = readFixture("rounded_seconds_gsi16.gsi");
    // Three words write 60 seconds - record 4's two and record 5's V - and
    // they are one warning, at the first of them, which it names.
    const ReadWarning* summary = warningAbout(read, 4, "3 angle words write 60 seconds");
    ASSERT_NE(summary, nullptr) << allWarnings(read);
    EXPECT_NE(summary->message.find("word 21 '0000000004459600'"), std::string::npos)
        << summary->message;
    EXPECT_NE(summary->message.find("next minute"), std::string::npos) << summary->message;
    const auto aboutSixty = std::ranges::count_if(read.warnings, [](const ReadWarning& warning) {
        return warning.message.find("60 seconds") != std::string::npos;
    });
    EXPECT_EQ(aboutSixty, 1);
    // None of the three is refused: that warning and record 6's are all.
    EXPECT_EQ(read.warnings.size(), 2U) << allWarnings(read);
}

TEST(LeicaGsi, SecondsPastSixtyAreNoRoundingAndTheirWordIsStillRefusedByRecord)
{
    const ReadResult read = readFixture("rounded_seconds_gsi16.gsi");
    const survey::SurveyStation& setup = read.project.stations.at(0);
    // Record 6, P3: Hz "21.324+0000000009000610" = 090 00 61.0. No rounding of
    // seconds under 60 reaches 61, so the word is refused where it is, and
    // only it: the shot's zenith and distance are read.
    const ReadWarning* refused = warningAbout(read, 6, "the seconds run to 59");
    ASSERT_NE(refused, nullptr) << allWarnings(read);
    EXPECT_NE(refused->message.find("word 21 '0000000009000610'"), std::string::npos)
        << refused->message;
    EXPECT_NE(refused->message.find("that value was not read"), std::string::npos)
        << refused->message;
    EXPECT_TRUE(toTarget<survey::HorizontalDirectionObservation>(setup, "P3").empty());
    EXPECT_EQ(toTarget<survey::ZenithAngleObservation>(setup, "P3").size(), 1U);
    EXPECT_EQ(toTarget<survey::DistanceObservation>(setup, "P3").size(), 1U);
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
    // Records 1-3 write a point in all nine coordinate words; record 4's three
    // ("81..00+01240000" first) and record 5's one leave it to the unit digit:
    // 4 of 13, the fewer, so they are the ones named, from record 4.
    const ReadWarning* mixed = warningAbout(read, 4, "4 length words leave the decimal point");
    ASSERT_NE(mixed, nullptr) << allWarnings(read);
    EXPECT_NE(mixed->message.find("'01240000'"), std::string::npos) << mixed->message;
    EXPECT_NE(mixed->message.find("file's other 9 write one"), std::string::npos)
        << mixed->message;
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
    // Record 1 is a point block and record 7, "410007+000000TR", the last:
    // the shape of a job recorded After Point, so TR codes 203 (record 6).
    const survey::UnpositionedPoint* p203 = unpositioned(read.project, "203");
    ASSERT_NE(p203, nullptr);
    EXPECT_EQ(p203->code, "TR");
    EXPECT_NE(warningAbout(read, 7, "After Point"), nullptr) << allWarnings(read);
    EXPECT_EQ(read.project.metadata.at("code blocks belong to"), "the point before them");
}

// Leica's TPS1200 Technical Reference Manual (version 5.0, 16.3 "Coding &
// Linework Settings"): <Rec Free Code: Before Point / After Point> "determines
// if a free code is stored before or after the point", and no GSI word says
// which. The two fixtures hold the same survey recorded each way - points 1
// and 2 KERB, point 3 TREE - and must read alike.
TEST(LeicaGsi, TheSameCodesRecordedBeforeOrAfterTheirPointsCodeTheSamePoints)
{
    for (const char* name : {"codes_before_gsi8.gsi", "codes_after_gsi8.gsi"}) {
        SCOPED_TRACE(name);
        const bool after = std::string_view(name) == "codes_after_gsi8.gsi";
        const ReadResult read = readFixture(name);
        ASSERT_EQ(read.project.points.size(), 3U);
        const survey::SurveyPoint* p1 = point(read.project, "1");
        const survey::SurveyPoint* p2 = point(read.project, "2");
        const survey::SurveyPoint* p3 = point(read.project, "3");
        ASSERT_NE(p1, nullptr);
        ASSERT_NE(p2, nullptr);
        ASSERT_NE(p3, nullptr);
        EXPECT_EQ(p1->code, "KERB");
        EXPECT_EQ(p2->code, "KERB");
        EXPECT_EQ(p3->code, "TREE");
        // No code was displaced into another point's metadata.
        EXPECT_FALSE(p3->metadata.contains("code block"));
        // "81..00+00002000": unit 0, the last digit a millimetre, 2.000 m.
        EXPECT_DOUBLE_EQ(p3->easting, 2.0);
        EXPECT_DOUBLE_EQ(p3->northing, 2.1);

        ASSERT_EQ(read.project.features.size(), 2U);
        EXPECT_EQ(read.project.features[0].code, "KERB");
        EXPECT_EQ(read.project.features[0].pointIds, (std::vector<std::string>{"1", "2"}));
        // A feature starts at its first point's block: record 1 when the code
        // follows the point, record 2 when it precedes it.
        EXPECT_EQ(read.project.features[0].source.recordNumber, after ? 1U : 2U);
        EXPECT_EQ(read.project.features[1].code, "TREE");
        EXPECT_EQ(read.project.features[1].pointIds, (std::vector<std::string>{"3"}));

        EXPECT_EQ(read.project.metadata.at("code blocks belong to"),
                  after ? "the point before them" : "the point after them");
        EXPECT_TRUE(says(read.notCarried, "<Rec Free Code:>"));
        // The After Point file is told so at its first code block, record 2;
        // the Before Point file needs no warning.
        EXPECT_EQ(warningAbout(read, 2, "After Point") != nullptr, after) << allWarnings(read);
        EXPECT_EQ(read.warnings.empty(), !after) << allWarnings(read);
    }
}

// The reviewer's case: a code block ending a round of shots. The file begins
// with a code block, so it reads Before Point and the last code block has no
// point after it. The block before it (record 6) is BS, face right, which was
// NOT the last point the file named first (101, record 4).
TEST(LeicaGsi, ACodeBlockEndingARoundOfShotsGoesToThePointOfTheBlockBeforeIt)
{
    const std::string text =
        "410001+0000CTRL \r\n"
        "110002+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n"
        "110003+000000BS 21.102+00000000 22.102+10000000 31..00+00100000 \r\n"
        "110004+00000101 21.102+05000000 22.102+09500000 31..00+00025000 \r\n"
        "110005+00000101 21.102+25000000 22.102+30500000 31..00+00025000 \r\n"
        "110006+000000BS 21.102+20000000 22.102+30000000 31..00+00100000 \r\n"
        "410007+0000KERB \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    const survey::UnpositionedPoint* bs = unpositioned(read->project, "BS");
    const survey::UnpositionedPoint* p101 = unpositioned(read->project, "101");
    ASSERT_NE(bs, nullptr);
    ASSERT_NE(p101, nullptr);
    EXPECT_EQ(bs->code, "KERB");
    EXPECT_TRUE(p101->code.empty()) << p101->code;
    EXPECT_NE(warningAbout(*read, 7, "attached to the point before it, BS"), nullptr)
        << allWarnings(*read);
    // CTRL, the first block, codes the point after it: the station.
    const survey::SurveyPoint* station = point(read->project, "STN1");
    ASSERT_NE(station, nullptr);
    EXPECT_EQ(station->code, "CTRL");
    EXPECT_EQ(read->project.metadata.at("code blocks belong to"), "the point after them");
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

TEST(LeicaGsi, AFirstShotToAPointPositionedEarlierIsTheBacksightAndOnePositionedByItselfIsNot)
{
    // Record 1 keys in CP01 (input mode 1, "81..10"); record 2 sets up on
    // STN1; its first shot (record 3) is to CP01, which the file positioned
    // before it: the backsight. Setup 2's first shot (record 6) is to point
    // 7, whose coordinates come in that same block, measured: no backsight.
    const std::string text =
        "110001+0000CP01 81..10+00100000 82..10+00300000 83..10+00050000 \r\n"
        "110002+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n"
        "110003+0000CP01 21.102+00000000 22.102+10000000 31..00+00100000 \r\n"
        "110004+00000006 21.102+10000000 22.102+10000000 31..00+00020000 \r\n"
        "110005+0000STN2 84..10+00140000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n"
        "110006+00000007 21.102+00000000 22.102+10000000 31..00+00010000 81..00+00150000 "
        "82..00+00200000 83..00+00050000 \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    const survey::SurveyProject& project = read->project;
    ASSERT_EQ(project.stations.size(), 2U);
    EXPECT_EQ(project.stations[0].backsightPointId, "CP01");
    EXPECT_TRUE(project.stations[0].metadata.contains("backsight"));
    EXPECT_TRUE(project.stations[1].backsightPointId.empty());
    EXPECT_FALSE(project.stations[1].metadata.contains("backsight"));
    EXPECT_TRUE(says(read->notCarried, "1 of 2 setups took the point of their first shot"));
    EXPECT_TRUE(says(read->notCarried, "cannot be oriented until its backsight is known"));
    // CP01 was keyed in, so it is known to the reduction as entered.
    EXPECT_EQ(point(project, "CP01")->coordinateSource, survey::CoordinateSource::Entered);
}

// A traverse is often begun on a mark whose coordinates the job states only
// when the instrument stands on it later. The first setup's first shot is to
// that mark, and a reduction, which has the whole file, can orient on it.
TEST(LeicaGsi, AFirstShotToAPointTheFileGivesCoordinatesOnlyLaterIsStillTheBacksight)
{
    // Setup 1 on STN1 (record 1) shoots STN2 first (record 2); STN2 is keyed
    // in only by its own setup, record 4. Setup 2's first shot is to STN1,
    // keyed in before it.
    const std::string text =
        "110001+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n"
        "110002+0000STN2 21.102+00000000 22.102+10000000 31..00+00100000 \r\n"
        "110003+00000101 21.102+05000000 22.102+10000000 31..00+00020000 \r\n"
        "110004+0000STN2 84..10+00100000 85..10+00300000 86..10+00050000 88..10+00001500 \r\n"
        "110005+0000STN1 21.102+00000000 22.102+10000000 31..00+00100000 \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    const survey::SurveyProject& project = read->project;
    ASSERT_EQ(project.stations.size(), 2U);
    EXPECT_EQ(project.stations[0].backsightPointId, "STN2");
    ASSERT_TRUE(project.stations[0].metadata.contains("backsight"));
    EXPECT_NE(project.stations[0].metadata.at("backsight").find("later in the file"),
              std::string::npos)
        << project.stations[0].metadata.at("backsight");
    EXPECT_EQ(project.stations[1].backsightPointId, "STN1");
    EXPECT_TRUE(says(read->notCarried,
                     "2 of 2 setups took the point of their first shot as the backsight"));
    EXPECT_TRUE(says(read->notCarried, "1 of them from coordinates later in the file"));
    EXPECT_FALSE(says(read->notCarried, "cannot be oriented until its backsight is known"));
}

// The other side of that rule: a point positioned only by the setup's own
// shots was positioned with the orientation the backsight is to give.
TEST(LeicaGsi, AFirstShotToAPointOnlyItsOwnSetupPositionsNamesNoBacksight)
{
    // Record 2, the setup's first shot, is to 7, unpositioned then; record 3
    // shoots 7 again and gives the coordinates the instrument computed from
    // this setup (81-83, input mode 0: measured).
    const std::string text =
        "110001+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n"
        "110002+00000007 21.102+00000000 22.102+10000000 31..00+00010000 \r\n"
        "110003+00000007 21.102+20000000 22.102+30000000 31..00+00010000 81..00+00100000 "
        "82..00+00210000 83..00+00050000 \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    ASSERT_EQ(read->project.stations.size(), 1U);
    EXPECT_TRUE(read->project.stations[0].backsightPointId.empty());
    EXPECT_FALSE(read->project.stations[0].metadata.contains("backsight"));
    EXPECT_NE(point(read->project, "7"), nullptr); // positioned, but by this setup
    EXPECT_TRUE(says(read->notCarried, "no setup's first shot"));
}

// The carry is made in whole numbers before the angle becomes a real, so a
// carried word is the same double as the word written with its carry made,
// whatever the word's width. Adding 60/3600 of a degree as a real is not:
// 267 + 46/60 + 60/3600 is a double 1.8e-15 rad from 267 + 47/60, which is
// why this test's angle is 267 46 60.0 (354 02 60.0, for one, comes out the
// same both ways and would not tell them apart).
TEST(LeicaGsi, ACarriedAngleIsExactlyTheAngleWrittenWithItsCarryInEitherWidth)
{
    // A GSI-8 station block, then GSI-16 and GSI-8 shots: P1 at 267 46 60.0,
    // P2 at 267 47 00.0 - 267.78333... deg = 4.673700848632148885 rad, exact
    // rationals and a 50-digit pi - and P3 the GSI-8 "26746600". P4's Hz is
    // 359 59 60.0: the carry runs through the minutes into the degrees, 360
    // 00 00.0, the circle's zero. Its V "08459600" is 084 59 60.0 = 085 00
    // 00.0 = 1.483529864195180140 rad.
    const std::string text =
        "110001+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n"
        "*110002+00000000000000P1 21.324+0000000026746600 22.324+0000000009000000 \r\n"
        "*110003+00000000000000P2 21.324+0000000026747000 22.324+0000000009000000 \r\n"
        "110004+000000P3 21.324+26746600 22.324+09000000 \r\n"
        "110005+000000P4 21.324+35959600 22.324+08459600 \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    const survey::SurveyStation& setup = read->project.stations.at(0);
    const auto p1 = toTarget<survey::HorizontalDirectionObservation>(setup, "P1");
    const auto p2 = toTarget<survey::HorizontalDirectionObservation>(setup, "P2");
    const auto p3 = toTarget<survey::HorizontalDirectionObservation>(setup, "P3");
    const auto p4 = toTarget<survey::HorizontalDirectionObservation>(setup, "P4");
    ASSERT_EQ(p1.size(), 1U);
    ASSERT_EQ(p2.size(), 1U);
    ASSERT_EQ(p3.size(), 1U);
    ASSERT_EQ(p4.size(), 1U);
    EXPECT_NEAR(p1[0]->direction, 4.673700848632148885, kAngleTolerance);
    EXPECT_EQ(p1[0]->direction, p2[0]->direction);
    EXPECT_EQ(p3[0]->direction, p1[0]->direction);
    // Zero on the circle, from either side of it.
    EXPECT_LT(std::min(p4[0]->direction, 2.0 * kPi - p4[0]->direction), kAngleTolerance)
        << p4[0]->direction;
    const auto v4 = toTarget<survey::ZenithAngleObservation>(setup, "P4");
    ASSERT_EQ(v4.size(), 1U);
    EXPECT_NEAR(v4[0]->angle, 1.483529864195180140, kAngleTolerance);
    EXPECT_NE(warningAbout(*read, 2, "4 angle words write 60 seconds"), nullptr)
        << allWarnings(*read);
}

TEST(LeicaGsi, SixtySecondsWithTenthsOrMinutesOfSixtyAreNoRoundingAndAreRefused)
{
    // What a rounding of the seconds does not write, each refused at its
    // record, with nothing carried:
    //   record 2: 084 01 60.1 - the seconds past 60.0;
    //   record 3: 084 60 00.0 - minutes of 60, which a rounding of the
    //             seconds leaves only if a second carry was also dropped,
    //             and no file has shown one (docs/survey.md, "Leica GSI");
    //   record 4: 084 01 99.0.
    const std::string text =
        "*110001+000000000000STN1 88..10+0000000000001500 \r\n"
        "*110002+00000000000000P1 21.324+0000000000000000 22.324+0000000008401601 \r\n"
        "*110003+00000000000000P2 21.324+0000000000000000 22.324+0000000008460000 \r\n"
        "*110004+00000000000000P3 21.324+0000000000000000 22.324+0000000008401990 \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    const survey::SurveyStation& setup = read->project.stations.at(0);
    for (const char* id : {"P1", "P2", "P3"}) {
        EXPECT_TRUE(toTarget<survey::ZenithAngleObservation>(setup, id).empty()) << id;
        EXPECT_EQ(toTarget<survey::HorizontalDirectionObservation>(setup, id).size(), 1U) << id;
    }
    EXPECT_NE(warningAbout(*read, 2, "the seconds run to 59"), nullptr) << allWarnings(*read);
    EXPECT_NE(warningAbout(*read, 3, "the minutes run to 59"), nullptr) << allWarnings(*read);
    EXPECT_NE(warningAbout(*read, 4, "the seconds run to 59"), nullptr) << allWarnings(*read);
    EXPECT_FALSE(says(std::vector<std::string>{allWarnings(*read)}, "60 seconds"));
}

// GSI right-justifies text and pads it with '0' (GET 11 and 41: "0000A110" is
// point A110), so a code word of nothing but zeros is an empty one - as
// Leica's Format Manager writes an unset code information word,
// "43....+00000000" (Reference Guide V1.0, Annex 2) - not the code "0". Read
// as "0", a job that records word 71 empty on every shot gives every point one
// code and runs one feature through all of them.
TEST(LeicaGsi, AnAllZeroRemarkOrCodeInformationWordIsEmptyNotTheCodeZero)
{
    const std::string text =
        "*110001+000000000000STN1 84..10+0000000000100000 85..10+0000000000200000 "
        "88..10+0000000000001500 \r\n"
        "*110002+00000000000000P1 21.324+0000000000000000 22.324+0000000009000000 "
        "71....+0000000000000000 \r\n"
        "*110003+00000000000000P2 21.324+0000000001000000 22.324+0000000009000000 "
        "71....+0000000000000000 \r\n"
        "*110004+00000000000000P3 21.324+0000000002000000 22.324+0000000009000000 "
        "71....+000000000000KERB \r\n"
        "*410005+000000000000TREE 42....+0000000000000000 43....+00000000000000A1 \r\n"
        "*110006+00000000000000P4 21.324+0000000003000000 22.324+0000000009000000 \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    const survey::SurveyProject& project = read->project;
    for (const char* id : {"P1", "P2"}) {
        const survey::UnpositionedPoint* uncoded = unpositioned(project, id);
        ASSERT_NE(uncoded, nullptr) << id;
        EXPECT_TRUE(uncoded->code.empty()) << id << ": '" << uncoded->code << "'";
    }
    ASSERT_NE(unpositioned(project, "P3"), nullptr);
    EXPECT_EQ(unpositioned(project, "P3")->code, "KERB");
    // The code block (record 5) codes the point after it; its first
    // information word is empty, its second is A1.
    const survey::UnpositionedPoint* p4 = unpositioned(project, "P4");
    ASSERT_NE(p4, nullptr);
    EXPECT_EQ(p4->code, "TREE");
    EXPECT_FALSE(p4->metadata.contains("code block information 1"));
    ASSERT_TRUE(p4->metadata.contains("code block information 2"));
    EXPECT_EQ(p4->metadata.at("code block information 2"), "A1");
    // P1 and P2 are uncoded, so no feature joins them: KERB and TREE only.
    ASSERT_EQ(project.features.size(), 2U);
    EXPECT_EQ(project.features[0].code, "KERB");
    EXPECT_EQ(project.features[1].code, "TREE");
    // Said once, at the first: the words 71 of records 2 and 3 and 42 of 5.
    const ReadWarning* note =
        warningAbout(*read, 2, "3 remark or code information words are all zeros");
    ASSERT_NE(note, nullptr) << allWarnings(*read);
    EXPECT_NE(note->message.find("word 71"), std::string::npos) << note->message;
}

// TargetInfo keeps the prism constant each distance was measured with, and
// the setup the one it began with (data_model.hpp). A shot whose word 51
// states another is a changed prism or a wrong setting - in one file seen,
// one of 15 distances from a setup to a mark was measured with +23 mm and is
// 23 mm longer than the other 14 - so the import counts them and names the
// first.
TEST(LeicaGsi, DistancesMeasuredWithAnotherPrismConstantThanTheirSetupsAreCountedAndKeepTheirOwn)
{
    // Word 51 "+0012+000" is 12 ppm and 0 mm; record 3's "+0012+023" is 23 mm.
    const std::string text =
        "110001+0000STN1 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n"
        "110002+000000BS 21.102+00000000 22.102+10000000 31..00+00100000 51..1.+0012+000 "
        "87..10+00001500 \r\n"
        "110003+00000101 21.102+05000000 22.102+10000000 31..00+00020023 51..1.+0012+023 \r\n"
        "110004+00000102 21.102+06000000 22.102+10000000 31..00+00030000 51..1.+0012+000 \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    const survey::SurveyStation& setup = read->project.stations.at(0);
    ASSERT_TRUE(setup.instrument.prismConstant.has_value());
    EXPECT_DOUBLE_EQ(*setup.instrument.prismConstant, 0.0);
    const auto to101 = toTarget<survey::DistanceObservation>(setup, "101");
    const auto to102 = toTarget<survey::DistanceObservation>(setup, "102");
    ASSERT_EQ(to101.size(), 1U);
    ASSERT_EQ(to102.size(), 1U);
    ASSERT_TRUE(to101[0]->target.prismConstant.has_value());
    EXPECT_DOUBLE_EQ(*to101[0]->target.prismConstant, 0.023);
    ASSERT_TRUE(to102[0]->target.prismConstant.has_value());
    EXPECT_DOUBLE_EQ(*to102[0]->target.prismConstant, 0.0);
    const ReadWarning* note = warningAbout(
        *read, 3, "1 distance was measured with a prism constant other than the one its setup");
    ASSERT_NE(note, nullptr) << allWarnings(*read);
    EXPECT_NE(note->message.find("0.023 m in setup STN1, which began with 0.0 m"),
              std::string::npos)
        << note->message;
}

// A monitoring pillar set up every half hour for a year is some 17,500 setups
// on one point. Each gets its own id - STN1, STN1 (2), STN1 (3) ... - and
// making the k-th must not cost more than making the first: the file of
// setups on one point must read in about the time of the same file with
// every setup on a different point. (A search for a free "(n)" from 2
// upwards - 1 + 2 + ... + 2,999, some 4.5 million id lookups - read 3,000
// setups on one point in 11.5 s and 3,000 on different points in 0.25 s,
// Debug build.)
TEST(LeicaGsi, ThousandsOfSetupsOnOnePointEachGetAnIdInNoMoreTimeThanSetupsOnDifferentPoints)
{
    constexpr int kSetups = 3000;
    const auto job = [](bool onePoint) {
        std::string text;
        for (int i = 1; i <= kSetups; ++i) {
            // 8 characters of GSI-8 data: STN1 padded, or 10000001 ... 10003000.
            const std::string id = onePoint ? std::string("0000STN1") : std::to_string(10000000 + i);
            text += "110001+" + id +
                    " 84..10+00100000 85..10+00200000 86..10+00050000 88..10+00001500 \r\n"
                    "110002+00000101 21.102+00000000 22.102+10000000 31..00+00010000 \r\n";
        }
        return text;
    };
    const std::string onePoint = job(true);
    const std::string manyPoints = job(false);
    const FormatReader reader = formatRegistry().reader(kLeicaGsiFormatId);
    ASSERT_NE(reader, nullptr);

    const auto manyStart = std::chrono::steady_clock::now();
    const Result<ReadResult> many = reader(manyPoints, "many.gsi", ReadOptions{});
    const auto oneStart = std::chrono::steady_clock::now();
    const Result<ReadResult> one = reader(onePoint, "one.gsi", ReadOptions{});
    const auto oneEnd = std::chrono::steady_clock::now();
    ASSERT_TRUE(many.ok()) << many.error().message;
    ASSERT_TRUE(one.ok()) << one.error().message;

    const std::vector<survey::SurveyStation>& stations = one->project.stations;
    ASSERT_EQ(stations.size(), static_cast<std::size_t>(kSetups));
    EXPECT_EQ(stations[0].setup.id, "STN1");
    EXPECT_EQ(stations[1].setup.id, "STN1 (2)");
    EXPECT_EQ(stations[kSetups - 1].setup.id, "STN1 (3000)");
    std::set<std::string> ids;
    for (const survey::SurveyStation& station : stations) {
        EXPECT_EQ(station.setup.pointId, "STN1");
        ids.insert(station.setup.id);
    }
    EXPECT_EQ(ids.size(), static_cast<std::size_t>(kSetups)); // no two alike
    EXPECT_EQ(many->project.stations.size(), static_cast<std::size_t>(kSetups));

    const double manySeconds = std::chrono::duration<double>(oneStart - manyStart).count();
    const double oneSeconds = std::chrono::duration<double>(oneEnd - oneStart).count();
    std::cout << "[ setups ] " << kSetups << " on one point " << oneSeconds << " s, on "
              << kSetups << " points " << manySeconds << " s\n";
    // Proportional reading gives about 1:1. The allowance of five times and
    // half a second is for a machine shared with other builds.
    EXPECT_LT(oneSeconds, 5.0 * manySeconds + 0.5);
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
    // and a staff reading, three-digit word 330 - after the DNA section's
    // special code block for line levelling BF, "410000+?......1".
    const std::string text = "410000+?......1 \r\n"
                             "110001+0000A110 32...8+02505387 330.08+00125972 \r\n";
    Result<ReadResult> read = readText(text);
    ASSERT_TRUE(read.ok()) << read.error().message;
    EXPECT_TRUE(read->project.stations.empty());
    const survey::UnpositionedPoint* a110 = unpositioned(read->project, "A110");
    ASSERT_NE(a110, nullptr);
    EXPECT_NE(warningAbout(*read, 2, "levelling"), nullptr) << allWarnings(*read);
    // The levelling method is not the point's code, and the import says so.
    EXPECT_TRUE(a110->code.empty());
    EXPECT_NE(warningAbout(*read, 1, "special code block"), nullptr) << allWarnings(*read);
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
                             "coordinates_gsi8.gsi", "codes_before_gsi8.gsi",
                             "codes_after_gsi8.gsi", "rounded_seconds_gsi16.gsi"}) {
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
                             "coordinates_gsi8.gsi", "damaged_gsi8.gsi", "codes_before_gsi8.gsi",
                             "codes_after_gsi8.gsi", "rounded_seconds_gsi16.gsi"}) {
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
    RecordProperty("megabytes_per_second", katana::core::formatExactReal(size / seconds));
}
