// Survey jobs (cad/survey_job.hpp): import, re-adjust and remove, each one
// undo step, with the reduction and the reader INJECTED - these tests hand
// the commands a fake reduction whose coordinates are known, so what is
// tested is the job and the drawing, not the reduction's arithmetic.

#include <gtest/gtest.h>

#include <filesystem>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <variant>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"
#include "katana/storage/project_store.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
namespace survey = katana::survey;
namespace fs = std::filesystem;

namespace {

struct Coordinates {
    double northing = 0.0;
    double easting = 0.0;
    std::optional<double> elevation{};
};

// Stands in for survey::reduceAndAdjust: whatever `points` holds when it is
// called is what the "reduction" computes, and every call is recorded.
struct FakeReduction {
    std::map<std::string, Coordinates> points;
    // Named by the file with no coordinates and left so by the "reduction",
    // as survey::reduceAndAdjust leaves a point it does not compute - and a
    // control point it takes from the drawing, which it does not compute.
    std::vector<std::string> unpositioned{};
    std::optional<katana::core::Error> failure{};
    int calls = 0;
    survey::ReductionSettings lastSettings{};
    survey::ReductionContext lastContext{};
    survey::SurveyProject lastRaw{};
};

ReductionFunction reductionOf(const std::shared_ptr<FakeReduction>& fake)
{
    return [fake](const survey::SurveyProject& raw, const survey::ReductionSettings& settings,
                  const survey::ReductionContext& context) -> Result<survey::ReductionOutcome> {
        ++fake->calls;
        fake->lastRaw = raw;
        fake->lastSettings = settings;
        fake->lastContext = context;
        if (fake->failure) {
            return *fake->failure;
        }
        survey::ReductionOutcome outcome;
        outcome.reduced.name = raw.name;
        for (const auto& [id, at] : fake->points) {
            survey::SurveyPoint point;
            point.id = id;
            point.northing = at.northing;
            point.easting = at.easting;
            point.elevation = at.elevation;
            point.code = "PEG";
            point.coordinateSource = survey::CoordinateSource::Calculated;
            outcome.reduced.points.push_back(point);
            survey::ComputedPoint computed;
            computed.id = id;
            computed.northing = at.northing;
            computed.easting = at.easting;
            computed.elevation = at.elevation;
            outcome.points.push_back(computed);
        }
        for (const std::string& id : fake->unpositioned) {
            survey::UnpositionedPoint point;
            point.id = id;
            outcome.reduced.unpositionedPoints.push_back(point);
        }
        outcome.report.createdUtc = context.createdUtc;
        outcome.report.input = context.input;
        outcome.report.settings = settings;
        return outcome;
    };
}

// The raw project a reader would have returned for the job's bytes; the fake
// reduction ignores its content, so a name is enough to recognise it.
survey::SurveyProject rawProject(std::string name)
{
    survey::SurveyProject raw;
    raw.name = std::move(name);
    return raw;
}

SurveyJobImport importRequest(std::string layer = "survey/day1")
{
    SurveyJobImport request;
    request.job.name = "DAY1.GSI";
    request.job.formatId = "leica-gsi";
    request.job.parserVersion = "1.0";
    request.job.sourceFileName = "DAY1.GSI";
    request.job.sourceBytes = std::string("*110001+0000000000000101\r\n\0\xff", 29);
    request.job.siblingFiles = {SurveyJobFile{"DAY1.X01", "sibling"}};
    request.job.settings.method = survey::AdjustmentMethod::Traverse;
    request.job.layer = layer;
    request.raw = rawProject("day one");
    request.context.createdUtc = "2026-09-24T10:00:00Z";
    request.importOptions.layer = layer;
    return request;
}

// The reader the application would bind to surveyio::readSurvey; here it
// checks it was handed the job's own stored bytes and records the call.
SurveyJobReader readerFor(int* calls, std::string expectedBytes)
{
    return [calls, expectedBytes](const SurveyJob& job) -> Result<survey::SurveyProject> {
        ++*calls;
        if (job.sourceBytes != expectedBytes) {
            return katana::core::makeError(ErrorCode::InvalidArgument, "not the stored bytes");
        }
        return rawProject("re-read " + job.sourceFileName);
    };
}

const Entity* pointEntity(const Document& document, std::string_view id)
{
    const Entity* found = nullptr;
    document.model().entities.forEach([&](const Entity& entity) {
        const auto property = entity.properties.find("point");
        if (property != entity.properties.end() &&
            katana::entity::toString(property->second) == id) {
            found = &entity;
        }
    });
    return found;
}

const SurveyJobPoint* placedPoint(const SurveyJob& job, std::string_view id)
{
    for (const SurveyJobPoint& placed : job.placedPoints) {
        if (placed.pointId == id) {
            return &placed;
        }
    }
    return nullptr;
}

katana::geometry::Point2 positionOf(const Entity& entity)
{
    return std::get<katana::entity::PointGeometry>(entity.geometry).position;
}

std::optional<double> elevationOf(const Entity& entity)
{
    return katana::entity::heightsOf(entity.properties, 1).front();
}

// Imports a job whose reduction computes 101, 102 and 103, and returns its id.
std::string importThreePoints(Document& document, const std::shared_ptr<FakeReduction>& fake,
                              ExistingPointPolicy policy = ExistingPointPolicy::Refuse)
{
    fake->points = {{"101", {6'250'000.0, 300'000.0, 10.0}},
                    {"102", {6'250'010.0, 300'020.0, 11.0}},
                    {"103", {6'250'030.0, 300'005.0, std::nullopt}}};
    SurveyJobImport request = importRequest();
    request.existingPoints = policy;
    auto command = std::make_unique<ImportSurveyJobCommand>(document, request, reductionOf(fake));
    auto* raw = command.get();
    const auto status = document.execute(std::move(command));
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    return raw->jobId();
}

// Somebody else's survey point, as the import wizard would have drawn it.
EntityId drawForeignPoint(Document& document, std::string id, double northing, double easting)
{
    survey::SurveyProject project;
    survey::SurveyPoint point;
    point.id = std::move(id);
    point.northing = northing;
    point.easting = easting;
    point.elevation = 5.0;
    project.points.push_back(point);
    SurveyImportOptions options;
    options.layer = "control";
    auto command = importSurveyPoints(document, project, options, ExistingPointPolicy::Refuse);
    EXPECT_TRUE(command.ok());
    EXPECT_TRUE(document.execute(std::move(*command)).ok());
    return document.lastCreatedEntities().front();
}

} // namespace

// ---- The Document's list -------------------------------------------------------------

TEST(SurveyJobs, ANewDocumentHasNoJobs)
{
    Document document;
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.findSurveyJob("job-1"), nullptr);
    EXPECT_EQ(document.surveyJobsGeneration(), 0U);
}

TEST(SurveyJobs, TheCommandsDoorFindsAndCountsChangesToTheJobList)
{
    Document document;
    SurveyJob job;
    job.id = "job-1";
    job.name = "site.gsi";
    SurveyJobAccess::jobs(document).push_back(job);
    SurveyJobAccess::changed(document);
    ASSERT_NE(document.findSurveyJob("job-1"), nullptr);
    EXPECT_EQ(document.findSurveyJob("job-1")->name, "site.gsi");
    EXPECT_EQ(document.surveyJobsGeneration(), 1U);
}

// ---- Import --------------------------------------------------------------------------

TEST(SurveyJobImportCommand, ImportingKeepsTheWholeJobAndDrawsItsPointsOnTheChosenLayer)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    const SurveyJobImport request = importRequest();
    const std::string id = importThreePoints(document, fake);

    ASSERT_FALSE(id.empty());
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    const SurveyJob& job = document.surveyJobs().front();
    EXPECT_EQ(job.id, id);
    // Everything the request carried is kept as it was...
    EXPECT_EQ(job.sourceBytes, request.job.sourceBytes);
    EXPECT_EQ(job.siblingFiles, request.job.siblingFiles);
    EXPECT_EQ(job.settings, request.job.settings);
    EXPECT_EQ(job.formatId, "leica-gsi");
    EXPECT_EQ(job.layer, "survey/day1");
    // ...the reduction ran with the job's settings on the raw project...
    EXPECT_EQ(fake->calls, 1); // once: validate's result is reused by execute
    EXPECT_EQ(fake->lastSettings, request.job.settings);
    EXPECT_EQ(fake->lastRaw.name, "day one");
    // ...and its report is the job's, rendered.
    EXPECT_EQ(job.reportCreatedUtc, "2026-09-24T10:00:00Z");
    EXPECT_EQ(job.importedUtc, "2026-09-24T10:00:00Z");
    EXPECT_FALSE(job.reportText.empty());
    EXPECT_FALSE(job.reportHtml.empty());

    // Three points on the job's layer, the survey model's (N, E) as (x, y).
    ASSERT_EQ(job.createdEntities.size(), 3U);
    ASSERT_EQ(job.placedPoints.size(), 3U);
    for (const SurveyJobPoint& placed : job.placedPoints) {
        const Entity* entity = document.model().entities.find(placed.entity);
        ASSERT_NE(entity, nullptr) << placed.pointId;
        EXPECT_EQ(entity->layer, "survey/day1");
        EXPECT_EQ(positionOf(*entity), katana::geometry::Point2(placed.easting, placed.northing));
        EXPECT_EQ(elevationOf(*entity), placed.elevation);
    }
    const Entity* p102 = pointEntity(document, "102");
    ASSERT_NE(p102, nullptr);
    EXPECT_EQ(positionOf(*p102), katana::geometry::Point2(300'020.0, 6'250'010.0));
    EXPECT_FALSE(elevationOf(*pointEntity(document, "103")).has_value()); // absent, not 0
}

TEST(SurveyJobImportCommand, OneUndoRemovesThePointsAndTheJobAndRedoBringsBothBackUnchanged)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    const std::string id = importThreePoints(document, fake);
    const SurveyJob imported = document.surveyJobs().front();
    const auto generation = document.surveyJobsGeneration();

    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.model().entities.size(), 0U);
    EXPECT_GT(document.surveyJobsGeneration(), generation);

    ASSERT_TRUE(document.redo().ok());
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    EXPECT_EQ(document.surveyJobs().front(), imported); // same id, same entity ids, all of it
    for (const SurveyJobPoint& placed : imported.placedPoints) {
        EXPECT_TRUE(document.model().entities.contains(placed.entity)) << placed.pointId;
    }
    EXPECT_EQ(fake->calls, 1) << "redo replays, it does not reduce again";
}

TEST(SurveyJobImportCommand, AReductionThatFailsImportsNothingAndSaysWhy)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    fake->failure = katana::core::makeError(ErrorCode::AdjustmentFailure,
                                            "the network has no control, so it cannot be held");
    const auto revision = document.modelRevision();
    const auto status = document.execute(
        std::make_unique<ImportSurveyJobCommand>(document, importRequest(), reductionOf(fake)));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::AdjustmentFailure);
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.model().entities.size(), 0U);
    EXPECT_EQ(document.modelRevision(), revision);
}

TEST(SurveyJobImportCommand, AJobNumberIsNeverHandedOutTwiceEvenAfterAnUndo)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    const std::string first = importThreePoints(document, fake);
    ASSERT_TRUE(document.undo().ok());
    // The same file again, after the first import was undone: a report or a
    // log line naming the first job must never come to mean this one.
    const std::string second = importThreePoints(document, fake);
    EXPECT_NE(first, second);
    fake->points.clear(); // a job that draws nothing still takes a number
    auto command =
        std::make_unique<ImportSurveyJobCommand>(document, importRequest(), reductionOf(fake));
    auto* raw = command.get();
    ASSERT_TRUE(document.execute(std::move(command)).ok());
    const std::string third = raw->jobId();
    const std::string fourth = importThreePoints(document, fake, ExistingPointPolicy::KeepBoth);
    EXPECT_NE(third, second);
    EXPECT_NE(fourth, third);
    EXPECT_EQ(document.surveyJobs().size(), 3U);
}

TEST(SurveyJobImportCommand, AControlPointTakenFromTheDrawingIsNotDrawnAgain)
{
    Document document;
    const EntityId control = drawForeignPoint(document, "CP1", 6'249'990.0, 299'990.0);
    auto fake = std::make_shared<FakeReduction>();
    fake->points = {{"CP1", {6'249'990.0, 299'990.0, 5.0}}, {"201", {6'250'001.0, 300'001.0, {}}}};
    SurveyJobImport request = importRequest();
    request.job.settings.control.push_back(survey::ControlSelection{
        survey::ControlPoint::fixedHorizontal("CP1"), survey::ControlOrigin::Drawing});
    ASSERT_TRUE(document
                    .execute(std::make_unique<ImportSurveyJobCommand>(document, request,
                                                                      reductionOf(fake)))
                    .ok());
    const SurveyJob& job = document.surveyJobs().front();
    ASSERT_EQ(job.placedPoints.size(), 1U);
    EXPECT_EQ(job.placedPoints.front().pointId, "201");
    EXPECT_EQ(drawingSurveyPoints(document).size(), 2U); // CP1 once, and 201
    EXPECT_TRUE(document.model().entities.contains(control));
}

// A file that gives no coordinates for the point it is held at - a traverse
// whose first station is taken from the drawing - leaves that point among the
// reduction's unpositioned ones, since the drawing, not the reduction, placed
// it. It has a position, so it is not one of the points the import says it
// could not draw for want of one: that sentence once counted it, and told the
// person to reduce the observations to place a point already held.
TEST(SurveyJobImportCommand, AControlPointTakenFromTheDrawingIsNotCountedAsHavingNoPosition)
{
    Document document;
    drawForeignPoint(document, "CP1", 6'249'990.0, 299'990.0);
    auto fake = std::make_shared<FakeReduction>();
    fake->points = {{"201", {6'250'001.0, 300'001.0, {}}}};
    fake->unpositioned = {"CP1", "205"};
    SurveyJobImport request = importRequest();
    request.job.settings.control.push_back(survey::ControlSelection{
        survey::ControlPoint::fixedHorizontal("CP1"), survey::ControlOrigin::Drawing});
    auto command = std::make_unique<ImportSurveyJobCommand>(document, request, reductionOf(fake));
    const ImportSurveyJobCommand* job = command.get();
    ASSERT_TRUE(document.execute(std::move(command)).ok());
    ASSERT_NE(job->report(), nullptr);
    std::vector<std::string> unplaced;
    for (const auto& warning : job->report()->warnings) {
        if (warning.text.find("named in the source with no coordinates") != std::string::npos) {
            unplaced.push_back(warning.text);
        }
    }
    // 205 alone: the file names it, and nothing placed it.
    ASSERT_EQ(unplaced.size(), 1U);
    EXPECT_TRUE(unplaced.front().starts_with("1 point(s) are named")) << unplaced.front();
    EXPECT_EQ(document.surveyJobs().front().placedPoints.size(), 1U);
}

TEST(SurveyJobImportCommand, APointTheDrawingAlreadyHasFollowsTheChosenPolicyAndIsReported)
{
    Document document;
    drawForeignPoint(document, "102", 1.0, 2.0);
    auto fake = std::make_shared<FakeReduction>();
    fake->points = {{"101", {10.0, 20.0, {}}}, {"102", {11.0, 21.0, {}}}};

    const auto refused = document.execute(
        std::make_unique<ImportSurveyJobCommand>(document, importRequest(), reductionOf(fake)));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::AlreadyExists);
    EXPECT_TRUE(document.surveyJobs().empty());

    SurveyJobImport skip = importRequest();
    skip.existingPoints = ExistingPointPolicy::Skip;
    auto command = std::make_unique<ImportSurveyJobCommand>(document, skip, reductionOf(fake));
    auto* raw = command.get();
    ASSERT_TRUE(document.execute(std::move(command)).ok());
    const SurveyJob& job = document.surveyJobs().front();
    ASSERT_EQ(job.placedPoints.size(), 1U);
    EXPECT_EQ(job.placedPoints.front().pointId, "101");
    // The skip is in the report of what happened to the data.
    ASSERT_NE(raw->report(), nullptr);
    bool reported = false;
    for (const auto& warning : raw->report()->warnings) {
        reported = reported || (warning.text.find("not imported") != std::string::npos &&
                                warning.text.find("102") != std::string::npos);
    }
    EXPECT_TRUE(reported);
}

// A field file the reader accepts (it takes up to 1 GiB) but the project
// could never save - SQLite holds at most 1,000,000,000 bytes in a value or a
// row - is refused at the import, with a sentence naming the file, before the
// reduction runs and before anything is drawn. Accepted, it would make every
// later save of the whole project fail. The bytes are never written, so the
// string costs address space, not memory.
TEST(SurveyJobImportCommand, AFieldFileTooLargeForTheProjectIsRefusedBeforeAnythingIsDone)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    fake->points = {{"101", {6'250'000.0, 300'000.0, 10.0}}};
    SurveyJobImport request = importRequest();
    request.job.sourceBytes.resize_and_overwrite(1'020'000'000,
                                                 [](char*, std::size_t size) { return size; });
    const auto revision = document.modelRevision();
    const auto status = document.execute(
        std::make_unique<ImportSurveyJobCommand>(document, std::move(request), reductionOf(fake)));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(status.error().message.find("DAY1.GSI"), std::string::npos)
        << status.error().message;
    EXPECT_NE(status.error().message.find("too large"), std::string::npos)
        << status.error().message;
    EXPECT_EQ(fake->calls, 0) << "refused before the reduction";
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.model().entities.size(), 0U);
    EXPECT_EQ(document.modelRevision(), revision);
}

// validate() can see only the file; the report and the point lists come with
// execute(). A file exactly as large as a job may be, once the rest of what
// the request holds is counted, passes the first check - and the job it makes
// does not. It is refused whole: nothing left drawn, no job on the list.
TEST(SurveyJobImportCommand, AJobItsReportAndPointsTakePastWhatAProjectHoldsIsRefusedWhole)
{
    using katana::storage::ProjectStore;
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    fake->points = {{"101", {6'250'000.0, 300'000.0, 10.0}}};
    SurveyJobImport request = importRequest();
    SurveyJob rest = request.job;
    rest.sourceBytes.clear();
    const std::uint64_t file =
        ProjectStore::kMaxSurveyJobBytes - ProjectStore::surveyJobRowBytes(rest);
    request.job.sourceBytes.resize_and_overwrite(file,
                                                 [](char*, std::size_t size) { return size; });
    ASSERT_EQ(ProjectStore::surveyJobRowBytes(request.job), ProjectStore::kMaxSurveyJobBytes);

    const auto status = document.execute(
        std::make_unique<ImportSurveyJobCommand>(document, std::move(request), reductionOf(fake)));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(status.error().message.find("DAY1.GSI is too large"), std::string::npos)
        << status.error().message;
    EXPECT_EQ(fake->calls, 1); // past the first check, refused at the second
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.model().entities.size(), 0U);
}

TEST(SurveyJobImportCommand, AJobSurvivesSavingAndOpeningAndANewDrawingHasNone)
{
    const fs::path root = fs::temp_directory_path() / "katana-cad-tests" / "survey-job-save";
    std::error_code ignored;
    fs::remove_all(root, ignored);
    fs::create_directories(root);
    SurveyJob saved;
    {
        Document document;
        auto fake = std::make_shared<FakeReduction>();
        importThreePoints(document, fake);
        saved = document.surveyJobs().front();
        ASSERT_TRUE(document.saveAs(root / "site.katana").ok());
        EXPECT_EQ(document.surveyJobs().front(), saved) << "the save lends the jobs, not keeps them";
        ASSERT_TRUE(document.save().ok()); // twice: the tables are rewritten, not appended to
    }
    Document reopened;
    const auto opened = reopened.open(root / "site.katana");
    ASSERT_TRUE(opened.ok()) << opened.error().describe();
    ASSERT_EQ(reopened.surveyJobs().size(), 1U);
    EXPECT_EQ(reopened.surveyJobs().front(), saved);
    reopened.newDocument();
    EXPECT_TRUE(reopened.surveyJobs().empty());
    fs::remove_all(root, ignored);
}

// ---- Re-adjust -----------------------------------------------------------------------

TEST(SurveyJobReadjustCommand, ReadjustingMovesOnlyTheJobsPointsAndOneUndoPutsEverythingBack)
{
    Document document;
    const EntityId foreign = drawForeignPoint(document, "900", 6'250'100.0, 300'100.0);
    auto fake = std::make_shared<FakeReduction>();
    const std::string id = importThreePoints(document, fake);
    const SurveyJob before = document.surveyJobs().front();
    const Entity foreignBefore = *document.model().entities.find(foreign);
    std::map<EntityId, Entity> entitiesBefore;
    document.model().entities.forEach(
        [&](const Entity& entity) { entitiesBefore.emplace(entity.id, entity); });

    // The new settings shift 101 and 102 by a few millimetres and give 103 a height.
    fake->points = {{"101", {6'250'000.004, 300'000.002, 10.001}},
                    {"102", {6'250'010.0, 300'020.0, 11.0}},
                    {"103", {6'250'029.997, 300'005.0, 3.25}},
                    {"900", {1.0, 1.0, 1.0}}}; // the job's reduction knows a 900 too
    SurveyJobReadjustment request;
    request.jobId = id;
    request.settings = before.settings;
    request.settings.method = survey::AdjustmentMethod::Network;
    request.settings.traverseRule = survey::TraverseRule::LeastSquares;
    request.context.createdUtc = "2026-09-25T08:00:00Z";
    request.parserVersion = "1.1";
    int reads = 0;
    auto command = std::make_unique<ReadjustSurveyJobCommand>(
        document, request, readerFor(&reads, before.sourceBytes), reductionOf(fake));
    auto* raw = command.get();
    const auto status = document.execute(std::move(command));
    ASSERT_TRUE(status.ok()) << status.error().describe();
    EXPECT_EQ(reads, 1);
    EXPECT_EQ(fake->lastRaw.name, "re-read DAY1.GSI");

    // The previous run handed to the reduction is what the job placed.
    ASSERT_EQ(fake->lastContext.previous.size(), 3U);
    EXPECT_EQ(fake->lastContext.previous[0].id, before.placedPoints[0].pointId);
    EXPECT_EQ(fake->lastContext.previous[0].northing, before.placedPoints[0].northing);

    const Entity* p101 = pointEntity(document, "101");
    ASSERT_NE(p101, nullptr);
    EXPECT_EQ(p101->id, before.placedPoints[0].entity) << "moved, not re-created";
    EXPECT_EQ(positionOf(*p101), katana::geometry::Point2(300'000.002, 6'250'000.004));
    EXPECT_EQ(elevationOf(*p101), 10.001);
    EXPECT_EQ(elevationOf(*pointEntity(document, "103")), 3.25);
    EXPECT_EQ(*document.model().entities.find(foreign), foreignBefore) << "not the job's point";
    // 900 is somebody else's id: not drawn over, and said so.
    EXPECT_EQ(raw->changes().notDrawn, std::vector<std::string>{"900"});
    EXPECT_EQ(raw->changes().moved, (std::vector<std::string>{"101", "103"}));

    const SurveyJob& after = document.surveyJobs().front();
    EXPECT_EQ(after.settings, request.settings);
    EXPECT_EQ(after.parserVersion, "1.1");
    EXPECT_EQ(after.reportCreatedUtc, "2026-09-25T08:00:00Z");
    EXPECT_EQ(after.sourceBytes, before.sourceBytes);
    EXPECT_EQ(after.placedPoints[0].northing, 6'250'000.004);

    // ONE undo: coordinates, settings, report - exactly.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.surveyJobs().front(), before);
    std::map<EntityId, Entity> entitiesAfterUndo;
    document.model().entities.forEach(
        [&](const Entity& entity) { entitiesAfterUndo.emplace(entity.id, entity); });
    EXPECT_EQ(entitiesAfterUndo, entitiesBefore);

    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(document.surveyJobs().front().settings, request.settings);
    EXPECT_EQ(positionOf(*pointEntity(document, "101")),
              katana::geometry::Point2(300'000.002, 6'250'000.004));
}

TEST(SurveyJobReadjustCommand, APointMovedByHandIsReportedAndKeptOrOverwrittenAsAsked)
{
    for (const HandEditPolicy policy : {HandEditPolicy::Keep, HandEditPolicy::Overwrite}) {
        SCOPED_TRACE(toString(policy));
        Document document;
        auto fake = std::make_shared<FakeReduction>();
        const std::string id = importThreePoints(document, fake);
        const EntityId p102 = pointEntity(document, "102")->id;
        // The person nudges 102 half a metre east.
        ASSERT_TRUE(document
                        .execute(katana::commands::moveEntities(
                            {p102}, katana::geometry::Vec2(0.5, 0.0)))
                        .ok());

        fake->points["102"] = {6'250'010.010, 300'020.010, 11.0};
        SurveyJobReadjustment request;
        request.jobId = id;
        request.handEdits = policy;
        int reads = 0;
        auto command = std::make_unique<ReadjustSurveyJobCommand>(
            document, request, readerFor(&reads, document.surveyJobs().front().sourceBytes),
            reductionOf(fake));
        auto* raw = command.get();
        ASSERT_TRUE(document.execute(std::move(command)).ok());

        EXPECT_EQ(raw->changes().editedByHand, std::vector<std::string>{"102"});
        ASSERT_NE(raw->report(), nullptr);
        bool reported = false;
        for (const auto& warning : raw->report()->warnings) {
            reported = reported || (warning.text.find("Point 102") != std::string::npos &&
                                    warning.text.find("by hand") != std::string::npos);
        }
        EXPECT_TRUE(reported);
        const auto position = positionOf(*document.model().entities.find(p102));
        if (policy == HandEditPolicy::Keep) {
            EXPECT_EQ(position, katana::geometry::Point2(300'020.5, 6'250'010.0));
        } else {
            EXPECT_EQ(position, katana::geometry::Point2(300'020.010, 6'250'010.010));
        }
    }
}

TEST(SurveyJobReadjustCommand, APointDeletedByHandStaysDeletedUnlessOverwriteDrawsItAgain)
{
    for (const HandEditPolicy policy : {HandEditPolicy::Keep, HandEditPolicy::Overwrite}) {
        SCOPED_TRACE(toString(policy));
        Document document;
        auto fake = std::make_shared<FakeReduction>();
        const std::string id = importThreePoints(document, fake);
        ASSERT_TRUE(document
                        .execute(katana::commands::deleteEntities(
                            {pointEntity(document, "103")->id}))
                        .ok());

        SurveyJobReadjustment request;
        request.jobId = id;
        request.handEdits = policy;
        int reads = 0;
        auto command = std::make_unique<ReadjustSurveyJobCommand>(
            document, request, readerFor(&reads, document.surveyJobs().front().sourceBytes),
            reductionOf(fake));
        auto* raw = command.get();
        ASSERT_TRUE(document.execute(std::move(command)).ok());
        EXPECT_EQ(raw->changes().deletedByHand, std::vector<std::string>{"103"});
        const Entity* p103 = pointEntity(document, "103");
        if (policy == HandEditPolicy::Keep) {
            EXPECT_EQ(p103, nullptr);
            EXPECT_TRUE(raw->changes().created.empty());
        } else {
            ASSERT_NE(p103, nullptr);
            EXPECT_EQ(p103->layer, "survey/day1");
            EXPECT_EQ(raw->changes().created, std::vector<std::string>{"103"});
        }
        EXPECT_EQ(document.surveyJobs().front().placedPoints.size(), 3U);
    }
}

TEST(SurveyJobReadjustCommand, APointNoLongerComputedIsDeletedAndANewOneIsDrawnOnTheJobsLayer)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    const std::string id = importThreePoints(document, fake);
    const EntityId old103 = pointEntity(document, "103")->id;
    fake->points.erase("103");
    fake->points["104"] = {6'250'040.0, 300'040.0, 12.0};

    SurveyJobReadjustment request;
    request.jobId = id;
    int reads = 0;
    auto command = std::make_unique<ReadjustSurveyJobCommand>(
        document, request, readerFor(&reads, document.surveyJobs().front().sourceBytes),
        reductionOf(fake));
    auto* raw = command.get();
    ASSERT_TRUE(document.execute(std::move(command)).ok());
    EXPECT_EQ(raw->changes().removed, std::vector<std::string>{"103"});
    EXPECT_EQ(raw->changes().created, std::vector<std::string>{"104"});
    EXPECT_FALSE(document.model().entities.contains(old103));
    const Entity* p104 = pointEntity(document, "104");
    ASSERT_NE(p104, nullptr);
    EXPECT_EQ(p104->layer, "survey/day1");
    const SurveyJob& job = document.surveyJobs().front();
    ASSERT_EQ(job.placedPoints.size(), 3U);
    EXPECT_EQ(job.placedPoints.back().pointId, "104");
    EXPECT_EQ(job.placedPoints.back().entity, p104->id);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.model().entities.contains(old103));
    EXPECT_EQ(pointEntity(document, "104"), nullptr);
}

// A control mark the job drew itself - held from the FILE at the import - and
// then held FROM THE DRAWING by a re-adjustment. The new run took the point
// where the drawing has it, so there is nothing to move it to; deleting it
// would take away the very mark the adjustment was just held to, and the
// next "Edit adjustment" with the same settings would find no CP1 to hold.
TEST(SurveyJobReadjustCommand, AJobPointTheNewSettingsHoldFromTheDrawingIsLeftWhereItIs)
{
    for (const HandEditPolicy policy : {HandEditPolicy::Keep, HandEditPolicy::Overwrite}) {
        SCOPED_TRACE(toString(policy));
        Document document;
        auto fake = std::make_shared<FakeReduction>();
        fake->points = {{"CP1", {6'249'990.0, 299'990.0, 5.0}},
                        {"201", {6'250'001.0, 300'001.0, {}}}};
        SurveyJobImport request = importRequest();
        request.job.settings.control.push_back(survey::ControlSelection{
            survey::ControlPoint::fixedHorizontal("CP1"), survey::ControlOrigin::File});
        auto import = std::make_unique<ImportSurveyJobCommand>(document, request, reductionOf(fake));
        auto* imported = import.get();
        ASSERT_TRUE(document.execute(std::move(import)).ok());
        const std::string id = imported->jobId();
        ASSERT_EQ(document.surveyJobs().front().placedPoints.size(), 2U); // CP1 is the job's
        const Entity cp1Before = *pointEntity(document, "CP1");

        SurveyJobReadjustment readjust;
        readjust.jobId = id;
        readjust.handEdits = policy;
        readjust.settings = document.surveyJobs().front().settings;
        readjust.settings.control.front().origin = survey::ControlOrigin::Drawing;
        fake->points["201"] = {6'250'001.004, 300'001.0, {}};
        for (int run = 1; run <= 2; ++run) { // the second run needs CP1 still on the drawing
            SCOPED_TRACE(run);
            auto context = reductionContextFor(document);
            ASSERT_TRUE(context.ok()) << context.error().describe();
            ASSERT_EQ(context->drawingPoints.size(), 2U);
            readjust.context = *context;
            int reads = 0;
            auto command = std::make_unique<ReadjustSurveyJobCommand>(
                document, readjust, readerFor(&reads, document.surveyJobs().front().sourceBytes),
                reductionOf(fake));
            auto* raw = command.get();
            const auto status = document.execute(std::move(command));
            ASSERT_TRUE(status.ok()) << status.error().describe();
            EXPECT_TRUE(raw->changes().removed.empty());
            EXPECT_TRUE(raw->changes().editedByHand.empty());
            EXPECT_EQ(raw->changes().moved,
                      run == 1 ? std::vector<std::string>{"201"} : std::vector<std::string>{});

            const Entity* cp1 = pointEntity(document, "CP1");
            ASSERT_NE(cp1, nullptr) << "the mark the adjustment was held to";
            EXPECT_EQ(*cp1, cp1Before);
            const SurveyJob& job = document.surveyJobs().front();
            ASSERT_EQ(job.placedPoints.size(), 2U);
            const SurveyJobPoint* placed = placedPoint(job, "CP1"); // still the job's, for Remove
            ASSERT_NE(placed, nullptr);
            EXPECT_EQ(placed->entity, cp1Before.id);
        }
    }
}

// The same mark, moved by hand before it is held from the drawing: the new
// run held it where the person put it, so under either policy that is where
// it stays - "overwrite" has no other coordinates to give it - and the report
// says why it did not move.
TEST(SurveyJobReadjustCommand, AJobPointMovedByHandAndThenHeldFromTheDrawingStaysWhereItWasPut)
{
    for (const HandEditPolicy policy : {HandEditPolicy::Keep, HandEditPolicy::Overwrite}) {
        SCOPED_TRACE(toString(policy));
        Document document;
        auto fake = std::make_shared<FakeReduction>();
        fake->points = {{"CP1", {6'249'990.0, 299'990.0, 5.0}},
                        {"201", {6'250'001.0, 300'001.0, {}}}};
        SurveyJobImport request = importRequest();
        request.job.settings.control.push_back(survey::ControlSelection{
            survey::ControlPoint::fixedHorizontal("CP1"), survey::ControlOrigin::File});
        auto import = std::make_unique<ImportSurveyJobCommand>(document, request, reductionOf(fake));
        auto* imported = import.get();
        ASSERT_TRUE(document.execute(std::move(import)).ok());
        const EntityId cp1 = pointEntity(document, "CP1")->id;
        // The person re-marks CP1 0.25 m north: (299 990, 6 249 990.25).
        ASSERT_TRUE(document
                        .execute(katana::commands::moveEntities(
                            {cp1}, katana::geometry::Vec2(0.0, 0.25)))
                        .ok());

        SurveyJobReadjustment readjust;
        readjust.jobId = imported->jobId();
        readjust.handEdits = policy;
        readjust.settings = document.surveyJobs().front().settings;
        readjust.settings.control.front().origin = survey::ControlOrigin::Drawing;
        auto context = reductionContextFor(document);
        ASSERT_TRUE(context.ok()) << context.error().describe();
        readjust.context = *context;
        int reads = 0;
        auto command = std::make_unique<ReadjustSurveyJobCommand>(
            document, readjust, readerFor(&reads, document.surveyJobs().front().sourceBytes),
            reductionOf(fake));
        auto* raw = command.get();
        const auto status = document.execute(std::move(command));
        ASSERT_TRUE(status.ok()) << status.error().describe();

        const Entity* moved = document.model().entities.find(cp1);
        ASSERT_NE(moved, nullptr);
        EXPECT_EQ(positionOf(*moved), katana::geometry::Point2(299'990.0, 6'249'990.25));
        EXPECT_TRUE(raw->changes().removed.empty());
        EXPECT_EQ(raw->changes().editedByHand, std::vector<std::string>{"CP1"});
        bool reported = false;
        for (const auto& warning : raw->report()->warnings) {
            reported = reported || (warning.text.find("Point CP1") != std::string::npos &&
                                    warning.text.find("from the drawing") != std::string::npos);
        }
        EXPECT_TRUE(reported);
        // Still recorded where the JOB put it, so the edit stays visible as
        // one: a later run that computes CP1 again sees a hand-moved mark.
        const SurveyJobPoint* placed = placedPoint(document.surveyJobs().front(), "CP1");
        ASSERT_NE(placed, nullptr);
        EXPECT_EQ(placed->northing, 6'249'990.0);
    }
}

namespace {

// The survey point entity whose `property` reads `value`.
const Entity* entityWith(const Document& document, std::string_view property,
                         std::string_view value)
{
    const Entity* found = nullptr;
    document.model().entities.forEach([&](const Entity& entity) {
        const auto field = entity.properties.find(property);
        if (field != entity.properties.end() && katana::entity::toString(field->second) == value) {
            found = &entity;
        }
    });
    return found;
}

// An import drawn with every drawing option away from its default: a layer
// per field code, and the point's fields under other property names.
SurveyJobImport importDrawnPerCode()
{
    SurveyJobImport request = importRequest();
    request.importOptions.layerPerCode = true;
    request.importOptions.pointNumberProperty = "ptno";
    request.importOptions.codeProperty = "fieldcode";
    request.importOptions.descriptionProperty = "remark";
    request.importOptions.recordSource = false;
    return request;
}

// Every point the fake reduction computes has code PEG, so with a layer per
// code each one belongs on survey/day1/PEG, numbered under "ptno".
void expectDrawnPerCode(const Document& document, std::string_view pointId)
{
    SCOPED_TRACE(std::string(pointId));
    const Entity* entity = entityWith(document, "ptno", pointId);
    if (entity == nullptr) { // drawn under the default name: find it to show where it went
        entity = entityWith(document, "point", pointId);
    }
    ASSERT_NE(entity, nullptr);
    EXPECT_EQ(entity->layer, "survey/day1/PEG");
    EXPECT_EQ(entity->properties.count("point"), 0U);
    ASSERT_EQ(entity->properties.count("fieldcode"), 1U);
    EXPECT_EQ(katana::entity::toString(entity->properties.at("fieldcode")), "PEG");
}

SurveyJobReadjustment readjustmentOf(const Document& document, HandEditPolicy policy)
{
    SurveyJobReadjustment request;
    request.jobId = document.surveyJobs().front().id;
    request.settings = document.surveyJobs().front().settings;
    request.handEdits = policy;
    return request;
}

} // namespace

// The job's later points belong with its first ones: a point a re-adjustment
// computes for the first time, and one deleted by hand that Overwrite draws
// again, go where and as the import put the others - on the PEG layer, so
// switching that layer off hides all of the job's pegs, not just the first.
TEST(SurveyJobReadjustCommand, APointDrawnByAReadjustmentIsDrawnAsTheImportDrewTheOthers)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    fake->points = {{"101", {6'250'000.0, 300'000.0, 10.0}}};
    ASSERT_TRUE(document
                    .execute(std::make_unique<ImportSurveyJobCommand>(
                        document, importDrawnPerCode(), reductionOf(fake)))
                    .ok());
    expectDrawnPerCode(document, "101");

    // A new point...
    fake->points["102"] = {6'250'010.0, 300'020.0, 11.0};
    int reads = 0;
    auto first = std::make_unique<ReadjustSurveyJobCommand>(
        document, readjustmentOf(document, HandEditPolicy::Keep),
        readerFor(&reads, document.surveyJobs().front().sourceBytes), reductionOf(fake));
    auto* firstRaw = first.get();
    const auto added = document.execute(std::move(first));
    ASSERT_TRUE(added.ok()) << added.error().describe();
    EXPECT_EQ(firstRaw->changes().created, std::vector<std::string>{"102"});
    expectDrawnPerCode(document, "102");

    // ...and a deleted one drawn again.
    ASSERT_TRUE(
        document.execute(katana::commands::deleteEntities({entityWith(document, "ptno", "101")->id}))
            .ok());
    auto second = std::make_unique<ReadjustSurveyJobCommand>(
        document, readjustmentOf(document, HandEditPolicy::Overwrite),
        readerFor(&reads, document.surveyJobs().front().sourceBytes), reductionOf(fake));
    auto* secondRaw = second.get();
    const auto redrawn = document.execute(std::move(second));
    ASSERT_TRUE(redrawn.ok()) << redrawn.error().describe();
    EXPECT_EQ(secondRaw->changes().created, std::vector<std::string>{"101"});
    expectDrawnPerCode(document, "101");
}

// The drawing options are part of the job, so they are saved with it: a
// re-adjustment in a later session draws as the import did too.
TEST(SurveyJobReadjustCommand, TheImportsDrawingOptionsSurviveSavingAndOpening)
{
    const fs::path root = fs::temp_directory_path() / "katana-cad-tests" / "survey-job-options";
    std::error_code ignored;
    fs::remove_all(root, ignored);
    fs::create_directories(root);
    auto fake = std::make_shared<FakeReduction>();
    fake->points = {{"101", {6'250'000.0, 300'000.0, 10.0}}};
    {
        Document document;
        ASSERT_TRUE(document
                        .execute(std::make_unique<ImportSurveyJobCommand>(
                            document, importDrawnPerCode(), reductionOf(fake)))
                        .ok());
        ASSERT_TRUE(document.saveAs(root / "site.katana").ok());
    }
    Document reopened;
    const auto opened = reopened.open(root / "site.katana");
    ASSERT_TRUE(opened.ok()) << opened.error().describe();
    fake->points["102"] = {6'250'010.0, 300'020.0, 11.0};
    int reads = 0;
    const auto status = reopened.execute(std::make_unique<ReadjustSurveyJobCommand>(
        reopened, readjustmentOf(reopened, HandEditPolicy::Keep),
        readerFor(&reads, reopened.surveyJobs().front().sourceBytes), reductionOf(fake)));
    ASSERT_TRUE(status.ok()) << status.error().describe();
    expectDrawnPerCode(reopened, "101");
    expectDrawnPerCode(reopened, "102");
    fs::remove_all(root, ignored);
}

// The options come back from a project file, which may be damaged or edited
// by hand: each malformed form ends the run before the reduction, with a
// sentence naming the job, and changes nothing - never a guess at the options.
TEST(SurveyJobReadjustCommand, DrawingOptionsThatCannotBeReadEndTheRunAndChangeNothing)
{
    const std::vector<std::pair<std::string, ErrorCode>> cases = {
        {"layer-per-code=true\n", ErrorCode::ParseFailure},            // no version line
        {"katana-survey-import-options=x\n", ErrorCode::ParseFailure}, // not a number
        {"katana-survey-import-options=0\n", ErrorCode::ParseFailure},
        {"katana-survey-import-options=1\nlayer-per-code\n", ErrorCode::ParseFailure},
        {"katana-survey-import-options=1\nlayer-per-code=maybe\n", ErrorCode::ParseFailure},
        {"katana-survey-import-options=1\ncode-property=%4\n", ErrorCode::ParseFailure},
        {"katana-survey-import-options=1\ncode-property=%G1\n", ErrorCode::ParseFailure},
        {std::string("\0\xff\r\n=", 5), ErrorCode::ParseFailure},
        {"katana-survey-import-options=2\nlayer-per-code=true\n", ErrorCode::Unsupported},
    };
    for (const auto& [text, code] : cases) {
        SCOPED_TRACE(text);
        Document document;
        auto fake = std::make_shared<FakeReduction>();
        const std::string id = importThreePoints(document, fake);
        SurveyJobAccess::jobs(document).front().importOptions = text;
        const SurveyJob before = document.surveyJobs().front();
        const auto revision = document.modelRevision();
        const int calls = fake->calls;
        int reads = 0;
        const auto status = document.execute(std::make_unique<ReadjustSurveyJobCommand>(
            document, readjustmentOf(document, HandEditPolicy::Overwrite),
            readerFor(&reads, before.sourceBytes), reductionOf(fake)));
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(status.error().code, code);
        EXPECT_NE(status.error().message.find(id), std::string::npos) << status.error().message;
        EXPECT_EQ(fake->calls, calls) << "refused before the reduction";
        EXPECT_EQ(document.surveyJobs().front(), before);
        EXPECT_EQ(document.modelRevision(), revision);
    }
}

// A key a later build added is skipped, as the reduction settings' is, and
// the rest still apply. A property name holding a line break and a '%' comes
// back as it was: the text form encodes them.
TEST(SurveyJobReadjustCommand, ADrawingOptionALaterBuildAddedIsSkippedAndTheRestStillApply)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    fake->points = {{"101", {6'250'000.0, 300'000.0, 10.0}}};
    SurveyJobImport request = importDrawnPerCode();
    request.importOptions.pointNumberProperty = "pt%\nno";
    ASSERT_TRUE(document
                    .execute(std::make_unique<ImportSurveyJobCommand>(document, request,
                                                                      reductionOf(fake)))
                    .ok());
    ASSERT_NE(entityWith(document, "pt%\nno", "101"), nullptr);
    SurveyJobAccess::jobs(document).front().importOptions += "label-height=2.5\n";

    fake->points["102"] = {6'250'010.0, 300'020.0, 11.0};
    int reads = 0;
    const auto status = document.execute(std::make_unique<ReadjustSurveyJobCommand>(
        document, readjustmentOf(document, HandEditPolicy::Keep),
        readerFor(&reads, document.surveyJobs().front().sourceBytes), reductionOf(fake)));
    ASSERT_TRUE(status.ok()) << status.error().describe();
    const Entity* p102 = entityWith(document, "pt%\nno", "102");
    ASSERT_NE(p102, nullptr);
    EXPECT_EQ(p102->layer, "survey/day1/PEG");
}

// A job exactly as large as a project keeps, re-adjusted into one more point:
// the job would no longer fit, so the re-adjustment is refused and changes
// nothing, rather than leaving every later save of the drawing to fail.
TEST(SurveyJobReadjustCommand, AReadjustmentThatWouldTakeTheJobPastWhatAProjectHoldsIsRefused)
{
    using katana::storage::ProjectStore;
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    const std::string id = importThreePoints(document, fake);
    SurveyJob& job = SurveyJobAccess::jobs(document).front();
    // No stored report to start with, so the re-adjustment can only make the
    // job larger (a report where there was none, and one more point). Left
    // in place, the import's report can be longer than the re-adjustment's
    // (measured on main at 75acaf0: the job came out 393 bytes smaller even
    // with its fourth point), and a job that still fits is rightly accepted.
    job.reportText.clear();
    job.reportHtml.clear();
    const std::uint64_t rest = ProjectStore::surveyJobRowBytes(job) - job.sourceBytes.size();
    job.sourceBytes.resize_and_overwrite(ProjectStore::kMaxSurveyJobBytes - rest,
                                         [](char*, std::size_t size) { return size; });
    ASSERT_EQ(ProjectStore::surveyJobRowBytes(job), ProjectStore::kMaxSurveyJobBytes);
    const auto settings = job.settings;
    const auto reportText = job.reportText;
    const auto placed = job.placedPoints;
    const auto created = job.createdEntities;
    std::map<EntityId, Entity> entitiesBefore;
    document.model().entities.forEach(
        [&](const Entity& entity) { entitiesBefore.emplace(entity.id, entity); });

    fake->points["104"] = {6'250'040.0, 300'040.0, 12.0};
    fake->points["101"] = {6'250'000.004, 300'000.0, 10.0};
    SurveyJobReadjustment request = readjustmentOf(document, HandEditPolicy::Keep);
    request.settings.method = survey::AdjustmentMethod::Network;
    // The stored bytes are not compared here: that would read 900 MB.
    const SurveyJobReader reader = [](const SurveyJob&) -> Result<survey::SurveyProject> {
        return rawProject("re-read");
    };
    const auto status = document.execute(
        std::make_unique<ReadjustSurveyJobCommand>(document, request, reader, reductionOf(fake)));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(status.error().message.find("survey job " + id + " (DAY1.GSI) is too large"),
              std::string::npos)
        << status.error().message;

    const SurveyJob& after = document.surveyJobs().front();
    EXPECT_EQ(after.settings, settings);
    EXPECT_EQ(after.reportText, reportText);
    EXPECT_EQ(after.placedPoints, placed);
    EXPECT_EQ(after.createdEntities, created);
    std::map<EntityId, Entity> entitiesAfter;
    document.model().entities.forEach(
        [&](const Entity& entity) { entitiesAfter.emplace(entity.id, entity); });
    EXPECT_EQ(entitiesAfter, entitiesBefore) << "101 not moved, 104 not drawn";
}

TEST(SurveyJobReadjustCommand, AReaderOrReductionThatFailsChangesNothing)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    const std::string id = importThreePoints(document, fake);
    const SurveyJob before = document.surveyJobs().front();
    const auto revision = document.modelRevision();

    SurveyJobReadjustment request;
    request.jobId = id;
    const SurveyJobReader broken = [](const SurveyJob&) -> Result<survey::SurveyProject> {
        return katana::core::makeError(ErrorCode::ParseFailure,
                                       "record 12 of DAY1.GSI cannot be read");
    };
    auto status = document.execute(
        std::make_unique<ReadjustSurveyJobCommand>(document, request, broken, reductionOf(fake)));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::ParseFailure);

    fake->failure = katana::core::makeError(ErrorCode::AdjustmentFailure, "datum defect");
    int reads = 0;
    status = document.execute(std::make_unique<ReadjustSurveyJobCommand>(
        document, request, readerFor(&reads, before.sourceBytes), reductionOf(fake)));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::AdjustmentFailure);

    request.jobId = "job-does-not-exist";
    status = document.execute(std::make_unique<ReadjustSurveyJobCommand>(
        document, request, readerFor(&reads, before.sourceBytes), reductionOf(fake)));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::NotFound);

    EXPECT_EQ(document.surveyJobs().front(), before);
    EXPECT_EQ(document.modelRevision(), revision);
}

// ---- Remove --------------------------------------------------------------------------

TEST(SurveyJobRemoveCommand, RemovingAJobWithItsPointsDeletesBothAndUndoRestoresBoth)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    const std::string id = importThreePoints(document, fake);
    const SurveyJob before = document.surveyJobs().front();
    const EntityId foreign = drawForeignPoint(document, "900", 1.0, 2.0);

    ASSERT_TRUE(
        document.execute(std::make_unique<RemoveSurveyJobCommand>(document, id, true)).ok());
    EXPECT_TRUE(document.surveyJobs().empty());
    for (const EntityId created : before.createdEntities) {
        EXPECT_FALSE(document.model().entities.contains(created));
    }
    EXPECT_TRUE(document.model().entities.contains(foreign));

    ASSERT_TRUE(document.undo().ok());
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    EXPECT_EQ(document.surveyJobs().front(), before);
    for (const EntityId created : before.createdEntities) {
        EXPECT_TRUE(document.model().entities.contains(created));
    }
}

TEST(SurveyJobRemoveCommand, RemovingAJobWithoutItsPointsLeavesThePointsInTheDrawing)
{
    Document document;
    auto fake = std::make_shared<FakeReduction>();
    const std::string id = importThreePoints(document, fake);
    const auto created = document.surveyJobs().front().createdEntities;
    ASSERT_TRUE(
        document.execute(std::make_unique<RemoveSurveyJobCommand>(document, id, false)).ok());
    EXPECT_TRUE(document.surveyJobs().empty());
    for (const EntityId entity : created) {
        EXPECT_TRUE(document.model().entities.contains(entity));
    }
    const auto missing =
        document.execute(std::make_unique<RemoveSurveyJobCommand>(document, id, false));
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
}

// ---- The drawing's part of the reduction -----------------------------------------------

TEST(SurveyJobContext, TheDrawingsSurveyPointsAreOfferedAsControlByTheirIds)
{
    Document document;
    drawForeignPoint(document, "CP1", 6'249'990.0, 299'990.0);
    const auto context = reductionContextFor(document);
    ASSERT_TRUE(context.ok()) << context.error().describe();
    ASSERT_EQ(context->drawingPoints.size(), 1U);
    EXPECT_EQ(context->drawingPoints[0].id, "CP1");
    EXPECT_EQ(context->drawingPoints[0].northing, 6'249'990.0);
    EXPECT_EQ(context->drawingPoints[0].easting, 299'990.0);
    EXPECT_EQ(context->drawingPoints[0].elevation, 5.0);
    // A local drawing: nothing a projection could answer, so nothing is.
    EXPECT_FALSE(context->gridScaleFactor);
    EXPECT_FALSE(context->geoidSeparation);
    EXPECT_FALSE(context->geocentricToGrid);
    EXPECT_FALSE(context->geodeticToGrid);
}

// EPSG:28356, GDA94 / MGA zone 56: Transverse Mercator, k0 = 0.9996, false
// easting 500 000 m, central meridian 153 E, GRS80.
//
// On the central meridian the point scale factor IS k0, by definition of the
// projection: 0.9996 at E 500 000 whatever the northing.
//
// 100 km east of it, at N 6 250 000 (latitude about 33.89 S), by hand:
//   x = 100 000 / 0.9996 = 100 040.016 m on the ellipsoid
//   sin(33.89) = 0.557600, e2 = 0.00669438, w = 1 - e2 sin2 = 0.9979186
//   nu = a / sqrt(w) = 6 378 137 / 0.9989588 = 6 384 785 m
//   rho = a (1 - e2) / w^1.5 = 6 335 439 / 0.9968795 = 6 355 271 m
//   rho nu = 4.057704e13 m2
//   k = k0 (1 + x2 / (2 rho nu) + x4 / (24 rho2 nu2))
//     = 0.9996 (1 + 1.233211e-4 + 2.53e-9) = 0.9997233
// The terms left out are below 1e-9; the latitude is known to 0.01 degree,
// which moves rho nu by under 0.05% and k by under 1e-7.
TEST(SurveyJobContext, TheGridScaleFactorComesFromTheDrawingsProjection)
{
    Document document;
    katana::storage::ProjectMetadata metadata = document.metadata();
    metadata.coordinateSystem = "EPSG:28356";
    document.setMetadata(metadata);
    const auto context = reductionContextFor(document);
    ASSERT_TRUE(context.ok()) << context.error().describe();
    ASSERT_TRUE(context->gridScaleFactor);

    const auto onMeridian = context->gridScaleFactor(6'250'000.0, 500'000.0, 0.0);
    ASSERT_TRUE(onMeridian.has_value());
    EXPECT_NEAR(*onMeridian, 0.9996, 1e-9);

    const auto east = context->gridScaleFactor(6'250'000.0, 600'000.0, 0.0);
    ASSERT_TRUE(east.has_value());
    EXPECT_NEAR(*east, 0.9997233, 2e-7);

    // Symmetric about the central meridian: 100 km west is the same.
    const auto west = context->gridScaleFactor(6'250'000.0, 400'000.0, 0.0);
    ASSERT_TRUE(west.has_value());
    EXPECT_NEAR(*west, *east, 1e-9);

    // No geoid model and no GNSS frame: left empty, never guessed.
    EXPECT_FALSE(context->geoidSeparation);
    EXPECT_FALSE(context->geodeticToGrid);
}

TEST(SurveyJobContext, TheHorizontalPartOfACompoundSystemGivesTheScaleFactor)
{
    Document document;
    katana::storage::ProjectMetadata metadata = document.metadata();
    metadata.coordinateSystem = "EPSG:28356+5711"; // MGA zone 56 + AHD height
    document.setMetadata(metadata);
    const auto context = reductionContextFor(document);
    ASSERT_TRUE(context.ok()) << context.error().describe();
    ASSERT_TRUE(context->gridScaleFactor);
    EXPECT_NEAR(*context->gridScaleFactor(6'250'000.0, 500'000.0, 0.0), 0.9996, 1e-9);
}

TEST(SurveyJobContext, ACoordinateSystemThatCannotBeReadIsAnErrorNotALocalDrawing)
{
    Document document;
    katana::storage::ProjectMetadata metadata = document.metadata();
    metadata.coordinateSystem = "EPSG:999999";
    document.setMetadata(metadata);
    const auto context = reductionContextFor(document);
    ASSERT_FALSE(context.ok());
    EXPECT_NE(context.error().message.find("EPSG:999999"), std::string::npos);
}

// ---- The reduction a job on the drawing runs ------------------------------------------

namespace {

survey::SurveyPoint surveyPoint(std::string id, double northing, double easting,
                                std::optional<double> elevation)
{
    survey::SurveyPoint point;
    point.id = std::move(id);
    point.northing = northing;
    point.easting = easting;
    point.elevation = elevation;
    return point;
}

// CP1 held from the drawing, fixed in all three.
survey::ReductionSettings holdingFromTheDrawing(std::string id)
{
    survey::ReductionSettings settings;
    settings.control.push_back(
        survey::ControlSelection{survey::ControlPoint::fixed3d(std::move(id)),
                                 survey::ControlOrigin::Drawing});
    return settings;
}

// A file that names CP1 and gives it no coordinates, and gives A some: all a
// reduction of control and entered points needs.
survey::SurveyProject namingCp1()
{
    survey::SurveyProject raw;
    raw.points.push_back(surveyPoint("A", 5'000'050.0, 500'050.0, 101.0));
    survey::UnpositionedPoint cp1;
    cp1.id = "CP1";
    raw.unpositionedPoints.push_back(cp1);
    return raw;
}

// A survey point drawn beside whatever the drawing has, one of its id included.
void drawAlongside(Document& document, std::string id, double northing, double easting,
                   std::optional<double> elevation)
{
    survey::SurveyProject project;
    project.points.push_back(surveyPoint(std::move(id), northing, easting, elevation));
    SurveyImportOptions options;
    options.layer = "control";
    auto command = importSurveyPoints(document, project, options, ExistingPointPolicy::KeepBoth);
    ASSERT_TRUE(command.ok()) << command.error().describe();
    ASSERT_TRUE(document.execute(std::move(*command)).ok());
}

std::vector<std::string> warningsOf(const survey::ReductionOutcome& outcome)
{
    std::vector<std::string> texts;
    for (const survey::ReportMessage& warning : outcome.report.warnings) {
        texts.push_back(warning.text);
    }
    return texts;
}

} // namespace

// The reduction holds the first point of an id, and which is first is only
// the order the points were drawn in: held from a drawing that has CP1 at two
// places, a job would sit on either with nothing to say which (100 km apart
// here). Refused instead, with both places, in the drawing's order.
TEST(SurveyJobDrawingReduction, AnIdTheDrawingHasAtTwoPlacesIsNotHeldAndBothPlacesAreNamed)
{
    survey::ReductionContext context;
    context.drawingPoints = {surveyPoint("CP1", 5'000'000.0, 500'000.0, 100.0),
                             surveyPoint("CP1", 5'000'000.0, 600'000.0, 100.0)};
    const auto outcome = reduceForDrawing(namingCp1(), holdingFromTheDrawing("CP1"), context);
    ASSERT_FALSE(outcome.ok());
    EXPECT_EQ(outcome.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(outcome.error().message,
              "Control point CP1 is on the drawing 2 times, at E 500000.0000 N 5000000.0000 "
              "Z 100.0000 and E 600000.0000 N 5000000.0000 Z 100.0000, so which one to hold "
              "is not known; rename or delete all but one.");

    // Three, one with no height - a mark of its own, since a height held from
    // one would not be held from it.
    context.drawingPoints = {surveyPoint("CP1", 5'000'000.0, 500'000.0, 100.0),
                             surveyPoint("CP1", 5'000'000.0, 500'000.0, std::nullopt),
                             surveyPoint("CP1", 5'000'000.0, 500'000.0, 100.0)};
    const auto three = reduceForDrawing(namingCp1(), holdingFromTheDrawing("CP1"), context);
    ASSERT_FALSE(three.ok());
    EXPECT_EQ(three.error().message,
              "Control point CP1 is on the drawing 3 times, at E 500000.0000 N 5000000.0000 "
              "Z 100.0000, E 500000.0000 N 5000000.0000 (no height) and E 500000.0000 "
              "N 5000000.0000 Z 100.0000, so which one to hold is not known; rename or delete "
              "all but one.");
}

// Points of one id that are one ground mark - closer than
// math::tolerance::kCoordinate, 0.1 mm, across and in height - hold the same
// job whichever is taken, so they are held. 0.05 mm apart is one mark; 0.15
// mm apart, across or in height, is two.
TEST(SurveyJobDrawingReduction, PointsOfOneIdAtOneMarkAreHeldAndTwoMarksAreNot)
{
    survey::ReductionContext context;
    context.drawingPoints = {surveyPoint("CP1", 5'000'000.0, 500'000.0, 100.0),
                             surveyPoint("CP1", 5'000'000.00005, 500'000.0, 100.00005)};
    const auto one = reduceForDrawing(namingCp1(), holdingFromTheDrawing("CP1"), context);
    ASSERT_TRUE(one.ok()) << one.error().describe();
    for (const std::string& warning : warningsOf(*one)) {
        EXPECT_EQ(warning.find("Control point CP1"), std::string::npos) << warning;
    }

    for (const auto& [northing, height] :
         {std::pair{5'000'000.00015, 100.0}, std::pair{5'000'000.0, 100.00015}}) {
        SCOPED_TRACE(height);
        context.drawingPoints = {surveyPoint("CP1", 5'000'000.0, 500'000.0, 100.0),
                                 surveyPoint("CP1", northing, 500'000.0, height)};
        const auto two = reduceForDrawing(namingCp1(), holdingFromTheDrawing("CP1"), context);
        ASSERT_FALSE(two.ok());
        EXPECT_EQ(two.error().code, ErrorCode::InvalidArgument);
    }
}

// Held from the drawing, a point the file never names - measured to or from
// by nothing in it - changes nothing, which the report says before anything
// else: an id typed wrong is the likely reason.
TEST(SurveyJobDrawingReduction, AHeldIdTheFileNeverNamesIsReportedFirstAsChangingNothing)
{
    survey::ReductionContext context;
    context.drawingPoints = {surveyPoint("BM1", 7'000'000.0, 700'000.0, 10.0)};
    const auto outcome = reduceForDrawing(namingCp1(), holdingFromTheDrawing("BM1"), context);
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const std::vector<std::string> warnings = warningsOf(*outcome);
    ASSERT_FALSE(warnings.empty());
    EXPECT_EQ(warnings.front(), "Control point BM1 is held from the drawing, but the file names "
                                "no point BM1, so holding it changes nothing.");

    // Named with no coordinates, as CP1 is: holding it is what places it.
    context.drawingPoints = {surveyPoint("CP1", 5'000'000.0, 500'000.0, 100.0)};
    const auto named = reduceForDrawing(namingCp1(), holdingFromTheDrawing("CP1"), context);
    ASSERT_TRUE(named.ok()) << named.error().describe();
    for (const std::string& warning : warningsOf(*named)) {
        EXPECT_EQ(warning.find("Control point CP1"), std::string::npos) << warning;
    }
}

// Held from the drawing, a point the file gives other coordinates is held
// where the drawing has it; the report says so with both, first, since the
// file's other coordinates stay in the file's own frame.
TEST(SurveyJobDrawingReduction, AHeldIdTheFileGivesOtherCoordinatesIsReportedWithBoth)
{
    survey::SurveyProject raw = namingCp1();
    raw.unpositionedPoints.clear();
    raw.points.push_back(surveyPoint("CP1", 5'000'000.0, 500'000.0, 100.0));
    survey::ReductionContext context;
    context.drawingPoints = {surveyPoint("CP1", 6'000'000.0, 600'000.0, 200.0)};
    const auto moved = reduceForDrawing(raw, holdingFromTheDrawing("CP1"), context);
    ASSERT_TRUE(moved.ok()) << moved.error().describe();
    ASSERT_FALSE(warningsOf(*moved).empty());
    EXPECT_EQ(warningsOf(*moved).front(),
              "Control point CP1 is held where the drawing has it, E 600000.0000 "
              "N 6000000.0000 Z 200.0000, not where the file gives it, E 500000.0000 "
              "N 5000000.0000 Z 100.0000.");

    // The drawing's with no height is not the file's with one.
    context.drawingPoints = {surveyPoint("CP1", 5'000'000.0, 500'000.0, std::nullopt)};
    survey::ReductionSettings plan = holdingFromTheDrawing("CP1");
    plan.control.front().point.elevation.constraint = survey::ControlConstraint::Free;
    const auto flat = reduceForDrawing(raw, plan, context);
    ASSERT_TRUE(flat.ok()) << flat.error().describe();
    ASSERT_FALSE(warningsOf(*flat).empty());
    EXPECT_EQ(warningsOf(*flat).front(),
              "Control point CP1 is held where the drawing has it, E 500000.0000 "
              "N 5000000.0000 (no height), not where the file gives it, E 500000.0000 "
              "N 5000000.0000 Z 100.0000.");

    // One mark with the file's: nothing to say.
    context.drawingPoints = {surveyPoint("CP1", 5'000'000.00005, 500'000.0, 100.0)};
    const auto same = reduceForDrawing(raw, holdingFromTheDrawing("CP1"), context);
    ASSERT_TRUE(same.ok()) << same.error().describe();
    for (const std::string& warning : warningsOf(*same)) {
        EXPECT_EQ(warning.find("Control point CP1"), std::string::npos) << warning;
    }
}

// The import command's own reduction is that one, so the refusal reaches
// every import that does not hand it another - the SURVEY IMPORT line's -
// with the drawing as it was.
TEST(SurveyJobImportCommand, ItsReductionRefusesAnIdTheDrawingHasAtTwoPlaces)
{
    Document document;
    drawAlongside(document, "CP1", 5'000'000.0, 500'000.0, 100.0);
    drawAlongside(document, "CP1", 5'000'000.0, 600'000.0, 100.0);
    const std::size_t entities = document.model().entities.size();
    SurveyJobImport request = importRequest();
    request.job.settings = holdingFromTheDrawing("CP1");
    request.raw = namingCp1();
    auto context = reductionContextFor(document);
    ASSERT_TRUE(context.ok()) << context.error().describe();
    request.context = *context;
    const auto status = document.execute(std::make_unique<ImportSurveyJobCommand>(document, request));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(status.error().message.find("Control point CP1 is on the drawing 2 times"),
              std::string::npos)
        << status.error().describe();
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.model().entities.size(), entities);
}
