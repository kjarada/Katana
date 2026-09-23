// The drawing's survey points (include/katana/cad/survey_points.hpp): the list
// the Point Manager and the Point Report show, the export's points, what an
// import does with ids the drawing already has, and the stated transformation.
//
// Coordinates differ in magnitude between easting and northing (E ~ 500 000,
// N ~ 7 000 000 or E ~ 1 000, N ~ 5 000), so a transposition cannot pass.
//
// In customisation/ only because that test directory is globbed.

#include <gtest/gtest.h>

#include <optional>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
namespace cmd = katana::commands;
namespace survey = katana::survey;

namespace {

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

survey::SurveyPoint pointAt(std::string id, double northing, double easting,
                            std::optional<double> elevation = std::nullopt, std::string code = {})
{
    survey::SurveyPoint point;
    point.id = std::move(id);
    point.northing = northing;
    point.easting = easting;
    point.elevation = elevation;
    point.code = std::move(code);
    point.source.format = "Delimited points";
    point.source.fileName = "site.csv";
    return point;
}

survey::SurveyProject projectOf(std::vector<survey::SurveyPoint> points)
{
    survey::SurveyProject project;
    project.units.linear = survey::LinearUnit::Metres;
    project.points = std::move(points);
    return project;
}

// Imports under the default options, or fails the test.
void importInto(Document& document, const survey::SurveyProject& project,
                ExistingPointPolicy policy = ExistingPointPolicy::Refuse)
{
    auto command = importSurveyPoints(document, project, {}, policy);
    ASSERT_TRUE(command.ok()) << command.error().describe();
    ASSERT_NE(*command, nullptr);
    ASSERT_TRUE(document.execute(std::move(*command)).ok());
}

EntityId addCadPoint(Document& document, Point2 position)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{position};
    entity.layer = document.currentLayer();
    EXPECT_TRUE(document.execute(cmd::createEntities({std::move(entity)})).ok());
    return document.lastCreatedEntities().front();
}

} // namespace

TEST(SurveyPoints, ImportedPointsListBackAsTheyWentInWithAnAbsentElevationStillAbsent)
{
    Document document;
    importInto(document, projectOf({pointAt("101", 7000000.5, 500000.25, 12.5, "CP"),
                                    pointAt("102", 7000010.0, 500020.0)}));
    const auto points = drawingSurveyPoints(document);
    ASSERT_EQ(points.size(), 2u);
    EXPECT_EQ(points[0].id, "101");
    EXPECT_EQ(points[0].code, "CP");
    EXPECT_DOUBLE_EQ(points[0].easting, 500000.25);
    EXPECT_DOUBLE_EQ(points[0].northing, 7000000.5);
    ASSERT_TRUE(points[0].elevation.has_value());
    EXPECT_DOUBLE_EQ(*points[0].elevation, 12.5);
    EXPECT_EQ(points[0].sourceFile, "site.csv");
    EXPECT_EQ(points[0].layer, "survey/points");
    EXPECT_EQ(points[1].id, "102");
    EXPECT_FALSE(points[1].elevation.has_value()) << "absent is not zero";

    // And back to the survey model, north first, for a writer.
    const survey::SurveyPoint back = toSurveyPoint(points[0]);
    EXPECT_DOUBLE_EQ(back.northing, 7000000.5);
    EXPECT_DOUBLE_EQ(back.easting, 500000.25);
    EXPECT_EQ(back.code, "CP");
    EXPECT_FALSE(toSurveyPoint(points[1]).elevation.has_value());
}

TEST(SurveyPoints, APointEntityWithNoNumberIsNotASurveyPointAndIsCountedWhenAskedFor)
{
    Document document;
    importInto(document, projectOf({pointAt("7", 5000.0, 1000.0, 3.0)}));
    const EntityId cad = addCadPoint(document, Point2(1001.0, 5001.0));
    EXPECT_EQ(drawingSurveyPoints(document).size(), 1u);

    const EntityId survey = drawingSurveyPoints(document).front().entity;
    const std::vector<EntityId> asked{cad, survey, EntityId{999999}};
    const SurveyPointPick pick = surveyPointsAmong(document, asked);
    ASSERT_EQ(pick.points.size(), 1u);
    EXPECT_EQ(pick.points.front().id, "7");
    EXPECT_EQ(pick.notSurveyPoints, 2u) << "the CAD point and the id the drawing lacks";
}

TEST(SurveyPoints, AnImportRefusedForIdsAlreadyInTheDrawingNamesThemAndChangesNothing)
{
    Document document;
    importInto(document, projectOf({pointAt("1", 5000.0, 1000.0), pointAt("2", 5010.0, 1010.0)}));
    const auto command = importSurveyPoints(
        document, projectOf({pointAt("2", 5020.0, 1020.0), pointAt("3", 5030.0, 1030.0)}), {},
        ExistingPointPolicy::Refuse);
    ASSERT_FALSE(command.ok());
    EXPECT_EQ(command.error().code, ErrorCode::AlreadyExists);
    EXPECT_EQ(command.error().context, "2");
    EXPECT_EQ(drawingSurveyPoints(document).size(), 2u);
}

TEST(SurveyPoints, SkippingKeepsTheDrawingsPointAndImportsOnlyTheNewIds)
{
    Document document;
    importInto(document, projectOf({pointAt("1", 5000.0, 1000.0)}));
    SurveyPointImportReport report;
    auto command = importSurveyPoints(
        document, projectOf({pointAt("1", 5555.0, 1555.0), pointAt("2", 5010.0, 1010.0)}), {},
        ExistingPointPolicy::Skip, &report);
    ASSERT_TRUE(command.ok()) << command.error().describe();
    EXPECT_EQ(report.skipped, 1u);
    EXPECT_EQ(report.import.points, 1u);
    ASSERT_EQ(report.existingIds, std::vector<std::string>{"1"});
    ASSERT_TRUE(document.execute(std::move(*command)).ok());

    const auto points = drawingSurveyPoints(document);
    ASSERT_EQ(points.size(), 2u);
    EXPECT_EQ(points[0].id, "1");
    EXPECT_DOUBLE_EQ(points[0].easting, 1000.0) << "the drawing's point 1 is kept";
    EXPECT_EQ(points[1].id, "2");
}

TEST(SurveyPoints, EveryPointSkippedIsNothingToDoNotAnError)
{
    Document document;
    importInto(document, projectOf({pointAt("1", 5000.0, 1000.0)}));
    SurveyPointImportReport report;
    const auto command = importSurveyPoints(document, projectOf({pointAt("1", 5.0, 1.0)}), {},
                                            ExistingPointPolicy::Skip, &report);
    ASSERT_TRUE(command.ok());
    EXPECT_EQ(*command, nullptr);
    EXPECT_EQ(report.skipped, 1u);
}

TEST(SurveyPoints, ReplacingIsOneCommandThatOneUndoPutsBack)
{
    Document document;
    importInto(document, projectOf({pointAt("1", 5000.0, 1000.0, 10.0)}));
    SurveyPointImportReport report;
    auto command = importSurveyPoints(
        document, projectOf({pointAt("1", 5555.0, 1555.0, 20.0), pointAt("2", 5010.0, 1010.0)}),
        {}, ExistingPointPolicy::Replace, &report);
    ASSERT_TRUE(command.ok()) << command.error().describe();
    EXPECT_EQ(report.replaced, 1u);
    ASSERT_TRUE(document.execute(std::move(*command)).ok());

    auto points = drawingSurveyPoints(document);
    ASSERT_EQ(points.size(), 2u);
    EXPECT_EQ(points[0].id, "1");
    EXPECT_DOUBLE_EQ(points[0].easting, 1555.0) << "the file's point 1 replaced the drawing's";
    EXPECT_DOUBLE_EQ(*points[0].elevation, 20.0);

    ASSERT_TRUE(document.undo().ok());
    points = drawingSurveyPoints(document);
    ASSERT_EQ(points.size(), 1u);
    EXPECT_DOUBLE_EQ(points[0].easting, 1000.0);
    EXPECT_DOUBLE_EQ(*points[0].elevation, 10.0);
}

TEST(SurveyPoints, KeepingBothLeavesTwoPointsOfThatIdAndSaysSo)
{
    Document document;
    importInto(document, projectOf({pointAt("1", 5000.0, 1000.0)}));
    SurveyPointImportReport report;
    auto command = importSurveyPoints(document, projectOf({pointAt("1", 5555.0, 1555.0)}), {},
                                      ExistingPointPolicy::KeepBoth, &report);
    ASSERT_TRUE(command.ok());
    ASSERT_TRUE(document.execute(std::move(*command)).ok());
    EXPECT_EQ(drawingSurveyPoints(document).size(), 2u);
    ASSERT_FALSE(report.warnings.empty());
    EXPECT_TRUE(contains(report.warnings.front(), "in the drawing twice")) << report.warnings.front();
}

TEST(SurveyPoints, TheSouthernUtmZoneIsTheNorthernOneTenMillionMetresUp)
{
    // UTM by definition (NGA.SIG.0012): the northern and southern zones of the
    // same number share the projection and differ only in the false northing,
    // 0 in the north and 10 000 000 m in the south. So a WGS 84 point at
    // (E 400 000, N 5 000) in zone 30N (EPSG:32630) is (400 000, 10 005 000)
    // in zone 30S (EPSG:32730), and the height is not touched.
    auto moved = transformSurveyProject(
        projectOf({pointAt("A", 5000.0, 400000.0, 25.0), pointAt("B", 6000.0, 410000.0)}), 32630,
        32730);
    ASSERT_TRUE(moved.ok()) << moved.error().describe();
    ASSERT_EQ(moved->points.size(), 2u);
    EXPECT_NEAR(moved->points[0].easting, 400000.0, 1e-6);
    EXPECT_NEAR(moved->points[0].northing, 10005000.0, 1e-6);
    ASSERT_TRUE(moved->points[0].elevation.has_value());
    EXPECT_DOUBLE_EQ(*moved->points[0].elevation, 25.0);
    EXPECT_FALSE(moved->points[1].elevation.has_value()) << "absent stays absent";
    EXPECT_NEAR(moved->points[1].northing, 10006000.0, 1e-6);
    EXPECT_EQ(moved->coordinateSystem.epsgCode, 32730);
    EXPECT_FALSE(moved->coordinateSystem.unknown);
    EXPECT_TRUE(moved->metadata.contains("transformed from"));
}

TEST(SurveyPoints, ASystemInUsSurveyFeetIsFedFeetAndItsFeetComeBackAsMetres)
{
    // EPSG:32118 (NAD83 / New York Long Island, metres) and EPSG:2263 (the same
    // projection and datum in US survey feet) describe the SAME grid, so a
    // point keeps its metres through a transformation between them - but only
    // if the metres are turned into feet for the second system's side and
    // back. Read as if it were metres, 300 000 ftUS would come back as
    // 300 000 x 1200/3937 = 91 440.18 m.
    auto moved =
        transformSurveyProject(projectOf({pointAt("1", 60000.0, 300000.0)}), 32118, 2263);
    ASSERT_TRUE(moved.ok()) << moved.error().describe();
    EXPECT_NEAR(moved->points[0].easting, 300000.0, 1e-4);
    EXPECT_NEAR(moved->points[0].northing, 60000.0, 1e-4);
}

TEST(SurveyPoints, AGeographicSystemIsRefusedBecausePointsAreGridCoordinates)
{
    const auto moved = transformSurveyProject(projectOf({pointAt("1", 5000.0, 1000.0)}), 4326,
                                              32630);
    ASSERT_FALSE(moved.ok());
    EXPECT_EQ(moved.error().code, ErrorCode::Unsupported);
    EXPECT_FALSE(declaredSystemFromEpsg(0).ok());
    const auto declared = declaredSystemFromEpsg(32630);
    ASSERT_TRUE(declared.ok()) << declared.error().describe();
    EXPECT_EQ(declared->epsgCode, 32630);
}

TEST(SurveyPoints, ThePointReportAndItsCsvLeaveAnAbsentElevationBlank)
{
    Document document;
    importInto(document, projectOf({pointAt("101", 7000000.5, 500000.25, 12.5, "CP"),
                                    pointAt("10,2", 7000010.0, 500020.0)}));
    const auto points = drawingSurveyPoints(document);
    const std::string report = formatPointReport(points, 3);
    EXPECT_TRUE(contains(report, "500000.250")) << report;
    EXPECT_TRUE(contains(report, "7000000.500")) << report;
    EXPECT_TRUE(contains(report, "2 point(s), 1 without an elevation")) << report;
    // The second point's elevation is empty between its northing and its
    // (empty) code; its id holds a comma and is quoted.
    EXPECT_EQ(pointReportCsv(points, 3),
              "Point,Easting,Northing,Elevation,Code,Description,Source file\r\n"
              "101,500000.250,7000000.500,12.500,CP,,site.csv\r\n"
              "\"10,2\",500020.000,7000010.000,,,,site.csv\r\n");
}

TEST(SurveyPoints, TheImportReportSaysWhatWillBeCreatedOrWhatBlocksIt)
{
    Document document;
    SurveyImportSummary summary;
    summary.fileName = "site.csv";
    summary.format = "Delimited points 1.0";
    summary.recordsRead = 2;
    summary.coordinateSystem = "unknown";
    summary.units = "metres";
    summary.readerWarnings = {"1 point has no elevation"};
    SurveyPointImportReport report;
    const auto command = importSurveyPoints(
        document, projectOf({pointAt("1", 5000.0, 1000.0), pointAt("2", 5010.0, 1010.0)}), {},
        ExistingPointPolicy::Refuse, &report);
    ASSERT_TRUE(command.ok());
    const std::string text = formatSurveyImportReport(summary, report);
    EXPECT_TRUE(contains(text, "Records read: 2")) << text;
    EXPECT_TRUE(contains(text, "  - 1 point has no elevation.")) << text;
    EXPECT_TRUE(contains(text, "Points to create: 2")) << text;
    EXPECT_TRUE(contains(text, "Layers to create: survey/points")) << text;

    const katana::core::Error blocking{ErrorCode::AlreadyExists, "ids already there", "2"};
    const std::string blocked = formatSurveyImportReport(summary, report, &blocking);
    EXPECT_TRUE(contains(blocked, "Import blocked:")) << blocked;
    EXPECT_FALSE(contains(blocked, "Points to create")) << blocked;
}
