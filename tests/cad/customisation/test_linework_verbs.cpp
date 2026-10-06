// LINEWORK on the command line (include/katana/cad/linework_verbs.hpp), and
// surveyImportFinish, which says what the drawing's customisation asks of a
// survey import.
//
// Every expected value is worked by hand from the points a test lists: a
// point is (x, y), its number is the property "point" and its code "code", as
// a survey import writes them. Entity ids are monotonic, so in a drawing that
// held nothing the n-th entity made is entity n; a survey job takes one id
// for its own number before its points take theirs.

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/linework.hpp"
#include "katana/cad/linework_verbs.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"

namespace cmd = katana::commands;
using namespace katana::cad;
using katana::core::ErrorCode;
using katana::core::Result;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyRule;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
namespace survey = katana::survey;

namespace {

// A kerb is a LINE on KERB, drawn plain in the customisation's own colour
// "kerb red"; a mark is a POINT on MARKS.
katana::entity::SurveyMap kerbMap()
{
    katana::entity::SurveyMap map;
    SurveyRule kerb;
    kerb.key = "KB*";
    kerb.model = "KERB";
    kerb.colour = "kerb red";
    kerb.breakline = SurveyBreakline::Line;
    EXPECT_TRUE(map.add(kerb).ok());
    SurveyRule mark;
    mark.key = "MK";
    mark.model = "MARKS";
    mark.breakline = SurveyBreakline::Point;
    EXPECT_TRUE(map.add(mark).ok());
    return map;
}

struct Shot {
    std::string number;
    double x = 0.0;
    double y = 0.0;
    std::string code;
};

// The shots as point entities on `layer`, made by ONE command: one undo step,
// and ids in the order listed.
std::vector<EntityId> addPoints(Document& document, const std::vector<Shot>& shots,
                                const std::string& layer = "0")
{
    std::vector<Entity> entities;
    for (const Shot& shot : shots) {
        Entity entity;
        entity.geometry = katana::entity::PointGeometry{Point2(shot.x, shot.y)};
        entity.layer = layer;
        if (!shot.number.empty()) {
            entity.properties.insert_or_assign("point", katana::entity::PropertyValue(shot.number));
        }
        if (!shot.code.empty()) {
            entity.properties.insert_or_assign("code", katana::entity::PropertyValue(shot.code));
        }
        entities.push_back(std::move(entity));
    }
    const auto status = document.execute(cmd::createEntities(std::move(entities)));
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    return document.lastCreatedEntities();
}

// Two kerbs and a mark:
//   KB1: 1 (0,0)  2 (10,0)  3 (20,0)
//   KB2: 4 (0,5)  5 (10,5)
//   MK:  6 (5,9)
std::vector<EntityId> twoKerbsAndAMark(Document& document)
{
    return addPoints(document, {{"1", 0, 0, "KB1"},
                                {"2", 10, 0, "KB1"},
                                {"3", 20, 0, "KB1"},
                                {"4", 0, 5, "KB2"},
                                {"5", 10, 5, "KB2"},
                                {"6", 5, 9, "MK"}});
}

// A shot with a height, as a survey import draws one: a point's height is
// the one writer's (entity::setHeights).
Entity levelledPoint(const std::string& number, double x, double y, const std::string& code,
                     double height)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{Point2(x, y)};
    entity.properties.insert_or_assign("point", katana::entity::PropertyValue(number));
    entity.properties.insert_or_assign("code", katana::entity::PropertyValue(code));
    katana::entity::setHeights(entity.properties, {height});
    return entity;
}

// A polyline somebody left in the drawing, carrying `code` as linework's own
// lines carry it and, when `string` is not empty, a string number beside it.
Entity codedLine(std::vector<Point2> vertices, bool closed, const std::string& code,
                 const std::string& string, const std::vector<std::optional<double>>& heights,
                 const std::string& layer)
{
    Entity entity;
    entity.geometry = Polyline2{std::move(vertices), closed};
    entity.layer = layer;
    entity.properties.insert_or_assign("code", katana::entity::PropertyValue(code));
    if (!string.empty()) {
        entity.properties.insert_or_assign("string", katana::entity::PropertyValue(string));
    }
    katana::entity::setHeights(entity.properties, heights);
    return entity;
}

// The entities, made by ONE command, in the order given.
std::vector<EntityId> addEntities(Document& document, std::vector<Entity> entities)
{
    const auto status = document.execute(cmd::createEntities(std::move(entities)));
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    return document.lastCreatedEntities();
}

std::string run(Document& document, const std::string& line)
{
    CommandInterpreter interpreter(document);
    const auto reply = interpreter.run(line);
    EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
    return reply.ok() ? *reply : std::string{};
}

std::vector<std::string> linesOf(const std::string& reply)
{
    std::vector<std::string> lines;
    std::size_t start = 0;
    while (start <= reply.size()) {
        const std::size_t end = std::min(reply.find('\n', start), reply.size());
        lines.push_back(reply.substr(start, end - start));
        start = end + 1;
    }
    return lines;
}

std::vector<const Entity*> polylinesOf(const Document& document)
{
    std::vector<const Entity*> found;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<Polyline2>(entity.geometry)) {
            found.push_back(&entity);
        }
    });
    return found;
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

// ---- a survey job, as tests/cad/test_survey_job_finish.cpp makes one -----------------
//
// The reduction is injected: it returns the shots and the features listed,
// which is a field file that keeps code and string number apart.

struct FieldShot {
    std::string id;
    double northing = 0.0;
    double easting = 0.0;
    std::string code;
};

struct FakeField {
    std::vector<FieldShot> shots;
    std::vector<survey::SurveyFeature> features;
};

ReductionFunction reductionOf(const std::shared_ptr<FakeField>& fake)
{
    return [fake](const survey::SurveyProject& raw, const survey::ReductionSettings& settings,
                  const survey::ReductionContext& context) -> Result<survey::ReductionOutcome> {
        survey::ReductionOutcome outcome;
        outcome.reduced.name = raw.name;
        for (const FieldShot& shot : fake->shots) {
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
    f.name = number;
    f.pointIds = std::move(ids);
    return f;
}

// "KJ*": a joint is a LINE on JOINTS.
katana::entity::SurveyMap jointMap()
{
    katana::entity::SurveyMap map;
    SurveyRule joint;
    joint.key = "KJ*";
    joint.model = "JOINTS";
    joint.breakline = SurveyBreakline::Line;
    EXPECT_TRUE(map.add(joint).ok());
    return map;
}

// Imports a job of two strings of the joint code KJ, two shots each, on the
// layer "day1" (a survey point is (northing, easting); drawn, (x, y) is
// (easting, northing)):
//   KJ 01: point 1 (N 0, E 0) and point 2 (N 0, E 10)
//   KJ 02: point 3 (N 5, E 0) and point 4 (N 5, E 10)
// In a drawing that held nothing the job is job-1, the points are entities 2
// to 5 and - coded and strung - the lines KJ 01 and KJ 02 are entities 6 and 7.
void importTwoJoints(Document& document, bool codes, bool linework)
{
    auto fake = std::make_shared<FakeField>();
    fake->shots = {{"1", 0.0, 0.0, "KJ"}, {"2", 0.0, 10.0, "KJ"}, {"3", 5.0, 0.0, "KJ"},
                   {"4", 5.0, 10.0, "KJ"}};
    fake->features = {feature("KJ", "01", {"1", "2"}), feature("KJ", "02", {"3", "4"})};
    SurveyJobImport request;
    request.job.name = "DAY1.FLD";
    request.job.formatId = "opcode-field-file";
    request.job.parserVersion = "1.0";
    request.job.sourceFileName = "DAY1.FLD";
    request.job.sourceBytes = "the field file's bytes";
    request.job.layer = "day1";
    request.raw.name = "day one";
    request.context.createdUtc = "2026-10-06T10:00:00Z";
    request.importOptions.layer = "day1";
    request.finish.codes = codes;
    request.finish.linework = linework;
    const auto status = document.execute(std::make_unique<ImportSurveyJobCommand>(
        document, std::move(request), reductionOf(fake)));
    ASSERT_TRUE(status.ok()) << status.error().describe();
}

} // namespace

// ---- the grammar ------------------------------------------------------------------------

TEST(LineworkVerb, AWordTheGrammarDoesNotTakeIsRefusedByNameAndNothingIsDrawn)
{
    const struct {
        const char* line;
        ErrorCode code;
        const char* said;
        const char* word;
    } refusals[] = {
        {"LINEWORK ORDER", ErrorCode::ParseFailure, "ORDER needs number or entity after it",
         "ORDER"},
        {"LINEWORK ORDER sideways", ErrorCode::ParseFailure, "ORDER takes number", "sideways"},
        {"LINEWORK ORDER number ORDER entity", ErrorCode::ParseFailure, "ORDER is given twice",
         "ORDER"},
        // A scope after the verb's own words is not read as one.
        {"LINEWORK PREVIEW DRAWING", ErrorCode::ParseFailure,
         "the scope and its WHERE come first", "DRAWING"},
        {"LINEWORK ORDER entity WHERE TYPE=point", ErrorCode::ParseFailure,
         "the scope and its WHERE come first", "WHERE"},
        {"LINEWORK KEEP", ErrorCode::ParseFailure, "not a LINEWORK word", "KEEP"},
        // The shared scope parser's own refusals, in its words.
        {"LINEWORK DRAWING SELECTION", ErrorCode::InvalidArgument, "give one scope", "SELECTION"},
        {"LINEWORK WHERE TYPE=blob", ErrorCode::InvalidArgument, "", "blob"},
        {"LINEWORK LAYERS nowhere", ErrorCode::NotFound, "layer does not exist", "nowhere"},
        // Headless there is no view: VIEW is refused in favour of AREA.
        {"LINEWORK VIEW", ErrorCode::InvalidState, "AREA x0,y0,x1,y1", ""},
    };
    for (const auto& refusal : refusals) {
        SCOPED_TRACE(refusal.line);
        Document document;
        document.setSurveyMap(kerbMap());
        twoKerbsAndAMark(document);
        CommandInterpreter interpreter(document);
        const auto reply = interpreter.run(refusal.line);
        ASSERT_FALSE(reply.ok()) << *reply;
        // The type of a WHERE value is the scope parser's to refuse, with
        // whatever code it uses: only that it names the word is held here.
        if (std::string_view(refusal.said).empty()) {
            EXPECT_NE(reply.error().describe().find(refusal.word), std::string::npos)
                << reply.error().describe();
        } else {
            EXPECT_EQ(reply.error().code, refusal.code) << reply.error().describe();
            EXPECT_NE(reply.error().message.find(refusal.said), std::string::npos)
                << reply.error().describe();
            EXPECT_NE(reply.error().describe().find(refusal.word), std::string::npos)
                << reply.error().describe();
        }
        EXPECT_TRUE(polylinesOf(document).empty());
        EXPECT_EQ(document.history().undoCount(), 1U) << "the points, and no step after them";
    }
}

TEST(LineworkVerb, TheVerbAndItsWordsAreReadInAnyCase)
{
    Document document;
    document.setSurveyMap(kerbMap());
    twoKerbsAndAMark(document);
    EXPECT_EQ(run(document, "linework drawing order ENTITY preview"),
              run(document, "LINEWORK DRAWING ORDER entity PREVIEW"));
}

// ---- the scope ---------------------------------------------------------------------------

TEST(LineworkVerb, WithNoScopeWordItTakesTheSelectionWhenThereIsOneElseTheDrawingAndSaysWhich)
{
    Document document;
    document.setSurveyMap(kerbMap());
    const std::vector<EntityId> ids = twoKerbsAndAMark(document);
    ASSERT_EQ(ids.size(), 6U);

    // Nothing selected: the whole drawing. Six points; KB1 and KB2 are lines
    // of 3 and 2 points, and the mark is a point code.
    EXPECT_EQ(linesOf(run(document, "LINEWORK PREVIEW")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=6 considered=6 lines=2 unplaced=1 notes=0 "
                  "preview=yes",
                  "string name=KB1 key=KB* number=1 points=3 vertices=3 closed=no layer=KERB",
                  "string name=KB2 key=KB* number=2 points=2 vertices=2 closed=no layer=KERB",
                  "unplaced reason=\"its code is a point code\" points=1"}));

    // Points 4 and 5 selected: the selection, which is KB2 alone.
    document.selection().set({ids[3], ids[4]});
    EXPECT_EQ(linesOf(run(document, "LINEWORK PREVIEW")),
              (std::vector<std::string>{
                  "linework scope=selection matched=2 considered=2 lines=1 unplaced=0 notes=0 "
                  "preview=yes",
                  "string name=KB2 key=KB* number=2 points=2 vertices=2 closed=no layer=KERB"}));
    // A scope word is the scope, whatever is selected.
    EXPECT_EQ(linesOf(run(document, "LINEWORK DRAWING PREVIEW")).front(),
              "linework scope=drawing matched=6 considered=6 lines=2 unplaced=1 notes=0 "
              "preview=yes");
    // SELECTION with nothing selected takes nothing, and says so: no failure.
    document.selection().clear();
    EXPECT_EQ(run(document, "LINEWORK SELECTION PREVIEW"),
              "linework scope=selection matched=0 considered=0 lines=0 unplaced=0 notes=0 "
              "preview=yes");
    // A bare WHERE filters the default scope: the drawing, nothing being
    // selected, and of it the two points whose code is KB2.
    EXPECT_EQ(linesOf(run(document, "LINEWORK WHERE PROP=code:KB2 PREVIEW")).front(),
              "linework scope=drawing where=\"PROP=code:KB2\" matched=2 considered=2 lines=1 "
              "unplaced=0 notes=0 preview=yes");
}

TEST(LineworkVerb, ALayerAnAreaAndAFilterEachStringOnlyWhatTheyTake)
{
    Document document;
    document.setSurveyMap(kerbMap());
    run(document, "LAYER NEW north");
    run(document, "LAYER NEW south");
    //   KB1 on north: 1 (0,100)  2 (10,100)
    //   KB2 on south: 3 (0,0)    4 (10,0)
    addPoints(document, {{"1", 0, 100, "KB1"}, {"2", 10, 100, "KB1"}}, "north");
    addPoints(document, {{"3", 0, 0, "KB2"}, {"4", 10, 0, "KB2"}}, "south");

    EXPECT_EQ(linesOf(run(document, "LINEWORK LAYERS north PREVIEW")),
              (std::vector<std::string>{
                  "linework scope=layers layers=north sublayers=yes matched=2 considered=2 "
                  "lines=1 unplaced=0 notes=0 preview=yes",
                  "string name=KB1 key=KB* number=1 points=2 vertices=2 closed=no layer=KERB"}));
    // The window (-1,-1) to (11,1) holds points 3 and 4 only.
    EXPECT_EQ(linesOf(run(document, "LINEWORK AREA -1,-1,11,1 PREVIEW")),
              (std::vector<std::string>{
                  "linework scope=area area=-1,-1,11,1 matched=2 considered=2 lines=1 "
                  "unplaced=0 notes=0 preview=yes",
                  "string name=KB2 key=KB* number=2 points=2 vertices=2 closed=no layer=KERB"}));
    EXPECT_EQ(linesOf(run(document, "LINEWORK DRAWING WHERE LAYER=south PREVIEW")).front(),
              "linework scope=drawing where=\"LAYER=south\" matched=2 considered=2 lines=1 "
              "unplaced=0 notes=0 preview=yes");
}

// ---- preview, and the run ----------------------------------------------------------------

TEST(LineworkVerb, APreviewPlansAndChangesNothing)
{
    Document document;
    document.setSurveyMap(kerbMap());
    twoKerbsAndAMark(document);
    const std::uint64_t revision = document.modelRevision();

    const std::string reply = run(document, "LINEWORK PREVIEW");

    EXPECT_NE(reply.find(" lines=2 "), std::string::npos) << reply;
    EXPECT_EQ(reply.find("entity="), std::string::npos) << "nothing was drawn to name";
    EXPECT_EQ(document.model().entities.size(), 6U);
    EXPECT_FALSE(document.model().layers.contains("KERB"));
    EXPECT_EQ(document.history().undoCount(), 1U);
    EXPECT_EQ(document.modelRevision(), revision);
}

TEST(LineworkVerb, ARunDrawsStyledLinesAsOneUndoStepAndRemovesNoPoint)
{
    Document document;
    document.setSurveyMap(kerbMap());
    // The rule's colour is a name of the customisation's own: only the
    // Document's resolver knows it is #AA0000.
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("kerb red", Color{170, 0, 0, 255}).ok());
    document.setColourTable(colours);
    twoKerbsAndAMark(document);

    // The points are entities 1 to 6, so the lines are 7 and 8, in the order
    // the reply lists them.
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=6 considered=6 lines=2 unplaced=1 notes=0",
                  "string name=KB1 key=KB* number=1 points=3 vertices=3 closed=no layer=KERB "
                  "entity=7",
                  "string name=KB2 key=KB* number=2 points=2 vertices=2 closed=no layer=KERB "
                  "entity=8",
                  "unplaced reason=\"its code is a point code\" points=1"}));

    const Entity* first = document.model().entities.find(7);
    const Entity* second = document.model().entities.find(8);
    ASSERT_NE(first, nullptr);
    ASSERT_NE(second, nullptr);
    EXPECT_EQ(verticesOf(*first), (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(20, 0)}));
    EXPECT_EQ(verticesOf(*second), (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    EXPECT_EQ(first->layer, "KERB");
    EXPECT_EQ(textProperty(*first, "code"), "KB1");
    // A plain line in a colour: the style is named "Plain" (survey_coding.hpp,
    // which names a style after its linestyle, else its symbol, else that).
    EXPECT_EQ(first->style, "Plain");
    const katana::entity::Style* style = document.model().styles.find("Plain");
    ASSERT_NE(style, nullptr);
    ASSERT_TRUE(style->color.has_value());
    EXPECT_EQ(*style->color, (Color{170, 0, 0, 255}));

    // No point was removed, and none was coded: that is CODE's.
    std::size_t points = 0;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<katana::entity::PointGeometry>(entity.geometry)) {
            ++points;
            EXPECT_EQ(entity.layer, "0");
            EXPECT_EQ(entity.style, "");
        }
    });
    EXPECT_EQ(points, 6U);

    // One step: the lines, their layer and their style go together.
    EXPECT_EQ(document.history().undoCount(), 2U);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().entities.size(), 6U);
    EXPECT_TRUE(polylinesOf(document).empty());
    EXPECT_FALSE(document.model().layers.contains("KERB"));
    EXPECT_EQ(document.model().styles.find("Plain"), nullptr);
    EXPECT_EQ(document.history().undoCount(), 1U);
}

TEST(LineworkVerb, TheControlCodesAreTheOnesTheDocumentHolds)
{
    Document document;
    document.setSurveyMap(kerbMap());
    //   KB1: 1 (0,0)  2 (10,0)  3 (20,0) "S"  4 (30,0)
    addPoints(document, {{"1", 0, 0, "KB1"},
                         {"2", 10, 0, "KB1"},
                         {"3", 20, 0, "KB1 S"},
                         {"4", 30, 0, "KB1"}});

    // By the default spellings "S" is no control: one line through all four,
    // and the token is noted.
    EXPECT_EQ(linesOf(run(document, "LINEWORK PREVIEW")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=4 considered=4 lines=1 unplaced=0 notes=1 "
                  "preview=yes",
                  "string name=KB1 key=KB* number=1 points=4 vertices=4 closed=no layer=KERB",
                  "note kind=\"unknown token\" count=1"}));

    // The customisation spells start "S": point 3 begins a second line.
    LineworkCodes codes;
    codes.start = "S";
    ASSERT_TRUE(document.setLineworkCodes(codes).ok());
    EXPECT_EQ(linesOf(run(document, "LINEWORK PREVIEW")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=4 considered=4 lines=2 unplaced=0 notes=0 "
                  "preview=yes",
                  "string name=KB1 key=KB* number=1 points=2 vertices=2 closed=no layer=KERB",
                  "string name=KB1 key=KB* number=1 points=2 vertices=2 closed=no layer=KERB"}));
}

TEST(LineworkVerb, OrderSaysWhetherAStringRunsByPointNumberOrAsItsPointsWereDrawn)
{
    Document document;
    document.setSurveyMap(kerbMap());
    // Drawn in this order: number 3 at (0,0), 1 at (10,0), 2 at (20,0), and a
    // shot with no number at (30,0).
    addPoints(document, {{"3", 0, 0, "KB1"},
                         {"1", 10, 0, "KB1"},
                         {"2", 20, 0, "KB1"},
                         {"", 30, 0, "KB1"}});

    // ORDER number is the default, said aloud.
    EXPECT_EQ(run(document, "LINEWORK ORDER number PREVIEW"), run(document, "LINEWORK PREVIEW"));
    // By number: 1, 2, 3; the shot with no number has no place.
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=4 considered=4 lines=1 unplaced=1 notes=0",
                  "string name=KB1 key=KB* number=1 points=3 vertices=3 closed=no layer=KERB "
                  "entity=5",
                  "unplaced reason=\"no point number to order it by\" points=1"}));
    EXPECT_EQ(verticesOf(*document.model().entities.find(5)),
              (std::vector<Point2>{Point2(10, 0), Point2(20, 0), Point2(0, 0)}));
    ASSERT_TRUE(document.undo().ok());

    // As drawn: all four, in the order they were made.
    EXPECT_EQ(linesOf(run(document, "LINEWORK ORDER entity")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=4 considered=4 lines=1 unplaced=0 notes=0",
                  "string name=KB1 key=KB* number=1 points=4 vertices=4 closed=no layer=KERB "
                  "entity=6"}));
    EXPECT_EQ(verticesOf(*document.model().entities.find(6)),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(20, 0), Point2(30, 0)}));
}

// The fields of a string record that a line of plain shots cannot tell apart:
// how many points against how many vertices, whether it closed, whether it is
// a join.
TEST(LineworkVerb, AStringRecordSaysItsPointsItsVerticesWhetherItClosedAndWhetherItIsAJoin)
{
    Document document;
    document.setSurveyMap(kerbMap());
    //   KB1: 1 (0,0)   2 (10,0)   3 (10,5) "RECT"   three points, the rectangle on the
    //        side 1-2 as wide as 3 stands off it: 4 vertices, the last (0,5)
    //   KB2: 4 (0,20)  5 (10,20)  6 (10,30) "CL"    a closed triangle
    //   KB3: 7 (0,40)  8 (10,40) "JPN 1"            a run of two, and a join to point 1
    addPoints(document, {{"1", 0, 0, "KB1"},
                         {"2", 10, 0, "KB1"},
                         {"3", 10, 5, "KB1 RECT"},
                         {"4", 0, 20, "KB2"},
                         {"5", 10, 20, "KB2"},
                         {"6", 10, 30, "KB2 CL"},
                         {"7", 0, 40, "KB3"},
                         {"8", 10, 40, "KB3 JPN 1"}});

    // By name, and of one name its runs before its joins: entities 9 to 12.
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=8 considered=8 lines=4 unplaced=0 notes=0",
                  "string name=KB1 key=KB* number=1 points=3 vertices=4 closed=yes layer=KERB "
                  "entity=9",
                  "string name=KB2 key=KB* number=2 points=3 vertices=3 closed=yes layer=KERB "
                  "entity=10",
                  "string name=KB3 key=KB* number=3 points=2 vertices=2 closed=no layer=KERB "
                  "entity=11",
                  "string name=KB3 key=KB* number=3 points=2 vertices=2 closed=no layer=KERB "
                  "join=yes entity=12"}));

    const auto shapeOf = [&document](EntityId id) -> const Polyline2& {
        return std::get<Polyline2>(document.model().entities.find(id)->geometry);
    };
    ASSERT_EQ(polylinesOf(document).size(), 4U);
    EXPECT_EQ(shapeOf(9).vertices,
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(10, 5), Point2(0, 5)}));
    EXPECT_TRUE(shapeOf(9).closed);
    EXPECT_EQ(shapeOf(10).vertices,
              (std::vector<Point2>{Point2(0, 20), Point2(10, 20), Point2(10, 30)}));
    EXPECT_TRUE(shapeOf(10).closed);
    EXPECT_EQ(shapeOf(11).vertices, (std::vector<Point2>{Point2(0, 40), Point2(10, 40)}));
    EXPECT_FALSE(shapeOf(11).closed);
    // The join runs from the point that asks for it to the point it names.
    EXPECT_EQ(shapeOf(12).vertices, (std::vector<Point2>{Point2(10, 40), Point2(0, 0)}));
    EXPECT_FALSE(shapeOf(12).closed);
}

// Each reason a point is in no line is a record, in the order the reasons are
// declared (linework.hpp, UnplacedReason), whatever order the points come in.
TEST(LineworkVerb, EachReasonAPointIsInNoLineIsARecordInTheOrderTheReasonsAreDeclared)
{
    Document document;
    document.setSurveyMap(kerbMap());
    //   6 and 7, KB3, stand in one place        every point of its string is in one place
    //   5, KB2, is alone                        the only point of its string
    //   a KB1 shot with no number               no point number to order it by
    //   3, MK                                   its code is a point code
    //   2, ZZ, which no rule knows              no rule for its code and no control code
    //   1, with no code                         no code
    addPoints(document, {{"6", 30, 0, "KB3"},
                         {"7", 30, 0, "KB3"},
                         {"5", 20, 0, "KB2"},
                         {"", 9, 9, "KB1"},
                         {"3", 5, 9, "MK"},
                         {"2", 5, 0, "ZZ"},
                         {"1", 0, 0, ""}});

    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=7 considered=7 lines=0 unplaced=7 notes=0",
                  "unplaced reason=\"no code\" points=1",
                  "unplaced reason=\"no rule for its code and no control code\" points=1",
                  "unplaced reason=\"its code is a point code\" points=1",
                  "unplaced reason=\"no point number to order it by\" points=1",
                  "unplaced reason=\"the only point of its string\" points=1",
                  "unplaced reason=\"every point of its string is in one place\" points=2"}));
    EXPECT_TRUE(polylinesOf(document).empty());
    EXPECT_EQ(document.history().undoCount(), 1U) << "the points; nothing was drawn";
}

// ---- points their survey job has strung --------------------------------------------------

TEST(LineworkVerb, PointsTheirSurveyJobHasStrungAreLeftOutAndCounted)
{
    Document document;
    document.setSurveyMap(jointMap());
    importTwoJoints(document, true, true);
    ASSERT_EQ(polylinesOf(document).size(), 2U);
    // Two shots of another string, drawn by hand and no job's: 10 (0,20) and
    // 11 (10,20), entities 8 and 9.
    addPoints(document, {{"10", 0, 20, "KJ77"}, {"11", 10, 20, "KJ77"}});

    // The drawing is the job's four points and two lines and the two loose
    // points: 8 entities. The four are left out; of the other four, two are
    // points, and they are one line, entity 10.
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=8 considered=2 lines=1 unplaced=0 notes=0",
                  "left_out=4 reason=strung-by-their-job",
                  "string name=KJ77 key=KJ* number=77 points=2 vertices=2 closed=no "
                  "layer=JOINTS entity=10"}));
    // The job's lines were not drawn a second time: its two, and the new one.
    EXPECT_EQ(polylinesOf(document).size(), 3U);
    EXPECT_EQ(verticesOf(*document.model().entities.find(10)),
              (std::vector<Point2>{Point2(0, 20), Point2(10, 20)}));
    // And the job still owns exactly what it did.
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    EXPECT_EQ(document.surveyJobs().front().createdEntities,
              (std::vector<EntityId>{2, 3, 4, 5, 6, 7}));
}

TEST(LineworkVerb, AJobImportedWithLineworkOffIsStrungByTheVerb)
{
    Document document;
    document.setSurveyMap(jointMap());
    importTwoJoints(document, true, false); // coded, not strung
    ASSERT_TRUE(polylinesOf(document).empty());

    // Its points carry their string numbers beside the code, so KJ in string
    // 01 and KJ in string 02 are two lines: entities 6 and 7.
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=4 considered=4 lines=2 unplaced=0 notes=0",
                  "string name=KJ01 key=KJ* number=01 points=2 vertices=2 closed=no "
                  "layer=JOINTS entity=6",
                  "string name=KJ02 key=KJ* number=02 points=2 vertices=2 closed=no "
                  "layer=JOINTS entity=7"}));
    EXPECT_EQ(verticesOf(*document.model().entities.find(6)),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    EXPECT_EQ(verticesOf(*document.model().entities.find(7)),
              (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
}

TEST(LineworkVerb, APointWhoseStringTheJobNoLongerHasALineOfIsStrungAgain)
{
    Document document;
    document.setSurveyMap(jointMap());
    importTwoJoints(document, true, true);
    // The line of KJ 01, entity 6, is deleted by hand.
    ASSERT_TRUE(document.execute(cmd::deleteEntities({6})).ok());

    // Points 3 and 4 are still on the job's line KJ 02 and are left out;
    // points 1 and 2 have no line, and are strung: entity 8.
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=5 considered=2 lines=1 unplaced=0 notes=0",
                  "left_out=2 reason=strung-by-their-job",
                  "string name=KJ01 key=KJ* number=01 points=2 vertices=2 closed=no "
                  "layer=JOINTS entity=8"}));
    EXPECT_EQ(verticesOf(*document.model().entities.find(8)),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    EXPECT_EQ(polylinesOf(document).size(), 2U);
}

TEST(LineworkVerb, AJobWhoseOptionsCannotBeReadRefusesTheRunThatTouchesItAndIsNamed)
{
    Document document;
    document.setSurveyMap(jointMap());
    importTwoJoints(document, true, true);
    addPoints(document, {{"10", 0, 20, "KJ77"}, {"11", 10, 20, "KJ77"}});
    // The job's option text, damaged: whether it strung its points is not known.
    SurveyJobAccess::jobs(document).front().importOptions = "not the options text";
    const std::size_t steps = document.history().undoCount();

    CommandInterpreter interpreter(document);
    const auto refused = interpreter.run("LINEWORK");
    ASSERT_FALSE(refused.ok()) << *refused;
    EXPECT_EQ(refused.error().code, ErrorCode::ParseFailure);
    EXPECT_NE(refused.error().context.find(
                  "LINEWORK cannot tell whether survey job job-1 strung its own points"),
              std::string::npos)
        << refused.error().describe();
    EXPECT_EQ(polylinesOf(document).size(), 2U);
    EXPECT_EQ(document.history().undoCount(), steps);

    // A scope that takes nothing of that job does not read it: the loose
    // points are on layer 0, the job's on JOINTS.
    EXPECT_EQ(linesOf(run(document, "LINEWORK LAYERS 0 PREVIEW")).front(),
              "linework scope=layers layers=0 sublayers=yes matched=2 considered=2 lines=1 "
              "unplaced=0 notes=0 preview=yes");
}

// The other way a job's options cannot be read: a later Katana wrote them.
// Its version line is all this one reads of them (survey_job.hpp).
TEST(LineworkVerb, AJobDrawnByANewerKatanaRefusesTheRunAsUnsupportedAndIsNamed)
{
    Document document;
    document.setSurveyMap(jointMap());
    importTwoJoints(document, true, true);
    SurveyJobAccess::jobs(document).front().importOptions =
        "katana-survey-import-options=2\ndraw-linework=true\n";
    const std::size_t steps = document.history().undoCount();

    CommandInterpreter interpreter(document);
    const auto refused = interpreter.run("LINEWORK");
    ASSERT_FALSE(refused.ok()) << *refused;
    EXPECT_EQ(refused.error().code, ErrorCode::Unsupported);
    EXPECT_NE(refused.error().message.find(
                  "survey job job-1 was drawn by a newer Katana (drawing options version 2)"),
              std::string::npos)
        << refused.error().describe();
    EXPECT_NE(refused.error().context.find(
                  "LINEWORK cannot tell whether survey job job-1 strung its own points"),
              std::string::npos)
        << refused.error().describe();
    EXPECT_EQ(polylinesOf(document).size(), 2U);
    EXPECT_EQ(document.history().undoCount(), steps);
}

// left_out= counts what the scope took and the job strung - not the rest of
// that job's points, which nobody asked about.
TEST(LineworkVerb, OnlyThePointsTheScopeTookAreCountedAsLeftToTheirJob)
{
    Document document;
    document.setSurveyMap(jointMap());
    importTwoJoints(document, true, true);

    // Entities 2 and 3 are points 1 and 2, the string KJ 01; the job's other
    // two points are not selected.
    document.selection().set({2, 3});
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=selection matched=2 considered=0 lines=0 unplaced=0 notes=0",
                  "left_out=2 reason=strung-by-their-job"}));
    // One point of each string - entity 3 of KJ 01 and entity 4 of KJ 02 -
    // and the line of KJ 01, entity 6, which is no point.
    document.selection().set({3, 4, 6});
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=selection matched=3 considered=0 lines=0 unplaced=0 notes=0",
                  "left_out=2 reason=strung-by-their-job"}));
    EXPECT_EQ(polylinesOf(document).size(), 2U);
}

// ---- a line the drawing already holds ------------------------------------------------------

TEST(LineworkVerb, ASecondRunDrawsNoLineTheDrawingAlreadyHolds)
{
    Document document;
    document.setSurveyMap(kerbMap());
    twoKerbsAndAMark(document);
    // The first run draws KB1 and KB2, entities 7 and 8.
    ASSERT_EQ(linesOf(run(document, "LINEWORK")).front(),
              "linework scope=drawing matched=6 considered=6 lines=2 unplaced=1 notes=0");
    ASSERT_EQ(document.history().undoCount(), 2U);
    const std::uint64_t revision = document.modelRevision();

    // The second takes the 6 points and the 2 lines, and finds both lines
    // drawn: they run through 3 + 2 points. The mark is still a point code.
    const std::vector<std::string> again = {
        "linework scope=drawing matched=8 considered=6 lines=0 unplaced=1 notes=0",
        "left_out=5 reason=already-drawn lines=2",
        "unplaced reason=\"its code is a point code\" points=1"};
    EXPECT_EQ(linesOf(run(document, "LINEWORK")), again);
    EXPECT_EQ(document.model().entities.size(), 8U);
    EXPECT_EQ(polylinesOf(document).size(), 2U);
    EXPECT_EQ(document.history().undoCount(), 2U) << "a run that draws nothing is no step";
    EXPECT_EQ(document.modelRevision(), revision);

    // A preview answers alike.
    std::vector<std::string> preview = again;
    preview.front() += " preview=yes";
    EXPECT_EQ(linesOf(run(document, "LINEWORK PREVIEW")), preview);
}

TEST(LineworkVerb, AStringThatHasGainedAPointIsAnotherLineAndIsDrawnWhileTheOthersAreNot)
{
    Document document;
    document.setSurveyMap(kerbMap());
    twoKerbsAndAMark(document);
    run(document, "LINEWORK"); // KB1 and KB2: entities 7 and 8
    // A fourth shot of KB1, point 7 at (30,0): entity 9.
    addPoints(document, {{"7", 30, 0, "KB1"}});

    // KB1 through four points is not the line of three that entity 7 is, and
    // is drawn: entity 10. KB2 is as entity 8 has it, and is not.
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=9 considered=7 lines=1 unplaced=1 notes=0",
                  "left_out=2 reason=already-drawn lines=1",
                  "string name=KB1 key=KB* number=1 points=4 vertices=4 closed=no layer=KERB "
                  "entity=10",
                  "unplaced reason=\"its code is a point code\" points=1"}));
    ASSERT_NE(document.model().entities.find(10), nullptr);
    EXPECT_EQ(verticesOf(*document.model().entities.find(10)),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(20, 0), Point2(30, 0)}));
    // The line that was there is nobody's to remove but the person's.
    ASSERT_NE(document.model().entities.find(7), nullptr);
    EXPECT_EQ(verticesOf(*document.model().entities.find(7)),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(20, 0)}));
    EXPECT_EQ(polylinesOf(document).size(), 3U);
}

// What "the drawing already holds it" means: the code, the string number, the
// vertices and whether they close, and the heights - and nothing else. Two
// levelled shots of KB1, point 1 at (0,0) 5 m high and point 2 at (10,0) 6 m
// high, beside one polyline somebody left in the drawing.
TEST(LineworkVerb, OnlyALineOfTheSameCodeStringNumberVerticesAndHeightsIsTheLineAlreadyDrawn)
{
    const std::vector<Point2> through = {Point2(0, 0), Point2(10, 0)};
    const std::vector<std::optional<double>> surveyed = {5.0, 6.0};
    const struct {
        const char* what;
        std::vector<Point2> vertices;
        bool closed;
        const char* code;
        const char* string;
        std::vector<std::optional<double>> heights;
        const char* layer;
        bool theLine;
    } cases[] = {
        {"the very line", through, false, "KB1", "", surveyed, "0", true},
        {"the very line, since moved to another layer", through, false, "KB1", "", surveyed,
         "elsewhere", true},
        {"another code", through, false, "KB2", "", surveyed, "0", false},
        {"a string number beside the code", through, false, "KB1", "1", surveyed, "0", false},
        {"another vertex", {Point2(0, 0), Point2(10, 1)}, false, "KB1", "", surveyed, "0", false},
        {"a vertex more", {Point2(0, 0), Point2(10, 0), Point2(20, 0)}, false, "KB1", "",
         {5.0, 6.0, 7.0}, "0", false},
        {"closed, where the string is open", through, true, "KB1", "", surveyed, "0", false},
        {"another height", through, false, "KB1", "", {5.0, 7.0}, "0", false},
        {"no heights", through, false, "KB1", "", {}, "0", false},
    };
    for (const auto& given : cases) {
        SCOPED_TRACE(given.what);
        Document document;
        document.setSurveyMap(kerbMap());
        run(document, "LAYER NEW elsewhere");
        // The shots are entities 1 and 2, the line that was there entity 3.
        addEntities(document,
                    {levelledPoint("1", 0, 0, "KB1", 5.0), levelledPoint("2", 10, 0, "KB1", 6.0)});
        addEntities(document, {codedLine(given.vertices, given.closed, given.code, given.string,
                                         given.heights, given.layer)});

        const std::vector<std::string> reply = linesOf(run(document, "LINEWORK"));
        if (given.theLine) {
            EXPECT_EQ(reply,
                      (std::vector<std::string>{
                          "linework scope=drawing matched=3 considered=2 lines=0 unplaced=0 notes=0",
                          "left_out=2 reason=already-drawn lines=1"}));
            EXPECT_EQ(document.model().entities.size(), 3U);
            // Nothing is created for a line that is not drawn.
            EXPECT_FALSE(document.model().layers.contains("KERB"));
        } else {
            EXPECT_EQ(reply,
                      (std::vector<std::string>{
                          "linework scope=drawing matched=3 considered=2 lines=1 unplaced=0 notes=0",
                          "string name=KB1 key=KB* number=1 points=2 vertices=2 closed=no "
                          "layer=KERB entity=4"}));
            const Entity* line = document.model().entities.find(4);
            ASSERT_NE(line, nullptr);
            EXPECT_EQ(verticesOf(*line), through);
            EXPECT_EQ(katana::entity::heightsOf(line->properties, 2), surveyed);
        }
    }
}

// What the reviewer of this verb found: a job's line erased by hand, its
// points strung again by the verb - and then again by every run after.
TEST(LineworkVerb, ALineTheVerbDrewWhereAJobsWasErasedIsNotDrawnASecondTime)
{
    Document document;
    document.setSurveyMap(jointMap());
    importTwoJoints(document, true, true);
    ASSERT_TRUE(document.execute(cmd::deleteEntities({6})).ok()); // the line of KJ 01
    run(document, "LINEWORK"); // strings points 1 and 2 again: entity 8
    ASSERT_EQ(polylinesOf(document).size(), 2U);

    // The four points, the job's line 7 and the verb's line 8. Points 3 and 4
    // are their job's; points 1 and 2 are on a line no job's record names,
    // and the drawing holds it.
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=6 considered=2 lines=0 unplaced=0 notes=0",
                  "left_out=2 reason=strung-by-their-job",
                  "left_out=2 reason=already-drawn lines=1"}));
    EXPECT_EQ(polylinesOf(document).size(), 2U);
}

// The rule is the verb's to ask for: processLinework itself draws again, as
// it always did and as an import's own run must (two imports of one file
// each draw, and own, their lines).
TEST(LineworkAlreadyDrawn, OnlyARunToldToLeavesOutTheLinesTheDrawingHolds)
{
    Document document;
    document.setSurveyMap(kerbMap());
    twoKerbsAndAMark(document);
    LineworkOptions options; // every point of the drawing, the defaults
    auto first = processLinework(document, options);
    ASSERT_TRUE(first.ok()) << first.error().describe();
    ASSERT_NE(first->command, nullptr);
    ASSERT_TRUE(document.execute(std::move(first->command)).ok());
    ASSERT_EQ(polylinesOf(document).size(), 2U); // KB1 and KB2: entities 7 and 8

    const auto again = processLinework(document, options);
    ASSERT_TRUE(again.ok()) << again.error().describe();
    EXPECT_NE(again->command, nullptr);
    EXPECT_EQ(again->report.strings.size(), 2U);
    EXPECT_TRUE(again->report.alreadyDrawn.empty());

    options.skipLinesAlreadyDrawn = true;
    const auto told = processLinework(document, options);
    ASSERT_TRUE(told.ok()) << told.error().describe();
    EXPECT_EQ(told->command, nullptr) << "nothing to build";
    EXPECT_TRUE(told->report.strings.empty());
    ASSERT_EQ(told->report.alreadyDrawn.size(), 2U);
    EXPECT_EQ(told->report.alreadyDrawn[0].name, "KB1");
    EXPECT_EQ(told->report.alreadyDrawn[0].points, (std::vector<EntityId>{1, 2, 3}));
    EXPECT_EQ(told->report.alreadyDrawn[0].vertices, 3U);
    EXPECT_EQ(told->report.alreadyDrawn[0].layer, "KERB");
    EXPECT_EQ(told->report.alreadyDrawn[1].name, "KB2");
    EXPECT_EQ(told->report.alreadyDrawn[1].points, (std::vector<EntityId>{4, 5}));
    // Their points are in a line: only the mark is in none.
    ASSERT_EQ(told->report.unplaced.size(), 1U);
    EXPECT_EQ(told->report.unplaced.front().id, 6U);

    // And no point is removed on account of a line this run did not draw.
    options.keepPoints = false;
    const auto replacing = processLinework(document, options);
    ASSERT_TRUE(replacing.ok()) << replacing.error().describe();
    EXPECT_EQ(replacing->command, nullptr);
    EXPECT_EQ(replacing->report.pointsRemoved, 0U);
}

// ---- nothing to go on --------------------------------------------------------------------

TEST(LineworkVerb, WithNoSurveyCodesLoadedOnlyAControlCodeMakesALine)
{
    Document document; // no survey map
    //   XX: 1 (0,0) "ST"  2 (10,0) "END";  YY: 3 (5,5)
    addPoints(document, {{"1", 0, 0, "XX ST"}, {"2", 10, 0, "XX END"}, {"3", 5, 5, "YY"}});

    // Asked for by name, a control code still draws: XX, on its points'
    // layer, with nothing to style it - which the note says. YY has neither
    // a rule nor a control code.
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=3 considered=3 lines=1 unplaced=1 notes=1",
                  "string name=XX key=\"\" number=\"\" points=2 vertices=2 closed=no layer=0 "
                  "entity=4",
                  "unplaced reason=\"no rule for its code and no control code\" points=1",
                  "note kind=\"no rule for its code: nothing styles the line\" count=1"}));
    const Entity* line = document.model().entities.find(4);
    ASSERT_NE(line, nullptr);
    EXPECT_EQ(verticesOf(*line), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    EXPECT_EQ(line->style, "");
}

// What HELP LINEWORK says of a code no rule makes a line: a control code
// joins the points that carry one, and a shot of the same code between them
// that carries none is no part of the line.
//   KB: 1 (0,0) "ST"   2 (10,0)   3 (10,10) "CL"
TEST(LineworkVerb, WithNoRuleForACodeOnlyItsPointsThatCarryAControlCodeAreJoined)
{
    const std::vector<Shot> shots = {
        {"1", 0, 0, "KB ST"}, {"2", 10, 0, "KB"}, {"3", 10, 10, "KB CL"}};

    // No rule: points 1 and 3 are the string, which is then two points and
    // cannot close; point 2 is in no line.
    Document document;
    addPoints(document, shots);
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=3 considered=3 lines=1 unplaced=1 notes=2",
                  "string name=KB key=\"\" number=\"\" points=2 vertices=2 closed=no layer=0 "
                  "entity=4",
                  "unplaced reason=\"no rule for its code and no control code\" points=1",
                  "note kind=\"closing a string of two points, drawn open\" count=1",
                  "note kind=\"no rule for its code: nothing styles the line\" count=1"}));
    ASSERT_NE(document.model().entities.find(4), nullptr);
    EXPECT_EQ(verticesOf(*document.model().entities.find(4)),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 10)}));

    // With a rule that makes KB a line all three are its points, and it
    // closes on the third.
    Document ruled;
    ruled.setSurveyMap(kerbMap());
    addPoints(ruled, shots);
    EXPECT_EQ(linesOf(run(ruled, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=3 considered=3 lines=1 unplaced=0 notes=0",
                  "string name=KB key=KB* number=\"\" points=3 vertices=3 closed=yes layer=KERB "
                  "entity=4"}));
    ASSERT_NE(ruled.model().entities.find(4), nullptr);
    EXPECT_EQ(verticesOf(*ruled.model().entities.find(4)),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(10, 10)}));
}

TEST(LineworkVerb, PointsWithNoCodeAndADrawingWithNoPointsAreAnsweredWithZerosAndNoUndoStep)
{
    Document empty;
    EXPECT_EQ(run(empty, "LINEWORK"),
              "linework scope=drawing matched=0 considered=0 lines=0 unplaced=0 notes=0");
    EXPECT_EQ(empty.history().undoCount(), 0U);

    Document document;
    document.setSurveyMap(kerbMap());
    addPoints(document, {{"1", 0, 0, ""}, {"2", 10, 0, ""}, {"3", 20, 0, ""}});
    EXPECT_EQ(linesOf(run(document, "LINEWORK")),
              (std::vector<std::string>{
                  "linework scope=drawing matched=3 considered=3 lines=0 unplaced=3 notes=0",
                  "unplaced reason=\"no code\" points=3"}));
    EXPECT_EQ(document.history().undoCount(), 1U) << "the points; a run that draws nothing is no step";

    // A scope that takes entities none of which is a point: looked at, none
    // considered.
    Document lines;
    run(lines, "LINE 0,0 10,0");
    EXPECT_EQ(run(lines, "LINEWORK"),
              "linework scope=drawing matched=1 considered=0 lines=0 unplaced=0 notes=0");
}

TEST(LineworkVerb, AReplyListsFiftyLinesAndCountsTheRest)
{
    Document document;
    document.setSurveyMap(kerbMap());
    // 52 kerbs, KB1 to KB52, two shots each.
    std::vector<Shot> shots;
    for (int kerb = 1; kerb <= 52; ++kerb) {
        const std::string code = "KB" + std::to_string(kerb);
        shots.push_back({std::to_string(2 * kerb - 1), 0.0, static_cast<double>(kerb), code});
        shots.push_back({std::to_string(2 * kerb), 10.0, static_cast<double>(kerb), code});
    }
    addPoints(document, shots);

    const std::vector<std::string> reply = linesOf(run(document, "LINEWORK PREVIEW"));
    ASSERT_EQ(kLineworkStringsListed, 50U);
    ASSERT_EQ(reply.size(), 52U) << "the first record, 50 lines and the count of the rest";
    EXPECT_EQ(reply.front(), "linework scope=drawing matched=104 considered=104 lines=52 "
                             "unplaced=0 notes=0 preview=yes");
    for (std::size_t i = 1; i <= 50; ++i) {
        EXPECT_EQ(reply[i].rfind("string name=KB", 0), 0U) << reply[i];
    }
    EXPECT_EQ(reply.back(), "strings_more=2");
}

// ---- help --------------------------------------------------------------------------------

TEST(LineworkVerb, TheHelpNamesTheVerbAndHelpLineworkIsItsOwnText)
{
    const std::string help = CommandInterpreter::helpText();
    EXPECT_NE(help.find("LINEWORK [scope] [WHERE k=v ...] [ORDER number|entity] [PREVIEW]"),
              std::string::npos);
    EXPECT_NE(help.find("HELP LINEWORK"), std::string::npos);

    Document document;
    const std::string own = run(document, "HELP LINEWORK");
    EXPECT_EQ(own, lineworkVerbHelp());
    for (const char* words : {"LINEWORK [<scope>] [WHERE k=v ...] [ORDER number|entity] [PREVIEW]",
                              "left_out=<n> reason=strung-by-their-job", "strings_more=<n>",
                              "ONE undo step"}) {
        EXPECT_NE(own.find(words), std::string::npos) << words;
    }
    // A reply, the help included, never holds the word a script's check for
    // a failed line looks for.
    EXPECT_EQ(own.find("error"), std::string::npos);
    EXPECT_TRUE(isLineworkVerb("linework"));
    EXPECT_FALSE(isLineworkVerb("LINE"));
}

// The help says what the verb does and no more: the record of a line already
// drawn, that a control code alone joins only the points that carry one, and
// that a job imported with linework off is strung by code and not as its
// file strung it (each is a test above, or in tests/app/test_survey_verbs.cpp).
TEST(LineworkVerb, TheHelpSaysWhatIsLeftOutAndWhatStringingByCodeDoesNotDo)
{
    const std::string help = lineworkVerbHelp();
    for (const char* words :
         {"left_out=<n> reason=already-drawn lines=<m>", "a line the drawing already holds",
          "is strung only through the points that carry a", "strings its points BY THEIR CODES",
          "comes out as one open line", "[join=yes]"}) {
        EXPECT_NE(help.find(words), std::string::npos) << words;
    }
    // And the summary in HELP names the second thing left out.
    EXPECT_NE(CommandInterpreter::helpText().find("and lines already drawn, are left out"),
              std::string::npos);
}

// ---- what a survey import does beyond its points -----------------------------------------

TEST(SurveyImportFinishChoice, WithNoSurveyCodesLoadedNothingIsHandedOnWhateverWasAsked)
{
    const Document document; // the default: both switches on, no survey codes
    const SurveyImportFinish unsaid = surveyImportFinish(document);
    EXPECT_TRUE(unsaid.codesAsked);
    EXPECT_TRUE(unsaid.lineworkAsked);
    EXPECT_FALSE(unsaid.surveyCodesLoaded);
    EXPECT_FALSE(unsaid.options.codes);
    EXPECT_FALSE(unsaid.options.linework);
    EXPECT_FALSE(unsaid.options.any()) << "the import is the plain one";

    const SurveyImportFinish said = surveyImportFinish(document, true, true);
    EXPECT_TRUE(said.codesAsked);
    EXPECT_FALSE(said.options.any());
}

TEST(SurveyImportFinishChoice, TheLineSaysWhetherAndWhereItSaysNothingTheDocumentsSwitchDoes)
{
    Document document;
    document.setSurveyMap(kerbMap());

    const SurveyImportFinish unsaid = surveyImportFinish(document);
    EXPECT_TRUE(unsaid.surveyCodesLoaded);
    EXPECT_TRUE(unsaid.options.codes);
    EXPECT_TRUE(unsaid.options.linework);

    // Said on the line, each on its own.
    const SurveyImportFinish noCodes = surveyImportFinish(document, false, std::nullopt);
    EXPECT_FALSE(noCodes.codesAsked);
    EXPECT_FALSE(noCodes.options.codes);
    EXPECT_TRUE(noCodes.options.linework);
    const SurveyImportFinish noLines = surveyImportFinish(document, std::nullopt, false);
    EXPECT_TRUE(noLines.options.codes);
    EXPECT_FALSE(noLines.lineworkAsked);
    EXPECT_FALSE(noLines.options.linework);

    // The customisation's switches are what an unsaid word takes ...
    document.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = false});
    const SurveyImportFinish off = surveyImportFinish(document);
    EXPECT_FALSE(off.codesAsked);
    EXPECT_FALSE(off.lineworkAsked);
    EXPECT_FALSE(off.options.any());
    // ... and a said one overrides.
    const SurveyImportFinish on = surveyImportFinish(document, true, std::nullopt);
    EXPECT_TRUE(on.options.codes);
    EXPECT_FALSE(on.options.linework);
}

// Each word against ITS OWN switch, both ways: LINEWORK on over a linework
// switch that is off, and each word off over a switch that is on.
TEST(SurveyImportFinishChoice, AWordSaidOverridesItsOwnSwitchWhicheverWayEitherStands)
{
    Document document;
    document.setSurveyMap(kerbMap());
    document.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = false});

    const SurveyImportFinish lines = surveyImportFinish(document, std::nullopt, true);
    EXPECT_FALSE(lines.codesAsked);
    EXPECT_TRUE(lines.lineworkAsked);
    EXPECT_FALSE(lines.options.codes);
    EXPECT_TRUE(lines.options.linework);
    const SurveyImportFinish both = surveyImportFinish(document, true, true);
    EXPECT_TRUE(both.options.codes);
    EXPECT_TRUE(both.options.linework);

    document.setAutomation({.codesOnSurveyImport = true, .lineworkOnSurveyImport = true});
    const SurveyImportFinish noLines = surveyImportFinish(document, std::nullopt, false);
    EXPECT_TRUE(noLines.codesAsked);
    EXPECT_FALSE(noLines.lineworkAsked);
    EXPECT_TRUE(noLines.options.codes);
    EXPECT_FALSE(noLines.options.linework);
    const SurveyImportFinish neither = surveyImportFinish(document, false, false);
    EXPECT_FALSE(neither.codesAsked);
    EXPECT_FALSE(neither.lineworkAsked);
    EXPECT_FALSE(neither.options.any());
}

TEST(SurveyImportFinishChoice, TheColoursTheControlCodesAndTheOrderAreTheDocuments)
{
    Document document;
    document.setSurveyMap(kerbMap());
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("kerb red", Color{170, 0, 0, 255}).ok());
    document.setColourTable(colours);
    LineworkCodes codes;
    codes.start = "S";
    codes.close = "SHUT";
    ASSERT_TRUE(document.setLineworkCodes(codes).ok());

    const SurveyImportFinish finish = surveyImportFinish(document);
    EXPECT_EQ(finish.options.controls, codes);
    EXPECT_EQ(finish.options.order, LineworkOrder::PointNumber);
    ASSERT_TRUE(static_cast<bool>(finish.options.coding.colourOf));
    // The customisation's own name, then a standard one, then no name at all.
    const auto own = finish.options.coding.colourOf("kerb red");
    ASSERT_TRUE(own.has_value());
    EXPECT_EQ(*own, (Color{170, 0, 0, 255}));
    const auto standard = finish.options.coding.colourOf("blue");
    ASSERT_TRUE(standard.has_value());
    EXPECT_EQ(*standard, (Color{0, 0, 255, 255}));
    EXPECT_FALSE(finish.options.coding.colourOf("no such colour").has_value());
}
