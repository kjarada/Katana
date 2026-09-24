// The Topcon GTS readers (src/katana_surveyio/topcon_gts.cpp): GTS-7 read,
// GTS-6 recognised and refused.
//
// data/gts/setup.gt7 is hand-built from the record list in the Topcon Link
// Reference Manual (P/N 7010-0522, appendix C, "GTS-7 Raw Format"); the
// northing-first test uses that manual's own worked sample. Every expected
// value is worked below from the record text.

#include <gtest/gtest.h>

#include <chrono>
#include <cmath>
#include <iostream>
#include <numbers>
#include <string>

#include "surveyio/topcon_test_support.hpp"

using namespace katana::surveyio;
namespace survey = katana::survey;
using topcon_test::dms;
using topcon_test::fixture;
using topcon_test::observationsTo;

namespace {

constexpr std::string_view kGts7 = "topcon-gts7";
constexpr std::string_view kGts6 = "topcon-gts6";
constexpr double kAngleTolerance = 1e-12;
constexpr double kPi = std::numbers::pi;

ReadResult readSetup()
{
    const std::string bytes = fixture("gts/setup.gt7");
    auto result = topcon_test::read(kGts7, bytes, "setup.gt7");
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(result).value() : ReadResult{};
}

// The first lines of the GTS-6 sample printed in the Topcon Link manual (C-5).
constexpr std::string_view kGts6Sample =
    "_'MARK_(STAT_)1.52000_+ST1_ W+000049020m09757060-03726440d+0000138560+0000070470-"
    "0000002580***+0000+000000_*STAT_,1.60000_+1_\r\n"
    "?+00003448m0781803+0910223d+00003376***+00+00000_*TREE_,1.60000\r\n";

} // namespace

TEST(TopconGts, Gts7IsRegisteredAsAnImportableFormatWithTheLinkManualsExtensions)
{
    const auto format = formatRegistry().find(kGts7);
    ASSERT_TRUE(format.ok());
    EXPECT_TRUE(format->canImport);
    EXPECT_EQ(format->manufacturer, Manufacturer::Topcon);
    EXPECT_EQ(format->parserVersion, "1.0");
    EXPECT_EQ(format->extensions, (std::vector<std::string>{"gt7", "gts7", "gts", "raw", "dat"}));
    EXPECT_TRUE(format->reads.observations && format->reads.stations && format->reads.features);
    EXPECT_FALSE(format->reads.gnss);
    EXPECT_NE(formatRegistry().reader(kGts7), nullptr);
}

TEST(TopconGts, AStationRecordBecomesASetupWithItsHeightCodeCoordinatesAndBacksight)
{
    const ReadResult result = readSetup();
    const survey::SurveyProject& project = result.project;
    // JOB C:\Jobs\site.raw: only the name part of a path survives.
    EXPECT_EQ(project.name, "site.raw");
    EXPECT_EQ(project.metadata.at("job description"), "Test job");
    EXPECT_EQ(project.metadata.at("header"), "TTools v1.0");
    EXPECT_EQ(project.metadata.at("surveyor"), "Katana");
    EXPECT_EQ(project.metadata.at("date"), "24/09/26 10:15");
    EXPECT_EQ(project.units.linear, survey::LinearUnit::Metres);
    EXPECT_EQ(project.units.angular, survey::AngularUnit::DegreesMinutesSeconds);
    ASSERT_EQ(project.stations.size(), 1u);
    const survey::SurveyStation& station = project.stations[0];
    EXPECT_EQ(station.setup.id, "A");
    EXPECT_DOUBLE_EQ(station.setup.instrumentHeight, 1.5);
    EXPECT_EQ(station.instrument.model, "GTS-7");
    EXPECT_EQ(station.backsightPointId, "B");
    ASSERT_TRUE(station.backsightAzimuth.has_value());
    EXPECT_NEAR(*station.backsightAzimuth, kPi / 4.0, kAngleTolerance); // 45.00000
    EXPECT_EQ(station.metadata.at("backsight bearing (radians)"), "0");
    EXPECT_EQ(station.metadata.at("temperature, pressure (units not stated)"), "20.0, 1013");
    EXPECT_EQ(station.metadata.at("notes"), "end, of setup");

    // XYZ after STN is the station's; after BKB the backsight's.
    const survey::SurveyPoint* a = topcon_test::point(project, "A");
    ASSERT_NE(a, nullptr);
    EXPECT_DOUBLE_EQ(a->northing, 1000.0);
    EXPECT_DOUBLE_EQ(a->easting, 2000.0);
    EXPECT_DOUBLE_EQ(*a->elevation, 50.0);
    EXPECT_EQ(a->code, "STAT");
    const survey::SurveyPoint* b = topcon_test::point(project, "B");
    ASSERT_NE(b, nullptr);
    EXPECT_DOUBLE_EQ(b->northing, 1100.0);
    EXPECT_DOUBLE_EQ(b->easting, 2100.0);
    EXPECT_NE(topcon_test::unpositioned(project, "C"), nullptr);
}

TEST(TopconGts, ShotsGiveDirectionZenithAndDistanceWithTheirPointingAndHeights)
{
    const ReadResult result = readSetup();
    const survey::SurveyStation& station = result.project.stations.at(0);
    EXPECT_EQ(station.observations.size(), 14u); // 3 + 3 + 3 + 3 + 2, worked per shot below

    // BS B,1.60000 / SD 45.00000,89.30000,141.42600: pointing 1, face left.
    const auto bDirection = observationsTo<survey::HorizontalDirectionObservation>(station, "B");
    const auto bZenith = observationsTo<survey::ZenithAngleObservation>(station, "B");
    const auto bDistance = observationsTo<survey::DistanceObservation>(station, "B");
    ASSERT_EQ(bDirection.size(), 1u);
    ASSERT_EQ(bZenith.size(), 1u);
    ASSERT_EQ(bDistance.size(), 1u);
    EXPECT_NEAR(bDirection[0].direction, kPi / 4.0, kAngleTolerance);
    EXPECT_NEAR(bZenith[0].angle, dms(89, 30, 0), kAngleTolerance);
    EXPECT_DOUBLE_EQ(bDistance[0].distance, 141.426);
    EXPECT_DOUBLE_EQ(bDistance[0].instrumentHeight, 1.5);
    EXPECT_DOUBLE_EQ(bDistance[0].targetHeight, 1.6);
    EXPECT_EQ(bDistance[0].pointing, (survey::Pointing{1, survey::Face::Left}));
    EXPECT_EQ(bDistance[0].source.recordNumber, 14u);

    // SD -90.30300: a negative circle reading, 360 - 90 30' 30" = 269 29' 30".
    const auto cDirection = observationsTo<survey::HorizontalDirectionObservation>(station, "C");
    ASSERT_EQ(cDirection.size(), 1u);
    EXPECT_NEAR(cDirection[0].direction, dms(269, 29, 30), kAngleTolerance);
    EXPECT_EQ(cDirection[0].pointing, (survey::Pointing{2, survey::Face::Left}));
    EXPECT_DOUBLE_EQ(observationsTo<survey::DistanceObservation>(station, "C")[0].targetHeight,
                     1.7);

    // SD 100.00000,268.59450: face right; 360 - 268 59' 45" = 91 00' 15".
    const auto dZenith = observationsTo<survey::ZenithAngleObservation>(station, "D");
    ASSERT_EQ(dZenith.size(), 1u);
    EXPECT_NEAR(dZenith[0].angle, dms(91, 0, 15), kAngleTolerance);
    EXPECT_EQ(dZenith[0].pointing, (survey::Pointing{3, survey::Face::Right}));

    // HV 200.00000,90.00000: no distance; a zenith of exactly 90 is face left.
    const auto fZenith = observationsTo<survey::ZenithAngleObservation>(station, "F");
    ASSERT_EQ(fZenith.size(), 1u);
    EXPECT_NEAR(fZenith[0].angle, kPi / 2.0, kAngleTolerance);
    EXPECT_TRUE(observationsTo<survey::DistanceObservation>(station, "F").empty());
    EXPECT_NEAR(observationsTo<survey::HorizontalDirectionObservation>(station, "F")[0].direction,
                dms(200, 0, 0), kAngleTolerance);
}

TEST(TopconGts, AHorizontalDistanceRecordGivesAHorizontalDistanceAndAGroundLevelDifference)
{
    const ReadResult result = readSetup();
    const survey::SurveyStation& station = result.project.stations.at(0);
    // FS E,1.80000 / HD 180.00000,50.00000,-1.25000 with HI 1.50000.
    const auto flat = observationsTo<survey::DistanceObservation>(station, "E");
    ASSERT_EQ(flat.size(), 1u);
    EXPECT_EQ(flat[0].kind, survey::DistanceKind::Horizontal);
    EXPECT_DOUBLE_EQ(flat[0].distance, 50.0);
    const auto rise = observationsTo<survey::LevelDifferenceObservation>(station, "E");
    ASSERT_EQ(rise.size(), 1u);
    // VD + HI - HT = -1.25 + 1.5 - 1.8 = -1.55 (to the double: -1.25 + 1.5 is
    // exact, 0.25 - 1.8 rounds as the reader's own expression does).
    EXPECT_DOUBLE_EQ(rise[0].heightDifference, -1.25 + 1.5 - 1.8);
    EXPECT_DOUBLE_EQ(rise[0].length, 50.0);
    EXPECT_EQ(observationsTo<survey::HorizontalDirectionObservation>(station, "E")[0].pointing,
              (survey::Pointing{4, survey::Face::Unknown}));
}

TEST(TopconGts, CodesAndStringNumbersBecomeFeaturesAndOffsetsAreKeptNotApplied)
{
    const ReadResult result = readSetup();
    const survey::SurveyProject& project = result.project;
    const survey::SurveyFeature* kerb = topcon_test::feature(project, "KB");
    ASSERT_NE(kerb, nullptr);
    EXPECT_EQ(kerb->name, "1");
    EXPECT_EQ(kerb->pointIds, (std::vector<std::string>{"C", "D"}));
    const survey::SurveyFeature* stations = topcon_test::feature(project, "STAT");
    ASSERT_NE(stations, nullptr);
    EXPECT_EQ(stations->pointIds, (std::vector<std::string>{"A", "E"}));
    const survey::UnpositionedPoint* e = topcon_test::unpositioned(project, "E");
    ASSERT_NE(e, nullptr);
    EXPECT_EQ(e->metadata.at("offset radial, tangential, vertical (m)"), "0.1, 0, 0");
    EXPECT_EQ(e->metadata.at("traverse foresight"), "record 19");
    EXPECT_TRUE(topcon_test::anyWarningContains(result, 21, "not applied"));
    EXPECT_EQ(result.recordsRead, 24u);
    EXPECT_EQ(result.recordsSkipped, 0u);
}

TEST(TopconGts, WhatTheFormatCannotSayIsListedAsNotCarried)
{
    const ReadResult result = readSetup();
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(result, "no prism constant"));
    EXPECT_TRUE(topcon_test::anyNotCarriedContains(result, "without units"));
    EXPECT_EQ(result.project.stations.at(0).instrument.atmosphericPpmState,
              survey::CorrectionState::Unknown);
    EXPECT_FALSE(result.project.stations.at(0).instrument.temperatureCelsius.has_value());
}

// The record list labels XYZ "X(easting), Y(northing)"; the manual's own sample
// says otherwise. Station MARK is (N 10, E 10, Z 0.5) in the manual's
// coordinate listing; the raw sample gives ST1 as "13.85600,7.04700,-0.25800",
// observes it at the BKB circle reading, then radiates ST2 and prints ST2's
// coordinates. Radiating ST2 from what the reader returns reproduces the
// printed "14.87000,10.67900,-0.20400" to 2 mm only if XYZ is northing first.
TEST(TopconGts, TheXyzRecordIsNorthingFirstAsTheManualsOwnSampleGeometryShows)
{
    const std::string bytes = "UNITS         M,D\n"
                              "STN           MARK,1.52000,STAT\n"
                              "XYZ           10.00000,10.00000,0.50000\n"
                              "BKB           ST1,0.0000,322.33160\n"
                              "XYZ           13.85600,7.04700,-0.25800\n"
                              "BS            ST1,1.60000\n"
                              "SD            -37.26440,97.57060,4.90200\n"
                              "SS            ST2,1.60000,STAT\n"
                              "SD            7.56170,97.13460,4.95600\n"
                              "XYZ           14.87000,10.67900,-0.20400\n";
    const auto result = topcon_test::read(kGts7, bytes, "sample.gt7");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyProject& project = result->project;
    const survey::SurveyPoint* mark = topcon_test::point(project, "MARK");
    const survey::SurveyPoint* st1 = topcon_test::point(project, "ST1");
    const survey::SurveyPoint* st2 = topcon_test::point(project, "ST2");
    ASSERT_TRUE(mark && st1 && st2);
    const survey::SurveyStation& station = project.stations.at(0);
    // Orientation: the grid bearing MARK -> ST1 minus the circle reading on it.
    const double bearing =
        std::atan2(st1->easting - mark->easting, st1->northing - mark->northing);
    const double orientation = bearing - *station.backsightAzimuth;
    const auto direction = observationsTo<survey::HorizontalDirectionObservation>(station, "ST2");
    const auto zenith = observationsTo<survey::ZenithAngleObservation>(station, "ST2");
    const auto slope = observationsTo<survey::DistanceObservation>(station, "ST2");
    ASSERT_TRUE(direction.size() == 1 && zenith.size() == 1 && slope.size() == 1);
    const double azimuth = direction[0].direction + orientation;
    const double horizontal = slope[0].distance * std::sin(zenith[0].angle);
    const double northing = mark->northing + horizontal * std::cos(azimuth);
    const double easting = mark->easting + horizontal * std::sin(azimuth);
    const double elevation = *mark->elevation + slope[0].distance * std::cos(zenith[0].angle) +
                             station.setup.instrumentHeight - slope[0].targetHeight;
    EXPECT_NEAR(northing, st2->northing, 0.002);
    EXPECT_NEAR(easting, st2->easting, 0.002);
    EXPECT_NEAR(elevation, *st2->elevation, 0.002);
    EXPECT_EQ(st2->coordinateSource, survey::CoordinateSource::Calculated);
}

TEST(TopconGts, FeetAndGonsAreConvertedAndTheUnstatedFootIsAWarning)
{
    const std::string bytes = "UNITS F,G\n"
                              "STN P1,5.00000,\n"
                              "SS P2,6.00000,EP\n"
                              "SD 100.0000,100.0000,100.000\n";
    const auto result = topcon_test::read(kGts7, bytes, "feet.gt7");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::SurveyStation& station = result->project.stations.at(0);
    EXPECT_DOUBLE_EQ(station.setup.instrumentHeight, 5.0 * 0.3048);
    const auto slope = observationsTo<survey::DistanceObservation>(station, "P2");
    ASSERT_EQ(slope.size(), 1u);
    EXPECT_DOUBLE_EQ(slope[0].distance, 100.0 * 0.3048);
    EXPECT_DOUBLE_EQ(slope[0].targetHeight, 6.0 * 0.3048);
    // 100 gon is a quarter circle, for the direction and the zenith alike.
    EXPECT_NEAR(observationsTo<survey::HorizontalDirectionObservation>(station, "P2")[0].direction,
                kPi / 2.0, kAngleTolerance);
    EXPECT_NEAR(observationsTo<survey::ZenithAngleObservation>(station, "P2")[0].angle, kPi / 2.0,
                kAngleTolerance);
    EXPECT_TRUE(topcon_test::anyWarningContains(*result, 1, "which foot"));
    EXPECT_EQ(result->project.units.angular, survey::AngularUnit::Gons);
}

TEST(TopconGts, AMeasurementBeforeAnyUnitsRecordIsRefusedRatherThanGuessed)
{
    const auto result = topcon_test::read(kGts7, "STN A,1.5,STAT\nSS B,1.6,X\nSD 1,90,10\n",
                                          "nounits.gt7");
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, katana::core::ErrorCode::FileImportFailure);
    EXPECT_NE(result.error().message.find("record 1"), std::string::npos);
}

TEST(TopconGts, AMeasurementWithNoShotHeaderIsASkippedRecordNamingItsLine)
{
    const auto result = topcon_test::read(
        kGts7, "UNITS M,D\nSTN A,1.5,STAT\nSD 1.0000,90.0000,10.0\nBOGUS 1,2\n", "orphan.gt7");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(topcon_test::anyWarningContains(*result, 3, "no BS, FS or SS header"));
    EXPECT_TRUE(topcon_test::anyWarningContains(*result, 4, "not a GTS-7 record"));
    EXPECT_EQ(result->recordsSkipped, 2u);
}

TEST(TopconGts, Gts6IsRecognisedAndRefusedWithWhatToExportInstead)
{
    const Detection detection = detectFormat(probeOf(kGts6Sample, "job.raw"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kGts6);
    const auto result = topcon_test::read(kGts6, kGts6Sample, "job.raw");
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, katana::core::ErrorCode::Unsupported);
    EXPECT_NE(result.error().message.find("GTS-7 raw"), std::string::npos);
    EXPECT_NE(result.error().message.find("RW5"), std::string::npos);
}

TEST(TopconGts, DetectionIdentifiesGts7ByItsControlWordsWhateverTheExtension)
{
    const std::string bytes = fixture("gts/setup.gt7");
    for (const char* name : {"setup.gt7", "setup.raw", "setup.dat", "setup.txt"}) {
        const Detection detection = detectFormat(probeOf(bytes, name));
        ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << name << ": "
                                                                     << detection.summary();
        EXPECT_EQ(detection.format()->id, kGts7) << name;
    }
}

TEST(TopconGts, TheGtsProbesRuleOutRw5AndCoordinateFiles)
{
    for (const char* name : {"rw5/setup_metres.rw5", "rw5/feet_grads.rw5", "rw5/gnss.rw5",
                             "rw5/journal.rw5", "survey_points_pnezd.csv",
                             "survey_points_headerless.txt", "survey_points_headerless_penz.txt",
                             "survey_points_pnezd_utm30s.csv"}) {
        const std::string bytes = fixture(name);
        for (const auto& probed : formatRegistry().probeAll(probeOf(bytes, name))) {
            if (probed.formatId == kGts7 || probed.formatId == kGts6) {
                EXPECT_LT(probed.signature.confidence, 0.5) << name << " as " << probed.formatId
                                                            << ": " << probed.signature.evidence;
            }
        }
    }
    // And GTS-6 is not taken for GTS-7, nor the other way round.
    for (const auto& probed : formatRegistry().probeAll(probeOf(kGts6Sample, "job.raw"))) {
        if (probed.formatId == kGts7) {
            EXPECT_LT(probed.signature.confidence, 0.5);
        }
    }
}

TEST(TopconGts, EveryTruncationAndRandomBytesGiveAnErrorOrAResultNeverACrash)
{
    const std::size_t reads =
        topcon_test::readEveryTruncationAndNoise(kGts7, fixture("gts/setup.gt7"), "setup.gt7") +
        topcon_test::readEveryTruncationAndNoise(kGts6, kGts6Sample, "job.raw");
    EXPECT_GT(reads, 1500u);
}

TEST(TopconGts, AReadOfALargeSyntheticJobReadsEveryRecordAndReportsItsRate)
{
    std::size_t megabytes = 2;
    if (const char* wanted = std::getenv("KATANA_SURVEYIO_THROUGHPUT_MB")) {
        megabytes = static_cast<std::size_t>(std::strtoul(wanted, nullptr, 10));
    }
    std::string bytes = "JOB           throughput,synthetic\nUNITS         M,D\n";
    std::size_t setups = 0;
    std::size_t shots = 0;
    while (bytes.size() < megabytes * 1024 * 1024) {
        ++setups;
        const std::string station = "S" + std::to_string(setups);
        bytes += "STN           " + station + ",1.52000,STAT\n";
        bytes += "XYZ           5000.00000,5000.00000,100.00000\n";
        bytes += "BKB           B" + std::to_string(setups) + ",0.0000,45.00000\n";
        for (int shot = 0; shot < 100; ++shot) {
            ++shots;
            bytes += "SS            " + std::to_string(shots) + ",1.60000,KB," +
                     std::to_string(shot % 7) + "\n";
            bytes += "SD            " + std::to_string(shot * 3) + ".15300,91.00150," +
                     std::to_string(10 + shot) + ".23450\n";
        }
    }
    // The reader on its own, then the whole door (readSurvey adds the size
    // caps and survey::validateProject, which is not this reader's cost).
    const FormatReader reader = formatRegistry().reader(kGts7);
    ASSERT_NE(reader, nullptr);
    double readerSeconds = 0.0;
    {
        const auto start = std::chrono::steady_clock::now();
        const auto alone = reader(bytes, "throughput.gt7", ReadOptions{});
        readerSeconds =
            std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
        EXPECT_TRUE(alone.ok());
    } // freed here, outside both timings
    const auto start = std::chrono::steady_clock::now();
    const auto result = topcon_test::read(kGts7, bytes, "throughput.gt7");
    const double seconds =
        std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->recordsRead, 2 + setups * 3 + shots * 2);
    EXPECT_EQ(result->recordsSkipped, 0u);
    const double mb = static_cast<double>(bytes.size()) / (1024.0 * 1024.0);
    std::cout << "GTS-7 throughput: " << mb << " MB, " << shots << " shots; reader alone "
              << readerSeconds << " s, " << mb / readerSeconds << " MB/s; readSurvey (with "
              << "validateProject) " << seconds << " s, " << mb / seconds << " MB/s\n";
}

TEST(TopconGts, APointOccupiedManyTimesGivesNumberedSetupsInLinearTime)
{
    // Numbering by search from 2 each time would make this 200 million
    // string comparisons; counted per point it is 20,000 steps.
    std::string bytes = "UNITS M,D\n";
    for (int occupation = 0; occupation < 20000; ++occupation) {
        bytes += "STN A,1.50000,STAT\n";
    }
    const auto result = topcon_test::read(kGts7, bytes, "many.gt7");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result->project.stations.size(), 20000u);
    EXPECT_EQ(result->project.stations[0].setup.id, "A");
    EXPECT_EQ(result->project.stations[1].setup.id, "A (2)");
    EXPECT_EQ(result->project.stations[19999].setup.id, "A (20000)");
}
