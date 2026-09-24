// Trimble JobXML reader (src/katana_surveyio/trimble_jobxml.cpp).
//
// The fixture, data/trimble_jxl/tps_gnss_job.jxl, is synthetic and was built
// by hand from the JobXML 5.72 schema. Its setup and its Reductions section
// were worked out by hand, like this:
//
//   Setup on CP1 (N 1000, E 2000, H 50), instrument height 1.500, oriented on
//   CP2 (N 1100, E 2000) with circle reading 0 00 00: CP2 is due north, so
//   the grid azimuth of the backsight is 0 and the orientation correction 0.
//
//   Point 100, target height 1.800, prism constant 0:
//     face 1  HA 90.0    VA 85.0            SD 50.000
//     face 2  HA 270.0   VA 275.0 (= 85.0)  SD 50.000
//     mean    azimuth 90, zenith 85
//     HD = 50 sin 85 = 50 x 0.9961946981 = 49.8097349
//     VD = 50 cos 85 = 50 x 0.0871557427 =  4.3577871
//     N = 1000 + 49.8097 cos 90 = 1000.0000
//     E = 2000 + 49.8097 sin 90 = 2049.8097
//     H = 50 + 1.500 + 4.3578 - 1.800 = 54.0578
//
//   Point 101, target height 0, prism constant -0.0344 (added to the raw EDM
//   distance, which the schema says carries none):
//     HA 135.0  VA 90.0  SD 20.000 -> 19.9656 horizontal
//     N = 1000 + 19.9656 cos 135 = 1000 - 19.9656 x 0.7071067812 = 985.8822
//     E = 2000 + 19.9656 sin 135 = 2000 + 14.1178             = 2014.1178
//     H = 50 + 1.500 + 0 - 0 = 51.5000
//
// Those are the values written into the fixture's Reductions section. The
// cross-check test radiates the PARSED observations the same way and has to
// land on them, which tests every unit conversion and the face-2 zenith rule
// end to end.

#include <gtest/gtest.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <numbers>
#include <random>
#include <sstream>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/reader.hpp"

using namespace katana::surveyio;
namespace survey = katana::survey;
using katana::core::ErrorCode;
using katana::core::Result;

namespace {

constexpr std::string_view kId = "trimble-jobxml";
constexpr double kDeg = std::numbers::pi / 180.0;

std::filesystem::path dataDir()
{
    return std::filesystem::path(__FILE__).parent_path() / "data";
}

std::string readFile(const std::filesystem::path& path)
{
    std::ifstream stream(path, std::ios::binary);
    std::ostringstream out;
    out << stream.rdbuf();
    return out.str();
}

const std::string& fixture()
{
    static const std::string bytes = readFile(dataDir() / "trimble_jxl" / "tps_gnss_job.jxl");
    return bytes;
}

Result<ReadResult> readJxl(std::string_view bytes, std::string_view name = "tps_gnss_job.jxl")
{
    return readSurvey(formatRegistry(), kId, bytes, name);
}

const ReadResult& fixtureResult()
{
    static const ReadResult result = [] {
        Result<ReadResult> read = readJxl(fixture());
        if (!read) {
            ADD_FAILURE() << read.error().describe();
            return ReadResult{};
        }
        return std::move(read).value();
    }();
    return result;
}

const survey::SurveyPoint* findPoint(const survey::SurveyProject& project, std::string_view id)
{
    for (const survey::SurveyPoint& point : project.points) {
        if (point.id == id) {
            return &point;
        }
    }
    return nullptr;
}

const survey::UnpositionedPoint* findNamed(const survey::SurveyProject& project, std::string_view id)
{
    for (const survey::UnpositionedPoint& point : project.unpositionedPoints) {
        if (point.id == id) {
            return &point;
        }
    }
    return nullptr;
}

bool anyWarningContains(const ReadResult& result, std::string_view text)
{
    for (const ReadWarning& warning : result.warnings) {
        if (describe(warning).find(text) != std::string::npos) {
            return true;
        }
    }
    return false;
}

template <typename T>
std::vector<const T*> observationsOf(const std::vector<survey::Observation>& list, std::string_view to)
{
    std::vector<const T*> found;
    for (const survey::Observation& observation : list) {
        if (const T* typed = std::get_if<T>(&observation)) {
            if constexpr (requires(const T& t) { t.to; }) {
                if (typed->to == to) {
                    found.push_back(typed);
                }
            } else {
                if (typed->point == to) {
                    found.push_back(typed);
                }
            }
        }
    }
    return found;
}

const survey::SurveyStation& onlyStation()
{
    const survey::SurveyProject& project = fixtureResult().project;
    EXPECT_EQ(project.stations.size(), 1u);
    return project.stations.front();
}

} // namespace

// ---- Registration and detection ------------------------------------------------------

TEST(TrimbleJobXml, isRegisteredAsAnImportableTrimbleFormatThatSaysWhatItReads)
{
    Result<FormatDescriptor> format = formatRegistry().find(kId);
    ASSERT_TRUE(format.ok());
    EXPECT_EQ(format->manufacturer, Manufacturer::Trimble);
    EXPECT_TRUE(format->canImport);
    EXPECT_FALSE(format->canExport);
    EXPECT_EQ(format->parserVersion, "1.0");
    EXPECT_EQ(format->extensions, std::vector<std::string>{"jxl"});
    EXPECT_TRUE(format->reads.observations && format->reads.stations &&
                format->reads.instrumentSettings && format->reads.gnss && format->reads.points);
    EXPECT_NE(formatRegistry().reader(kId), nullptr);
}

TEST(TrimbleJobXml, theFixtureIsIdentifiedAsJobXmlWithItsRootElementAsEvidence)
{
    const Detection detection = detectFormat(probeOf(fixture(), "tps_gnss_job.jxl"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kId);
    EXPECT_NE(detection.candidates().front().evidence.find("JOBFile"), std::string::npos);
}

TEST(TrimbleJobXml, aJobXmlFileWithAnotherExtensionIsStillIdentifiedByItsRoot)
{
    const Detection detection = detectFormat(probeOf(fixture(), "export.xml"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kId);
}

TEST(TrimbleJobXml, otherFormatsAreNotClaimedByTheJobXmlProbe)
{
    // Every fixture of another format in the suite's data folder, and short
    // samples in the shape of the other instrument formats.
    std::vector<std::pair<std::string, std::string>> others = {
        {"landxml.xml", "<?xml version=\"1.0\"?>\n<LandXML version=\"1.2\"><CgPoints/></LandXML>\n"},
        {"job.gsi", "*110001+0000000000000A01 21.022+0000000009000000 22.022+0000000009000000\n"},
        {"job.rw5", "JB,NMJOB,DT03-05-2024,TM09:00:00\nMO,AD0,UN2,SF1.00000000,EC0,EO0.0,AU0\n"},
        {"obs.24o", "     3.04           OBSERVATION DATA    M                   RINEX VERSION / TYPE\n"},
        {"job.gts", "_'CP1_(1.500)_+CP2_ ?+00100008m0892552+0000000d\n"},
        {"synthetic_job.dc", readFile(dataDir() / "trimble_dc" / "synthetic_job.dc")},
    };
    for (const std::filesystem::directory_entry& entry :
         std::filesystem::directory_iterator(dataDir())) {
        if (entry.is_regular_file()) {
            others.emplace_back(entry.path().filename().string(), readFile(entry.path()));
        }
    }
    for (const auto& [name, bytes] : others) {
        for (const auto& result : formatRegistry().probeAll(probeOf(bytes, name))) {
            if (result.formatId == kId) {
                EXPECT_EQ(result.signature.confidence, 0.0) << name;
            }
        }
    }
}

TEST(TrimbleJobXml, aJxlNamedFileThatIsNotJobXmlIsOnlyWeaklySuggested)
{
    const std::string bytes = "<?xml version=\"1.0\"?><Other/>";
    const Detection detection = detectFormat(probeOf(bytes, "odd.jxl"));
    EXPECT_NE(detection.outcome(), DetectionOutcome::Identified);
}

// ---- The project ---------------------------------------------------------------------

TEST(TrimbleJobXml, theJobNameUnitsAndDeclaredCoordinateSystemComeFromTheFile)
{
    const ReadResult& result = fixtureResult();
    EXPECT_EQ(result.formatId, kId);
    EXPECT_EQ(result.parserVersion, "1.0");
    EXPECT_EQ(result.project.name, "SYNTH-01");
    // The schema: values are metres and decimal degrees whatever the display
    // units (this job displays feet and DMS).
    EXPECT_EQ(result.project.units.linear, survey::LinearUnit::Metres);
    EXPECT_EQ(result.project.units.angular, survey::AngularUnit::DecimalDegrees);
    EXPECT_EQ(result.project.coordinateSystem,
              survey::DeclaredCoordinateSystem::named("Australia / MGA2020 Zone 56"));
    EXPECT_EQ(result.project.metadata.at("jxl.environment.CoordinateSystem/DatumName"), "GDA2020");
    EXPECT_EQ(result.project.metadata.at("jxl.UnitsRecord.DistanceUnits"), "InternationalFeet");
    EXPECT_EQ(result.project.metadata.at("jxl.productVersion"), "2024.10");
    EXPECT_EQ(result.project.source.formatVersion, "JobXML 5.72");
}

TEST(TrimbleJobXml, keyedInGridPointsArePositionedAndAControlClassPointIsControl)
{
    const survey::SurveyProject& project = fixtureResult().project;
    const survey::SurveyPoint* cp1 = findPoint(project, "CP1");
    ASSERT_NE(cp1, nullptr);
    EXPECT_EQ(cp1->northing, 1000.0);
    EXPECT_EQ(cp1->easting, 2000.0);
    EXPECT_EQ(cp1->elevation, 50.0);
    EXPECT_EQ(cp1->coordinateSource, survey::CoordinateSource::Entered);
    EXPECT_EQ(cp1->code, "CONTROL");
    ASSERT_EQ(project.controlPoints.size(), 1u);
    EXPECT_EQ(project.controlPoints.front(), survey::ControlPoint::fixed3d("CP1"));
    const survey::SurveyPoint* cp2 = findPoint(project, "CP2");
    ASSERT_NE(cp2, nullptr);
    EXPECT_EQ(cp2->northing, 1100.0);
    // Observed-only points are named, never placed at a made-up position.
    EXPECT_EQ(findPoint(project, "100"), nullptr);
    ASSERT_NE(findNamed(project, "100"), nullptr);
    EXPECT_EQ(findNamed(project, "100")->code, "KB&FL"); // &amp; decoded
    EXPECT_EQ(findNamed(project, "101")->description, "Gum");
    EXPECT_EQ(findNamed(project, "101")->metadata.at("feature"), "Tree");
    EXPECT_EQ(findNamed(project, "101")->metadata.at("attribute.Species"), "Eucalyptus");
}

TEST(TrimbleJobXml, aSetupCarriesItsInstrumentAndAtmosphereWithTheCorrectionStatesTheSchemaStates)
{
    const survey::SurveyStation& station = onlyStation();
    EXPECT_EQ(station.setup.id, "CP1#1");
    EXPECT_EQ(station.setup.pointId, "CP1");
    EXPECT_EQ(station.setup.instrumentHeight, 1.5);
    const survey::InstrumentSettings& settings = station.instrument;
    EXPECT_EQ(settings.make, "Trimble");
    EXPECT_EQ(settings.model, "S7");
    EXPECT_EQ(settings.serialNumber, "37310042");
    EXPECT_EQ(settings.pressureHectopascals, 1013.25);
    EXPECT_EQ(settings.temperatureCelsius, 20.0);
    EXPECT_EQ(settings.atmosphericPpm, 0.0);
    // ApplyPPMToRawDistances true: the raw distances still need it.
    EXPECT_EQ(settings.atmosphericPpmState, survey::CorrectionState::NotApplied);
    EXPECT_EQ(settings.refractionCoefficient, 0.142);
    EXPECT_EQ(settings.curvatureRefractionState, survey::CorrectionState::NotApplied);
    EXPECT_EQ(settings.scaleFactor, 1.0);
    EXPECT_EQ(settings.scaleFactorState, survey::CorrectionState::NotApplied);
    EXPECT_EQ(settings.time.year, 2024);
    EXPECT_EQ(settings.time.hour, 9);
    EXPECT_EQ(settings.time.minute, 3);
    EXPECT_EQ(settings.time.timeSystem, ""); // no zone in the file: not guessed
    EXPECT_EQ(station.metadata.at("jxl.applyEarthCurvatureCorrection"), "false");
}

TEST(TrimbleJobXml, theBacksightRecordOrientsTheSetupWithTheFaceOneCircleReading)
{
    const survey::SurveyStation& station = onlyStation();
    EXPECT_EQ(station.backsightPointId, "CP2");
    ASSERT_TRUE(station.backsightAzimuth.has_value());
    EXPECT_EQ(*station.backsightAzimuth, 0.0);
    EXPECT_EQ(station.metadata.at("jxl.orientationCorrection_deg"), "0.0");
}

TEST(TrimbleJobXml, oneShotBecomesDirectionZenithAndSlopeDistanceSharingOnePointingAndFace)
{
    const survey::SurveyStation& station = onlyStation();
    const auto directions = observationsOf<survey::HorizontalDirectionObservation>(station.observations, "100");
    const auto zeniths = observationsOf<survey::ZenithAngleObservation>(station.observations, "100");
    const auto distances = observationsOf<survey::DistanceObservation>(station.observations, "100");
    ASSERT_EQ(directions.size(), 2u);
    ASSERT_EQ(zeniths.size(), 2u);
    ASSERT_EQ(distances.size(), 2u);
    // Face 1: 90 degrees = pi/2 exactly as read; zenith 85 degrees.
    EXPECT_DOUBLE_EQ(directions[0]->direction, std::numbers::pi / 2.0);
    EXPECT_DOUBLE_EQ(zeniths[0]->angle, 85.0 * kDeg);
    EXPECT_EQ(distances[0]->distance, 50.0);
    EXPECT_EQ(distances[0]->kind, survey::DistanceKind::Slope);
    EXPECT_EQ(directions[0]->pointing, zeniths[0]->pointing);
    EXPECT_EQ(directions[0]->pointing, distances[0]->pointing);
    EXPECT_EQ(directions[0]->pointing.face, survey::Face::Left);
    EXPECT_NE(directions[0]->pointing.index, 0u);
    // Face 2 is a separate pointing.
    EXPECT_EQ(directions[1]->pointing.face, survey::Face::Right);
    EXPECT_NE(directions[1]->pointing.index, directions[0]->pointing.index);
    EXPECT_EQ(zeniths[0]->instrumentHeight, 1.5);
    EXPECT_EQ(zeniths[0]->targetHeight, 1.8);
    EXPECT_EQ(distances[0]->targetHeight, 1.8);
}

TEST(TrimbleJobXml, aFaceTwoCircleReadingIsKeptRawAndItsZenithIsTheAngleItMeasures)
{
    const auto& observations = onlyStation().observations;
    const auto directions = observationsOf<survey::HorizontalDirectionObservation>(observations, "100");
    const auto zeniths = observationsOf<survey::ZenithAngleObservation>(observations, "100");
    ASSERT_EQ(directions.size(), 2u);
    // 270 degrees as read (the reduction pairs the faces).
    EXPECT_DOUBLE_EQ(directions[1]->direction, 1.5 * std::numbers::pi);
    // A reading of 275 measures a zenith angle of 360 - 275 = 85 degrees.
    EXPECT_DOUBLE_EQ(zeniths[1]->angle, 85.0 * kDeg);
    EXPECT_EQ(zeniths[1]->pointing.face, survey::Face::Right);
}

TEST(TrimbleJobXml, standardErrorsInDegreesBecomeRadiansAndMissingOnesUseTheDefaultPrecision)
{
    const auto& observations = onlyStation().observations;
    const auto directions = observationsOf<survey::HorizontalDirectionObservation>(observations, "100");
    const auto distances = observationsOf<survey::DistanceObservation>(observations, "100");
    // 0.000277777... degrees = 1" = pi / 648000 = 4.8481368e-6 rad.
    EXPECT_NEAR(directions[0]->sigma, std::numbers::pi / 648000.0, 1e-15);
    EXPECT_EQ(distances[0]->sigma, 0.00105);
    // Face 2 states none: the reader's precision (3", and 2 mm + 2 ppm at
    // 50 m = hypot(0.002, 0.0001) = 0.0020025 m).
    const survey::ObservationPrecision precision{};
    EXPECT_EQ(directions[1]->sigma, precision.direction);
    EXPECT_NEAR(distances[1]->sigma, 0.0020024984, 1e-9);
}

TEST(TrimbleJobXml, prismConstantAndTargetHeightComeFromTheTargetRecordTheShotNames)
{
    const auto distances = observationsOf<survey::DistanceObservation>(onlyStation().observations, "101");
    ASSERT_EQ(distances.size(), 1u);
    EXPECT_EQ(distances[0]->target.prismConstant, -0.0344);
    EXPECT_EQ(distances[0]->target.prismConstantState, survey::CorrectionState::NotApplied);
    EXPECT_EQ(distances[0]->target.targetType, "CustomPrism");
    EXPECT_EQ(distances[0]->targetHeight, 0.0);
    EXPECT_EQ(distances[0]->distance, 20.0); // raw: the constant is not added here
}

TEST(TrimbleJobXml, deletedRecordsAreNotImportedAndAWarningNamesThem)
{
    const ReadResult& result = fixtureResult();
    EXPECT_EQ(findNamed(result.project, "102"), nullptr);
    EXPECT_TRUE(anyWarningContains(result, "1 record(s) the surveyor deleted"));
    EXPECT_TRUE(anyWarningContains(result, "0000000e"));
}

TEST(TrimbleJobXml, aMeanTurnedAngleIsNotReadAsAnotherObservation)
{
    const ReadResult& result = fixtureResult();
    // Two shots to 100 (faces 1 and 2), not three.
    EXPECT_EQ(observationsOf<survey::DistanceObservation>(onlyStation().observations, "100").size(), 2u);
    EXPECT_TRUE(anyWarningContains(result, "1 mean turned angle record(s)"));
}

TEST(TrimbleJobXml, recordKindsAndElementsItDoesNotReadAreCountedInWarnings)
{
    const ReadResult& result = fixtureResult();
    EXPECT_TRUE(anyWarningContains(result, "1 SurveyEventRecord record(s) were not read"));
    EXPECT_TRUE(anyWarningContains(result, "PointRecord ComputedGrid in 1 record(s)"));
    EXPECT_TRUE(anyWarningContains(result, "PointRecord QualityControl1 in 1 record(s)"));
    EXPECT_TRUE(anyWarningContains(result, "PointRecord Precision in 1 record(s)"));
    // Every warning has the file name in it, so a person can find it.
    for (const ReadWarning& warning : result.warnings) {
        EXPECT_EQ(warning.fileName, "tps_gnss_job.jxl");
    }
    // The FieldBook holds 26 records. Skipped: the SurveyEventRecord (a kind
    // this reader does not import), point 102 (deleted) and the mean turned
    // angle to 100 (the mean of shots read from their own records) = 3.
    // Read: the other 23 - 13 setup, equipment, line, note and units records
    // and 10 point records (CP1, CP2, the backsight shot to CP2, two shots to
    // 100, the shot to 101, BASE, G100, G200, G300). Each record once.
    EXPECT_EQ(result.recordsSkipped, 3u);
    EXPECT_EQ(result.recordsRead, 23u);
}

TEST(TrimbleJobXml, anRtkVectorBecomesAGeocentricBaselineWithCovarianceAndBothAntennas)
{
    const auto vectors = observationsOf<survey::GnssGeocentricBaselineObservation>(
        fixtureResult().project.observations, "G100");
    ASSERT_EQ(vectors.size(), 1u);
    const survey::GnssGeocentricBaselineObservation& vector = *vectors[0];
    EXPECT_EQ(vector.from, "BASE");
    EXPECT_EQ(vector.delta, (survey::GeocentricCoordinate{10.123, -5.456, 7.789}));
    EXPECT_EQ(vector.covariance, (survey::GnssCovariance3{4.0e-5, 9.0e-5, 1.6e-4, 1.0e-6, -2.0e-6, 3.0e-6}));
    EXPECT_EQ(vector.solution, survey::GnssSolution::Fixed);
    EXPECT_EQ(vector.referenceFrame, "WGS 84");
    // The base antenna record states a ReducedHeight (1.720, measured 1.650):
    // the controller's height to the phase centre, carried as such.
    EXPECT_EQ(vector.fromAntenna.height, 1.72);
    EXPECT_EQ(vector.fromAntenna.method, survey::AntennaHeightMethod::PhaseCentre);
    EXPECT_NE(vector.fromAntenna.measuredTo.find("measured 1.65 m"), std::string::npos)
        << vector.fromAntenna.measuredTo;
    // The rover's states none: the measured height and the method's words.
    EXPECT_EQ(vector.toAntenna.height, 2.0);
    EXPECT_EQ(vector.toAntenna.type, "R12i Internal");
    EXPECT_EQ(vector.toAntenna.method, survey::AntennaHeightMethod::Vertical);
    EXPECT_EQ(vector.toAntenna.measuredTo, "BottomOfAntennaMount");
}

TEST(TrimbleJobXml, aNetworkRtkPositionIsGeocentricAndAnAutonomousOneGeodeticInRadians)
{
    const survey::SurveyProject& project = fixtureResult().project;
    const auto network = observationsOf<survey::GnssGlobalPositionObservation>(project.observations, "G200");
    ASSERT_EQ(network.size(), 1u);
    ASSERT_TRUE(network[0]->geocentric.has_value());
    EXPECT_EQ(*network[0]->geocentric, (survey::GeocentricCoordinate{-4646000.125, 2553000.25, -3534000.5}));
    EXPECT_EQ(network[0]->solution, survey::GnssSolution::Fixed);

    const auto autonomous = observationsOf<survey::GnssGlobalPositionObservation>(project.observations, "G300");
    ASSERT_EQ(autonomous.size(), 1u);
    ASSERT_TRUE(autonomous[0]->geodetic.has_value());
    // -33.865 deg x pi / 180 = -0.5910557512 rad; 151.2094 deg = 2.6391018900 rad.
    EXPECT_NEAR(autonomous[0]->geodetic->latitude, -0.5910557511878798, 1e-15);
    EXPECT_NEAR(autonomous[0]->geodetic->longitude, 2.6391018899651137, 1e-15);
    EXPECT_EQ(autonomous[0]->geodetic->ellipsoidalHeight, 45.0);
    EXPECT_EQ(autonomous[0]->solution, survey::GnssSolution::Autonomous);
    // QualityControl3 sigmas 2, 3, 5 m -> variances 4, 9, 25 m^2 (north/east/up).
    EXPECT_EQ(autonomous[0]->covariance, (survey::GnssCovariance3{4.0, 9.0, 25.0, 0.5, 0.0, 0.0}));
}

TEST(TrimbleJobXml, aKeyedInLatitudeLongitudeIsNamedWithItsValuesInMetadataNotPlaced)
{
    const ReadResult& result = fixtureResult();
    const survey::UnpositionedPoint* base = findNamed(result.project, "BASE");
    ASSERT_NE(base, nullptr);
    EXPECT_EQ(base->metadata.at("wgs84.latitude_deg"), std::to_string(-33.865));
    EXPECT_TRUE(anyWarningContains(result, "keyed-in or local latitude and longitude"));
}

TEST(TrimbleJobXml, lineRecordsBecomeFeaturesAndNotesProjectMetadata)
{
    const survey::SurveyProject& project = fixtureResult().project;
    ASSERT_EQ(project.features.size(), 1u);
    EXPECT_EQ(project.features[0].name, "L1");
    EXPECT_EQ(project.features[0].code, "BOUNDARY");
    EXPECT_EQ(project.features[0].pointIds, (std::vector<std::string>{"CP1", "CP2"}));
    // Text and CDATA join into one note, markup characters intact.
    EXPECT_EQ(project.metadata.at("jxl.note.0000001a"), "Wind gusting; shots <after> 10:00 repeated");
}

// ---- The controller's reductions, as a cross-check ---------------------------------------

TEST(TrimbleJobXml, theControllersReducedCoordinatesAreKeptInPointMetadataAsWritten)
{
    const ReadResult& result = fixtureResult();
    const survey::UnpositionedPoint* hundred = findNamed(result.project, "100");
    ASSERT_NE(hundred, nullptr);
    EXPECT_EQ(hundred->metadata.at("trimble.reductions.north"), "1000.0000");
    EXPECT_EQ(hundred->metadata.at("trimble.reductions.east"), "2049.8097");
    EXPECT_EQ(hundred->metadata.at("trimble.reductions.elevation"), "54.0578");
    EXPECT_EQ(hundred->metadata.at("trimble.reductions.recordId"), "0000000a");
    EXPECT_EQ(findNamed(result.project, "G100")->metadata.at("trimble.reductions.height"), "46.1");
    // ZZZ is in Reductions only: said, not invented.
    EXPECT_TRUE(anyWarningContains(result, "1 point(s) in the controller's Reductions section"));
}

TEST(TrimbleJobXml, radiatingTheParsedShotsReproducesTheControllersReducedCoordinates)
{
    // See the working at the top of this file: this repeats it with the
    // values the reader produced, so a wrong unit, a wrong face-2 rule or a
    // prism constant of the wrong sign lands away from the controller's answer.
    const survey::SurveyProject& project = fixtureResult().project;
    const survey::SurveyStation& station = onlyStation();
    const survey::SurveyPoint& at = *findPoint(project, "CP1");
    const survey::SurveyPoint& backsight = *findPoint(project, "CP2");
    const double backsightAzimuth =
        std::atan2(backsight.easting - at.easting, backsight.northing - at.northing);
    const double orientation = backsightAzimuth - *station.backsightAzimuth;

    for (const std::string target : {"100", "101"}) {
        const auto directions = observationsOf<survey::HorizontalDirectionObservation>(station.observations, target);
        const auto zeniths = observationsOf<survey::ZenithAngleObservation>(station.observations, target);
        const auto distances = observationsOf<survey::DistanceObservation>(station.observations, target);
        double sumDirection = 0.0;
        double sumZenith = 0.0;
        double sumDistance = 0.0;
        for (std::size_t i = 0; i < directions.size(); ++i) {
            const double face = directions[i]->pointing.face == survey::Face::Right ? std::numbers::pi : 0.0;
            sumDirection += std::remainder(directions[i]->direction - face, 2.0 * std::numbers::pi);
            sumZenith += zeniths[i]->angle;
            sumDistance += distances[i]->distance + distances[i]->target.prismConstant.value_or(0.0);
        }
        const double n = static_cast<double>(directions.size());
        const double azimuth = sumDirection / n + orientation;
        const double zenith = sumZenith / n;
        const double slope = sumDistance / n;
        const double horizontal = slope * std::sin(zenith);
        const double north = at.northing + horizontal * std::cos(azimuth);
        const double east = at.easting + horizontal * std::sin(azimuth);
        const double height = *at.elevation + station.setup.instrumentHeight +
                              slope * std::cos(zenith) - zeniths[0]->targetHeight;

        const auto& metadata = findNamed(project, target)->metadata;
        // The controller wrote four decimals: agreement to half the last digit.
        EXPECT_NEAR(north, std::stod(metadata.at("trimble.reductions.north")), 5.1e-5) << target;
        EXPECT_NEAR(east, std::stod(metadata.at("trimble.reductions.east")), 5.1e-5) << target;
        EXPECT_NEAR(height, std::stod(metadata.at("trimble.reductions.elevation")), 5.1e-5) << target;
    }
}

// ---- Failure and edge cases -------------------------------------------------------------

TEST(TrimbleJobXml, aFileWhoseRootIsNotJobFileIsRefusedNamingWhatItFound)
{
    Result<ReadResult> read = readJxl("<?xml version=\"1.0\"?><LandXML/>");
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::FileImportFailure);
    EXPECT_NE(read.error().message.find("<LandXML>"), std::string::npos);
}

TEST(TrimbleJobXml, aTruncatedJobIsRefusedAsIncompleteRatherThanHalfImported)
{
    const std::string cut = fixture().substr(0, fixture().size() / 2);
    Result<ReadResult> read = readJxl(cut);
    ASSERT_FALSE(read.ok());
    EXPECT_NE(read.error().message.find("incomplete"), std::string::npos) << read.error().message;
}

TEST(TrimbleJobXml, aDocumentTypeDeclarationIsRefused)
{
    Result<ReadResult> read =
        readJxl("<?xml version=\"1.0\"?><!DOCTYPE JOBFile [<!ENTITY x \"y\">]><JOBFile/>");
    ASSERT_FALSE(read.ok());
    EXPECT_NE(read.error().message.find("DOCTYPE"), std::string::npos);
}

TEST(TrimbleJobXml, anUndefinedEntityInAValueIsRefused)
{
    Result<ReadResult> read = readJxl(
        "<JOBFile version=\"5.72\"><FieldBook><NoteRecord ID=\"1\"><Notes><Note>&evil;</Note>"
        "</Notes></NoteRecord></FieldBook></JOBFile>");
    ASSERT_FALSE(read.ok());
    EXPECT_NE(read.error().message.find("entity"), std::string::npos) << read.error().message;
}

TEST(TrimbleJobXml, nestingDeeperThanTheLimitIsRefusedNotRecursedInto)
{
    std::string deep = "<JOBFile><FieldBook><NoteRecord ID=\"1\">";
    for (int i = 0; i < 200; ++i) {
        deep += "<a>";
    }
    Result<ReadResult> read = readJxl(deep);
    ASSERT_FALSE(read.ok());
    EXPECT_NE(read.error().message.find("nested deeper"), std::string::npos) << read.error().message;
}

TEST(TrimbleJobXml, anEndTagThatDoesNotMatchIsRefusedWithItsLine)
{
    Result<ReadResult> read = readJxl("<JOBFile>\n<FieldBook>\n</Reductions>\n</JOBFile>");
    ASSERT_FALSE(read.ok());
    EXPECT_NE(read.error().message.find("line 3"), std::string::npos) << read.error().message;
}

TEST(TrimbleJobXml, aNumberThatIsNotANumberIsAWarningAndTheShotKeepsTheRest)
{
    std::string bytes = fixture();
    const std::string from = "<EDMDistance>20.000</EDMDistance>";
    bytes.replace(bytes.find(from), from.size(), "<EDMDistance>twenty</EDMDistance>");
    Result<ReadResult> read = readJxl(bytes);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(anyWarningContains(*read, "Circle/EDMDistance 'twenty' is not a number"));
    const auto& observations = read->project.stations.front().observations;
    EXPECT_TRUE(observationsOf<survey::DistanceObservation>(observations, "101").empty());
    EXPECT_EQ(observationsOf<survey::HorizontalDirectionObservation>(observations, "101").size(), 1u);
}

TEST(TrimbleJobXml, twoCoordinateRecordsForOneNameKeepTheControlOneAndWarnWithTheOther)
{
    const std::string job =
        "<JOBFile version=\"5.72\"><FieldBook>"
        "<PointRecord ID=\"1\"><Name>A</Name><Method>Coordinates</Method><SurveyMethod>KeyedIn</SurveyMethod>"
        "<Classification>Normal</Classification><Deleted>false</Deleted>"
        "<Grid><North>1</North><East>2</East><Elevation/></Grid></PointRecord>"
        "<PointRecord ID=\"2\"><Name>A</Name><Method>Coordinates</Method><SurveyMethod>KeyedIn</SurveyMethod>"
        "<Classification>Control</Classification><Deleted>false</Deleted>"
        "<Grid><North>5</North><East>6</East><Elevation/></Grid></PointRecord>"
        "</FieldBook><Reductions/></JOBFile>";
    Result<ReadResult> read = readJxl(job);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->project.points.size(), 1u);
    EXPECT_EQ(read->project.points[0].northing, 5.0);
    EXPECT_FALSE(read->project.points[0].elevation.has_value()); // empty element = no height
    EXPECT_EQ(read->project.controlPoints, (std::vector<survey::ControlPoint>{
                                               survey::ControlPoint::fixedHorizontal("A")}));
    EXPECT_TRUE(anyWarningContains(*read, "kept the control record 2 and not the one at N 1 E 2"));
    // Both read: record 1 was imported, then replaced by record 2.
    EXPECT_EQ(read->recordsRead, 2u);
    EXPECT_EQ(read->recordsSkipped, 0u);
}

TEST(TrimbleJobXml, aSetupWithNoInstrumentHeightOrAtmosphereSaysSoBeforeImport)
{
    const std::string job =
        "<JOBFile version=\"5.72\"><FieldBook>"
        "<StationRecord ID=\"1\"><StationName>S</StationName><TheodoliteHeight/></StationRecord>"
        "<PointRecord ID=\"2\"><Name>T</Name><Method>DirectReading</Method><Deleted>false</Deleted>"
        "<Circle><HorizontalCircle>10</HorizontalCircle><VerticalCircle/><EDMDistance/>"
        "<Face>FaceNull</Face></Circle><StationID>1</StationID></PointRecord>"
        "</FieldBook></JOBFile>";
    Result<ReadResult> read = readJxl(job);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(anyWarningContains(*read, "has no instrument height"));
    ASSERT_EQ(read->notCarried.size(), 3u);
    EXPECT_NE(read->notCarried[0].find("atmospheric correction state is unknown"), std::string::npos);
    EXPECT_NE(read->notCarried[1].find("without an instrument height"), std::string::npos);
    EXPECT_NE(read->notCarried[2].find("without a target height"), std::string::npos);
    const auto& observations = read->project.stations.front().observations;
    ASSERT_EQ(observations.size(), 1u);
    EXPECT_EQ(survey::observationPointing(observations[0])->face, survey::Face::Unknown);
}

TEST(TrimbleJobXml, eachRecordIsCountedOnceAsReadOnlyWhenSomethingOfItWasImported)
{
    // Fifteen records, numbered by their IDs. Read: 1 (a setup), 14 (a shot
    // whose direction is kept though its distance is refused) and 15 (a shot)
    // = 3. Skipped, each with its warning: 2 has no station name, 3 orients
    // a setup that is not in the file, 4 has no point name, 5 is a shot from
    // a setup that is not in the file, 6 a shot from S to S, 7 a line from S
    // to S, 8 a GNSS vector with no base, 9 is deleted, 10 is a mean turned
    // angle, 11 a shot with no reading, 12 a method this reader does not turn
    // into anything, 13 a shot whose only value (a negative distance) is
    // refused = 12. 3 + 12 = 15: no record counted twice or not at all.
    const std::string job =
        "<JOBFile version=\"5.72\"><FieldBook>\n"
        "<StationRecord ID=\"1\"><StationName>S</StationName><TheodoliteHeight>1.5"
        "</TheodoliteHeight></StationRecord>\n"
        "<StationRecord ID=\"2\"><StationName/><TheodoliteHeight>1.5</TheodoliteHeight>"
        "</StationRecord>\n"
        "<BackBearingRecord ID=\"3\"><StationRecordID>99</StationRecordID><BackSight>B</BackSight>"
        "</BackBearingRecord>\n"
        "<PointRecord ID=\"4\"><Name/><Deleted>false</Deleted><Grid><North>1</North><East>2</East>"
        "</Grid></PointRecord>\n"
        "<PointRecord ID=\"5\"><Name>T</Name><Deleted>false</Deleted><Circle><HorizontalCircle>10"
        "</HorizontalCircle></Circle><StationID>99</StationID></PointRecord>\n"
        "<PointRecord ID=\"6\"><Name>S</Name><Deleted>false</Deleted><Circle><HorizontalCircle>10"
        "</HorizontalCircle></Circle><StationID>1</StationID></PointRecord>\n"
        "<LineRecord ID=\"7\"><Name>L</Name><Method>TwoPoints</Method><StartPoint>S</StartPoint>"
        "<EndPoint>S</EndPoint><Deleted>false</Deleted></LineRecord>\n"
        "<PointRecord ID=\"8\"><Name>G</Name><Deleted>false</Deleted><ECEFDeltas><DeltaX>1</DeltaX>"
        "<DeltaY>2</DeltaY><DeltaZ>3</DeltaZ></ECEFDeltas></PointRecord>\n"
        "<PointRecord ID=\"9\"><Name>T</Name><Deleted>true</Deleted><Circle><HorizontalCircle>10"
        "</HorizontalCircle></Circle><StationID>1</StationID></PointRecord>\n"
        "<PointRecord ID=\"10\"><Name>T</Name><Method>MeanTurnedAngle</Method><Deleted>false"
        "</Deleted><MTA><HorizontalAngle>10</HorizontalAngle></MTA></PointRecord>\n"
        "<PointRecord ID=\"11\"><Name>U</Name><Deleted>false</Deleted><Circle><Face>Face1</Face>"
        "</Circle><StationID>1</StationID></PointRecord>\n"
        "<PointRecord ID=\"12\"><Name>V</Name><Method>LaserOffset</Method><Deleted>false</Deleted>"
        "</PointRecord>\n"
        "<PointRecord ID=\"13\"><Name>W</Name><Deleted>false</Deleted><Circle><EDMDistance>-5"
        "</EDMDistance></Circle><StationID>1</StationID></PointRecord>\n"
        "<PointRecord ID=\"14\"><Name>X</Name><Deleted>false</Deleted><Circle><HorizontalCircle>20"
        "</HorizontalCircle><EDMDistance>-5</EDMDistance></Circle><StationID>1</StationID>"
        "</PointRecord>\n"
        "<PointRecord ID=\"15\"><Name>T</Name><Deleted>false</Deleted><Circle><HorizontalCircle>30"
        "</HorizontalCircle><VerticalCircle>90</VerticalCircle></Circle><StationID>1</StationID>"
        "</PointRecord>\n"
        "</FieldBook></JOBFile>\n";
    Result<ReadResult> read = readJxl(job, "counts.jxl");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->recordsRead, 3u);
    EXPECT_EQ(read->recordsSkipped, 12u);
    // Records 14 and 15: HA to X; HA and ZA to T.
    ASSERT_EQ(read->project.stations.size(), 1u);
    EXPECT_EQ(read->project.stations.front().observations.size(), 3u);
    for (const char* said : {"counts.jxl record 3: a station record with no station name",
                             "counts.jxl record 4: backsight record 3 names setup record 99",
                             "counts.jxl record 5: a point record with no name",
                             "counts.jxl record 6: observation to T names setup record 99",
                             "counts.jxl record 7: observation from S to itself",
                             "counts.jxl record 8: line L does not join two different points",
                             "counts.jxl record 9: the GNSS vector to G has no base point",
                             "1 record(s) the surveyor deleted", "1 mean turned angle record(s)",
                             "counts.jxl record 12: observation to U records no circle reading",
                             "counts.jxl record 13: point V was recorded by the method LaserOffset",
                             "counts.jxl record 14: slope distance not imported",
                             "counts.jxl record 15: slope distance not imported"}) {
        EXPECT_TRUE(anyWarningContains(*read, said)) << said;
    }
}

// ---- Robustness ---------------------------------------------------------------------------

TEST(TrimbleJobXml, eachShotRecordsTheLineItsRecordStartsOnWhateverTheLineEndings)
{
    // one_setup.jxl, counted by hand in an editor: the StationRecord starts
    // on line 6, the shot to B (PointRecord 5) on line 8, the shot to C
    // (PointRecord 7) on line 10. Most of its lines are well over the eight
    // bytes the line counter takes at a time. With CR LF line endings the
    // lines are the same: a CR is not a new line.
    std::string lf = readFile(dataDir() / "trimble_jxl" / "one_setup.jxl");
    std::string crlf;
    for (const char c : lf) {
        if (c == '\n') {
            crlf.push_back('\r');
        }
        crlf.push_back(c);
    }
    for (const std::string& bytes : {lf, crlf}) {
        Result<ReadResult> read = readJxl(bytes, "one_setup.jxl");
        ASSERT_TRUE(read.ok()) << read.error().describe();
        ASSERT_EQ(read->project.stations.size(), 1u);
        const survey::SurveyStation& station = read->project.stations.front();
        EXPECT_EQ(station.source.recordNumber, 6u);
        const auto toB = observationsOf<survey::DistanceObservation>(station.observations, "B");
        const auto toC = observationsOf<survey::ZenithAngleObservation>(station.observations, "C");
        ASSERT_EQ(toB.size(), 1u);
        ASSERT_EQ(toC.size(), 1u);
        EXPECT_EQ(toB[0]->source.recordNumber, 8u);
        EXPECT_EQ(toC[0]->source.recordNumber, 10u);
        EXPECT_EQ(toC[0]->source.fileName, "one_setup.jxl");
    }
}

TEST(TrimbleJobXml, aSmallJobReadsItsAppliedPpmFaceTwoZenithAndFloatVector)
{
    Result<ReadResult> read =
        readJxl(readFile(dataDir() / "trimble_jxl" / "one_setup.jxl"), "one_setup.jxl");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->project.stations.size(), 1u);
    const survey::SurveyStation& station = read->project.stations.front();
    // ApplyPPMToRawDistances false: the ppm is already in the distances.
    EXPECT_EQ(station.instrument.atmosphericPpmState, survey::CorrectionState::Applied);
    EXPECT_EQ(station.instrument.atmosphericPpm, 1.5);
    const auto zeniths = observationsOf<survey::ZenithAngleObservation>(station.observations, "C");
    ASSERT_EQ(zeniths.size(), 1u);
    // Face 2 reading 268.25 -> zenith 360 - 268.25 = 91.75 degrees.
    EXPECT_DOUBLE_EQ(zeniths[0]->angle, 91.75 * kDeg);
    const auto distances = observationsOf<survey::DistanceObservation>(station.observations, "C");
    ASSERT_EQ(distances.size(), 1u);
    EXPECT_EQ(distances[0]->target.prismConstant, 0.0175);
    EXPECT_EQ(distances[0]->targetHeight, 1.3);
    const auto vectors =
        observationsOf<survey::GnssGeocentricBaselineObservation>(read->project.observations, "G");
    ASSERT_EQ(vectors.size(), 1u);
    EXPECT_EQ(vectors[0]->solution, survey::GnssSolution::Float);
    EXPECT_FALSE(vectors[0]->covariance.stated());
    EXPECT_EQ(read->project.coordinateSystem, survey::DeclaredCoordinateSystem::named("Local"));
    EXPECT_EQ(read->project.points.front().code, "C&D");
}

TEST(TrimbleJobXml, everyTruncationOfASmallJobEndsInAnErrorOrAProjectNeverACrash)
{
    const std::string bytes = readFile(dataDir() / "trimble_jxl" / "one_setup.jxl");
    ASSERT_GT(bytes.size(), 1000u);
    std::size_t refused = 0;
    for (std::size_t length = 0; length <= bytes.size(); ++length) {
        Result<ReadResult> read = readJxl(std::string_view(bytes).substr(0, length));
        if (!read.ok()) {
            ++refused;
            EXPECT_FALSE(read.error().message.empty());
        }
    }
    // Only the whole file (and the whole file less trailing white space) reads.
    EXPECT_GE(refused, bytes.size() - 2);
}

TEST(TrimbleJobXml, randomBytesAndRandomlyDamagedJobsEndInAnErrorOrAProjectNeverACrash)
{
    std::mt19937 random(20240305u); // fixed seed: a failure must be repeatable
    std::uniform_int_distribution<int> byte(0, 255);
    for (int round = 0; round < 300; ++round) {
        std::string noise(static_cast<std::size_t>(random() % 2048), '\0');
        for (char& c : noise) {
            c = static_cast<char>(byte(random));
        }
        if (round % 3 == 0) {
            noise = "<JOBFile><FieldBook>" + noise; // get past the root check
        }
        (void)readJxl(noise);
    }
    const std::string& bytes = fixture();
    const std::string alphabet = "<>/&;\"'=!?[]- \nabc0123456789.";
    for (int round = 0; round < 400; ++round) {
        std::string damaged = bytes;
        const int edits = 1 + static_cast<int>(random() % 8);
        for (int e = 0; e < edits; ++e) {
            damaged[random() % damaged.size()] = alphabet[random() % alphabet.size()];
        }
        Result<ReadResult> read = readJxl(damaged);
        if (read.ok()) {
            EXPECT_TRUE(survey::validateProject(read->project).ok());
        }
    }
}

TEST(TrimbleJobXml, aRecordThatWouldHoldFarMoreThanItsOwnSizeIsSkippedWithAWarningAndTheRestRead)
{
    // A crafted record: an element with a 32 KiB name, holding 16,384 empty
    // elements. Every one of those is a value whose path repeats the long
    // name, so holding them all would take 16,384 x 32 KiB = 512 MiB for a
    // file of about 128 KiB - and twice the file, four times the memory. The
    // reader stops holding a record past its limit (16 MiB), skips it with a
    // warning on the line it starts on (2), and reads the point after it.
    const std::string longName(32 * 1024, 'A');
    std::string job = "<JOBFile version=\"5.72\"><FieldBook>\n<NoteRecord ID=\"1\"><" + longName + ">";
    for (int i = 0; i < 16384; ++i) {
        job += "<b/>";
    }
    job += "</" + longName + "></NoteRecord>\n"
           "<PointRecord ID=\"2\"><Name>P</Name><Method>Coordinates</Method><Deleted>false</Deleted>"
           "<Grid><North>1</North><East>2</East><Elevation>3</Elevation></Grid></PointRecord>\n"
           "</FieldBook></JOBFile>\n";
    const auto start = std::chrono::steady_clock::now();
    Result<ReadResult> read = readJxl(job, "crafted.jxl");
    const double seconds =
        std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(anyWarningContains(*read, "crafted.jxl record 2: the NoteRecord record holds more"))
        << (read->warnings.empty() ? std::string("no warning") : describe(read->warnings.front()));
    EXPECT_EQ(read->recordsSkipped, 1u);
    EXPECT_EQ(read->recordsRead, 1u);
    ASSERT_EQ(read->project.points.size(), 1u);
    EXPECT_EQ(read->project.points.front().id, "P");
    EXPECT_FALSE(read->project.metadata.contains("jxl.note.1")); // the note was not half read
    // It reads no more than the 16 MiB before it stops: well under a second
    // even in a Debug build; the bound is loose for a busy machine.
    EXPECT_LT(seconds, 10.0);
}

// ---- Throughput ----------------------------------------------------------------------------

namespace {

// A job of `setups` setups, each with `shots` shots on both faces, in the
// fixture's shape.
std::string syntheticJob(std::size_t targetBytes)
{
    std::string job = "<?xml version=\"1.0\"?>\n<JOBFile jobName=\"BIG\" version=\"5.72\" "
                      "product=\"Trimble Access\" productVersion=\"2024.10\" "
                      "TimeStamp=\"2024-03-05T09:00:00\">\n<FieldBook>\n"
                      "<AtmosphereRecord ID=\"a\"><Pressure>1013.25</Pressure><Temperature>20"
                      "</Temperature><PPM>0</PPM><ApplyPPMToRawDistances>true"
                      "</ApplyPPMToRawDistances></AtmosphereRecord>\n"
                      "<TargetRecord ID=\"t\"><PrismType>CustomPrism</PrismType><PrismConstant>0"
                      "</PrismConstant><TargetHeight>1.8</TargetHeight></TargetRecord>\n";
    job.reserve(targetBytes + 4096);
    std::size_t setup = 0;
    std::size_t record = 0;
    while (job.size() < targetBytes) {
        ++setup;
        const std::string stationId = "s" + std::to_string(setup);
        job += "<StationRecord ID=\"" + stationId + "\" TimeStamp=\"2024-03-05T09:03:00\">"
               "<StationName>STN" + std::to_string(setup) + "</StationName><TheodoliteHeight>1.5"
               "</TheodoliteHeight><AtmosphereID>a</AtmosphereID><ScaleFactor>1</ScaleFactor>"
               "</StationRecord>\n";
        for (int shot = 0; shot < 100; ++shot) {
            for (int face = 1; face <= 2; ++face) {
                ++record;
                job += "<PointRecord ID=\"p" + std::to_string(record) +
                       "\" TimeStamp=\"2024-03-05T09:05:00\">\n  <Name>P" + std::to_string(setup) +
                       "_" + std::to_string(shot) +
                       "</Name>\n  <Code>KB</Code>\n  <Method>DirectReading</Method>\n  "
                       "<SurveyMethod>Fix</SurveyMethod>\n  <Classification>Normal</Classification>\n"
                       "  <Deleted>false</Deleted>\n  <Circle>\n    <HorizontalCircle>" +
                       std::to_string(face == 1 ? 12.3456 + shot : 192.3456 + shot) +
                       "</HorizontalCircle>\n    <VerticalCircle>" +
                       std::to_string(face == 1 ? 88.1234 : 271.8766) +
                       "</VerticalCircle>\n    <EDMDistance>" + std::to_string(25.0 + shot * 0.5) +
                       "</EDMDistance>\n    <Face>Face" + std::to_string(face) +
                       "</Face>\n    <HorizontalCircleStandardError>0.000277777777777778"
                       "</HorizontalCircleStandardError>\n    <VerticalCircleStandardError>"
                       "0.000277777777777778</VerticalCircleStandardError>\n    "
                       "<EDMDistanceStandardError>0.00105</EDMDistanceStandardError>\n  </Circle>\n"
                       "  <StationID>" + stationId + "</StationID>\n  <TargetID>t</TargetID>\n"
                       "  <Pressure>1013.25</Pressure>\n  <Temperature>20</Temperature>\n"
                       "</PointRecord>\n";
            }
        }
    }
    job += "</FieldBook>\n<Reductions/>\n</JOBFile>\n";
    return job;
}

} // namespace

// Correctness only in ctest; the speed is printed. KATANA_SURVEY_THROUGHPUT_MB
// sets the size (default 4 MB, so the suite stays quick in a Debug build).
TEST(TrimbleJobXml, aLargeSyntheticJobReadsEveryShotAndReportsItsThroughput)
{
    std::size_t megabytes = 4;
    if (const char* requested = std::getenv("KATANA_SURVEY_THROUGHPUT_MB")) {
        megabytes = static_cast<std::size_t>(std::strtoul(requested, nullptr, 10));
    }
    const std::string job = syntheticJob(megabytes * 1024 * 1024);
    // The reader alone first (the parser's own speed), then through
    // readSurvey, which adds the project validation every import pays.
    const FormatReader reader = formatRegistry().reader(kId);
    ASSERT_TRUE(reader);
    const auto readerStart = std::chrono::steady_clock::now();
    Result<ReadResult> direct = reader(job, "big.jxl", ReadOptions{});
    const auto readerSeconds =
        std::chrono::duration<double>(std::chrono::steady_clock::now() - readerStart).count();
    ASSERT_TRUE(direct.ok()) << direct.error().describe();
    direct = ReadResult{}; // not timed: the next read should not share the heap with this one
    const auto start = std::chrono::steady_clock::now();
    Result<ReadResult> read = readJxl(job, "big.jxl");
    const auto seconds =
        std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
    ASSERT_TRUE(read.ok()) << read.error().describe();
    std::size_t observations = 0;
    for (const survey::SurveyStation& station : read->project.stations) {
        observations += station.observations.size();
    }
    // Every setup has 100 shots x 2 faces x (HA, ZA, SD).
    EXPECT_EQ(observations, read->project.stations.size() * 600);
    EXPECT_TRUE(read->warnings.empty());
    const double mb = static_cast<double>(job.size()) / (1024.0 * 1024.0);
    std::cout << "[ throughput ] JobXML: " << mb << " MB, " << observations << " observations; reader "
              << readerSeconds << " s = " << mb / readerSeconds << " MB/s; readSurvey (validation "
              << "included) " << seconds << " s = " << mb / seconds << " MB/s\n";
    RecordProperty("reader_megabytes_per_second", std::to_string(mb / readerSeconds));
    RecordProperty("megabytes_per_second", std::to_string(mb / seconds));
}

namespace {

// `count` keyed-in grid points of one class, one record each: the shape of a
// list of control marks imported into a job.
std::string keyedInJob(std::size_t count, std::string_view classification)
{
    std::string job = "<JOBFile version=\"5.72\"><FieldBook>\n";
    job.reserve(count * 240);
    for (std::size_t i = 1; i <= count; ++i) {
        const std::string n = std::to_string(i);
        job += "<PointRecord ID=\"" + n + "\"><Name>CM" + n +
               "</Name><Method>Coordinates</Method><SurveyMethod>KeyedIn</SurveyMethod>"
               "<Classification>" + std::string(classification) +
               "</Classification><Deleted>false</Deleted><Grid><North>" + n + "</North><East>" + n +
               "</East><Elevation>1</Elevation></Grid></PointRecord>\n";
    }
    job += "</FieldBook></JOBFile>\n";
    return job;
}

} // namespace

TEST(TrimbleJobXml, aControlListReadsInAboutTheTimeOfTheSameListOfOrdinaryPoints)
{
    // 12,000 control marks. Were each checked against every control point
    // listed before it, the read would make 12,000 x 11,999 / 2 = 72 million
    // name comparisons - several times the whole of the rest of the read,
    // and growing with the square of the list. Listed once each, a control
    // list is the same work as the same list classed Normal plus one entry
    // per point. Best of three, taken in turn, so a busy machine slows both;
    // a factor of 3 leaves room for the rest of its noise.
    constexpr std::size_t kCount = 12000;
    const std::string control = keyedInJob(kCount, "Control");
    const std::string normal = keyedInJob(kCount, "Normal");
    const FormatReader reader = formatRegistry().reader(kId);
    ASSERT_TRUE(reader);
    double best[2] = {1e9, 1e9};
    for (int round = 0; round < 3; ++round) {
        for (int kind = 0; kind < 2; ++kind) {
            const auto start = std::chrono::steady_clock::now();
            Result<ReadResult> read = reader(kind == 0 ? control : normal, "list.jxl", ReadOptions{});
            const double seconds =
                std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
            ASSERT_TRUE(read.ok()) << read.error().describe();
            ASSERT_EQ(read->project.points.size(), kCount);
            ASSERT_EQ(read->project.controlPoints.size(), kind == 0 ? kCount : 0u);
            best[kind] = std::min(best[kind], seconds);
        }
    }
    std::cout << "[ scaling ] JobXML: " << kCount << " control points " << best[0] << " s, as many "
              << "ordinary points " << best[1] << " s\n";
    EXPECT_LT(best[0], 3.0 * best[1]);
}

TEST(TrimbleJobXml, aControlMarkRecordedTwiceIsListedAsControlOnce)
{
    // A re-imported control list repeats its marks: A twice (the first kept,
    // with its height; the second skipped, its values in a warning), then B
    // with no height.
    const std::string job =
        "<JOBFile version=\"5.72\"><FieldBook>"
        "<PointRecord ID=\"1\"><Name>A</Name><Method>Coordinates</Method>"
        "<Classification>Control</Classification><Deleted>false</Deleted>"
        "<Grid><North>1</North><East>2</East><Elevation>3</Elevation></Grid></PointRecord>"
        "<PointRecord ID=\"2\"><Name>A</Name><Method>Coordinates</Method>"
        "<Classification>Control</Classification><Deleted>false</Deleted>"
        "<Grid><North>1</North><East>2</East><Elevation>3</Elevation></Grid></PointRecord>"
        "<PointRecord ID=\"3\"><Name>B</Name><Method>Coordinates</Method>"
        "<Classification>Control</Classification><Deleted>false</Deleted>"
        "<Grid><North>5</North><East>6</East><Elevation/></Grid></PointRecord>"
        "</FieldBook></JOBFile>";
    Result<ReadResult> read = readJxl(job);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->project.controlPoints,
              (std::vector<survey::ControlPoint>{survey::ControlPoint::fixed3d("A"),
                                                 survey::ControlPoint::fixedHorizontal("B")}));
    EXPECT_EQ(read->recordsRead, 2u);
    EXPECT_EQ(read->recordsSkipped, 1u);
}
