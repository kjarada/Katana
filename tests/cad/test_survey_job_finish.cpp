// A survey job that is coded and strung as it is imported
// (SurveyJobImport::finish, cad/survey_finish.hpp): the job owns the lines as
// it owns the points, its stored options remember that it was finished, and a
// re-adjustment finishes it again.
//
// The reduction and the reader are INJECTED, as in test_survey_job.cpp: the
// fake reduction returns points with known coordinates and codes and the
// features that string them, so every expected value is worked by hand from
// the shots a test lists. A survey point is (northing, easting) and a drawing
// point (x, y) = (easting, northing).

#include <gtest/gtest.h>

#include <algorithm>
#include <map>
#include <memory>
#include <optional>
#include <ostream>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_finish.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyRule;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
namespace survey = katana::survey;

namespace {

struct Shot {
    std::string id;
    double northing = 0.0;
    double easting = 0.0;
    std::string code;
};

// Stands in for survey::reduceAndAdjust on a field file that keeps code and
// string number apart: whatever `shots` and `features` hold when it is called
// is what the "reduction" returns, and every call is counted.
struct FakeField {
    std::vector<Shot> shots;
    std::vector<survey::SurveyFeature> features;
    int calls = 0;
};

ReductionFunction reductionOf(const std::shared_ptr<FakeField>& fake)
{
    return [fake](const survey::SurveyProject& raw, const survey::ReductionSettings& settings,
                  const survey::ReductionContext& context) -> Result<survey::ReductionOutcome> {
        ++fake->calls;
        survey::ReductionOutcome outcome;
        outcome.reduced.name = raw.name;
        for (const Shot& shot : fake->shots) {
            survey::SurveyPoint point;
            point.id = shot.id;
            point.northing = shot.northing;
            point.easting = shot.easting;
            point.code = shot.code;
            point.coordinateSource = survey::CoordinateSource::Calculated;
            outcome.reduced.points.push_back(point);
            survey::ComputedPoint computed;
            computed.id = shot.id;
            computed.northing = shot.northing;
            computed.easting = shot.easting;
            outcome.points.push_back(computed);
        }
        outcome.reduced.features = fake->features;
        outcome.report.createdUtc = context.createdUtc;
        outcome.report.input = context.input;
        outcome.report.settings = settings;
        return outcome;
    };
}

survey::SurveyFeature feature(const std::string& code, const std::string& number,
                              std::vector<std::string> ids)
{
    survey::SurveyFeature f;
    f.code = code;
    f.name = number; // the string number, as the field-file readers leave it
    f.pointIds = std::move(ids);
    return f;
}

// Two strings of the joint code KJ, two shots each:
//   KJ 01: point 1 (N 0, E 0)  and point 2 (N 0, E 10)
//   KJ 02: point 3 (N 5, E 0)  and point 4 (N 5, E 10)
std::shared_ptr<FakeField> twoJoints()
{
    auto fake = std::make_shared<FakeField>();
    fake->shots = {{"1", 0.0, 0.0, "KJ"}, {"2", 0.0, 10.0, "KJ"}, {"3", 5.0, 0.0, "KJ"},
                   {"4", 5.0, 10.0, "KJ"}};
    fake->features = {feature("KJ", "01", {"1", "2"}), feature("KJ", "02", {"3", "4"})};
    return fake;
}

// "KJ*": a joint is a LINE on SURVEY JOINT, drawn with "Joint Line".
katana::entity::SurveyMap jointMap()
{
    katana::entity::SurveyMap map;
    SurveyRule joint;
    joint.key = "KJ*";
    joint.model = "SURVEY JOINT";
    joint.linestyle = "Joint Line";
    joint.breakline = SurveyBreakline::Line;
    EXPECT_TRUE(map.add(joint).ok());
    return map;
}

SurveyJobImport importRequest(bool codes, bool linework)
{
    SurveyJobImport request;
    request.job.name = "DAY1.FLD";
    request.job.formatId = "opcode-field-file";
    request.job.parserVersion = "1.0";
    request.job.sourceFileName = "DAY1.FLD";
    request.job.sourceBytes = "the field file's bytes";
    // A top-level layer on purpose. Creating a layer under "survey" also
    // creates that parent, which the undo of ANY import leaves behind - the layer
    // command's undo removes only the layer it was given - and these tests
    // compare whole drawings to see what the finish leaves, not that.
    request.job.layer = "day1";
    request.raw.name = "day one";
    request.context.createdUtc = "2026-10-06T10:00:00Z";
    request.importOptions.layer = "day1";
    request.finish.codes = codes;
    request.finish.linework = linework;
    return request;
}

// Imports the job and returns its command, which the document now owns.
const ImportSurveyJobCommand* importJob(Document& document,
                                        const std::shared_ptr<FakeField>& fake,
                                        SurveyJobImport request)
{
    auto command =
        std::make_unique<ImportSurveyJobCommand>(document, std::move(request), reductionOf(fake));
    const ImportSurveyJobCommand* raw = command.get();
    const auto status = document.execute(std::move(command));
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    return raw;
}

// ... coded and strung, or neither.
const ImportSurveyJobCommand* importJob(Document& document,
                                        const std::shared_ptr<FakeField>& fake, bool finish)
{
    return importJob(document, fake, importRequest(finish, finish));
}

// Re-adjusts the document's one job with the settings it has, and returns
// the command, which the document now owns.
const ReadjustSurveyJobCommand* readjust(Document& document,
                                         const std::shared_ptr<FakeField>& fake,
                                         SurveyFinishOptions finish = {},
                                         HandEditPolicy handEdits = HandEditPolicy::Keep)
{
    SurveyJobReadjustment request;
    request.jobId = document.surveyJobs().front().id;
    request.settings = document.surveyJobs().front().settings;
    request.context.createdUtc = "2026-10-06T11:00:00Z";
    request.handEdits = handEdits;
    request.finish = std::move(finish);
    const SurveyJobReader reader = [](const SurveyJob& job) -> Result<survey::SurveyProject> {
        survey::SurveyProject raw;
        raw.name = "re-read " + job.sourceFileName;
        return raw;
    };
    auto command = std::make_unique<ReadjustSurveyJobCommand>(document, std::move(request), reader,
                                                              reductionOf(fake));
    const ReadjustSurveyJobCommand* raw = command.get();
    const auto status = document.execute(std::move(command));
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    return raw;
}

struct Drawing {
    std::map<EntityId, Entity> entities;
    std::vector<katana::entity::Layer> layers;
    std::vector<katana::entity::Style> styles;

    friend bool operator==(const Drawing&, const Drawing&) = default;
};

Drawing drawingOf(const Document& document)
{
    Drawing drawing;
    document.model().entities.forEach(
        [&](const Entity& entity) { drawing.entities.emplace(entity.id, entity); });
    drawing.layers = document.model().layers.all();
    drawing.styles = document.model().styles.all();
    return drawing;
}

// What a failed comparison prints: the ids and the names, which is enough to
// see WHAT is left over or missing.
void PrintTo(const Drawing& drawing, std::ostream* out)
{
    *out << "entities:";
    for (const auto& [id, entity] : drawing.entities) {
        *out << ' ' << id << '@' << entity.layer << '/' << entity.style;
    }
    *out << " layers:";
    for (const auto& layer : drawing.layers) {
        *out << " [" << layer.name << ']';
    }
    *out << " styles:";
    for (const auto& style : drawing.styles) {
        *out << " [" << style.name << ']';
    }
}

bool isLine(const Document& document, EntityId id)
{
    const Entity* entity = document.model().entities.find(id);
    return entity != nullptr && std::holds_alternative<Polyline2>(entity->geometry);
}

// The job's lines - its created entities that are polylines - in the order
// the job lists them.
std::vector<const Entity*> linesOf(const Document& document, const SurveyJob& job)
{
    std::vector<const Entity*> lines;
    for (const EntityId id : job.createdEntities) {
        if (isLine(document, id)) {
            lines.push_back(document.model().entities.find(id));
        }
    }
    return lines;
}

std::size_t lineCount(const Document& document)
{
    std::size_t count = 0;
    document.model().entities.forEach([&](const Entity& entity) {
        count += std::holds_alternative<Polyline2>(entity.geometry) ? 1 : 0;
    });
    return count;
}

const std::vector<Point2>& verticesOf(const Entity& line)
{
    return std::get<Polyline2>(line.geometry).vertices;
}

std::string textProperty(const Entity& entity, const std::string& key)
{
    const auto found = entity.properties.find(key);
    if (found == entity.properties.end()) {
        return "<absent>";
    }
    const auto* text = std::get_if<std::string>(&found->second);
    return text != nullptr ? *text : "<not text>";
}

const Entity* pointNumbered(const Document& document, const std::string& number)
{
    const Entity* found = nullptr;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<katana::entity::PointGeometry>(entity.geometry) &&
            textProperty(entity, "point") == number) {
            found = &entity;
        }
    });
    return found;
}

// The text an import with every drawing option at its default has always
// written for SurveyJob::importOptions: the versioned first line and the six
// keys, in the order the reader documents them.
const char* const kDefaultOptionsText = "katana-survey-import-options=1\n"
                                        "layer-per-code=false\n"
                                        "create-layers=true\n"
                                        "code-property=code\n"
                                        "point-number-property=point\n"
                                        "description-property=description\n"
                                        "record-source=true\n";

} // namespace

// ---- Import --------------------------------------------------------------------------

TEST(SurveyJobFinish, TheJobOwnsItsPointsAndItsLinesAndTheReductionRunsOnce)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();

    const ImportSurveyJobCommand* command = importJob(document, fake, true);

    EXPECT_EQ(fake->calls, 1) << "composed inside the job: validate's reduction is execute's";
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    const SurveyJob& job = document.surveyJobs().front();
    // Four points, then the two lines KJ 01 and KJ 02.
    ASSERT_EQ(job.createdEntities.size(), 6U);
    EXPECT_EQ(job.placedPoints.size(), 4U) << "the points alone: the count of points";
    for (std::size_t i = 0; i < job.createdEntities.size(); ++i) {
        EXPECT_EQ(isLine(document, job.createdEntities[i]), i >= 4) << i;
    }
    EXPECT_EQ(command->createdEntities(), job.createdEntities);
    EXPECT_EQ(document.model().entities.size(), 6U);

    const auto lines = linesOf(document, job);
    ASSERT_EQ(lines.size(), 2U);
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    EXPECT_EQ(verticesOf(*lines[1]), (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    EXPECT_EQ(lines[0]->layer, "SURVEY JOINT");
    EXPECT_EQ(lines[0]->style, "Joint Line");
    // The points were coded by their string names, KJ01 and KJ02 -> "KJ*".
    EXPECT_EQ(pointNumbered(document, "1")->layer, "SURVEY JOINT");
    EXPECT_EQ(pointNumbered(document, "1")->style, "Joint Line");
    EXPECT_EQ(textProperty(*pointNumbered(document, "1"), "string"), "01");
    EXPECT_EQ(textProperty(*pointNumbered(document, "4"), "string"), "02");

    const SurveyFinishReport* finish = command->finishReport();
    ASSERT_NE(finish, nullptr);
    EXPECT_EQ(finish->points, 4U);
    EXPECT_EQ(finish->whyNotCoded, SurveyFinishSkip::None);
    EXPECT_EQ(finish->coding.coded, 4U);
    EXPECT_EQ(finish->coding.matched, 4U);
    EXPECT_EQ(finish->whyNotStrung, SurveyFinishSkip::None);
    EXPECT_EQ(finish->lines.size(), 2U);
    EXPECT_EQ(finish->unplaced(), 0U);
    // In words: four points, all coded KJ and all ruled by "KJ*", which made
    // the one layer SURVEY JOINT and the one style Joint Line; two lines.
    EXPECT_EQ(describe(*finish),
              (std::vector<std::string>{
                  "Survey codes were applied to the 4 point(s) drawn: 4 carry a code and 4 of "
                  "those have a rule; 1 layer(s) and 1 style(s) were created.",
                  "Linework: 2 line(s) drawn."}));
    EXPECT_TRUE(finishWarnings(*finish).empty()) << "nothing was left over";
}

TEST(SurveyJobFinish, OneUndoRemovesPointsLinesAndJobAndRedoBringsThemBackUnchanged)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    const Drawing before = drawingOf(document);

    importJob(document, fake, true);
    const Drawing after = drawingOf(document);
    const SurveyJob imported = document.surveyJobs().front();
    ASSERT_EQ(after.entities.size(), 6U);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(drawingOf(document), before);
    EXPECT_FALSE(document.undo().ok()) << "it was one step";

    ASSERT_TRUE(document.redo().ok());
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    EXPECT_EQ(document.surveyJobs().front(), imported);
    EXPECT_EQ(drawingOf(document), after) << "the same ids, layers and styles";
    EXPECT_EQ(fake->calls, 1) << "redo replays, it does not reduce or plan again";
}

TEST(SurveyJobFinish, RemovingTheJobWithItsPointsLeavesNothingOfIt)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    const ImportSurveyJobCommand* command = importJob(document, fake, true);
    ASSERT_EQ(document.model().entities.size(), 6U);
    const Drawing withJob = drawingOf(document);
    const SurveyJob job = document.surveyJobs().front();

    ASSERT_TRUE(document
                    .execute(std::make_unique<RemoveSurveyJobCommand>(document, command->jobId(),
                                                                      true))
                    .ok());

    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.model().entities.size(), 0U) << "the lines went with the points";
    // And one undo puts the lines back with them: the same entities, as they
    // were, and the job that owns them.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(drawingOf(document), withJob);
    EXPECT_EQ(lineCount(document), 2U);
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    EXPECT_EQ(document.surveyJobs().front(), job);
}

TEST(SurveyJobFinish, WithTheOptionsOffTheJobIsImportedExactlyAsItAlwaysWas)
{
    Document document;
    document.setSurveyMap(jointMap()); // loaded, and not asked for
    auto fake = twoJoints();

    const ImportSurveyJobCommand* command = importJob(document, fake, false);

    const SurveyJob& job = document.surveyJobs().front();
    EXPECT_EQ(command->finishReport(), nullptr);
    EXPECT_EQ(job.importOptions, kDefaultOptionsText);
    ASSERT_EQ(job.createdEntities.size(), 4U);
    EXPECT_EQ(lineCount(document), 0U);
    for (const char* number : {"1", "2", "3", "4"}) {
        const Entity* point = pointNumbered(document, number);
        ASSERT_NE(point, nullptr) << number;
        EXPECT_EQ(point->layer, "day1") << number;
        EXPECT_EQ(point->style, "") << number;
        EXPECT_EQ(textProperty(*point, "code"), "KJ") << number;
        EXPECT_EQ(textProperty(*point, "string"), "<absent>") << number;
    }
    EXPECT_FALSE(document.model().layers.contains("SURVEY JOINT"));
    EXPECT_EQ(job.reportText.find("Survey codes"), std::string::npos);
    EXPECT_EQ(job.reportText.find("inework"), std::string::npos);
}

TEST(SurveyJobFinish, AFinishWithNothingToDoStillImportsTheJobAndItsReportSaysWhy)
{
    Document document; // no survey codes loaded
    auto fake = twoJoints();

    const ImportSurveyJobCommand* command = importJob(document, fake, true);

    const SurveyJob& job = document.surveyJobs().front();
    EXPECT_EQ(job.createdEntities.size(), 4U);
    EXPECT_EQ(lineCount(document), 0U);
    EXPECT_EQ(pointNumbered(document, "1")->layer, "day1");
    const SurveyFinishReport* finish = command->finishReport();
    ASSERT_NE(finish, nullptr);
    EXPECT_EQ(finish->whyNotCoded, SurveyFinishSkip::NoSurveyCodes);
    EXPECT_EQ(finish->whyNotStrung, SurveyFinishSkip::NoSurveyCodes);
    // Both steps were asked for and neither ran: each says why, and both
    // sentences are warnings of the job's report.
    const std::vector<std::string> sentences = {
        "Survey codes were not applied: no survey codes are loaded.",
        "No linework was drawn: no survey codes are loaded."};
    EXPECT_EQ(describe(*finish), sentences);
    EXPECT_EQ(finishWarnings(*finish), sentences);
    for (const std::string& sentence : sentences) {
        EXPECT_NE(job.reportText.find(sentence), std::string::npos) << sentence;
    }
    // The choice is the job's all the same: it was imported to be finished.
    const auto stored = readSurveyJobOptions(job.importOptions, job.id);
    ASSERT_TRUE(stored.ok());
    EXPECT_TRUE(stored->applyCodes);
    EXPECT_TRUE(stored->drawLinework);
}

// ---- The stored options ----------------------------------------------------------------

TEST(SurveyJobOptionsText, AJobRemembersThatItWasFinishedAndOneThatWasNotWritesWhatItAlwaysDid)
{
    Document document;
    document.setSurveyMap(jointMap());
    importJob(document, twoJoints(), true);
    EXPECT_EQ(document.surveyJobs().front().importOptions,
              std::string(kDefaultOptionsText) + "apply-codes=true\ndraw-linework=true\n");

    // Not asked for: the keys are not written at all.
    EXPECT_EQ(writeSurveyJobOptions(SurveyJobOptions{}), kDefaultOptionsText);
}

TEST(SurveyJobOptionsText, TheTextRoundTripsWithAndWithoutTheTwoKeys)
{
    for (const bool codes : {false, true}) {
        for (const bool linework : {false, true}) {
            SCOPED_TRACE(std::string(codes ? "codes " : "") + (linework ? "linework" : ""));
            SurveyJobOptions options;
            options.import.layerPerCode = true;
            options.import.codeProperty = "field%code";
            options.import.recordSource = false;
            options.applyCodes = codes;
            options.drawLinework = linework;

            const std::string text = writeSurveyJobOptions(options);
            EXPECT_EQ(text.find("apply-codes=true\n") != std::string::npos, codes);
            EXPECT_EQ(text.find("draw-linework=true\n") != std::string::npos, linework);
            const auto read = readSurveyJobOptions(text, "job-7");
            ASSERT_TRUE(read.ok()) << read.error().describe();
            EXPECT_EQ(read->applyCodes, codes);
            EXPECT_EQ(read->drawLinework, linework);
            EXPECT_TRUE(read->import.layerPerCode);
            EXPECT_EQ(read->import.codeProperty, "field%code");
            EXPECT_FALSE(read->import.recordSource);
            EXPECT_EQ(writeSurveyJobOptions(*read), text);
        }
    }
}

TEST(SurveyJobOptionsText, ATextWithoutTheKeysReadsAsOffAndAMalformedOneNamesTheJob)
{
    // The text of every job imported before the keys existed, and empty text.
    for (const std::string& text : {std::string(kDefaultOptionsText), std::string()}) {
        const auto old = readSurveyJobOptions(text, "job-7");
        ASSERT_TRUE(old.ok()) << old.error().describe();
        EXPECT_FALSE(old->applyCodes);
        EXPECT_FALSE(old->drawLinework);
    }
    const auto off =
        readSurveyJobOptions(std::string(kDefaultOptionsText) + "apply-codes=false\n", "job-7");
    ASSERT_TRUE(off.ok());
    EXPECT_FALSE(off->applyCodes);

    const auto bad =
        readSurveyJobOptions(std::string(kDefaultOptionsText) + "draw-linework=yes\n", "job-7");
    ASSERT_FALSE(bad.ok());
    EXPECT_EQ(bad.error().code, ErrorCode::ParseFailure);
    EXPECT_NE(bad.error().message.find("job-7"), std::string::npos) << bad.error().message;
}

// ---- Re-adjust -------------------------------------------------------------------------

TEST(SurveyJobFinish, AReadjustmentCodesItsNewPointsAndRedrawsTheJobsLinesInOneUndoStep)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    importJob(document, fake, true);
    const SurveyJob imported = document.surveyJobs().front();
    const std::vector<EntityId> oldLines = {imported.createdEntities[4],
                                            imported.createdEntities[5]};
    const Drawing before = drawingOf(document);

    // The new run moves point 2 two metres east and shoots a third point of
    // string 02:  KJ 01: 1 (N 0, E 0), 2 (N 0, E 12)
    //             KJ 02: 3 (N 5, E 0), 4 (N 5, E 10), 5 (N 5, E 20)
    fake->shots[1].easting = 12.0;
    fake->shots.push_back({"5", 5.0, 20.0, "KJ"});
    fake->features[1].pointIds.push_back("5");

    const ReadjustSurveyJobCommand* command = readjust(document, fake);

    EXPECT_EQ(command->changes().moved, std::vector<std::string>{"2"});
    EXPECT_EQ(command->changes().created, std::vector<std::string>{"5"});
    const SurveyJob& job = document.surveyJobs().front();

    // The new point is coded as its neighbours were: KJ02 -> "KJ*".
    const Entity* fifth = pointNumbered(document, "5");
    ASSERT_NE(fifth, nullptr);
    EXPECT_EQ(fifth->layer, "SURVEY JOINT");
    EXPECT_EQ(fifth->style, "Joint Line");
    EXPECT_EQ(textProperty(*fifth, "string"), "02");

    // The job's two lines are the SAME two entities, at the new coordinates:
    // KJ 01 through the moved point 2, KJ 02 on through the new point 5.
    EXPECT_EQ(lineCount(document), 2U);
    ASSERT_TRUE(isLine(document, oldLines[0]));
    ASSERT_TRUE(isLine(document, oldLines[1]));
    EXPECT_EQ(verticesOf(*document.model().entities.find(oldLines[0])),
              (std::vector<Point2>{Point2(0, 0), Point2(12, 0)}));
    EXPECT_EQ(verticesOf(*document.model().entities.find(oldLines[1])),
              (std::vector<Point2>{Point2(0, 5), Point2(10, 5), Point2(20, 5)}));
    EXPECT_EQ(document.model().entities.find(oldLines[0])->style, "Joint Line");
    // The job owns what it owned, in the order it listed it, and then the
    // point drawn now: four points, two lines, point 5.
    std::vector<EntityId> owned = imported.createdEntities;
    owned.push_back(fifth->id);
    EXPECT_EQ(job.createdEntities, owned);
    EXPECT_EQ(job.placedPoints.size(), 5U);

    const SurveyFinishReport* finish = command->finishReport();
    ASSERT_NE(finish, nullptr);
    EXPECT_EQ(finish->points, 1U) << "the one point drawn now";
    EXPECT_EQ(finish->coding.coded, 1U);
    EXPECT_EQ(finish->earlierLinesRedrawn, oldLines);
    EXPECT_TRUE(finish->earlierLinesRemoved.empty());
    EXPECT_TRUE(finish->lines.empty()) << "no line is a new entity";
    // In words: the layer and the style were there already.
    EXPECT_EQ(describe(*finish),
              (std::vector<std::string>{
                  "Survey codes were applied to the 1 point(s) drawn: 1 carry a code and 1 of "
                  "those have a rule; 0 layer(s) and 0 style(s) were created.",
                  "Linework: 0 line(s) drawn and 2 of the job's line(s) redrawn where this "
                  "adjustment puts them."}));
    // Nothing of that is a warning of the job's report.
    EXPECT_TRUE(finishWarnings(*finish).empty());
    EXPECT_EQ(job.reportText.find("Survey codes"), std::string::npos);
    EXPECT_EQ(job.reportText.find("inework"), std::string::npos);

    // One undo: the lines where they ran, the new point gone, the job as it
    // was; one redo: all of it again, the job included.
    const Drawing after = drawingOf(document);
    const SurveyJob readjusted = document.surveyJobs().front();
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(drawingOf(document), before);
    EXPECT_EQ(document.surveyJobs().front(), imported);
    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(drawingOf(document), after);
    EXPECT_EQ(document.surveyJobs().front(), readjusted);
    EXPECT_EQ(fake->calls, 2) << "the import's reduction and the re-adjustment's, no more";
}

TEST(SurveyJobFinish, AJobImportedWithoutAFinishIsNeverCodedByAReadjustment)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    importJob(document, fake, false);
    ASSERT_EQ(document.surveyJobs().front().importOptions, kDefaultOptionsText);

    fake->shots.push_back({"5", 5.0, 20.0, "KJ"});
    fake->features[1].pointIds.push_back("5");
    // The caller's two flags are not what decides: the job's stored text is.
    SurveyFinishOptions asked;
    asked.codes = true;
    asked.linework = true;
    const ReadjustSurveyJobCommand* command = readjust(document, fake, asked);

    EXPECT_EQ(command->changes().created, std::vector<std::string>{"5"});
    EXPECT_EQ(command->finishReport(), nullptr);
    const Entity* fifth = pointNumbered(document, "5");
    ASSERT_NE(fifth, nullptr);
    EXPECT_EQ(fifth->layer, "day1") << "drawn as the job's others, and not coded";
    EXPECT_EQ(fifth->style, "");
    EXPECT_EQ(textProperty(*fifth, "string"), "<absent>");
    EXPECT_EQ(lineCount(document), 0U);
    EXPECT_FALSE(document.model().layers.contains("SURVEY JOINT"));
}

TEST(SurveyJobFinish, WhenTheLineworkStepHasNothingToGoOnTheJobsLinesStayAndTheReportSaysSo)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    importJob(document, fake, true);
    const SurveyJob imported = document.surveyJobs().front();
    ASSERT_EQ(lineCount(document), 2U);

    // The survey codes are unloaded, then the job is re-adjusted: point 2
    // moves, and nothing says any more which codes are lines.
    document.setSurveyMap({});
    fake->shots[1].easting = 12.0;
    const ReadjustSurveyJobCommand* command = readjust(document, fake);

    EXPECT_EQ(command->changes().moved, std::vector<std::string>{"2"});
    const SurveyJob& job = document.surveyJobs().front();
    // Both lines stand, where the earlier run drew them, and are still the job's.
    EXPECT_EQ(job.createdEntities, imported.createdEntities);
    const auto lines = linesOf(document, job);
    ASSERT_EQ(lines.size(), 2U);
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));

    const SurveyFinishReport* finish = command->finishReport();
    ASSERT_NE(finish, nullptr);
    EXPECT_EQ(finish->whyNotStrung, SurveyFinishSkip::NoSurveyCodes);
    EXPECT_EQ(finish->earlierLinesKept, 2U);
    EXPECT_TRUE(finish->earlierLinesRemoved.empty());
    EXPECT_TRUE(finish->earlierLinesRedrawn.empty());
    // No point was new, so no coding was due; the lines are what to act on.
    const std::string stale = "No linework was drawn: no survey codes are loaded. The job's 2 "
                              "line(s) are as the earlier adjustment drew them.";
    EXPECT_EQ(describe(*finish),
              (std::vector<std::string>{
                  "No new points were drawn, so no survey codes were applied.", stale}));
    EXPECT_EQ(finishWarnings(*finish), std::vector<std::string>{stale});
    EXPECT_NE(job.reportText.find(stale), std::string::npos);
    EXPECT_EQ(job.reportText.find("No new points were drawn"), std::string::npos);
}

TEST(SurveyJobFinish, WhenNoRuleKnowsTheJobsCodesAnyMoreItsLinesStayAsTheyWere)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    importJob(document, fake, true);
    const SurveyJob imported = document.surveyJobs().front();

    // Other survey codes are loaded - there are rules, and none for KJ - and
    // then point 2 moves.
    katana::entity::SurveyMap other;
    SurveyRule kerb;
    kerb.key = "KB*";
    kerb.model = "SURVEY KERB";
    kerb.breakline = SurveyBreakline::Line;
    ASSERT_TRUE(other.add(kerb).ok());
    document.setSurveyMap(other);
    fake->shots[1].easting = 12.0;
    const ReadjustSurveyJobCommand* command = readjust(document, fake);

    EXPECT_EQ(command->changes().moved, std::vector<std::string>{"2"});
    const SurveyJob& job = document.surveyJobs().front();
    EXPECT_EQ(job.createdEntities, imported.createdEntities);
    const auto lines = linesOf(document, job);
    ASSERT_EQ(lines.size(), 2U);
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}))
        << "where the earlier run drew it, though point 2 is now at E 12";

    const SurveyFinishReport* finish = command->finishReport();
    ASSERT_NE(finish, nullptr);
    EXPECT_EQ(finish->whyNotStrung, SurveyFinishSkip::NoRuleMatches);
    EXPECT_EQ(finish->earlierLinesKept, 2U);
    EXPECT_TRUE(finish->earlierLinesRedrawn.empty());
    EXPECT_TRUE(finish->earlierLinesRemoved.empty());
    ASSERT_EQ(finish->unplacedFeatures.size(), 2U);
    EXPECT_EQ(finish->unplacedFeatures[0].reason, UnplacedFeatureReason::NoRule);
    const std::string stale = "No linework was drawn: no rule matches any of their codes. The "
                              "job's 2 line(s) are as the earlier adjustment drew them.";
    EXPECT_EQ(finishWarnings(*finish), std::vector<std::string>{stale});
    EXPECT_NE(job.reportText.find(stale), std::string::npos);
}

TEST(SurveyJobFinish, AReadjustmentThatChangesNothingLeavesTheJobsLinesAsTheyAre)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    importJob(document, fake, true);
    const SurveyJob imported = document.surveyJobs().front();
    const Drawing before = drawingOf(document);

    const ReadjustSurveyJobCommand* command = readjust(document, fake); // the same run again

    EXPECT_TRUE(command->changes().moved.empty());
    EXPECT_EQ(command->finishReport(), nullptr);
    EXPECT_EQ(drawingOf(document), before);
    EXPECT_EQ(document.surveyJobs().front().createdEntities, imported.createdEntities);
}

// ---- A finish with nothing to do, and what a job's report holds --------------------------

// The promise the front ends' defaults rest on: with no survey codes loaded,
// asking for the finish changes NOTHING in the drawing - not a layer, not a
// property. (The job's stored options do say it was asked for.)
TEST(SurveyJobFinish, WithNoSurveyCodesLoadedAFinishedImportDrawsExactlyWhatAPlainOneDraws)
{
    Document plain;
    Document finished; // neither has survey codes
    importJob(plain, twoJoints(), false);
    importJob(finished, twoJoints(), true);

    EXPECT_EQ(drawingOf(finished), drawingOf(plain));
    EXPECT_EQ(textProperty(*pointNumbered(finished, "1"), "string"), "<absent>");
    EXPECT_EQ(finished.surveyJobs().front().createdEntities,
              plain.surveyJobs().front().createdEntities);
}

TEST(SurveyJobFinish, AFinishThatDidAllItWasAskedAddsNoWarningToTheJobsReport)
{
    Document plain;
    Document finished;
    plain.setSurveyMap(jointMap());
    finished.setSurveyMap(jointMap());

    const ImportSurveyJobCommand* without = importJob(plain, twoJoints(), false);
    const ImportSurveyJobCommand* with = importJob(finished, twoJoints(), true);

    // Four points coded, two lines drawn, nothing left over: what ran is in
    // finishReport(), and the report's warnings are those of the same import
    // with no finish at all - the reduction's and the drawing's own.
    ASSERT_NE(with->finishReport(), nullptr);
    EXPECT_TRUE(finishWarnings(*with->finishReport()).empty());
    ASSERT_NE(with->report(), nullptr);
    ASSERT_NE(without->report(), nullptr);
    EXPECT_EQ(with->report()->warnings, without->report()->warnings);
    const SurveyJob& job = finished.surveyJobs().front();
    EXPECT_EQ(job.reportText.find("Survey codes"), std::string::npos);
    EXPECT_EQ(job.reportText.find("inework"), std::string::npos);
    EXPECT_EQ(job.reportText, plain.surveyJobs().front().reportText);
}

// ... and one that left something over adds exactly that: here two shots of
// a code no rule knows, which are neither coded nor strung.
TEST(SurveyJobFinish, WhatAFinishLeavesOverIsAWarningOfTheJobsReport)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    fake->shots.push_back({"5", 9.0, 0.0, "ZZ"});
    fake->shots.push_back({"6", 9.0, 10.0, "ZZ"});
    fake->features.push_back(feature("ZZ", "1", {"5", "6"}));

    const ImportSurveyJobCommand* command = importJob(document, fake, true);

    const SurveyFinishReport* finish = command->finishReport();
    ASSERT_NE(finish, nullptr);
    // Six points, all coded; the four KJ are ruled. ZZ 1 is a string of the
    // file that no rule makes anything of.
    const std::vector<std::string> left = {
        "No rule for the code(s): ZZ.",
        "1 string(s) of the file are in no line (no rule for its code: 1)."};
    EXPECT_EQ(describe(*finish),
              (std::vector<std::string>{
                  "Survey codes were applied to the 6 point(s) drawn: 6 carry a code and 4 of "
                  "those have a rule; 1 layer(s) and 1 style(s) were created.",
                  left[0], "Linework: 2 line(s) drawn.", left[1]}));
    EXPECT_EQ(finishWarnings(*finish), left);
    const SurveyJob& job = document.surveyJobs().front();
    ASSERT_NE(command->report(), nullptr);
    for (const std::string& sentence : left) {
        EXPECT_NE(job.reportText.find(sentence), std::string::npos) << sentence;
        EXPECT_EQ(std::count_if(command->report()->warnings.begin(),
                                command->report()->warnings.end(),
                                [&](const survey::ReportMessage& warning) {
                                    return warning.text == sentence;
                                }),
                  1)
            << sentence;
    }
    EXPECT_EQ(job.reportText.find("Linework: 2 line(s) drawn."), std::string::npos)
        << "what was done is not a warning";
}

// ---- Re-adjust: the job's lines are redrawn where they stand -----------------------------

TEST(SurveyJobFinish, AReadjustmentThatOnlyMovesPointsRedrawsTheJobsLinesAsTheSameEntities)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    importJob(document, fake, true);
    const SurveyJob imported = document.surveyJobs().front();
    const EntityId first = imported.createdEntities[4];  // KJ 01
    const EntityId second = imported.createdEntities[5]; // KJ 02
    const Entity secondBefore = *document.model().entities.find(second);

    // Point 2 moves two metres east: KJ 01 is then (N 0, E 0) to (N 0, E 12).
    fake->shots[1].easting = 12.0;
    const ReadjustSurveyJobCommand* command = readjust(document, fake);

    EXPECT_EQ(command->changes().moved, std::vector<std::string>{"2"});
    EXPECT_TRUE(command->changes().created.empty());
    const SurveyJob& job = document.surveyJobs().front();
    // The same six entities, in the same order: nothing deleted, nothing new.
    EXPECT_EQ(job.createdEntities, imported.createdEntities);
    EXPECT_EQ(document.model().entities.size(), 6U);
    ASSERT_TRUE(isLine(document, first));
    EXPECT_EQ(verticesOf(*document.model().entities.find(first)),
              (std::vector<Point2>{Point2(0, 0), Point2(12, 0)}));
    // KJ 02, none of whose points moved, is not touched at all.
    ASSERT_TRUE(isLine(document, second));
    EXPECT_EQ(*document.model().entities.find(second), secondBefore);

    const SurveyFinishReport* finish = command->finishReport();
    ASSERT_NE(finish, nullptr);
    EXPECT_EQ(finish->points, 0U) << "no point was drawn for the first time";
    EXPECT_EQ(finish->whyNotCoded, SurveyFinishSkip::NoPoints);
    EXPECT_EQ(finish->whyNotStrung, SurveyFinishSkip::None);
    // Both lines are strung again by this run - the second where it already
    // ran - and none is created or deleted.
    EXPECT_EQ(finish->earlierLinesRedrawn, (std::vector<EntityId>{first, second}));
    EXPECT_TRUE(finish->lines.empty());
    EXPECT_TRUE(finish->earlierLinesRemoved.empty());
    EXPECT_EQ(finish->earlierLinesKept, 0U);
    EXPECT_EQ(command->createdEntities(), std::vector<EntityId>{});
    EXPECT_EQ(describe(*finish),
              (std::vector<std::string>{
                  "No new points were drawn, so no survey codes were applied.",
                  "Linework: 0 line(s) drawn and 2 of the job's line(s) redrawn where this "
                  "adjustment puts them."}));
    EXPECT_TRUE(finishWarnings(*finish).empty());
}

// What deleting and redrawing the lines cost: everything that follows a
// line - its label with the place it was dragged to and the text typed over
// it - goes when the line does. The line is the same entity afterwards, so
// the label is still on it, and follows it.
TEST(SurveyJobFinish, ALabelOnAJobsLineStaysOnItThroughAReadjustment)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    importJob(document, fake, true);
    const EntityId line = document.surveyJobs().front().createdEntities[4]; // KJ 01
    katana::entity::LabelStyle style;
    style.name = "Side";
    style.kind = katana::entity::LabelKind::Segment;
    style.text = "{length}";
    ASSERT_TRUE(document.execute(katana::commands::createLabelStyle(style)).ok());
    // On the line's one segment, whose midpoint is (5, 0); dragged two metres
    // off it, with its text typed over.
    Entity labelEntity;
    labelEntity.geometry = katana::entity::LabelGeometry{.target = line,
                                                        .part = 0,
                                                        .style = "Side",
                                                        .anchor = Point2(5, 0),
                                                        .position = Point2(5, 2),
                                                        .textOverride = "joint 01"};
    ASSERT_TRUE(document.execute(katana::commands::createEntities({labelEntity})).ok());
    const EntityId label = document.lastCreatedEntities().front();

    fake->shots[1].easting = 12.0; // the line becomes (0, 0) to (12, 0)
    readjust(document, fake);

    const Entity* kept = document.model().entities.find(label);
    ASSERT_NE(kept, nullptr) << "the label went with the line it was on";
    const auto& geometry = std::get<katana::entity::LabelGeometry>(kept->geometry);
    EXPECT_EQ(geometry.target, line);
    EXPECT_EQ(geometry.textOverride, "joint 01");
    // It follows the segment's new midpoint, (6, 0), keeping its place beside
    // it: one metre east of where it was dragged to.
    EXPECT_EQ(geometry.anchor, Point2(6, 0));
    ASSERT_TRUE(geometry.position.has_value());
    EXPECT_EQ(*geometry.position, Point2(6, 2));

    // And one undo puts the line and the label back where they were.
    ASSERT_TRUE(document.undo().ok());
    const auto& before =
        std::get<katana::entity::LabelGeometry>(document.model().entities.find(label)->geometry);
    EXPECT_EQ(before.anchor, Point2(5, 0));
    EXPECT_EQ(*before.position, Point2(5, 2));
}

// Under Keep a point the person moved stays where they put it, and one they
// deleted stays deleted. A line of the job runs through each of its points
// where the point STANDS in the drawing - so through the moved point where it
// was moved to - and, for a point that is not in the drawing, where the run
// puts it: deleting a mark does not take a vertex out of the kerb.
TEST(SurveyJobFinish, UnderKeepALineRunsThroughAHandMovedPointWhereItStands)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    importJob(document, fake, true);
    const SurveyJob imported = document.surveyJobs().front();
    const EntityId second = pointNumbered(document, "2")->id;
    const EntityId fourth = pointNumbered(document, "4")->id;
    // By hand: point 2 one metre north, to (10, 1); point 4 deleted.
    ASSERT_TRUE(document
                    .execute(katana::commands::moveEntities({second},
                                                            katana::geometry::Vec2(0.0, 1.0)))
                    .ok());
    ASSERT_TRUE(document.execute(katana::commands::deleteEntities({fourth})).ok());

    // The new run moves points 1 and 3 two metres west and nothing else:
    //   KJ 01: 1 (N 0, E -2), 2 (N 0, E 10)     KJ 02: 3 (N 5, E -2), 4 (N 5, E 10)
    fake->shots[0].easting = -2.0;
    fake->shots[2].easting = -2.0;
    const ReadjustSurveyJobCommand* command = readjust(document, fake);

    EXPECT_EQ(command->changes().moved, (std::vector<std::string>{"1", "3"}));
    EXPECT_EQ(command->changes().editedByHand, std::vector<std::string>{"2"});
    EXPECT_EQ(command->changes().deletedByHand, std::vector<std::string>{"4"});
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(
                  document.model().entities.find(second)->geometry)
                  .position,
              Point2(10, 1))
        << "the hand edit is kept";
    EXPECT_FALSE(document.model().entities.contains(fourth));

    const auto lines = linesOf(document, document.surveyJobs().front());
    ASSERT_EQ(lines.size(), 2U);
    // KJ 01: from point 1 where the run puts it to point 2 where it stands.
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(-2, 0), Point2(10, 1)}));
    // KJ 02: from point 3 to where the run puts the deleted point 4.
    EXPECT_EQ(verticesOf(*lines[1]), (std::vector<Point2>{Point2(-2, 5), Point2(10, 5)}));
    // ... which the report says, in the job's own report too: one vertex.
    const SurveyFinishReport* finish = command->finishReport();
    ASSERT_NE(finish, nullptr);
    EXPECT_EQ(finish->verticesAtDeletedPoints, 1U);
    const std::string atDeleted =
        "1 vertex(es) of the job's lines are at a point deleted from the drawing by hand: a "
        "string the file numbered still runs through where this adjustment puts it.";
    EXPECT_EQ(finishWarnings(*finish), std::vector<std::string>{atDeleted});
    EXPECT_NE(document.surveyJobs().front().reportText.find(atDeleted), std::string::npos);
}

// Under Overwrite the deleted point is drawn again, so there is no vertex
// without a point, and nothing to say.
TEST(SurveyJobFinish, UnderOverwriteADeletedPointIsDrawnAgainAndItsLineRunsThroughIt)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    importJob(document, fake, true);
    const EntityId second = document.surveyJobs().front().createdEntities[5]; // KJ 02
    ASSERT_TRUE(document
                    .execute(katana::commands::deleteEntities({pointNumbered(document, "4")->id}))
                    .ok());

    const ReadjustSurveyJobCommand* command =
        readjust(document, fake, {}, HandEditPolicy::Overwrite);

    EXPECT_EQ(command->changes().deletedByHand, std::vector<std::string>{"4"});
    EXPECT_EQ(command->changes().created, std::vector<std::string>{"4"});
    const Entity* again = pointNumbered(document, "4");
    ASSERT_NE(again, nullptr);
    // Drawn now, so coded now, as a new point of the job is.
    EXPECT_EQ(again->layer, "SURVEY JOINT");
    EXPECT_EQ(textProperty(*again, "string"), "02");
    const SurveyFinishReport* finish = command->finishReport();
    ASSERT_NE(finish, nullptr);
    EXPECT_EQ(finish->points, 1U);
    EXPECT_EQ(finish->verticesAtDeletedPoints, 0U);
    EXPECT_TRUE(finishWarnings(*finish).empty());
    // KJ 02 is the line it was, where it ran.
    ASSERT_TRUE(isLine(document, second));
    EXPECT_EQ(verticesOf(*document.model().entities.find(second)),
              (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    EXPECT_EQ(finish->earlierLinesRedrawn.size(), 2U);
}

TEST(SurveyJobFinish, ALineTheNewRunNoLongerStringsIsDeletedAndTheReportSaysSo)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    importJob(document, fake, true);
    const SurveyJob imported = document.surveyJobs().front();
    const EntityId first = imported.createdEntities[4];  // KJ 01
    const EntityId second = imported.createdEntities[5]; // KJ 02
    const Drawing before = drawingOf(document);

    // The new run no longer computes point 4: string 02 is point 3 alone,
    // and one point makes no line.
    fake->shots.pop_back();
    fake->features[1].pointIds = {"3"};
    const ReadjustSurveyJobCommand* command = readjust(document, fake);

    EXPECT_EQ(command->changes().removed, std::vector<std::string>{"4"});
    EXPECT_TRUE(isLine(document, first)) << "KJ 01 is strung as it was, and stays";
    EXPECT_FALSE(document.model().entities.contains(second));
    const SurveyJob& job = document.surveyJobs().front();
    // Points 1, 2 and 3, and the one line.
    EXPECT_EQ(job.createdEntities,
              (std::vector<EntityId>{imported.createdEntities[0], imported.createdEntities[1],
                                     imported.createdEntities[2], first}));

    const SurveyFinishReport* finish = command->finishReport();
    ASSERT_NE(finish, nullptr);
    EXPECT_EQ(finish->earlierLinesRedrawn, std::vector<EntityId>{first});
    EXPECT_EQ(finish->earlierLinesRemoved, std::vector<EntityId>{second});
    ASSERT_EQ(finish->unplacedFeatures.size(), 1U);
    EXPECT_EQ(finish->unplacedFeatures[0].name, "KJ 02");
    EXPECT_EQ(finish->unplacedFeatures[0].reason, UnplacedFeatureReason::TooFewPoints);
    const std::vector<std::string> left = {
        "1 of the job's line(s) are no longer strung by this adjustment and were deleted, "
        "with anything attached to them.",
        "1 string(s) of the file are in no line (fewer than two of its points have a "
        "position: 1)."};
    EXPECT_EQ(describe(*finish),
              (std::vector<std::string>{
                  "No new points were drawn, so no survey codes were applied.",
                  "Linework: 0 line(s) drawn and 1 of the job's line(s) redrawn where this "
                  "adjustment puts them.",
                  left[0], left[1]}));
    EXPECT_EQ(finishWarnings(*finish), left);
    for (const std::string& sentence : left) {
        EXPECT_NE(job.reportText.find(sentence), std::string::npos) << sentence;
    }

    // One undo puts the point and the line back, as they were.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(drawingOf(document), before);
    EXPECT_EQ(document.surveyJobs().front(), imported);
}

// ---- One option without the other, and what a re-adjustment is handed --------------------

TEST(SurveyJobFinish, AJobImportedForCodesAloneIsStoredAndReadjustedForCodesAlone)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    const ImportSurveyJobCommand* imported = importJob(document, fake, importRequest(true, false));

    EXPECT_EQ(document.surveyJobs().front().importOptions,
              std::string(kDefaultOptionsText) + "apply-codes=true\n");
    EXPECT_EQ(document.surveyJobs().front().createdEntities.size(), 4U);
    EXPECT_EQ(lineCount(document), 0U);
    EXPECT_EQ(pointNumbered(document, "1")->layer, "SURVEY JOINT");
    ASSERT_NE(imported->finishReport(), nullptr);
    EXPECT_EQ(imported->finishReport()->whyNotCoded, SurveyFinishSkip::None);
    EXPECT_EQ(imported->finishReport()->whyNotStrung, SurveyFinishSkip::NotAsked);

    // A third shot of string 02. The caller's two flags say both; the job's
    // stored text says codes only, and that is what is done.
    fake->shots.push_back({"5", 5.0, 20.0, "KJ"});
    fake->features[1].pointIds.push_back("5");
    SurveyFinishOptions asked;
    asked.codes = true;
    asked.linework = true;
    const ReadjustSurveyJobCommand* command = readjust(document, fake, asked);

    const Entity* fifth = pointNumbered(document, "5");
    ASSERT_NE(fifth, nullptr);
    EXPECT_EQ(fifth->layer, "SURVEY JOINT");
    EXPECT_EQ(fifth->style, "Joint Line");
    EXPECT_EQ(lineCount(document), 0U) << "a job never strung is not strung now";
    ASSERT_NE(command->finishReport(), nullptr);
    EXPECT_EQ(command->finishReport()->coding.coded, 1U);
    EXPECT_EQ(command->finishReport()->whyNotStrung, SurveyFinishSkip::NotAsked);
    EXPECT_EQ(document.surveyJobs().front().createdEntities.size(), 5U);
}

TEST(SurveyJobFinish, AJobImportedForLineworkAloneIsStoredAndReadjustedForLineworkAlone)
{
    Document document;
    document.setSurveyMap(jointMap());
    auto fake = twoJoints();
    const ImportSurveyJobCommand* imported = importJob(document, fake, importRequest(false, true));

    const SurveyJob job = document.surveyJobs().front();
    EXPECT_EQ(job.importOptions, std::string(kDefaultOptionsText) + "draw-linework=true\n");
    ASSERT_EQ(job.createdEntities.size(), 6U);
    const EntityId second = job.createdEntities[5]; // KJ 02
    // The points are where the import put them, unstyled; the lines are on
    // their rule's layer in its style.
    EXPECT_EQ(pointNumbered(document, "1")->layer, "day1");
    EXPECT_EQ(pointNumbered(document, "1")->style, "");
    EXPECT_EQ(document.model().entities.find(second)->layer, "SURVEY JOINT");
    EXPECT_EQ(document.model().entities.find(second)->style, "Joint Line");
    ASSERT_NE(imported->finishReport(), nullptr);
    EXPECT_EQ(imported->finishReport()->whyNotCoded, SurveyFinishSkip::NotAsked);
    EXPECT_EQ(imported->finishReport()->whyNotStrung, SurveyFinishSkip::None);

    fake->shots.push_back({"5", 5.0, 20.0, "KJ"});
    fake->features[1].pointIds.push_back("5");
    SurveyFinishOptions asked;
    asked.codes = true;
    asked.linework = true;
    const ReadjustSurveyJobCommand* command = readjust(document, fake, asked);

    const Entity* fifth = pointNumbered(document, "5");
    ASSERT_NE(fifth, nullptr);
    EXPECT_EQ(fifth->layer, "day1") << "a job never coded is not coded now";
    EXPECT_EQ(fifth->style, "");
    // KJ 02 runs on through the new point, as the entity it was.
    ASSERT_TRUE(isLine(document, second));
    EXPECT_EQ(verticesOf(*document.model().entities.find(second)),
              (std::vector<Point2>{Point2(0, 5), Point2(10, 5), Point2(20, 5)}));
    EXPECT_EQ(lineCount(document), 2U);
    ASSERT_NE(command->finishReport(), nullptr);
    EXPECT_EQ(command->finishReport()->whyNotCoded, SurveyFinishSkip::NotAsked);
    EXPECT_EQ(command->finishReport()->whyNotStrung, SurveyFinishSkip::None);
    EXPECT_EQ(command->finishReport()->earlierLinesRedrawn,
              (std::vector<EntityId>{job.createdEntities[4], second}));
}

// HOW a job is finished again is the caller's: here the colour lookup, which
// is not job data. Handed on, the new point wears the style its neighbours
// were given; without it "red" is a name with no colour, the neighbours'
// style is not one that draws it, and a second style is made.
TEST(SurveyJobFinish, AReadjustmentStylesANewPointWithTheColourLookupItIsHanded)
{
    katana::entity::SurveyMap map;
    SurveyRule joint;
    joint.key = "KJ*";
    joint.model = "SURVEY JOINT";
    joint.linestyle = "Joint Line";
    joint.colour = "red";
    joint.breakline = SurveyBreakline::Line;
    ASSERT_TRUE(map.add(joint).ok());
    const auto red = [](std::string_view name) -> std::optional<katana::entity::Color> {
        return name == "red" ? std::optional(katana::entity::Color{255, 0, 0, 255})
                             : std::nullopt;
    };
    for (const bool handedOn : {true, false}) {
        SCOPED_TRACE(handedOn ? "with the lookup" : "without it");
        Document document;
        document.setSurveyMap(map);
        auto fake = twoJoints();
        SurveyJobImport request = importRequest(true, true);
        request.finish.coding.colourOf = red;
        importJob(document, fake, std::move(request));
        const katana::entity::Style* style = document.model().styles.find("Joint Line");
        ASSERT_NE(style, nullptr);
        ASSERT_EQ(style->color, std::optional(katana::entity::Color{255, 0, 0, 255}));
        ASSERT_EQ(document.model().styles.all().size(), 1U);

        fake->shots.push_back({"5", 5.0, 20.0, "KJ"});
        fake->features[1].pointIds.push_back("5");
        SurveyFinishOptions how;
        if (handedOn) {
            how.coding.colourOf = red;
        }
        const ReadjustSurveyJobCommand* command = readjust(document, fake, how);

        const Entity* fifth = pointNumbered(document, "5");
        ASSERT_NE(fifth, nullptr);
        ASSERT_NE(command->finishReport(), nullptr);
        if (handedOn) {
            EXPECT_EQ(fifth->style, "Joint Line");
            EXPECT_TRUE(command->finishReport()->stylesCreated().empty());
            EXPECT_EQ(document.model().styles.all().size(), 1U);
        } else {
            EXPECT_EQ(fifth->style, "Joint Line (red)");
            EXPECT_EQ(command->finishReport()->stylesCreated(),
                      std::vector<std::string>{"Joint Line (red)"});
        }
    }
}
