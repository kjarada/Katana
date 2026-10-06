// The survey finish (cad/survey_finish.hpp): an import that also codes what
// it drew and strings it, as ONE undo step - and the string names it is built
// on (a code followed by its string number).
//
// Every expected value here is worked by hand from the small project the test
// builds; the working is in the comment beside it. A survey point is
// (northing, easting) and a drawing point (x, y) = (easting, northing).

#include <gtest/gtest.h>

#include <map>
#include <memory>
#include <optional>
#include <ostream>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/linework.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/cad/survey_finish.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/commands/entity_commands.hpp"

namespace cmd = katana::commands;
using katana::cad::Document;
using katana::cad::ExistingPointPolicy;
using katana::cad::LineworkOptions;
using katana::cad::SurveyFeatureOptions;
using katana::cad::SurveyFinishOptions;
using katana::cad::SurveyFinishReport;
using katana::cad::SurveyFinishSkip;
using katana::cad::SurveyImportOptions;
using katana::cad::UnplacedFeatureReason;
using katana::cad::UnplacedReason;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyRule;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::survey::SurveyFeature;
using katana::survey::SurveyPoint;
using katana::survey::SurveyProject;

namespace {

void add(katana::entity::SurveyMap& map, const SurveyRule& rule)
{
    const auto added = map.add(rule);
    EXPECT_TRUE(added.ok()) << (added.ok() ? "" : added.error().describe());
}

SurveyRule lineRule(const std::string& key, const std::string& model,
                    const std::string& linestyle = {})
{
    SurveyRule rule;
    rule.key = key;
    rule.model = model;
    rule.linestyle = linestyle;
    rule.breakline = SurveyBreakline::Line;
    return rule;
}

// Every code gets the attribute Surveyed = yes: the kind of bare "*" rule a
// code file carries in its attribute sections. It says nothing about lines.
SurveyRule attributeForEveryCode()
{
    SurveyRule everything;
    everything.key = "*";
    everything.section = katana::entity::SurveySection::StringAttribute;
    everything.attributes = {katana::entity::SurveyAttribute{"text", "Surveyed", "yes"}};
    return everything;
}

// Shaped like a real code table, with invented names:
//   KJ*  a joint: a LINE on SURVEY JOINT, drawn with "Joint Line";
//   PS*  a survey mark: a POINT on SURVEY MARKS with the symbol "Peg", written
//        as a code file writes it - one rule for where it goes, one for its
//        symbol;
//   B1*  strings 10 to 19 of the one-letter code B: a LINE on SURVEY BARRIER;
//   *    the attribute rule above.
katana::entity::SurveyMap fieldMap()
{
    katana::entity::SurveyMap map;
    add(map, lineRule("KJ*", "SURVEY JOINT", "Joint Line"));

    SurveyRule mark;
    mark.key = "PS*";
    mark.model = "SURVEY MARKS";
    mark.breakline = SurveyBreakline::Point;
    add(map, mark);
    SurveyRule markSymbol;
    markSymbol.key = "PS*";
    markSymbol.section = katana::entity::SurveySection::VertexSymbol;
    katana::entity::SurveySymbol peg;
    peg.style = "Peg";
    peg.size = 0.5;
    markSymbol.symbol = peg;
    add(map, markSymbol);

    add(map, lineRule("B1*", "SURVEY BARRIER", "Barrier"));
    add(map, attributeForEveryCode());
    return map;
}

SurveyPoint point(const std::string& id, double northing, double easting, const std::string& code)
{
    SurveyPoint p;
    p.id = id;
    p.northing = northing;
    p.easting = easting;
    p.code = code;
    return p;
}

// A feature as the field-file readers' shared builder leaves one: the code,
// and the STRING NUMBER as its name.
SurveyFeature feature(const std::string& code, const std::string& number,
                      std::vector<std::string> ids)
{
    SurveyFeature f;
    f.code = code;
    f.name = number;
    f.pointIds = std::move(ids);
    return f;
}

// A field file whose records keep code and string number apart: every point
// has the code alone, and a feature per (code, number) strings its points in
// the order they were shot.
SurveyProject fieldFile()
{
    SurveyProject project;
    project.points = {
        point("1", 0.0, 0.0, "KJ"),  point("2", 0.0, 10.0, "KJ"),   // KJ, string 01
        point("3", 5.0, 0.0, "KJ"),  point("4", 5.0, 10.0, "KJ"),   // KJ, string 02
        point("5", 20.0, 0.0, "PS"), point("6", 20.0, 10.0, "PS"),  // two survey marks
        point("7", 30.0, 0.0, "B"),  point("8", 30.0, 10.0, "B"),   // B, string 12
        point("9", 40.0, 0.0, "ZZ"), point("10", 40.0, 10.0, "ZZ"), // a code with no rule
    };
    project.features = {feature("KJ", "01", {"1", "2"}), feature("KJ", "02", {"3", "4"}),
                        feature("PS", "01", {"5", "6"}), feature("B", "12", {"7", "8"}),
                        feature("ZZ", "1", {"9", "10"})};
    return project;
}

SurveyFinishOptions codesAndLinework()
{
    SurveyFinishOptions finish;
    finish.codes = true;
    finish.linework = true;
    return finish;
}

// An import onto a TOP-LEVEL layer, for the tests that compare the whole
// drawing after an undo. Creating the default "survey/points" also creates
// its parent "survey", and the undo of any import - with a finish or without
// - leaves that parent behind: the layer command's undo removes only the
// layer it was given. Those tests are about what the FINISH leaves.
SurveyImportOptions ontoATopLevelLayer()
{
    SurveyImportOptions options;
    options.layer = "field";
    return options;
}

// Everything a drawing holds that an import can change, by value.
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

// The import of `project` as a job makes it - its string numbers named onto
// the points when it is to be finished - wrapped in the finish and executed
// as one command. The report is what the finish filled.
std::shared_ptr<SurveyFinishReport>
importAndFinish(Document& document, SurveyProject project, const SurveyFinishOptions& finish,
                const SurveyImportOptions& options = {},
                ExistingPointPolicy policy = ExistingPointPolicy::Refuse)
{
    if (finish.any()) {
        katana::cad::nameSurveyStrings(project.points, project.features);
    }
    // What a job draws: the points, and nothing that refers to them (a
    // feature naming a point the policy skips would make the project of the
    // points that are left an inconsistent one).
    SurveyProject drawable;
    drawable.points = project.points;
    auto points = katana::cad::importSurveyPoints(document, drawable, options, policy);
    EXPECT_TRUE(points.ok()) << (points.ok() ? "" : points.error().describe());
    auto report = std::make_shared<SurveyFinishReport>();
    if (!points.ok()) {
        return report;
    }
    SurveyProject strings;
    strings.points = project.points;
    strings.features = project.features;
    auto command = katana::cad::withSurveyFinish(document, std::move(*points), std::move(strings),
                                                 options, finish, report);
    EXPECT_NE(command, nullptr);
    if (command != nullptr) {
        const auto status = document.execute(std::move(command));
        EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    }
    return report;
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

// The lines in the drawing, in the order they were created.
std::vector<const Entity*> linesOf(const Document& document)
{
    std::map<EntityId, const Entity*> byId;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<Polyline2>(entity.geometry)) {
            byId.emplace(entity.id, &entity);
        }
    });
    std::vector<const Entity*> lines;
    for (const auto& [id, entity] : byId) {
        lines.push_back(entity);
    }
    return lines;
}

std::size_t pointCount(const Document& document)
{
    std::size_t count = 0;
    document.model().entities.forEach([&](const Entity& entity) {
        count += std::holds_alternative<katana::entity::PointGeometry>(entity.geometry) ? 1 : 0;
    });
    return count;
}

const std::vector<Point2>& verticesOf(const Entity& line)
{
    return std::get<Polyline2>(line.geometry).vertices;
}

// A point as the survey import writes one, for the tests of the two
// functions on their own.
EntityId addPoint(Document& document, const Point2& at, const std::string& code,
                  const std::string& number, const std::string& string = {})
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{at};
    entity.properties.insert_or_assign("code", katana::entity::PropertyValue(code));
    entity.properties.insert_or_assign("point", katana::entity::PropertyValue(number));
    if (!string.empty()) {
        entity.properties.insert_or_assign(std::string(katana::cad::kSurveyStringProperty),
                                           katana::entity::PropertyValue(string));
    }
    EXPECT_TRUE(document.execute(cmd::createEntities({entity})).ok());
    return document.lastCreatedEntities().front();
}

} // namespace

// ---- string names ---------------------------------------------------------------------

TEST(SurveyFinishStringNames, ACodeIsLookedUpWithItsStringNumberAndFallsBackToTheCodeAlone)
{
    katana::entity::SurveyMap map = fieldMap();
    SurveyRule pit;
    pit.key = "PABB"; // exact: one string per code
    pit.model = "SURVEY PITS";
    add(map, pit);

    // "B" + "12" is "B12", which "B1*" answers; "B" alone has no rule.
    EXPECT_EQ(katana::cad::surveyLookupName(map, "B", "12"), "B12");
    // "KJ" + "01" and "KJ" + "02" are two names, both answered by "KJ*".
    EXPECT_EQ(katana::cad::surveyLookupName(map, "KJ", "01"), "KJ01");
    EXPECT_EQ(katana::cad::surveyLookupName(map, "KJ", "02"), "KJ02");
    // "PABB3" is not the exact key "PABB", and nothing but "*" answers it:
    // the code alone, which the exact key does answer.
    EXPECT_EQ(katana::cad::surveyLookupName(map, "PABB", "3"), "PABB");
    // "B7": "B1*" does not answer it and nothing answers "B", so it is
    // reported as the code it is, once, not once per string.
    EXPECT_EQ(katana::cad::surveyLookupName(map, "B", "7"), "B");
    // No string number: the code, as it always was. No code: no name.
    EXPECT_EQ(katana::cad::surveyLookupName(map, "KJ01", ""), "KJ01");
    EXPECT_EQ(katana::cad::surveyLookupName(map, "", "12"), "");
}

// "Numbering a string never loses a rule the code had" where another rule
// DOES answer the numbered name: a rule for a whole family of codes ("PA*")
// answers "PABB3", and answers PABB itself less well than PABB's own exact
// rule does.
TEST(SurveyFinishStringNames, ANumberedStringKeepsItsCodesExactRuleOverABroaderRuleForTheName)
{
    katana::entity::SurveyMap map;
    SurveyRule pit;
    pit.key = "PABB"; // exact: this code, one string
    pit.model = "SURVEY PITS";
    add(map, pit);
    SurveyRule family;
    family.key = "PA*"; // every code that begins PA
    family.model = "SURVEY PAVEMENT";
    add(map, family);

    // The family rule is all that answers "PABB3", and it is broader than the
    // code: PABB's exact rule is the more specific, so the code is looked up.
    EXPECT_EQ(katana::cad::surveyLookupName(map, "PABB", "3"), "PABB");
    // A code of the family with no exact rule keeps its name, as before.
    EXPECT_EQ(katana::cad::surveyLookupName(map, "PAVE", "3"), "PAVE3");

    // And so the point is coded by its own rule, on its own layer.
    Document document;
    document.setSurveyMap(map);
    const EntityId numbered = addPoint(document, Point2(0.0, 0.0), "PABB", "1", "3");
    auto planned = katana::cad::applySurveyCodes(document, {});
    ASSERT_TRUE(planned.ok()) << planned.error().describe();
    ASSERT_NE(*planned, nullptr);
    ASSERT_TRUE(document.execute(std::move(*planned)).ok());
    EXPECT_EQ(document.model().entities.find(numbered)->layer, "SURVEY PITS");

    // A prefix key that is the code itself, or reaches into the number, WAS
    // written for the numbered names of that code, beside the exact key for
    // the unnumbered one: there the name is looked up.
    SurveyRule strings;
    strings.key = "PABB*";
    strings.model = "SURVEY PIT STRINGS";
    add(map, strings);
    EXPECT_EQ(katana::cad::surveyLookupName(map, "PABB", "3"), "PABB3");
    EXPECT_EQ(katana::cad::surveyLookupName(map, "PABB", ""), "PABB");
}

TEST(SurveyFinishStringNames, APointIsCodedByItsCodeFollowedByItsStringNumber)
{
    Document document;
    document.setSurveyMap(fieldMap());
    const EntityId barrier = addPoint(document, Point2(0.0, 0.0), "B", "1", "12");
    const EntityId other = addPoint(document, Point2(5.0, 0.0), "B", "2", "7");
    const EntityId whole = addPoint(document, Point2(9.0, 0.0), "B12 ST", "3");

    katana::cad::SurveyCodingReport report;
    auto planned = katana::cad::applySurveyCodes(document, {}, &report);
    ASSERT_TRUE(planned.ok()) << planned.error().describe();
    ASSERT_NE(*planned, nullptr);
    ASSERT_TRUE(document.execute(std::move(*planned)).ok());

    // B in string 12 and "B12 ST" written whole are both B12 -> "B1*".
    EXPECT_EQ(document.model().entities.find(barrier)->layer, "SURVEY BARRIER");
    EXPECT_EQ(document.model().entities.find(barrier)->style, "Barrier");
    EXPECT_EQ(document.model().entities.find(whole)->layer, "SURVEY BARRIER");
    // B in string 7 is "B7": no rule, so it stays where it was.
    EXPECT_EQ(document.model().entities.find(other)->layer, "0");
    // Reported under the name looked up: B12 (two points), and B (one).
    ASSERT_EQ(report.codes.size(), 2u);
    EXPECT_EQ(report.codes[0].code, "B");
    EXPECT_EQ(report.codes[0].entities, 1u);
    EXPECT_FALSE(report.codes[0].matched);
    EXPECT_EQ(report.codes[1].code, "B12");
    EXPECT_EQ(report.codes[1].entities, 2u);
    EXPECT_TRUE(report.codes[1].matched);
}

TEST(SurveyFinishStringNames, PointsOfOneCodeInTwoStringsAreStrungAsTwoLinesThatCarryCodeAndNumber)
{
    Document document;
    document.setSurveyMap(fieldMap());
    // KJ string 01: points 1 and 2, and point 5 written "KJ01" whole.
    // KJ string 02: points 3 and 4.
    addPoint(document, Point2(0.0, 0.0), "KJ", "1", "01");
    addPoint(document, Point2(10.0, 0.0), "KJ", "2", "01");
    addPoint(document, Point2(0.0, 5.0), "KJ", "3", "02");
    addPoint(document, Point2(10.0, 5.0), "KJ", "4", "02");
    addPoint(document, Point2(20.0, 0.0), "KJ01", "5");

    auto planned = katana::cad::processLinework(document, LineworkOptions{});
    ASSERT_TRUE(planned.ok()) << planned.error().describe();
    ASSERT_NE(planned->command, nullptr);
    ASSERT_TRUE(document.execute(std::move(planned->command)).ok());

    // By point number: string KJ01 is 1, 2, 5 and string KJ02 is 3, 4.
    ASSERT_EQ(planned->report.strings.size(), 2u);
    EXPECT_EQ(planned->report.strings[0].name, "KJ01");
    EXPECT_EQ(planned->report.strings[0].key, "KJ*");
    EXPECT_EQ(planned->report.strings[0].number, "01");
    EXPECT_EQ(planned->report.strings[0].pointNumbers,
              (std::vector<std::string>{"1", "2", "5"}));
    EXPECT_EQ(planned->report.strings[1].name, "KJ02");
    EXPECT_EQ(planned->report.strings[1].number, "02");
    EXPECT_EQ(planned->report.strings[1].pointNumbers, (std::vector<std::string>{"3", "4"}));

    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(verticesOf(*lines[0]),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(20, 0)}));
    EXPECT_EQ(verticesOf(*lines[1]), (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    // Each carries what its first point carries, so the rule its points
    // found - "KJ*", by the name KJ01 - is the rule that styles it.
    EXPECT_EQ(textProperty(*lines[0], "code"), "KJ");
    EXPECT_EQ(textProperty(*lines[0], "string"), "01");
    EXPECT_EQ(textProperty(*lines[1], "string"), "02");
    EXPECT_EQ(lines[0]->layer, "SURVEY JOINT");
    EXPECT_EQ(lines[0]->style, "Joint Line");
    EXPECT_EQ(lines[1]->style, "Joint Line");
}

TEST(SurveyFinishStringNames, NamingStringsGivesAPointTheNumberOfTheFirstNamedFeatureOfItsOwnCode)
{
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KJ"), point("2", 0.0, 1.0, "KJ ST"),
                      point("3", 0.0, 2.0, "PS"), point("4", 0.0, 3.0, "KJ")};
    project.points[3].metadata["string"] = "77"; // the file's own column
    project.features = {
        feature("KB", "32", {"1"}),      // another code: not point 1's string
        feature("KJ", "01", {"1", "2"}), // the first of its own code
        feature("KJ", "02", {"1", "4"}), // a second: 1 keeps 01, 4 keeps the file's 77
        feature("PS", "", {"3"}),        // no number: nothing to give
    };

    katana::cad::nameSurveyStrings(project.points, project.features);

    EXPECT_EQ(project.points[0].metadata.at("string"), "01");
    // "KJ ST" is coded KJ: the first token is the code.
    EXPECT_EQ(project.points[1].metadata.at("string"), "01");
    EXPECT_EQ(project.points[2].metadata.count("string"), 0u);
    EXPECT_EQ(project.points[3].metadata.at("string"), "77");
}

// ---- features under the rules ----------------------------------------------------------

TEST(SurveyFeaturesRuled, OnlyAFeatureARuleMakesALineIsDrawnAndTheOthersSayWhy)
{
    Document document;
    document.setSurveyMap(fieldMap());
    const SurveyProject project = fieldFile();
    SurveyFeatureOptions options;
    options.onlyRuledLines = true;

    auto planned = katana::cad::drawSurveyFeatures(document, project, options);
    ASSERT_TRUE(planned.ok()) << planned.error().describe();
    ASSERT_NE(planned->command, nullptr);
    ASSERT_TRUE(document.execute(std::move(planned->command)).ok());

    // KJ 01, KJ 02 and B 12 are lines by "KJ*" and "B1*"; reported as code
    // and number, with the number the file gave.
    ASSERT_EQ(planned->report.strings.size(), 3u);
    EXPECT_EQ(planned->report.strings[0].name, "KJ 01");
    EXPECT_EQ(planned->report.strings[0].key, "KJ*");
    EXPECT_EQ(planned->report.strings[0].number, "01");
    EXPECT_EQ(planned->report.strings[1].name, "KJ 02");
    EXPECT_EQ(planned->report.strings[2].name, "B 12");
    EXPECT_EQ(planned->report.strings[2].key, "B1*");
    EXPECT_EQ(planned->report.strings[2].number, "12");
    EXPECT_TRUE(planned->report.notes.empty());

    // PS 01 is a point code by "PS*"; ZZ 1 is known to nothing but "*".
    ASSERT_EQ(planned->unplaced.size(), 2u);
    EXPECT_EQ(planned->unplaced[0].index, 2u);
    EXPECT_EQ(planned->unplaced[0].name, "PS 01");
    EXPECT_EQ(planned->unplaced[0].code, "PS");
    EXPECT_EQ(planned->unplaced[0].reason, UnplacedFeatureReason::PointCode);
    EXPECT_EQ(planned->unplaced[1].index, 4u);
    EXPECT_EQ(planned->unplaced[1].name, "ZZ 1");
    EXPECT_EQ(planned->unplaced[1].reason, UnplacedFeatureReason::NoRule);

    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 3u);
    EXPECT_EQ(textProperty(*lines[0], "code"), "KJ");
    EXPECT_EQ(textProperty(*lines[0], "string"), "01");
    EXPECT_EQ(textProperty(*lines[2], "code"), "B");
    EXPECT_EQ(textProperty(*lines[2], "string"), "12");
    // Styled by the same two: B12 -> "B1*" -> Barrier on SURVEY BARRIER.
    EXPECT_EQ(lines[2]->layer, "SURVEY BARRIER");
    EXPECT_EQ(lines[2]->style, "Barrier");
}

TEST(SurveyFeaturesRuled, WithoutTheOptionEveryFeatureIsStillDrawnAndNamedAsItWas)
{
    Document document;
    document.setSurveyMap(fieldMap());
    const SurveyProject project = fieldFile();

    auto planned = katana::cad::drawSurveyFeatures(document, project, {});
    ASSERT_TRUE(planned.ok()) << planned.error().describe();
    ASSERT_NE(planned->command, nullptr);
    ASSERT_TRUE(document.execute(std::move(planned->command)).ok());

    // All five, marks and the unknown code included, named by the name alone.
    ASSERT_EQ(planned->report.strings.size(), 5u);
    EXPECT_EQ(planned->report.strings[0].name, "01");
    EXPECT_EQ(planned->report.strings[3].name, "12");
    EXPECT_TRUE(planned->unplaced.empty());
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 5u);
    EXPECT_EQ(textProperty(*lines[0], "string"), "<absent>");
}

// ---- the finish ------------------------------------------------------------------------

TEST(SurveyFinish, AFieldFileIsCodedAndStrungByItsStringNames)
{
    Document document;
    document.setSurveyMap(fieldMap());

    const auto report = importAndFinish(document, fieldFile(), codesAndLinework());

    // ---- the points: ten drawn, none removed, each on its rule's layer ----
    EXPECT_EQ(report->points, 10u);
    EXPECT_EQ(pointCount(document), 10u);
    const std::vector<std::pair<std::string, std::string>> stringOfPoint = {
        {"1", "01"}, {"2", "01"}, {"3", "02"}, {"4", "02"}, {"5", "01"},
        {"6", "01"}, {"7", "12"}, {"8", "12"}, {"9", "1"},  {"10", "1"}};
    for (const auto& [number, string] : stringOfPoint) {
        SCOPED_TRACE(number);
        const Entity* entity = pointNumbered(document, number);
        ASSERT_NE(entity, nullptr);
        EXPECT_EQ(textProperty(*entity, "string"), string);
        // The bare "*" rule's attribute reaches every code, as CODE gives it.
        EXPECT_EQ(textProperty(*entity, "Surveyed"), "yes");
    }
    for (const char* number : {"1", "2", "3", "4"}) { // KJ01, KJ02 -> "KJ*"
        EXPECT_EQ(pointNumbered(document, number)->layer, "SURVEY JOINT") << number;
        EXPECT_EQ(pointNumbered(document, number)->style, "Joint Line") << number;
        EXPECT_EQ(textProperty(*pointNumbered(document, number), "code"), "KJ") << number;
    }
    for (const char* number : {"5", "6"}) { // PS01 -> "PS*"
        EXPECT_EQ(pointNumbered(document, number)->layer, "SURVEY MARKS") << number;
        EXPECT_EQ(pointNumbered(document, number)->style, "Peg") << number;
    }
    const katana::entity::Style* peg = document.model().styles.find("Peg");
    ASSERT_NE(peg, nullptr);
    EXPECT_EQ(peg->symbol, "Peg");
    EXPECT_EQ(peg->symbolSize, 0.5);
    for (const char* number : {"7", "8"}) { // B12 -> "B1*"
        EXPECT_EQ(pointNumbered(document, number)->layer, "SURVEY BARRIER") << number;
        EXPECT_EQ(pointNumbered(document, number)->style, "Barrier") << number;
    }
    for (const char* number : {"9", "10"}) { // ZZ: only "*", so it stays put
        EXPECT_EQ(pointNumbered(document, number)->layer, "survey/points") << number;
        EXPECT_EQ(pointNumbered(document, number)->style, "") << number;
    }

    // ---- what the coding says ----
    EXPECT_EQ(report->whyNotCoded, SurveyFinishSkip::None);
    EXPECT_EQ(report->coding.coded, 10u);
    EXPECT_EQ(report->coding.matched, 8u);       // all but the two ZZ
    EXPECT_EQ(report->coding.fallbackOnly, 2u);  // the two ZZ
    EXPECT_EQ(report->codesWithNoRule(), (std::vector<std::string>{"ZZ"}));
    std::vector<std::string> codes;
    for (const auto& row : report->coding.codes) {
        codes.push_back(row.code);
    }
    EXPECT_EQ(codes, (std::vector<std::string>{"B12", "KJ01", "KJ02", "PS01", "ZZ"}));
    EXPECT_EQ(report->coding.layersCreated,
              (std::vector<std::string>{"SURVEY BARRIER", "SURVEY JOINT", "SURVEY MARKS"}));
    EXPECT_EQ(report->coding.stylesCreated,
              (std::vector<std::string>{"Barrier", "Joint Line", "Peg"}));

    // ---- the lines: two KJ lines, not one; the B line; none through the
    //      marks, none through ZZ ----
    EXPECT_EQ(report->whyNotStrung, SurveyFinishSkip::None);
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 3u);
    EXPECT_EQ(report->lines.size(), 3u);
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    EXPECT_EQ(verticesOf(*lines[1]), (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    EXPECT_EQ(verticesOf(*lines[2]), (std::vector<Point2>{Point2(0, 30), Point2(10, 30)}));
    EXPECT_EQ(lines[0]->layer, "SURVEY JOINT");
    EXPECT_EQ(lines[0]->style, "Joint Line"); // the style its points were given
    EXPECT_EQ(lines[1]->layer, "SURVEY JOINT");
    EXPECT_EQ(lines[2]->layer, "SURVEY BARRIER");
    EXPECT_EQ(lines[2]->style, "Barrier");
    EXPECT_EQ(textProperty(*lines[0], "string"), "01");
    EXPECT_EQ(textProperty(*lines[1], "string"), "02");
    ASSERT_EQ(report->features.strings.size(), 3u);
    EXPECT_EQ(report->features.strings[0].name, "KJ 01");
    EXPECT_EQ(report->features.strings[1].name, "KJ 02");
    EXPECT_EQ(report->features.strings[2].name, "B 12");
    ASSERT_EQ(report->unplacedFeatures.size(), 2u);
    EXPECT_EQ(report->unplacedFeatures[0].name, "PS 01");
    EXPECT_EQ(report->unplacedFeatures[0].reason, UnplacedFeatureReason::PointCode);
    EXPECT_EQ(report->unplacedFeatures[1].name, "ZZ 1");
    EXPECT_EQ(report->unplacedFeatures[1].reason, UnplacedFeatureReason::NoRule);
    // Every point is named by a feature, so none is left to be strung by its
    // code: no point is in two lines of its own string.
    EXPECT_EQ(report->pointsInFeatures, 10u);
    EXPECT_EQ(report->strung.considered, 0u);
    EXPECT_TRUE(report->strung.strings.empty());
    EXPECT_EQ(report->unplaced(), 2u);
    // The lines made no layer or style the coding had not: one of each name.
    EXPECT_EQ(report->layersCreated(), report->coding.layersCreated);
    EXPECT_EQ(report->stylesCreated(), report->coding.stylesCreated);

    // ---- the same in words: what was done, and what was left over ----
    const std::string noRule = "No rule for the code(s): ZZ.";
    const std::string notStrung = "2 string(s) of the file are in no line (no rule for its "
                                  "code: 1; its code is a point code: 1).";
    EXPECT_EQ(katana::cad::describe(*report),
              (std::vector<std::string>{
                  "Survey codes were applied to the 10 point(s) drawn: 10 carry a code and 8 "
                  "of those have a rule; 3 layer(s) and 3 style(s) were created.",
                  noRule, "Linework: 3 line(s) drawn.", notStrung}));
    // What a person may have to act on: the code nobody wrote a rule for,
    // and its string. (The marks' string is in the count, and is no fault.)
    EXPECT_EQ(katana::cad::finishWarnings(*report), (std::vector<std::string>{noRule, notStrung}));
}

// A survey mark is a point: a file whose only strings in no line are those of
// point codes has nothing left over.
TEST(SurveyFinish, StringsAndPointsOfPointCodesAreCountedAndAreNoWarning)
{
    Document document;
    document.setSurveyMap(fieldMap());
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KJ"),   point("2", 0.0, 10.0, "KJ"),
                      point("3", 20.0, 0.0, "PS"),  point("4", 20.0, 10.0, "PS"),
                      point("5", 30.0, 0.0, "PS7"), point("6", 40.0, 0.0, "")};
    // Marks 3 and 4 are string 01 of PS; mark 5 is coded PS7 whole and point 6
    // has no code, and no feature names either.
    project.features = {feature("KJ", "01", {"1", "2"}), feature("PS", "01", {"3", "4"})};

    const auto report = importAndFinish(document, project, codesAndLinework());

    ASSERT_EQ(report->unplacedFeatures.size(), 1u);
    EXPECT_EQ(report->unplacedFeatures[0].reason, UnplacedFeatureReason::PointCode);
    ASSERT_EQ(report->strung.unplaced.size(), 2u);
    EXPECT_EQ(report->strung.unplaced[0].reason, UnplacedReason::PointCode);
    EXPECT_EQ(report->strung.unplaced[1].reason, UnplacedReason::NoCode);
    // Six points, five coded, all five ruled ("KJ*", "PS*"): two layers, two
    // styles. One line; one string and two points that are none.
    EXPECT_EQ(katana::cad::describe(*report),
              (std::vector<std::string>{
                  "Survey codes were applied to the 6 point(s) drawn: 5 carry a code and 5 of "
                  "those have a rule; 2 layer(s) and 2 style(s) were created.",
                  "Linework: 1 line(s) drawn.",
                  "1 string(s) of the file are in no line (its code is a point code: 1).",
                  "2 point(s) are in no line (no code: 1; its code is a point code: 1)."}));
    EXPECT_TRUE(katana::cad::finishWarnings(*report).empty());
}

TEST(SurveyFinish, OneUndoTakesBackPointsCodesAndLinesAndRedoRestoresExactlyTheSameDrawing)
{
    Document document;
    document.setSurveyMap(fieldMap());
    const Drawing before = drawingOf(document);

    const auto report =
        importAndFinish(document, fieldFile(), codesAndLinework(), ontoATopLevelLayer());
    const Drawing after = drawingOf(document);
    // Ten points and three lines; the created list is the points, then the lines.
    ASSERT_EQ(after.entities.size(), 13u);
    const auto created = document.lastCreatedEntities();
    ASSERT_EQ(created.size(), 13u);
    for (std::size_t i = 0; i < created.size(); ++i) {
        const Entity& entity = after.entities.at(created[i]);
        EXPECT_EQ(std::holds_alternative<Polyline2>(entity.geometry), i >= 10) << i;
    }
    EXPECT_EQ(std::vector<EntityId>(created.begin() + 10, created.end()), report->lines);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(drawingOf(document), before) << "points, lines, layers and styles all gone";
    EXPECT_FALSE(document.undo().ok()) << "it was ONE step: nothing is left to undo";

    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(drawingOf(document), after) << "the same ids, layers, styles and properties";
    EXPECT_EQ(document.lastCreatedEntities(), created);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(drawingOf(document), before);
}

TEST(SurveyFinish, AFileWithNoFeaturesIsStrungByItsCodes)
{
    Document document;
    katana::entity::SurveyMap map;
    add(map, lineRule("KB*", "SURVEY KERB"));
    SurveyRule tree;
    tree.key = "TR*";
    tree.model = "SURVEY DETAIL";
    tree.breakline = SurveyBreakline::Point;
    add(map, tree);
    document.setSurveyMap(map);

    // A delimited point list: the whole string name and its control codes in
    // the code column, and no feature at all.
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KB01 ST"), point("2", 0.0, 10.0, "KB01"),
                      point("3", 0.0, 20.0, "KB01 END"), point("4", 5.0, 0.0, "KB01 ST"),
                      point("5", 5.0, 10.0, "KB01 END"), point("6", 9.0, 9.0, "TR1")};

    const auto report = importAndFinish(document, project, codesAndLinework());

    // Coded by the string name, controls left out: five KB01 and one TR1.
    EXPECT_EQ(report->whyNotCoded, SurveyFinishSkip::None);
    EXPECT_EQ(report->coding.coded, 6u);
    EXPECT_EQ(report->coding.matched, 6u);
    ASSERT_EQ(report->coding.codes.size(), 2u);
    EXPECT_EQ(report->coding.codes[0].code, "KB01");
    EXPECT_EQ(report->coding.codes[0].entities, 5u);
    EXPECT_EQ(report->coding.codes[1].code, "TR1");
    EXPECT_EQ(pointNumbered(document, "3")->layer, "SURVEY KERB");
    EXPECT_EQ(pointNumbered(document, "6")->layer, "SURVEY DETAIL");
    EXPECT_EQ(textProperty(*pointNumbered(document, "1"), "string"), "<absent>");

    // In point-number order string KB01 is 1 ST, 2, 3 END, 4 ST, 5 END: a
    // line through 1, 2, 3 and a second through 4, 5. The tree is a point.
    EXPECT_EQ(report->whyNotStrung, SurveyFinishSkip::None);
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(verticesOf(*lines[0]),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(20, 0)}));
    EXPECT_EQ(verticesOf(*lines[1]), (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    EXPECT_EQ(lines[0]->layer, "SURVEY KERB");
    EXPECT_EQ(textProperty(*lines[0], "code"), "KB01");
    EXPECT_EQ(textProperty(*lines[0], "string"), "<absent>");
    EXPECT_TRUE(report->features.strings.empty());
    EXPECT_EQ(report->pointsInFeatures, 0u);
    ASSERT_EQ(report->strung.strings.size(), 2u);
    EXPECT_EQ(report->strung.strings[0].name, "KB01");
    EXPECT_EQ(report->strung.strings[0].key, "KB*");
    EXPECT_EQ(report->strung.strings[0].number, "01");
    ASSERT_EQ(report->strung.unplaced.size(), 1u);
    EXPECT_EQ(report->strung.unplaced[0].pointNumber, "6");
    EXPECT_EQ(report->strung.unplaced[0].reason, UnplacedReason::PointCode);
    EXPECT_EQ(pointCount(document), 6u) << "an import never deletes a point";
}

TEST(SurveyFinish, TheFallbackRuleAloneNeverMakesALineEvenWhenItSaysLine)
{
    katana::entity::SurveyMap map;
    add(map, lineRule("KB*", "SURVEY KERB"));
    SurveyRule everything; // a catch-all that says every code is a line
    everything.key = "*";
    everything.breakline = SurveyBreakline::Line;
    add(map, everything);

    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KB01"), point("2", 0.0, 10.0, "KB01"),
                      point("3", 5.0, 0.0, "ZZ ST"), point("4", 5.0, 10.0, "ZZ END")};

    Document document;
    document.setSurveyMap(map);
    const auto report = importAndFinish(document, project, codesAndLinework());

    // KB01 is a line by "KB*". ZZ has only "*" - and two control codes - and
    // neither makes it one here.
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    ASSERT_EQ(report->strung.unplaced.size(), 2u);
    EXPECT_EQ(report->strung.unplaced[0].pointNumber, "3");
    EXPECT_EQ(report->strung.unplaced[0].reason, UnplacedReason::NoRule);
    EXPECT_EQ(report->strung.unplaced[1].pointNumber, "4");
    EXPECT_EQ(report->strung.unplaced[1].reason, UnplacedReason::NoRule);

    // Asked for by hand, without the option, the same two points ARE a line:
    // that is Process Linework's rule, and it is unchanged.
    std::vector<EntityId> zz = {pointNumbered(document, "3")->id, pointNumbered(document, "4")->id};
    LineworkOptions byHand;
    byHand.ids = zz;
    auto planned = katana::cad::processLinework(document, byHand);
    ASSERT_TRUE(planned.ok()) << planned.error().describe();
    EXPECT_EQ(planned->report.strings.size(), 1u);
}

// The three ways a finish has nothing to do: each leaves the drawing exactly
// as the same import leaves it without a finish, and says why.
struct NothingToDo {
    const char* what;
    bool withMap;
    std::vector<SurveyPoint> points;
    SurveyFinishSkip why;
    const char* word; // the reply word of the design brief
    // What each step says of itself, in words.
    const char* codesSentence;
    const char* lineworkSentence;
};

TEST(SurveyFinish, WithNothingToDoThePointsAreDrawnAsWithoutItAndTheReportSaysWhy)
{
    const std::vector<NothingToDo> cases = {
        // No survey codes loaded: also a string by control codes is not drawn.
        {"empty map", false,
         {point("1", 0.0, 0.0, "KB01 ST"), point("2", 0.0, 10.0, "KB01 END")},
         SurveyFinishSkip::NoSurveyCodes, "no-survey-codes",
         "Survey codes were not applied: no survey codes are loaded.",
         "No linework was drawn: no survey codes are loaded."},
        // A coordinate list with no code column.
        {"no codes", true, {point("1", 0.0, 0.0, ""), point("2", 0.0, 10.0, "")},
         SurveyFinishSkip::NoCodesInFile, "no-codes-in-file",
         "Survey codes were not applied: none of the points carries a code.",
         "No linework was drawn: none of the points carries a code."},
        // Codes nothing but the bare "*" answers: not even its attribute is set.
        {"no rule", true, {point("1", 0.0, 0.0, "QQ ST"), point("2", 0.0, 10.0, "QQ END")},
         SurveyFinishSkip::NoRuleMatches, "no-rule-matches",
         "Survey codes were not applied: no rule matches any of their codes (QQ).",
         "No linework was drawn: no rule matches any of their codes."},
    };
    for (const NothingToDo& entry : cases) {
        SCOPED_TRACE(entry.what);
        SurveyProject project;
        project.points = entry.points;

        Document plain;
        Document finished;
        if (entry.withMap) {
            plain.setSurveyMap(fieldMap());
            finished.setSurveyMap(fieldMap());
        }
        (void)importAndFinish(plain, project, SurveyFinishOptions{});
        const auto report = importAndFinish(finished, project, codesAndLinework());

        EXPECT_EQ(drawingOf(finished), drawingOf(plain));
        EXPECT_EQ(report->points, 2u);
        EXPECT_EQ(report->whyNotCoded, entry.why);
        EXPECT_EQ(report->whyNotStrung, entry.why);
        EXPECT_EQ(katana::cad::toString(report->whyNotCoded), entry.word);
        EXPECT_TRUE(report->lines.empty());
        EXPECT_TRUE(report->layersCreated().empty());
        EXPECT_TRUE(report->stylesCreated().empty());
        // Said in words too, one sentence a step - and each is something to
        // act on: the step was asked for and did not happen.
        const std::vector<std::string> sentences = {entry.codesSentence, entry.lineworkSentence};
        EXPECT_EQ(katana::cad::describe(*report), sentences);
        EXPECT_EQ(katana::cad::finishWarnings(*report), sentences);
        // Still ONE undo step, which leaves what undoing the plain import
        // leaves (the parent of the default layer stays behind in both).
        ASSERT_TRUE(finished.undo().ok());
        ASSERT_TRUE(plain.undo().ok());
        EXPECT_TRUE(finished.model().entities.empty());
        EXPECT_EQ(drawingOf(finished), drawingOf(plain));
        EXPECT_FALSE(finished.undo().ok());
    }
}

TEST(SurveyFinish, CodesThatMatchNothingAreNamedInTheReport)
{
    Document document;
    document.setSurveyMap(fieldMap());
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "QQ"), point("2", 0.0, 10.0, "RR")};
    project.features = {feature("QQ", "5", {"1"}), feature("RR", "", {"2"})};

    const auto report = importAndFinish(document, project, codesAndLinework());

    EXPECT_EQ(report->whyNotCoded, SurveyFinishSkip::NoRuleMatches);
    EXPECT_EQ(report->coding.coded, 2u);
    EXPECT_EQ(report->coding.matched, 0u);
    EXPECT_EQ(report->codesWithNoRule(), (std::vector<std::string>{"QQ", "RR"}));
    EXPECT_TRUE(report->coding.stylesCreated.empty());
    EXPECT_EQ(report->coding.changed, 0u) << "nothing was changed, so nothing is claimed";
    EXPECT_EQ(report->whyNotStrung, SurveyFinishSkip::NoRuleMatches);
    // QQ 5 is a string of the file's own, and is refused as one. The feature
    // of RR has no name, so it is not one: its point is refused by its code.
    ASSERT_EQ(report->unplacedFeatures.size(), 1u);
    EXPECT_EQ(report->unplacedFeatures[0].name, "QQ 5");
    EXPECT_EQ(report->unplacedFeatures[0].index, 0u);
    EXPECT_EQ(report->unplacedFeatures[0].reason, UnplacedFeatureReason::NoRule);
    EXPECT_EQ(report->pointsInFeatures, 1u);
    ASSERT_EQ(report->strung.unplaced.size(), 1u);
    EXPECT_EQ(report->strung.unplaced[0].pointNumber, "2");
    EXPECT_EQ(report->strung.unplaced[0].reason, UnplacedReason::NoRule);
    EXPECT_TRUE(linesOf(document).empty());
    // Both steps say the same thing, naming the codes, and both are warnings.
    const std::vector<std::string> sentences = {
        "Survey codes were not applied: no rule matches any of their codes (QQ, RR).",
        "No linework was drawn: no rule matches any of their codes."};
    EXPECT_EQ(katana::cad::describe(*report), sentences);
    EXPECT_EQ(katana::cad::finishWarnings(*report), sentences);
    // The points still carry what the file said, for a rule added later: the
    // one thing a finish with nothing to do leaves behind, and only when
    // survey codes are loaded.
    EXPECT_EQ(textProperty(*pointNumbered(document, "1"), "string"), "5");
    EXPECT_EQ(textProperty(*pointNumbered(document, "1"), "Surveyed"), "<absent>");
}

TEST(SurveyFinish, WithBothOptionsOffTheCommandIsTheOneItWasGiven)
{
    Document document;
    document.setSurveyMap(fieldMap());
    const SurveyProject project = fieldFile();
    auto points =
        katana::cad::importSurveyPoints(document, project, {}, ExistingPointPolicy::Refuse);
    ASSERT_TRUE(points.ok());
    const cmd::Command* given = points->get();
    auto report = std::make_shared<SurveyFinishReport>();
    report->points = 99; // overwritten: the report is this call's

    auto command = katana::cad::withSurveyFinish(document, std::move(*points), project, {},
                                                 SurveyFinishOptions{}, report);

    EXPECT_EQ(command.get(), given) << "unwrapped: the very command, so the very behaviour";
    EXPECT_EQ(report->points, 0u);
    EXPECT_EQ(report->whyNotCoded, SurveyFinishSkip::NotAsked);
    EXPECT_EQ(report->whyNotStrung, SurveyFinishSkip::NotAsked);
    EXPECT_TRUE(katana::cad::describe(*report).empty());

    ASSERT_TRUE(document.execute(std::move(command)).ok());
    // Ten points on the import's layer, uncoded and unstrung, with no string
    // number: exactly what the import alone draws.
    Document alone;
    alone.setSurveyMap(fieldMap());
    auto same = katana::cad::importSurveyPoints(alone, project, {}, ExistingPointPolicy::Refuse);
    ASSERT_TRUE(same.ok());
    ASSERT_TRUE(alone.execute(std::move(*same)).ok());
    EXPECT_EQ(drawingOf(document), drawingOf(alone));
    EXPECT_EQ(textProperty(*pointNumbered(document, "1"), "string"), "<absent>");
    EXPECT_TRUE(linesOf(document).empty());
}

TEST(SurveyFinish, NothingToImportIsStillNoCommandAndTheReportSaysNoPoints)
{
    Document document;
    auto report = std::make_shared<SurveyFinishReport>();
    SurveyFinishOptions codesOnly;
    codesOnly.codes = true;

    auto command = katana::cad::withSurveyFinish(document, nullptr, {}, {}, codesOnly, report);

    EXPECT_EQ(command, nullptr);
    EXPECT_EQ(report->whyNotCoded, SurveyFinishSkip::NoPoints);
    EXPECT_EQ(report->whyNotStrung, SurveyFinishSkip::NotAsked);
}

// An empty id list means EVERY entity to applySurveyCodes and every point to
// processLinework. A wrapped command that draws no point must therefore code
// and string nothing - not the whole drawing.
TEST(SurveyFinish, ACommandThatDrawsNoPointLeavesTheRestOfTheDrawingAlone)
{
    Document document;
    katana::entity::SurveyMap map;
    add(map, lineRule("KB*", "SURVEY KERB"));
    document.setSurveyMap(map);
    // Two kerb points already there, uncoded: a CODE or a LINEWORK run over
    // the whole drawing would move them and join them.
    const EntityId first = addPoint(document, Point2(0.0, 0.0), "KB01", "1");
    const EntityId second = addPoint(document, Point2(10.0, 0.0), "KB01", "2");
    const Drawing before = drawingOf(document);

    katana::entity::Layer layer;
    layer.name = "empty";
    auto report = std::make_shared<SurveyFinishReport>();
    auto command = katana::cad::withSurveyFinish(document, cmd::createLayer(layer), {}, {},
                                                 codesAndLinework(), report);
    ASSERT_NE(command, nullptr);
    ASSERT_TRUE(document.execute(std::move(command)).ok());

    EXPECT_EQ(report->points, 0u);
    EXPECT_EQ(report->whyNotCoded, SurveyFinishSkip::NoPoints);
    EXPECT_EQ(report->whyNotStrung, SurveyFinishSkip::NoPoints);
    EXPECT_EQ(document.model().entities.find(first)->layer, "0");
    EXPECT_EQ(document.model().entities.find(second)->layer, "0");
    EXPECT_EQ(drawingOf(document).entities, before.entities);
    EXPECT_TRUE(document.model().layers.contains("empty"));
    EXPECT_FALSE(document.model().layers.contains("SURVEY KERB"));
}

TEST(SurveyFinish, AStepThatFailsTakesTheWholeImportWithItAndSaysWhichStep)
{
    Document document;
    document.setSurveyMap(fieldMap());
    const Drawing before = drawingOf(document);
    SurveyProject project = fieldFile();
    const SurveyImportOptions options = ontoATopLevelLayer();
    auto points =
        katana::cad::importSurveyPoints(document, project, options, ExistingPointPolicy::Refuse);
    ASSERT_TRUE(points.ok());

    // Start and end spelled alike: no control code can be read.
    SurveyFinishOptions finish = codesAndLinework();
    finish.controls.end = finish.controls.start;
    auto command =
        katana::cad::withSurveyFinish(document, std::move(*points), project, options, finish);
    ASSERT_NE(command, nullptr);
    const auto status = document.execute(std::move(command));

    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(drawingOf(document), before) << "refused before a point is drawn";
    EXPECT_FALSE(document.undo().ok()) << "and it left no undo step";

    // A failure AFTER the points are drawn: the features name a point the
    // project does not have, which drawSurveyFeatures refuses. The points
    // and their codes are taken back with it.
    auto again =
        katana::cad::importSurveyPoints(document, project, options, ExistingPointPolicy::Refuse);
    ASSERT_TRUE(again.ok());
    SurveyProject broken = project;
    broken.features.push_back(feature("KJ", "09", {"1", "404"}));
    auto second = katana::cad::withSurveyFinish(document, std::move(*again), broken, options,
                                                codesAndLinework());
    const auto failed = document.execute(std::move(second));
    ASSERT_FALSE(failed.ok());
    // survey::validateProject's own refusal of a reference to a point the
    // project lacks (data_model.hpp: NotFound), with the step named.
    EXPECT_EQ(failed.error().code, ErrorCode::NotFound);
    EXPECT_NE(failed.error().context.find("features"), std::string::npos)
        << failed.error().context;
    EXPECT_EQ(drawingOf(document), before);
    EXPECT_FALSE(document.undo().ok()) << "and this one left no undo step either";
}

// ---- which strings are the file's own, and which points are left to their codes --------

// A reader whose format has no string numbers makes a feature of every RUN of
// consecutive shots of one code, with no name. Shot in sections across a road
// - left kerb, right kerb, right kerb, left kerb - each kerb is then several
// runs, some of one point, and none of them is the kerb. The points' codes
// still say which string each is of, exactly as they do to Process Linework.
TEST(SurveyFinish, AFeatureWithNoNameIsNotAStringOfTheFileSoItsPointsAreStrungByTheirCodes)
{
    Document document;
    katana::entity::SurveyMap map;
    add(map, lineRule("KB*", "SURVEY KERB"));
    SurveyRule tree;
    tree.key = "TR*";
    tree.model = "SURVEY DETAIL";
    tree.breakline = SurveyBreakline::Point;
    add(map, tree);
    document.setSurveyMap(map);

    // Left kerb KB01 along E 0, right kerb KB02 along E 6, and a tree shot
    // between two shots of the left kerb.
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KB01"),  point("2", 0.0, 6.0, "KB02"),
                      point("3", 10.0, 6.0, "KB02"), point("4", 10.0, 0.0, "KB01"),
                      point("5", 20.0, 0.0, "KB01"), point("6", 25.0, 1.0, "TR1"),
                      point("7", 30.0, 0.0, "KB01")};
    // The runs of consecutive equal codes: 1 | 2 3 | 4 5 | 6 | 7.
    project.features = {feature("KB01", "", {"1"}), feature("KB02", "", {"2", "3"}),
                        feature("KB01", "", {"4", "5"}), feature("TR1", "", {"6"}),
                        feature("KB01", "", {"7"})};

    const auto report = importAndFinish(document, project, codesAndLinework());

    // By code, in point-number order: KB01 is 1, 4, 5, 7 and KB02 is 2, 3.
    EXPECT_EQ(report->whyNotStrung, SurveyFinishSkip::None);
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 0), Point2(0, 10),
                                                         Point2(0, 20), Point2(0, 30)}));
    EXPECT_EQ(verticesOf(*lines[1]), (std::vector<Point2>{Point2(6, 0), Point2(6, 10)}));
    EXPECT_EQ(textProperty(*lines[0], "code"), "KB01");
    EXPECT_EQ(textProperty(*lines[0], "string"), "<absent>");
    EXPECT_EQ(lines[0]->layer, "SURVEY KERB");
    // None of the five runs is drawn, or reported, as a string of the file.
    EXPECT_TRUE(report->features.strings.empty());
    EXPECT_TRUE(report->unplacedFeatures.empty());
    EXPECT_EQ(report->pointsInFeatures, 0u);
    EXPECT_EQ(report->strung.considered, 7u);
    ASSERT_EQ(report->strung.strings.size(), 2u);
    EXPECT_EQ(report->strung.strings[0].name, "KB01");
    EXPECT_EQ(report->strung.strings[0].pointNumbers,
              (std::vector<std::string>{"1", "4", "5", "7"}));
    EXPECT_EQ(report->strung.strings[1].name, "KB02");
    EXPECT_EQ(report->strung.strings[1].pointNumbers, (std::vector<std::string>{"2", "3"}));
    ASSERT_EQ(report->strung.unplaced.size(), 1u);
    EXPECT_EQ(report->strung.unplaced[0].pointNumber, "6");
    EXPECT_EQ(report->strung.unplaced[0].reason, UnplacedReason::PointCode);
}

// A controller's keyed line joins two points and has a name and a code of its
// own. Its ends are still points of THEIR code's string: the line does not
// take them out of it, whether a rule draws the line or not.
TEST(SurveyFinish, TheEndsOfANamedLineOfAnotherCodeStayInTheirOwnCodesString)
{
    for (const bool boundaryIsALine : {false, true}) {
        SCOPED_TRACE(boundaryIsALine ? "BDY* is a line" : "no rule for BDY");
        Document document;
        katana::entity::SurveyMap map;
        add(map, lineRule("KB*", "SURVEY KERB"));
        if (boundaryIsALine) {
            add(map, lineRule("BDY*", "SURVEY BOUNDARY"));
        }
        document.setSurveyMap(map);

        // Four kerb shots along N 0, and a line "L1" coded BDY keyed between
        // the second and the third.
        SurveyProject project;
        project.points = {point("1", 0.0, 0.0, "KB01"), point("2", 0.0, 10.0, "KB01"),
                          point("3", 0.0, 20.0, "KB01"), point("4", 0.0, 30.0, "KB01")};
        project.features = {feature("BDY", "L1", {"2", "3"})};

        const auto report = importAndFinish(document, project, codesAndLinework());

        const auto lines = linesOf(document);
        const std::vector<Point2> kerb = {Point2(0, 0), Point2(10, 0), Point2(20, 0),
                                          Point2(30, 0)};
        if (boundaryIsALine) {
            // The file's own string first, then the kerb through all four.
            ASSERT_EQ(lines.size(), 2u);
            EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(10, 0), Point2(20, 0)}));
            EXPECT_EQ(textProperty(*lines[0], "code"), "BDY");
            EXPECT_EQ(textProperty(*lines[0], "string"), "L1");
            EXPECT_EQ(lines[0]->layer, "SURVEY BOUNDARY");
            EXPECT_EQ(verticesOf(*lines[1]), kerb);
            EXPECT_TRUE(report->unplacedFeatures.empty());
        } else {
            ASSERT_EQ(lines.size(), 1u);
            EXPECT_EQ(verticesOf(*lines[0]), kerb);
            ASSERT_EQ(report->unplacedFeatures.size(), 1u);
            EXPECT_EQ(report->unplacedFeatures[0].name, "BDY L1");
            EXPECT_EQ(report->unplacedFeatures[0].reason, UnplacedFeatureReason::NoRule);
        }
        // The line is of another code, so it gave its ends no string number
        // and left them to their own.
        EXPECT_EQ(textProperty(*pointNumbered(document, "2"), "string"), "<absent>");
        EXPECT_EQ(report->pointsInFeatures, 0u);
        EXPECT_EQ(report->strung.considered, 4u);
        ASSERT_EQ(report->strung.strings.size(), 1u);
        EXPECT_EQ(report->strung.strings[0].pointNumbers,
                  (std::vector<std::string>{"1", "2", "3", "4"}));
    }
}

TEST(SurveyFinish, AFileWithStringsOfItsOwnAndPointsOutsideThemDrawsBothKindsOfLine)
{
    Document document;
    katana::entity::SurveyMap map;
    add(map, lineRule("KJ*", "SURVEY JOINT"));
    add(map, lineRule("KB*", "SURVEY KERB"));
    document.setSurveyMap(map);

    // Points 1 and 2 are string 01 of KJ, by a feature; 3, 4 and 5 are coded
    // KB01 whole and no feature names them.
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KJ"), point("2", 0.0, 10.0, "KJ"),
                      point("3", 5.0, 0.0, "KB01"), point("4", 5.0, 10.0, "KB01"),
                      point("5", 5.0, 20.0, "KB01")};
    project.features = {feature("KJ", "01", {"1", "2"})};

    const auto report = importAndFinish(document, project, codesAndLinework());

    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    EXPECT_EQ(textProperty(*lines[0], "code"), "KJ");
    EXPECT_EQ(textProperty(*lines[0], "string"), "01");
    EXPECT_EQ(lines[0]->layer, "SURVEY JOINT");
    EXPECT_EQ(verticesOf(*lines[1]),
              (std::vector<Point2>{Point2(0, 5), Point2(10, 5), Point2(20, 5)}));
    EXPECT_EQ(textProperty(*lines[1], "code"), "KB01");
    EXPECT_EQ(lines[1]->layer, "SURVEY KERB");
    // Two points were the feature's; the other three were strung by code.
    EXPECT_EQ(report->pointsInFeatures, 2u);
    EXPECT_EQ(report->strung.considered, 3u);
    ASSERT_EQ(report->features.strings.size(), 1u);
    ASSERT_EQ(report->strung.strings.size(), 1u);
    EXPECT_EQ(report->strung.strings[0].name, "KB01");
    EXPECT_EQ(report->lines.size(), 2u);
    EXPECT_EQ(report->unplaced(), 0u);
}

// "It acts on what the import created" with something else there to act on:
// the drawing already holds two kerb points of the very string name the
// import brings.
TEST(SurveyFinish, PointsAlreadyInTheDrawingAreNeitherCodedNorStrungWithTheImport)
{
    Document document;
    katana::entity::SurveyMap map;
    add(map, lineRule("KB*", "SURVEY KERB"));
    document.setSurveyMap(map);
    const EntityId first = addPoint(document, Point2(0.0, 0.0), "KB01", "1");
    const EntityId second = addPoint(document, Point2(10.0, 0.0), "KB01", "2");
    const Drawing before = drawingOf(document);
    const std::size_t steps = document.history().undoCount();

    SurveyProject project;
    project.points = {point("3", 5.0, 0.0, "KB01"), point("4", 5.0, 10.0, "KB01")};
    const auto report =
        importAndFinish(document, project, codesAndLinework(), ontoATopLevelLayer());

    // Points 3 and 4 alone: coded, and joined to each other and not to 1 or 2.
    EXPECT_EQ(report->points, 2u);
    EXPECT_EQ(report->coding.coded, 2u);
    EXPECT_EQ(report->strung.considered, 2u);
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    EXPECT_EQ(pointNumbered(document, "3")->layer, "SURVEY KERB");
    EXPECT_EQ(pointNumbered(document, "4")->layer, "SURVEY KERB");
    // The two that were there are as they were: uncoded, on layer "0".
    EXPECT_EQ(*document.model().entities.find(first), before.entities.at(first));
    EXPECT_EQ(*document.model().entities.find(second), before.entities.at(second));

    EXPECT_EQ(document.history().undoCount(), steps + 1);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(drawingOf(document), before);
}

// A longer copy of a file already imported, topped up with Skip: the import
// creates only the points that are new, and so draws only the strings that
// run through one of them - not every string of the file a second time.
TEST(SurveyFinish, AnImportDrawsOnlyTheStringsOfTheFileThatNameAPointItCreated)
{
    Document document;
    katana::entity::SurveyMap map;
    add(map, lineRule("KJ*", "SURVEY JOINT"));
    document.setSurveyMap(map);

    SurveyProject dayOne;
    dayOne.points = {point("1", 0.0, 0.0, "KJ"), point("2", 0.0, 10.0, "KJ"),
                     point("3", 5.0, 0.0, "KJ"), point("4", 5.0, 10.0, "KJ")};
    dayOne.features = {feature("KJ", "01", {"1", "2"}), feature("KJ", "02", {"3", "4"})};
    (void)importAndFinish(document, dayOne, codesAndLinework());
    ASSERT_EQ(linesOf(document).size(), 2u);

    // The same file a day later: one more shot, of string 02.
    SurveyProject dayTwo = dayOne;
    dayTwo.points.push_back(point("5", 5.0, 20.0, "KJ"));
    dayTwo.features[1].pointIds.push_back("5");
    const auto report =
        importAndFinish(document, dayTwo, codesAndLinework(), {}, ExistingPointPolicy::Skip);

    EXPECT_EQ(report->points, 1u) << "point 5 alone is new";
    // String 02 is drawn, through all three of its points; string 01, none of
    // whose points this import created, is not drawn again.
    ASSERT_EQ(report->lines.size(), 1u);
    const Entity* drawn = document.model().entities.find(report->lines.front());
    ASSERT_NE(drawn, nullptr);
    EXPECT_EQ(verticesOf(*drawn),
              (std::vector<Point2>{Point2(0, 5), Point2(10, 5), Point2(20, 5)}));
    ASSERT_EQ(report->features.strings.size(), 1u);
    EXPECT_EQ(report->features.strings[0].name, "KJ 02");
    EXPECT_TRUE(report->unplacedFeatures.empty());
    EXPECT_EQ(linesOf(document).size(), 3u);
    EXPECT_EQ(report->pointsInFeatures, 1u);
    EXPECT_EQ(textProperty(*pointNumbered(document, "5"), "string"), "02");
    EXPECT_EQ(pointNumbered(document, "5")->layer, "SURVEY JOINT");
}

// ---- one option without the other ------------------------------------------------------

TEST(SurveyFinish, CodesAloneMoveThePointsToTheirRulesAndDrawNoLine)
{
    Document document;
    document.setSurveyMap(fieldMap());
    SurveyFinishOptions codesOnly;
    codesOnly.codes = true;

    const auto report = importAndFinish(document, fieldFile(), codesOnly);

    EXPECT_EQ(report->whyNotCoded, SurveyFinishSkip::None);
    EXPECT_EQ(report->whyNotStrung, SurveyFinishSkip::NotAsked);
    EXPECT_TRUE(report->lines.empty());
    EXPECT_TRUE(linesOf(document).empty());
    EXPECT_EQ(document.lastCreatedEntities().size(), 10u);
    // Coded exactly as with both options: by code and string number.
    EXPECT_EQ(pointNumbered(document, "1")->layer, "SURVEY JOINT");
    EXPECT_EQ(pointNumbered(document, "5")->style, "Peg");
    EXPECT_EQ(pointNumbered(document, "7")->layer, "SURVEY BARRIER");
    EXPECT_EQ(textProperty(*pointNumbered(document, "7"), "string"), "12");
    EXPECT_EQ(pointNumbered(document, "9")->layer, "survey/points");
    EXPECT_EQ(report->coding.matched, 8u);
    EXPECT_EQ(report->unplaced(), 0u) << "nothing was strung, so nothing was left unstrung";
}

TEST(SurveyFinish, LineworkAloneDrawsTheStyledLinesAndLeavesThePointsAsTheImportDrewThem)
{
    Document document;
    document.setSurveyMap(fieldMap());
    SurveyFinishOptions lineworkOnly;
    lineworkOnly.linework = true;

    const auto report = importAndFinish(document, fieldFile(), lineworkOnly);

    EXPECT_EQ(report->whyNotCoded, SurveyFinishSkip::NotAsked);
    EXPECT_EQ(report->coding.coded, 0u);
    EXPECT_EQ(report->whyNotStrung, SurveyFinishSkip::None);
    // Every point where the import put it, unstyled, without the "*" rule's
    // attribute - and with its string number, which the lines are found by.
    for (const char* number : {"1", "2", "3", "4", "5", "6", "7", "8", "9", "10"}) {
        const Entity* entity = pointNumbered(document, number);
        ASSERT_NE(entity, nullptr) << number;
        EXPECT_EQ(entity->layer, "survey/points") << number;
        EXPECT_EQ(entity->style, "") << number;
        EXPECT_EQ(textProperty(*entity, "Surveyed"), "<absent>") << number;
    }
    EXPECT_EQ(textProperty(*pointNumbered(document, "3"), "string"), "02");

    // KJ 01, KJ 02 and B 12, each on its rule's layer in its rule's style.
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 3u);
    EXPECT_EQ(verticesOf(*lines[1]), (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    EXPECT_EQ(lines[0]->layer, "SURVEY JOINT");
    EXPECT_EQ(lines[0]->style, "Joint Line");
    EXPECT_EQ(lines[1]->style, "Joint Line");
    EXPECT_EQ(lines[2]->layer, "SURVEY BARRIER");
    EXPECT_EQ(lines[2]->style, "Barrier");
    // The layers and styles are the LINES' this time: the points made none.
    EXPECT_TRUE(report->coding.layersCreated.empty());
    EXPECT_EQ(report->layersCreated(),
              (std::vector<std::string>{"SURVEY BARRIER", "SURVEY JOINT"}));
    EXPECT_EQ(report->stylesCreated(), (std::vector<std::string>{"Barrier", "Joint Line"}));
    EXPECT_FALSE(document.model().layers.contains("SURVEY MARKS"));
}

// ---- what the options carry reaches the steps ------------------------------------------

TEST(SurveyFinish, TheColourLookupStylesThePointsAndBothKindsOfLineAlike)
{
    Document document;
    katana::entity::SurveyMap map;
    SurveyRule joint = lineRule("KJ*", "SURVEY JOINT", "Joint Line");
    joint.colour = "red";
    add(map, joint);
    document.setSurveyMap(map);

    // String 01 by a feature, and two points coded KJ05 whole that no feature
    // names: a line of the file's and a line strung by code, of one rule.
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KJ"), point("2", 0.0, 10.0, "KJ"),
                      point("3", 5.0, 0.0, "KJ05"), point("4", 5.0, 10.0, "KJ05")};
    project.features = {feature("KJ", "01", {"1", "2"})};
    SurveyFinishOptions finish = codesAndLinework();
    finish.coding.colourOf = [](std::string_view name) -> std::optional<katana::entity::Color> {
        return name == "red" ? std::optional(katana::entity::Color{255, 0, 0, 255})
                             : std::nullopt;
    };

    const auto report = importAndFinish(document, project, finish);

    // ONE style, coloured: a step that had lost the lookup would not know
    // "red" as a colour, could not reuse the points' style and would make a
    // second one for its lines.
    EXPECT_EQ(report->stylesCreated(), (std::vector<std::string>{"Joint Line"}));
    const katana::entity::Style* style = document.model().styles.find("Joint Line");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->color, std::optional(katana::entity::Color{255, 0, 0, 255}));
    EXPECT_EQ(document.model().styles.all().size(), 1u);
    for (const char* number : {"1", "2", "3", "4"}) {
        EXPECT_EQ(pointNumbered(document, number)->style, "Joint Line") << number;
    }
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(lines[0]->style, "Joint Line");
    EXPECT_EQ(lines[1]->style, "Joint Line");
}

TEST(SurveyFinish, TheControlCodesAndTheOrderAreTheOnesTheOptionsGive)
{
    katana::entity::SurveyMap map;
    add(map, lineRule("KB*", "SURVEY KERB"));
    // Four kerb shots; the first and the third say "BEG".
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KB01 BEG"), point("2", 0.0, 10.0, "KB01"),
                      point("3", 5.0, 0.0, "KB01 BEG"), point("4", 5.0, 10.0, "KB01")};
    {
        // BEG is no control code by default: one line through all four.
        Document document;
        document.setSurveyMap(map);
        (void)importAndFinish(document, project, codesAndLinework());
        ASSERT_EQ(linesOf(document).size(), 1u);
        EXPECT_EQ(verticesOf(*linesOf(document)[0]).size(), 4u);
    }
    {
        // Spelled as the start code, it begins a line at 1 and another at 3.
        Document document;
        document.setSurveyMap(map);
        SurveyFinishOptions finish = codesAndLinework();
        finish.controls.start = "BEG";
        (void)importAndFinish(document, project, finish);
        const auto lines = linesOf(document);
        ASSERT_EQ(lines.size(), 2u);
        EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
        EXPECT_EQ(verticesOf(*lines[1]), (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    }

    // The order: point "2" is listed, and so drawn, before point "1".
    SurveyProject backwards;
    backwards.points = {point("2", 0.0, 0.0, "KB01"), point("1", 0.0, 10.0, "KB01")};
    {
        Document document;
        document.setSurveyMap(map);
        (void)importAndFinish(document, backwards, codesAndLinework());
        ASSERT_EQ(linesOf(document).size(), 1u);
        // By point number, the default: from 1 (E 10) to 2 (E 0).
        EXPECT_EQ(verticesOf(*linesOf(document)[0]),
                  (std::vector<Point2>{Point2(10, 0), Point2(0, 0)}));
    }
    {
        Document document;
        document.setSurveyMap(map);
        SurveyFinishOptions finish = codesAndLinework();
        finish.order = katana::cad::LineworkOrder::EntityOrder;
        (void)importAndFinish(document, backwards, finish);
        ASSERT_EQ(linesOf(document).size(), 1u);
        // In the order drawn: from 2 (E 0) to 1 (E 10).
        EXPECT_EQ(verticesOf(*linesOf(document)[0]),
                  (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    }
}

TEST(SurveyFinish, AnImportUnderOtherPropertyNamesIsCodedAndStrungByThoseNames)
{
    Document document;
    katana::entity::SurveyMap map;
    add(map, lineRule("KJ*", "SURVEY JOINT"));
    add(map, lineRule("KB*", "SURVEY KERB"));
    document.setSurveyMap(map);
    SurveyImportOptions options;
    options.codeProperty = "fieldcode";
    options.pointNumberProperty = "ptno";

    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KJ"), point("2", 0.0, 10.0, "KJ"),
                      point("3", 5.0, 0.0, "KB01"), point("4", 5.0, 10.0, "KB01")};
    project.features = {feature("KJ", "01", {"1", "2"})};

    const auto report = importAndFinish(document, project, codesAndLinework(), options);

    EXPECT_EQ(report->coding.property, "fieldcode");
    EXPECT_EQ(report->coding.coded, 4u);
    EXPECT_EQ(report->coding.matched, 4u);
    // The same two lines as under the default names, carrying the code where
    // the points carry it.
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    EXPECT_EQ(textProperty(*lines[0], "fieldcode"), "KJ");
    EXPECT_EQ(textProperty(*lines[0], "code"), "<absent>");
    EXPECT_EQ(textProperty(*lines[0], "string"), "01");
    EXPECT_EQ(verticesOf(*lines[1]), (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    EXPECT_EQ(textProperty(*lines[1], "fieldcode"), "KB01");
    EXPECT_EQ(lines[1]->layer, "SURVEY KERB");
    // The points found by their number under its own name: two the feature's,
    // two left to their code.
    EXPECT_EQ(report->pointsInFeatures, 2u);
    EXPECT_EQ(report->strung.considered, 2u);
    EXPECT_EQ(report->strung.property, "fieldcode");
}

// The degenerate import: no point number is written, so a point cannot be
// found among the ids a feature lists. A point is then left to the file's
// strings when the file has a numbered string of its CODE, and strung by its
// code otherwise - in the order drawn, there being no number to order by.
TEST(SurveyFinish, AnImportThatWritesNoPointNumberTellsTheFilesPointsByTheirCode)
{
    Document document;
    katana::entity::SurveyMap map;
    add(map, lineRule("KJ*", "SURVEY JOINT"));
    add(map, lineRule("KB*", "SURVEY KERB"));
    document.setSurveyMap(map);
    SurveyImportOptions options;
    options.pointNumberProperty = "";
    SurveyFinishOptions finish = codesAndLinework();
    finish.order = katana::cad::LineworkOrder::EntityOrder;

    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KJ"), point("2", 0.0, 10.0, "KJ"),
                      point("3", 5.0, 0.0, "KB01"), point("4", 5.0, 10.0, "KB01")};
    project.features = {feature("KJ", "01", {"1", "2"})};

    const auto report = importAndFinish(document, project, finish, options);

    EXPECT_EQ(report->points, 4u);
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 2u);
    // KJ 01 from the file's own coordinates; KB01 by code, in entity order.
    EXPECT_EQ(verticesOf(*lines[0]), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    EXPECT_EQ(textProperty(*lines[0], "string"), "01");
    EXPECT_EQ(verticesOf(*lines[1]), (std::vector<Point2>{Point2(0, 5), Point2(10, 5)}));
    EXPECT_EQ(textProperty(*lines[1], "code"), "KB01");
    EXPECT_EQ(report->pointsInFeatures, 2u) << "the two KJ points: the file numbers a KJ string";
    EXPECT_EQ(report->strung.considered, 2u);
    EXPECT_TRUE(report->strung.unplaced.empty());
}

// ---- which rule decides line or point --------------------------------------------------

// "A rule more specific than the bare * makes it a line": the first such rule,
// most specific first, that SAYS line or point. A rule that says neither
// passes the question on to the next one - but never to "*".
TEST(LineworkRuled, TheFirstRuleThatSaysLineOrPointDecidesAndTheFallbackIsNeverAsked)
{
    katana::entity::SurveyMap map;
    SurveyRule everything; // every code is a line, says the catch-all
    everything.key = "*";
    everything.breakline = SurveyBreakline::Line;
    add(map, everything);
    SurveyRule mark; // where a mark goes, and nothing about lines
    mark.key = "PS*";
    mark.model = "SURVEY MARKS";
    add(map, mark);
    add(map, lineRule("KJ*", "SURVEY JOINT"));
    SurveyRule early; // the first ten joint strings: a layer, nothing about lines
    early.key = "KJ0*";
    early.model = "SURVEY JOINT EARLY";
    add(map, early);

    Document document;
    document.setSurveyMap(map);
    addPoint(document, Point2(0.0, 0.0), "PS1", "1");
    addPoint(document, Point2(10.0, 0.0), "PS1", "2");
    addPoint(document, Point2(0.0, 5.0), "KJ01", "3");
    addPoint(document, Point2(10.0, 5.0), "KJ01", "4");

    LineworkOptions options;
    options.onlyRuledLines = true;
    auto planned = katana::cad::processLinework(document, options);
    ASSERT_TRUE(planned.ok()) << planned.error().describe();

    // KJ01: "KJ0*" says neither, "KJ*" says line - a line, on the layer the
    // more specific rule names.
    ASSERT_EQ(planned->report.strings.size(), 1u);
    EXPECT_EQ(planned->report.strings[0].name, "KJ01");
    EXPECT_EQ(planned->report.strings[0].key, "KJ0*");
    EXPECT_EQ(planned->report.strings[0].layer, "SURVEY JOINT EARLY");
    // PS1: "PS*" says neither and only "*" is left, which is not asked: a
    // point code, and not "no rule" either - a rule does know it.
    ASSERT_EQ(planned->report.unplaced.size(), 2u);
    EXPECT_EQ(planned->report.unplaced[0].pointNumber, "1");
    EXPECT_EQ(planned->report.unplaced[0].reason, UnplacedReason::PointCode);
    EXPECT_EQ(planned->report.unplaced[1].reason, UnplacedReason::PointCode);

    // The same two questions asked of features.
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "PS"), point("2", 0.0, 10.0, "PS"),
                      point("3", 5.0, 0.0, "KJ"), point("4", 5.0, 10.0, "KJ")};
    project.features = {feature("PS", "1", {"1", "2"}), feature("KJ", "01", {"3", "4"})};
    SurveyFeatureOptions ruled;
    ruled.onlyRuledLines = true;
    auto features = katana::cad::drawSurveyFeatures(document, project, ruled);
    ASSERT_TRUE(features.ok()) << features.error().describe();
    ASSERT_EQ(features->report.strings.size(), 1u);
    EXPECT_EQ(features->report.strings[0].name, "KJ 01");
    ASSERT_EQ(features->unplaced.size(), 1u);
    EXPECT_EQ(features->unplaced[0].name, "PS 1");
    EXPECT_EQ(features->unplaced[0].reason, UnplacedFeatureReason::PointCode);
}

TEST(LineworkRuled, ALineOfPointsThatCarryTheirStringNumberApartReportsThatNumberWhole)
{
    Document document;
    document.setSurveyMap(fieldMap());
    // Code B in string 12: the key is "B1*", and what that key leaves of the
    // name "B12" is "2" - which is not the string's number.
    addPoint(document, Point2(0.0, 0.0), "B", "1", "12");
    addPoint(document, Point2(10.0, 0.0), "B", "2", "12");

    auto planned = katana::cad::processLinework(document, LineworkOptions{});
    ASSERT_TRUE(planned.ok()) << planned.error().describe();

    ASSERT_EQ(planned->report.strings.size(), 1u);
    EXPECT_EQ(planned->report.strings[0].name, "B12");
    EXPECT_EQ(planned->report.strings[0].key, "B1*");
    EXPECT_EQ(planned->report.strings[0].number, "12");
    EXPECT_EQ(planned->report.strings[0].layer, "SURVEY BARRIER");
}

TEST(SurveyFeaturesRuled, AFeatureWithNoNameIsDrawnByItsCodeAndCarriesNoStringNumber)
{
    Document document;
    document.setSurveyMap(fieldMap());
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, "KJ07"), point("2", 0.0, 10.0, "KJ07")};
    project.features = {feature("KJ07", "", {"1", "2"})};
    SurveyFeatureOptions options;
    options.onlyRuledLines = true;

    auto planned = katana::cad::drawSurveyFeatures(document, project, options);
    ASSERT_TRUE(planned.ok()) << planned.error().describe();
    ASSERT_NE(planned->command, nullptr);
    ASSERT_TRUE(document.execute(std::move(planned->command)).ok());

    // Called by its code, which "KJ*" answers and splits: number 07.
    ASSERT_EQ(planned->report.strings.size(), 1u);
    EXPECT_EQ(planned->report.strings[0].name, "KJ07");
    EXPECT_EQ(planned->report.strings[0].key, "KJ*");
    EXPECT_EQ(planned->report.strings[0].number, "07");
    const auto lines = linesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(textProperty(*lines[0], "code"), "KJ07");
    EXPECT_EQ(textProperty(*lines[0], "string"), "<absent>");
    EXPECT_EQ(lines[0]->layer, "SURVEY JOINT");
}

TEST(SurveyFinishStringNames, AFeatureNamingAPointThatIsNotAmongThePointsNamesTheOthers)
{
    // Point 9 is the drawing's control, or has no coordinates: not drawn, so
    // not in the list. The feature still names point 1.
    std::vector<SurveyPoint> points = {point("1", 0.0, 0.0, "KJ")};
    const std::vector<SurveyFeature> features = {feature("KJ", "01", {"9", "1"})};

    katana::cad::nameSurveyStrings(points, features);

    ASSERT_EQ(points.size(), 1u);
    EXPECT_EQ(points[0].metadata.at("string"), "01");
}

// ---- refusals --------------------------------------------------------------------------

TEST(SurveyFinish, TheCommandRefusesADocumentItWasNotPlannedFor)
{
    Document planned;
    planned.setSurveyMap(fieldMap());
    Document other;
    other.setSurveyMap(fieldMap());
    const SurveyProject project = fieldFile();
    auto points =
        katana::cad::importSurveyPoints(planned, project, {}, ExistingPointPolicy::Refuse);
    ASSERT_TRUE(points.ok());
    auto command = katana::cad::withSurveyFinish(planned, std::move(*points), project, {},
                                                 codesAndLinework());
    ASSERT_NE(command, nullptr);

    const auto status = other.execute(std::move(command));

    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidState);
    EXPECT_TRUE(other.model().entities.empty());
    EXPECT_TRUE(planned.model().entities.empty());
    EXPECT_EQ(other.history().undoCount(), 0u);
}

// ---- which features, and lines drawn before --------------------------------------------

TEST(SurveyFeaturesRuled, OnlyTheFeaturesAskedAboutAreDrawnOrReported)
{
    Document document;
    document.setSurveyMap(fieldMap());
    const SurveyProject project = fieldFile(); // KJ 01, KJ 02, PS 01, B 12, ZZ 1
    SurveyFeatureOptions options;
    options.onlyRuledLines = true;

    // KJ 02 and ZZ 1 alone: one line, one string with no rule - and nothing
    // said of the three that were not asked about.
    options.consider = {false, true, false, false, true};
    auto some = katana::cad::drawSurveyFeatures(document, project, options);
    ASSERT_TRUE(some.ok()) << some.error().describe();
    ASSERT_EQ(some->report.strings.size(), 1u);
    EXPECT_EQ(some->report.strings[0].name, "KJ 02");
    ASSERT_EQ(some->unplaced.size(), 1u);
    EXPECT_EQ(some->unplaced[0].index, 4u) << "its place among ALL the project's features";
    EXPECT_EQ(some->unplaced[0].name, "ZZ 1");

    // None of them is a list of falses, and is nothing to do - never "all".
    options.consider.assign(5, false);
    auto none = katana::cad::drawSurveyFeatures(document, project, options);
    ASSERT_TRUE(none.ok()) << none.error().describe();
    EXPECT_EQ(none->command, nullptr);
    EXPECT_TRUE(none->report.strings.empty());
    EXPECT_TRUE(none->unplaced.empty());

    // A list that is not one entry per feature could only be read by guessing.
    options.consider.assign(4, true);
    const auto wrong = katana::cad::drawSurveyFeatures(document, project, options);
    ASSERT_FALSE(wrong.ok());
    EXPECT_EQ(wrong.error().code, ErrorCode::InvalidArgument);

    // The whole project is checked whatever is asked about: a feature left
    // out that names a point the project lacks is still an inconsistency.
    SurveyProject broken = project;
    broken.features.push_back(feature("KJ", "09", {"1", "404"}));
    options.consider.assign(6, false);
    const auto refused = katana::cad::drawSurveyFeatures(document, broken, options);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::NotFound);
}

// A line an earlier run drew of the same string is redrawn as the SAME
// entity: what a re-adjusted survey job's lines rest on.
TEST(LineworkEarlierLines, ALineOfTheSameStringIsMovedNotReplacedAndTheOthersAreLeftAlone)
{
    Document document;
    katana::entity::SurveyMap map;
    add(map, lineRule("KB*", "SURVEY KERB", "Kerb"));
    document.setSurveyMap(map);
    const EntityId second = [&] {
        addPoint(document, Point2(0.0, 0.0), "KB01", "1");
        return addPoint(document, Point2(10.0, 0.0), "KB01", "2");
    }();
    addPoint(document, Point2(0.0, 5.0), "KB02", "3");
    const EntityId fourth = addPoint(document, Point2(10.0, 5.0), "KB02", "4");

    auto first = katana::cad::processLinework(document, LineworkOptions{});
    ASSERT_TRUE(first.ok()) << first.error().describe();
    ASSERT_TRUE(document.execute(std::move(first->command)).ok());
    const std::vector<EntityId> drawn = document.lastCreatedEntities();
    ASSERT_EQ(drawn.size(), 2u); // KB01, then KB02
    // The person puts the KB01 line on a layer of their own.
    katana::entity::Layer own;
    own.name = "checked";
    ASSERT_TRUE(document.execute(cmd::createLayer(own)).ok());
    ASSERT_TRUE(document.execute(cmd::setEntityLayer({drawn[0]}, "checked")).ok());
    const Entity kerbTwoBefore = *document.model().entities.find(drawn[1]);

    // Point 2 moves to (10, 2), and point 4 goes: KB02 is one point, no line.
    ASSERT_TRUE(
        document.execute(cmd::moveEntities({second}, katana::geometry::Vec2(0.0, 2.0))).ok());
    ASSERT_TRUE(document.execute(cmd::deleteEntities({fourth})).ok());

    LineworkOptions again;
    again.earlierLines = drawn;
    auto planned = katana::cad::processLinework(document, again);
    ASSERT_TRUE(planned.ok()) << planned.error().describe();
    ASSERT_EQ(planned->report.strings.size(), 1u);
    EXPECT_EQ(planned->report.strings[0].name, "KB01");
    EXPECT_EQ(planned->report.strings[0].redrawn, drawn[0]);
    EXPECT_EQ(planned->report.strings[0].layer, "checked") << "the layer it is on, not the rule's";
    EXPECT_TRUE(planned->report.layersCreated.empty());
    EXPECT_EQ(planned->report.styling, nullptr) << "no line is created, so none is styled";
    ASSERT_NE(planned->command, nullptr);
    ASSERT_TRUE(document.execute(std::move(planned->command)).ok());

    // The same entity, on the person's layer in the style it had, through
    // where the points now are - and nothing was created.
    EXPECT_TRUE(document.lastCreatedEntities().empty());
    const Entity* kerbOne = document.model().entities.find(drawn[0]);
    ASSERT_NE(kerbOne, nullptr);
    EXPECT_EQ(verticesOf(*kerbOne), (std::vector<Point2>{Point2(0, 0), Point2(10, 2)}));
    EXPECT_EQ(kerbOne->layer, "checked");
    EXPECT_EQ(kerbOne->style, "Kerb");
    EXPECT_EQ(linesOf(document).size(), 2u);
    // The KB02 line, which no line planned now is, is not this run's to touch.
    EXPECT_EQ(*document.model().entities.find(drawn[1]), kerbTwoBefore);

    // Asked again with nothing moved, the line already runs there: nothing to
    // do, and it is still named as the line of that string.
    auto unchanged = katana::cad::processLinework(document, again);
    ASSERT_TRUE(unchanged.ok()) << unchanged.error().describe();
    EXPECT_EQ(unchanged->command, nullptr);
    ASSERT_EQ(unchanged->report.strings.size(), 1u);
    EXPECT_EQ(unchanged->report.strings[0].redrawn, drawn[0]);

    // One undo of the redraw puts the line back where it ran.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(verticesOf(*document.model().entities.find(drawn[0])),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
}
