// Field-to-finish linework: joining coded survey points into lines.
//
// Every expected value here is worked by hand from the points the test
// builds; the working is in the comment beside it.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/linework.hpp"
#include "katana/commands/entity_commands.hpp"

namespace cmd = katana::commands;
using katana::cad::Document;
using katana::cad::LineworkCodes;
using katana::cad::LineworkControl;
using katana::cad::LineworkNoteKind;
using katana::cad::LineworkOptions;
using katana::cad::LineworkOrder;
using katana::cad::LineworkResult;
using katana::cad::UnplacedReason;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyRule;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

// Shaped like the reference mapfile's rules, with invented names: a water
// main and a kerb are lines, a tree is a point.
katana::entity::SurveyMap lineAndPointMap()
{
    katana::entity::SurveyMap map;
    SurveyRule water;
    water.key = "WM*";
    water.model = "SURVEY SERVICES";
    water.colour = "blue";
    water.linestyle = "WATR Main";
    water.breakline = SurveyBreakline::Line;
    EXPECT_TRUE(map.add(water).ok());

    SurveyRule kerb;
    kerb.key = "KB*";
    kerb.model = "SURVEY KERB";
    kerb.breakline = SurveyBreakline::Line;
    EXPECT_TRUE(map.add(kerb).ok());

    SurveyRule tree;
    tree.key = "TR*";
    tree.model = "SURVEY DETAIL";
    tree.breakline = SurveyBreakline::Point;
    EXPECT_TRUE(map.add(tree).ok());
    return map;
}

// A surveyed point as survey import writes one: its code in "code", its
// number in "point", its height in "elevation" when it has one.
EntityId addPoint(Document& document, const Point2& at, const std::string& code,
                  const std::string& number, std::optional<double> height = std::nullopt)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{at};
    if (!code.empty()) {
        entity.properties.insert_or_assign("code", katana::entity::PropertyValue(code));
    }
    if (!number.empty()) {
        entity.properties.insert_or_assign("point", katana::entity::PropertyValue(number));
    }
    if (height) {
        katana::entity::setHeights(entity.properties, {height});
    }
    EXPECT_TRUE(document.execute(cmd::createEntities({entity})).ok());
    const auto created = document.lastCreatedEntities();
    EXPECT_EQ(created.size(), 1u);
    return created.empty() ? katana::entity::kInvalidEntityId : created.front();
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

const Polyline2& shapeOf(const Entity& entity)
{
    return std::get<Polyline2>(entity.geometry);
}

// Plans and executes, failing the test on an error.
LineworkResult run(Document& document, const LineworkOptions& options = {})
{
    auto planned = katana::cad::processLinework(document, options);
    EXPECT_TRUE(planned.ok()) << (planned.ok() ? "" : planned.error().describe());
    if (!planned.ok()) {
        return {};
    }
    LineworkResult result = std::move(*planned);
    if (result.command) {
        EXPECT_TRUE(document.execute(std::move(result.command)).ok());
    }
    return result;
}

bool hasNote(const katana::cad::LineworkReport& report, LineworkNoteKind kind,
             const std::string& detail = {})
{
    for (const auto& note : report.notes) {
        if (note.kind == kind && (detail.empty() || note.detail == detail)) {
            return true;
        }
    }
    return false;
}

// The notes of one kind, in the order made.
std::vector<katana::cad::LineworkNote> notesOf(const katana::cad::LineworkReport& report,
                                               LineworkNoteKind kind)
{
    std::vector<katana::cad::LineworkNote> found;
    for (const auto& note : report.notes) {
        if (note.kind == kind) {
            found.push_back(note);
        }
    }
    return found;
}

} // namespace

// ---- the string-name rule -----------------------------------------------------------

TEST(StringName, APrefixKeyLeavesTheRemainderAsTheStringNumber)
{
    katana::entity::SurveyMap map;
    SurveyRule wide;
    wide.key = "W*";
    wide.model = "WIDE";
    ASSERT_TRUE(map.add(wide).ok());
    SurveyRule water;
    water.key = "WM*";
    water.model = "WATER";
    ASSERT_TRUE(map.add(water).ok());

    // WM* (prefix "WM", 2 characters) is longer than W*, so it decides: "WM01"
    // is "WM" + "01".
    const auto split = katana::cad::splitStringName(map, "WM01");
    EXPECT_TRUE(split.matched);
    EXPECT_FALSE(split.fallbackOnly);
    EXPECT_EQ(split.key, "WM*");
    EXPECT_EQ(split.number, "01");

    // Only W* matches "WX7": "W" + "X7".
    EXPECT_EQ(katana::cad::splitStringName(map, "WX7").number, "X7");
}

TEST(StringName, ACodeThatIsExactlyItsKeyHasNoNumberAndSoIsOneStringPerCode)
{
    katana::entity::SurveyMap map;
    SurveyRule water;
    water.key = "WM*";
    water.model = "WATER";
    ASSERT_TRUE(map.add(water).ok());
    SurveyRule exact;
    exact.key = "PABB";
    exact.model = "DETAIL";
    ASSERT_TRUE(map.add(exact).ok());

    const auto bare = katana::cad::splitStringName(map, "WM");
    EXPECT_EQ(bare.key, "WM*");
    EXPECT_EQ(bare.number, "") << "\"WM\" under WM* leaves nothing over";

    const auto exactSplit = katana::cad::splitStringName(map, "PABB");
    EXPECT_EQ(exactSplit.key, "PABB");
    EXPECT_EQ(exactSplit.number, "") << "an exact key is the whole code";
}

TEST(StringName, ACodeOnlyTheFallbackMatchesIsItsOwnKeyAndSaidToBeFallbackOnly)
{
    katana::entity::SurveyMap map;
    SurveyRule anything;
    anything.key = "*";
    anything.breakline = SurveyBreakline::Line;
    ASSERT_TRUE(map.add(anything).ok());

    const auto split = katana::cad::splitStringName(map, "ZZ9");
    EXPECT_TRUE(split.matched);
    EXPECT_TRUE(split.fallbackOnly);
    EXPECT_EQ(split.key, "ZZ9") << "not \"*\" with a string number of \"ZZ9\"";
    EXPECT_EQ(split.number, "");

    const auto none = katana::cad::splitStringName(katana::entity::SurveyMap{}, "ZZ9");
    EXPECT_FALSE(none.matched);
    EXPECT_EQ(none.key, "");
}

// ---- control codes -----------------------------------------------------------------

TEST(FieldCode, TheFirstTokenIsTheNameAndTheRestAreControls)
{
    const LineworkCodes codes;
    const auto start = katana::cad::parseFieldCode("KB1 ST", codes);
    EXPECT_EQ(start.name, "KB1");
    EXPECT_EQ(start.controls, std::vector<LineworkControl>{LineworkControl::Start});

    const auto close = katana::cad::parseFieldCode("  FL   CL ", codes);
    EXPECT_EQ(close.name, "FL") << "blanks around and between tokens are not tokens";
    EXPECT_EQ(close.controls, std::vector<LineworkControl>{LineworkControl::Close});

    const auto named = katana::cad::parseFieldCode("CL", codes);
    EXPECT_EQ(named.name, "CL") << "the first token is the name even spelled like a control";
    EXPECT_TRUE(named.controls.empty());
}

TEST(FieldCode, ControlsAreMatchedIgnoringCaseButTheNameKeepsItsCase)
{
    const auto parsed = katana::cad::parseFieldCode("kb1 st bc", LineworkCodes{});
    EXPECT_EQ(parsed.name, "kb1");
    EXPECT_EQ(parsed.controls,
              (std::vector<LineworkControl>{LineworkControl::Start, LineworkControl::ArcStart}));
}

TEST(FieldCode, AnUnknownTokenIsKeptToBeReportedNotDropped)
{
    const auto parsed = katana::cad::parseFieldCode("KB1 XX END 12", LineworkCodes{});
    EXPECT_EQ(parsed.controls, std::vector<LineworkControl>{LineworkControl::End});
    EXPECT_EQ(parsed.unknownTokens, (std::vector<std::string>{"XX", "12"}));
}

TEST(FieldCode, AJoinTakesThePointNumberAfterItAndSaysWhenThereIsNone)
{
    const auto joined = katana::cad::parseFieldCode("FN3 JPN 105", LineworkCodes{});
    EXPECT_EQ(joined.joinTo, "105");
    EXPECT_FALSE(joined.joinWithoutTarget);
    EXPECT_TRUE(joined.unknownTokens.empty()) << "105 is the join's, not unknown";

    const auto forgotten = katana::cad::parseFieldCode("FN3 JPN CL", LineworkCodes{});
    EXPECT_TRUE(forgotten.joinWithoutTarget) << "CL is a control, not a point called CL";
    EXPECT_TRUE(forgotten.has(LineworkControl::Close));
}

TEST(FieldCode, TheSpellingsAreDataAndAnEmptyOneSwitchesThatControlOff)
{
    LineworkCodes codes;
    codes.start = "B";
    codes.close = "";
    const auto parsed = katana::cad::parseFieldCode("KB1 B CL", codes);
    EXPECT_EQ(parsed.controls, std::vector<LineworkControl>{LineworkControl::Start});
    EXPECT_EQ(parsed.unknownTokens, std::vector<std::string>{"CL"});
}

TEST(LineworkCodes, AmbiguousOrUnmatchableSpellingsAreRefused)
{
    EXPECT_TRUE(katana::cad::validate(LineworkCodes{}).ok()) << "the defaults are sound";

    LineworkCodes same;
    same.end = "st"; // "ST" is start, and tokens are matched ignoring case
    EXPECT_FALSE(katana::cad::validate(same).ok());

    LineworkCodes blank;
    blank.close = "C L";
    EXPECT_FALSE(katana::cad::validate(blank).ok());

    LineworkCodes off;
    off.arcStart = "";
    off.arcEnd = "";
    EXPECT_TRUE(katana::cad::validate(off).ok()) << "two switched-off controls are not a clash";

    Document document;
    LineworkOptions options;
    options.codes = same;
    EXPECT_FALSE(katana::cad::processLinework(document, options).ok());
}

// ---- joining ------------------------------------------------------------------------

TEST(Linework, TwoStringsOfOneCodeMakeTwoLinesOnTheRulesLayerInTheStyleCodesWouldGive)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "WM01", "1");
    addPoint(document, Point2(0, 10), "WM02", "2");
    addPoint(document, Point2(10, 0), "WM01", "3");
    addPoint(document, Point2(10, 10), "WM02", "4");
    addPoint(document, Point2(20, 0), "WM01", "5");

    const LineworkResult result = run(document);
    const auto& report = result.report;
    EXPECT_EQ(report.property, "code") << "found, as applySurveyCodes finds it";
    ASSERT_EQ(report.strings.size(), 2u);
    // WM01 is points 1, 3, 5; WM02 is 2, 4. Names in order.
    EXPECT_EQ(report.strings[0].name, "WM01");
    EXPECT_EQ(report.strings[0].key, "WM*");
    EXPECT_EQ(report.strings[0].number, "01");
    EXPECT_EQ(report.strings[0].pointNumbers, (std::vector<std::string>{"1", "3", "5"}));
    EXPECT_FALSE(report.strings[0].closed);
    EXPECT_EQ(report.strings[1].name, "WM02");
    EXPECT_EQ(report.strings[1].pointNumbers, (std::vector<std::string>{"2", "4"}));
    EXPECT_TRUE(report.unplaced.empty());
    EXPECT_EQ(report.layersCreated, std::vector<std::string>{"SURVEY SERVICES"});

    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(shapeOf(*lines[0]).vertices,
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(20, 0)}));
    EXPECT_EQ(shapeOf(*lines[1]).vertices, (std::vector<Point2>{Point2(0, 10), Point2(10, 10)}));
    for (const Entity* line : lines) {
        EXPECT_EQ(line->layer, "SURVEY SERVICES") << "a rule's model is Katana's layer";
        // applySurveyCodes names a style after the rule's linestyle.
        EXPECT_EQ(line->style, "WATR Main");
    }
    EXPECT_EQ(std::get<std::string>(lines[0]->properties.at("code")), "WM01")
        << "the line carries its name, so codes apply to it again";

    ASSERT_NE(report.styling, nullptr);
    EXPECT_EQ(report.styling->matched, 2u) << "filled when the command ran";
    EXPECT_EQ(report.styling->stylesCreated, std::vector<std::string>{"WATR Main"});
}

TEST(Linework, StylingTouchesOnlyTheLinesNotTheOtherCodedPointsInTheDrawing)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "WM01", "1");
    addPoint(document, Point2(10, 0), "WM01", "2");
    const EntityId lone = addPoint(document, Point2(50, 50), "WM07", "3");

    run(document);
    const Entity* point = document.model().entities.find(lone);
    ASSERT_NE(point, nullptr);
    EXPECT_EQ(point->layer, "0") << "applying codes to the lines is not applying them to all";
    EXPECT_EQ(point->style, "");
}

TEST(Linework, StartAndEndSplitOneStringIntoSeveralLines)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    // END on 3 ends the first line; 4 begins another without being told.
    addPoint(document, Point2(0, 0), "KB1", "1");
    addPoint(document, Point2(1, 0), "KB1", "2");
    addPoint(document, Point2(2, 0), "KB1 END", "3");
    addPoint(document, Point2(3, 0), "KB1", "4");
    addPoint(document, Point2(4, 0), "KB1", "5");
    // ST on 8 begins a new line, so 6-7 is one and 8-9 another.
    addPoint(document, Point2(0, 5), "KB2", "6");
    addPoint(document, Point2(1, 5), "KB2", "7");
    addPoint(document, Point2(2, 5), "KB2 ST", "8");
    addPoint(document, Point2(3, 5), "KB2", "9");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 4u);
    EXPECT_EQ(report.strings[0].pointNumbers, (std::vector<std::string>{"1", "2", "3"}));
    EXPECT_EQ(report.strings[1].pointNumbers, (std::vector<std::string>{"4", "5"}));
    EXPECT_EQ(report.strings[2].pointNumbers, (std::vector<std::string>{"6", "7"}));
    EXPECT_EQ(report.strings[3].pointNumbers, (std::vector<std::string>{"8", "9"}));
    EXPECT_EQ(polylinesOf(document).size(), 4u);
}

TEST(Linework, CloseClosesTheLineBackToItsFirstPoint)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "KB1", "1");
    addPoint(document, Point2(10, 0), "KB1", "2");
    addPoint(document, Point2(10, 10), "KB1", "3");
    addPoint(document, Point2(0, 10), "KB1 CL", "4");
    addPoint(document, Point2(50, 0), "KB1", "5"); // after CL: a new line, alone
    addPoint(document, Point2(60, 0), "KB1", "6");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 2u);
    EXPECT_TRUE(report.strings[0].closed);
    EXPECT_FALSE(report.strings[1].closed);
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_TRUE(shapeOf(*lines[0]).closed);
    // Four vertices; the closing side is the polyline's own, not a fifth.
    EXPECT_EQ(shapeOf(*lines[0]).vertices.size(), 4u);
    // 10 + 10 + 10 + 10 around the square.
    EXPECT_DOUBLE_EQ(shapeOf(*lines[0]).length(), 40.0);
}

TEST(Linework, ALonePointMakesNoLineAndIsReported)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    const EntityId lone = addPoint(document, Point2(0, 0), "WM03", "7");

    auto planned = katana::cad::processLinework(document, {});
    ASSERT_TRUE(planned.ok());
    EXPECT_EQ(planned->command, nullptr) << "nothing to build is not an error";
    ASSERT_EQ(planned->report.unplaced.size(), 1u);
    EXPECT_EQ(planned->report.unplaced[0].id, lone);
    EXPECT_EQ(planned->report.unplaced[0].reason, UnplacedReason::LonePoint);
    EXPECT_EQ(planned->report.unplaced[0].pointNumber, "7");
    EXPECT_EQ(planned->report.unplaced[0].code, "WM03");
    // WM* names model SURVEY SERVICES, which the drawing lacks - but no line
    // goes on it, so no command creates it, and the report must not say one does.
    EXPECT_TRUE(planned->report.layersCreated.empty());
}

TEST(Linework, PointCodesAreLeftAloneAndEachIsReportedWithWhy)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    const EntityId tree1 = addPoint(document, Point2(0, 0), "TR01", "1");
    const EntityId tree2 = addPoint(document, Point2(5, 0), "TR01", "2");
    const EntityId stranger = addPoint(document, Point2(9, 0), "QQ", "3");
    const EntityId uncoded = addPoint(document, Point2(12, 0), "", "4");

    auto planned = katana::cad::processLinework(document, {});
    ASSERT_TRUE(planned.ok());
    EXPECT_EQ(planned->command, nullptr);
    const auto& unplaced = planned->report.unplaced;
    ASSERT_EQ(unplaced.size(), 4u) << "every point, in entity order";
    EXPECT_EQ(unplaced[0].id, tree1);
    EXPECT_EQ(unplaced[0].reason, UnplacedReason::PointCode);
    EXPECT_EQ(unplaced[1].id, tree2);
    EXPECT_EQ(unplaced[1].reason, UnplacedReason::PointCode);
    EXPECT_EQ(unplaced[2].id, stranger);
    EXPECT_EQ(unplaced[2].reason, UnplacedReason::NoRule);
    EXPECT_EQ(unplaced[3].id, uncoded);
    EXPECT_EQ(unplaced[3].reason, UnplacedReason::NoCode);
    EXPECT_EQ(planned->report.considered, 4u);
}

TEST(Linework, AControlCodeMakesALineEvenWithoutARuleAndSaysNothingStylesIt)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "XX ST", "1");
    addPoint(document, Point2(4, 0), "XX END", "2");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_EQ(report.strings[0].layer, "0") << "no rule, so the points' own layer";
    EXPECT_TRUE(hasNote(report, LineworkNoteKind::NoRuleForName, "XX"));
}

TEST(Linework, AnUnknownTokenIsReportedAndThePointStillPlaced)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    const EntityId odd = addPoint(document, Point2(0, 0), "KB1 ZIG", "1");
    addPoint(document, Point2(4, 0), "KB1", "2");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_EQ(report.strings[0].points.front(), odd);
    ASSERT_EQ(report.notes.size(), 1u);
    EXPECT_EQ(report.notes[0].kind, LineworkNoteKind::UnknownToken);
    EXPECT_EQ(report.notes[0].id, odd);
    EXPECT_EQ(report.notes[0].detail, "ZIG");
}

// ---- order --------------------------------------------------------------------------

TEST(Linework, PointNumberOrderIsNumericAndEntityOrderIsCreationOrder)
{
    // Created in the order A, B, C, numbered 10, 9, 11.
    auto make = [](Document& document) {
        document.setSurveyMap(lineAndPointMap());
        addPoint(document, Point2(0, 0), "KB1", "10");  // A
        addPoint(document, Point2(5, 5), "KB1", "9");   // B
        addPoint(document, Point2(10, 0), "KB1", "11"); // C
    };

    Document byNumber;
    make(byNumber);
    run(byNumber);
    const auto numbered = polylinesOf(byNumber);
    ASSERT_EQ(numbered.size(), 1u);
    // 9 < 10 < 11 as numbers (as text "10" < "11" < "9"): B, A, C.
    EXPECT_EQ(shapeOf(*numbered[0]).vertices,
              (std::vector<Point2>{Point2(5, 5), Point2(0, 0), Point2(10, 0)}));

    Document byEntity;
    make(byEntity);
    LineworkOptions options;
    options.order = LineworkOrder::EntityOrder;
    run(byEntity, options);
    const auto created = polylinesOf(byEntity);
    ASSERT_EQ(created.size(), 1u);
    EXPECT_EQ(shapeOf(*created[0]).vertices,
              (std::vector<Point2>{Point2(0, 0), Point2(5, 5), Point2(10, 0)}));
}

TEST(Linework, OrderingByNumberReportsAPointWithoutOne)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "KB1", "1");
    addPoint(document, Point2(4, 0), "KB1", "2");
    const EntityId nameless = addPoint(document, Point2(8, 0), "KB1", "");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_EQ(report.strings[0].points.size(), 2u);
    ASSERT_EQ(report.unplaced.size(), 1u);
    EXPECT_EQ(report.unplaced[0].id, nameless);
    EXPECT_EQ(report.unplaced[0].reason, UnplacedReason::NoPointNumber);
}

TEST(Linework, OnlyTheSelectedEntitiesAreProcessed)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    const EntityId a = addPoint(document, Point2(0, 0), "KB1", "1");
    const EntityId b = addPoint(document, Point2(4, 0), "KB1", "2");
    addPoint(document, Point2(8, 0), "KB1", "3");

    LineworkOptions options;
    options.ids = {b, a};
    const auto report = run(document, options).report;
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_EQ(report.strings[0].pointNumbers, (std::vector<std::string>{"1", "2"}));
    EXPECT_EQ(report.considered, 2u);
}

// ---- geometry the model can carry ------------------------------------------------

TEST(Linework, HeightsAreKeptPerVertexAndAMissingOneStaysMissing)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "KB1", "1", 10.5);
    addPoint(document, Point2(4, 0), "KB1", "2");
    addPoint(document, Point2(8, 0), "KB1", "3", 11.25);

    run(document);
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    const auto heights = katana::entity::heightsOf(lines[0]->properties, 3);
    ASSERT_EQ(heights.size(), 3u);
    EXPECT_EQ(heights[0], std::optional<double>(10.5));
    EXPECT_EQ(heights[1], std::nullopt) << "absent is not zero";
    EXPECT_EQ(heights[2], std::optional<double>(11.25));
}

TEST(Linework, ACurveIsChordedWithinTheToleranceThroughItsSurveyedPoints)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    // A half circle of radius 10 about the origin, anticlockwise.
    addPoint(document, Point2(10, 0), "KB1 BC", "1");
    addPoint(document, Point2(0, 10), "KB1", "2");
    addPoint(document, Point2(-10, 0), "KB1 EC", "3");

    LineworkOptions options;
    // Largest step t = 2 acos(1 - 0.35/10) = 2 acos(0.965) = 2 x 15.2 deg
    // = 30.4 deg. Each quarter (90 deg) needs ceil(90 / 30.4) = 3 chords of
    // 30 deg, whose sagitta is 10 (1 - cos 15 deg) = 0.341, within 0.35.
    options.chordTolerance = 0.35;
    const auto report = run(document, options).report;
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_EQ(report.strings[0].curves, 1u);
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    const auto& vertices = shapeOf(*lines[0]).vertices;
    // 1 + 3 + 3: vertices at 0, 30, 60, ... 180 degrees.
    ASSERT_EQ(vertices.size(), 7u);
    for (std::size_t i = 0; i < vertices.size(); ++i) {
        const double angle = static_cast<double>(i) * katana::math::kPi / 6.0;
        EXPECT_NEAR(vertices[i].x, 10.0 * std::cos(angle), 1e-9) << "vertex " << i;
        EXPECT_NEAR(vertices[i].y, 10.0 * std::sin(angle), 1e-9) << "vertex " << i;
    }
    EXPECT_EQ(vertices[3], Point2(0, 10)) << "the surveyed point is a vertex, exactly";
}

TEST(Linework, TheDefaultToleranceChordsAQuarterOfATenMetreRadiusIntoTwentyFive)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(10, 0), "KB1 BC", "1");
    addPoint(document, Point2(0, 10), "KB1", "2");
    addPoint(document, Point2(-10, 0), "KB1 EC", "3");

    run(document);
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    // t = 2 acos(1 - 0.005/10) = 2 acos(0.9995) = 2 x 0.031624 = 0.063248;
    // a quarter is 1.570796 / 0.063248 = 24.84, so 25 chords: 1 + 25 + 25.
    const auto& vertices = shapeOf(*lines[0]).vertices;
    EXPECT_EQ(vertices.size(), 51u);
    // Each chord spans 90/25 = 3.6 deg; its sagitta 10 (1 - cos 1.8 deg)
    // = 0.00493, within 5 mm.
    for (std::size_t i = 1; i < vertices.size(); ++i) {
        const Point2 middle = (vertices[i - 1] + vertices[i]) * 0.5;
        EXPECT_LE(10.0 - middle.length(), 0.005) << "chord " << i;
    }
}

TEST(Linework, ACurveWithNothingBetweenItsEndsIsDrawnStraightAndSaidSo)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "KB1 BC", "1");
    addPoint(document, Point2(10, 0), "KB1 EC", "2");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_EQ(report.strings[0].curves, 0u);
    EXPECT_EQ(report.strings[0].vertices, 2u);
    EXPECT_TRUE(hasNote(report, LineworkNoteKind::CurveTooShort));
}

TEST(Linework, ThreePointsAndRectangleMakeTheRectangleTheyDescribe)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "KB1", "1", 1.0);
    addPoint(document, Point2(10, 0), "KB1", "2", 2.0);
    addPoint(document, Point2(9, 4), "KB1 RECT", "3", 3.0);

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_TRUE(report.strings[0].rectangle);
    EXPECT_TRUE(report.strings[0].closed);
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    // Side (0,0)-(10,0); its left normal is (0,1) and (9,4) is 4 along it.
    EXPECT_EQ(shapeOf(*lines[0]).vertices,
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(10, 4), Point2(0, 4)}));
    EXPECT_TRUE(shapeOf(*lines[0]).closed);
    const auto heights = katana::entity::heightsOf(lines[0]->properties, 4);
    EXPECT_EQ(heights, (std::vector<std::optional<double>>{1.0, 2.0, std::nullopt, std::nullopt}))
        << "the constructed corners were not surveyed";
}

TEST(Linework, AJoinDrawsALineToTheNumberedPointAndPlacesIt)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "KB1", "101");
    addPoint(document, Point2(10, 0), "KB1 JPN 201", "102");
    const EntityId tree = addPoint(document, Point2(10, 5), "TR01", "201");
    addPoint(document, Point2(20, 0), "KB1 JPN 999", "103");

    const auto report = run(document).report;
    // KB1 101-102-103 is one line; 102 JPN 201 is a second.
    ASSERT_EQ(report.strings.size(), 2u);
    EXPECT_FALSE(report.strings[0].join);
    EXPECT_TRUE(report.strings[1].join);
    EXPECT_EQ(report.strings[1].pointNumbers, (std::vector<std::string>{"102", "201"}));
    EXPECT_TRUE(hasNote(report, LineworkNoteKind::JoinTargetMissing, "999"));
    for (const auto& unplaced : report.unplaced) {
        EXPECT_NE(unplaced.id, tree) << "a point a join reached is in a line";
    }
}

TEST(Linework, AJoinWithNoPointNumberIsReportedAndThePointStillPlaced)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    const EntityId forgot = addPoint(document, Point2(0, 0), "KB1 JPN", "1");
    addPoint(document, Point2(10, 0), "KB1", "2");

    const auto report = run(document).report;
    // KB1 1-2 is the one line; the join has no target, so no second line.
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_FALSE(report.strings[0].join);
    EXPECT_EQ(report.strings[0].points.front(), forgot);
    const auto notes = notesOf(report, LineworkNoteKind::JoinWithoutTarget);
    ASSERT_EQ(notes.size(), 1u);
    EXPECT_EQ(notes[0].id, forgot);
    EXPECT_EQ(notes[0].pointNumber, "1");
}

TEST(Linework, WithKeepPointsOffAJoinTargetNoRunPlacedIsKept)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    const EntityId first = addPoint(document, Point2(0, 0), "WM01", "1");
    const EntityId second = addPoint(document, Point2(10, 0), "WM01 JPN 3", "2");
    const EntityId tree = addPoint(document, Point2(10, 5), "TR01", "3");
    const EntityId uncoded = addPoint(document, Point2(20, 5), "", "4");
    const EntityId third = addPoint(document, Point2(20, 0), "WM01 JPN 4", "5");

    LineworkOptions options;
    options.keepPoints = false;
    const auto report = run(document, options).report;
    // WM01 is the run 1-2-5; 2 JPN 3 and 5 JPN 4 are two joins: three lines.
    ASSERT_EQ(report.strings.size(), 3u);
    // Only the run's points 1, 2 and 5 are replaced by lines. The tree (a
    // point code) and the uncoded point were only reached by joins: no line
    // of their own stands for them, so deleting them would lose a tree and
    // an uncoded point outright.
    EXPECT_EQ(report.pointsRemoved, 3u);
    EXPECT_EQ(document.model().entities.find(first), nullptr);
    EXPECT_EQ(document.model().entities.find(second), nullptr);
    EXPECT_EQ(document.model().entities.find(third), nullptr);
    const Entity* kept = document.model().entities.find(tree);
    ASSERT_NE(kept, nullptr) << "a point-coded join target is not the command's to delete";
    EXPECT_EQ(std::get<std::string>(kept->properties.at("code")), "TR01");
    EXPECT_EQ(std::get<std::string>(kept->properties.at("point")), "3");
    EXPECT_NE(document.model().entities.find(uncoded), nullptr);
    // Two points and three lines.
    EXPECT_EQ(document.model().entities.size(), 5u);
}

// ---- what could not be drawn as coded ----------------------------------------------

TEST(Linework, ClosingAStringOfTwoPointsDrawsItOpenAndSaysSo)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "KB1", "1");
    const EntityId closer = addPoint(document, Point2(10, 0), "KB1 CL", "2");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 1u);
    // Closed, 1-2-1 would run back over itself and enclose nothing.
    EXPECT_FALSE(report.strings[0].closed);
    EXPECT_EQ(report.strings[0].vertices, 2u);
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_FALSE(shapeOf(*lines[0]).closed);
    const auto notes = notesOf(report, LineworkNoteKind::CloseTooShort);
    ASSERT_EQ(notes.size(), 1u);
    EXPECT_EQ(notes[0].id, closer) << "on the point that asked to close";
}

TEST(Linework, ACurveThroughPointsInAStraightLineIsDrawnStraightAndNotCountedAsACurve)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    const EntityId begin = addPoint(document, Point2(0, 0), "KB1 BC", "1");
    addPoint(document, Point2(5, 0), "KB1", "2");
    addPoint(document, Point2(10, 0), "KB1 EC", "3");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 1u);
    // (0,0), (5,0), (10,0) are on no circle: the three points are joined
    // straight and nothing is chorded, so there is no curve to count.
    EXPECT_EQ(report.strings[0].curves, 0u);
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(shapeOf(*lines[0]).vertices,
              (std::vector<Point2>{Point2(0, 0), Point2(5, 0), Point2(10, 0)}));
    const auto notes = notesOf(report, LineworkNoteKind::CurveCollinear);
    ASSERT_EQ(notes.size(), 1u);
    EXPECT_EQ(notes[0].id, begin);
}

TEST(Linework, ACurveEndWithNoCurveBegunIsReportedAndTheLineDrawnStraight)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    const EntityId ender = addPoint(document, Point2(0, 0), "KB1 EC", "1");
    addPoint(document, Point2(10, 0), "KB1", "2");
    addPoint(document, Point2(10, 10), "KB1", "3");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_EQ(report.strings[0].curves, 0u);
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(shapeOf(*lines[0]).vertices,
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(10, 10)}));
    const auto notes = notesOf(report, LineworkNoteKind::CurveEndWithoutStart);
    ASSERT_EQ(notes.size(), 1u);
    EXPECT_EQ(notes[0].id, ender);
}

TEST(Linework, ACurveNeverEndedIsCurvedToTheLastPointAndTheNoteSaysWhatWasDone)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    // The half circle of radius 10 of the chording test, with its EC forgotten.
    const EntityId begin = addPoint(document, Point2(10, 0), "KB1 BC", "1");
    addPoint(document, Point2(0, 10), "KB1", "2");
    addPoint(document, Point2(-10, 0), "KB1", "3");
    // And a curve begun on a string's last point: there is nothing after it
    // to curve through.
    addPoint(document, Point2(0, 20), "KB2", "4");
    const EntityId late = addPoint(document, Point2(10, 20), "KB2 BC", "5");

    LineworkOptions options;
    options.chordTolerance = 0.35; // 30 degree chords, as worked in that test
    const auto report = run(document, options).report;
    ASSERT_EQ(report.strings.size(), 2u);
    // KB1: curved to its last point as if EC were on it - 1 + 3 + 3 vertices.
    EXPECT_EQ(report.strings[0].curves, 1u);
    EXPECT_EQ(report.strings[0].vertices, 7u);
    // KB2: 4-5 straight, no curve.
    EXPECT_EQ(report.strings[1].curves, 0u);
    EXPECT_EQ(report.strings[1].vertices, 2u);

    const auto notes = notesOf(report, LineworkNoteKind::CurveUnterminated);
    ASSERT_EQ(notes.size(), 2u);
    EXPECT_EQ(notes[0].id, begin);
    EXPECT_EQ(notes[0].detail, "taken to end at the string's last point");
    EXPECT_EQ(notes[1].id, late);
    EXPECT_EQ(notes[1].detail, "begun on the string's last point: nothing curved");
}

TEST(Linework, RectangleOnOtherThanThreePointsWithWidthSaysHowThePointsWereDrawnInstead)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    // KB1: four points - no rectangle to construct, drawn closed through them.
    addPoint(document, Point2(0, 0), "KB1", "1");
    addPoint(document, Point2(10, 0), "KB1", "2");
    addPoint(document, Point2(10, 5), "KB1", "3");
    const EntityId four = addPoint(document, Point2(0, 5), "KB1 RECT", "4");
    // KB2: two points - drawn open, since two points close on nothing.
    addPoint(document, Point2(0, 20), "KB2", "5");
    const EntityId two = addPoint(document, Point2(10, 20), "KB2 RECT", "6");
    // KB3: three points, the third on the first side's line - no width, so
    // drawn open. Side (0,40)-(10,40) has unit normal (0,1), and (5,40) is
    // (0,1).(5,0) = 0 from it.
    addPoint(document, Point2(0, 40), "KB3", "7");
    addPoint(document, Point2(10, 40), "KB3", "8");
    const EntityId flat = addPoint(document, Point2(5, 40), "KB3 RECT", "9");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 3u);
    for (const auto& built : report.strings) {
        EXPECT_FALSE(built.rectangle) << built.name;
    }
    EXPECT_TRUE(report.strings[0].closed);
    EXPECT_EQ(report.strings[0].vertices, 4u);
    EXPECT_FALSE(report.strings[1].closed);
    EXPECT_EQ(report.strings[1].vertices, 2u);
    EXPECT_FALSE(report.strings[2].closed);
    EXPECT_EQ(report.strings[2].vertices, 3u);
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 3u);
    EXPECT_TRUE(shapeOf(*lines[0]).closed) << "the note must say what was drawn";
    EXPECT_FALSE(shapeOf(*lines[1]).closed);
    EXPECT_FALSE(shapeOf(*lines[2]).closed);

    const auto notes = notesOf(report, LineworkNoteKind::RectangleShape);
    ASSERT_EQ(notes.size(), 3u);
    EXPECT_EQ(notes[0].id, four);
    EXPECT_EQ(notes[0].detail, "4 points: drawn closed through them");
    EXPECT_EQ(notes[1].id, two);
    EXPECT_EQ(notes[1].detail, "2 points: drawn open");
    EXPECT_EQ(notes[2].id, flat);
    EXPECT_EQ(notes[2].detail, "no width: drawn open");
    EXPECT_FALSE(hasNote(report, LineworkNoteKind::CloseTooShort))
        << "RECT on two points was never asked to close";
}

TEST(Linework, TwoPointsOfOneStringWithOneNumberAreBothPlacedAndReported)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "KB1", "1");
    const EntityId again = addPoint(document, Point2(5, 0), "KB1", "1");
    addPoint(document, Point2(10, 0), "KB1", "2");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 1u);
    // Equal numbers keep entity order: (0,0), (5,0), then 2 at (10,0).
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(shapeOf(*lines[0]).vertices,
              (std::vector<Point2>{Point2(0, 0), Point2(5, 0), Point2(10, 0)}));
    const auto notes = notesOf(report, LineworkNoteKind::DuplicatePointNumber);
    ASSERT_EQ(notes.size(), 1u);
    EXPECT_EQ(notes[0].id, again) << "on the second of the two";
    EXPECT_EQ(notes[0].detail, "1");
}

TEST(Linework, EntitiesAskedAboutThatAreNotPointsAreCountedNotProcessed)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    const EntityId a = addPoint(document, Point2(0, 0), "KB1", "1");
    const EntityId b = addPoint(document, Point2(10, 0), "KB1", "2");
    Entity other;
    other.geometry = Polyline2{{Point2(0, 50), Point2(10, 50)}, false};
    other.properties.insert_or_assign("code", katana::entity::PropertyValue("KB9"));
    ASSERT_TRUE(document.execute(cmd::createEntities({other})).ok());
    const EntityId line = document.lastCreatedEntities().front();

    LineworkOptions options;
    options.ids = {a, line, b};
    const auto report = run(document, options).report;
    EXPECT_EQ(report.considered, 2u) << "the two points";
    EXPECT_EQ(report.notPoints, 1u) << "the polyline";
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_EQ(report.strings[0].name, "KB1");
    EXPECT_TRUE(report.unplaced.empty()) << "a polyline is not an unplaced point";
    const Entity* untouched = document.model().entities.find(line);
    ASSERT_NE(untouched, nullptr);
    EXPECT_EQ(untouched->layer, "0") << "not a point, so not styled as a line here";
}

// ---- refusals -----------------------------------------------------------------------

TEST(Linework, AChordToleranceThatIsNotAPositiveFiniteDistanceIsRefused)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "KB1", "1");
    addPoint(document, Point2(10, 0), "KB1", "2");

    // Zero would make the chord step zero and the chord count infinite; a
    // negative or NaN one measures nothing; an infinite one allows no chords.
    for (const double tolerance :
         {0.0, -1.0, std::numeric_limits<double>::quiet_NaN(),
          std::numeric_limits<double>::infinity()}) {
        LineworkOptions options;
        options.chordTolerance = tolerance;
        const auto planned = katana::cad::processLinework(document, options);
        ASSERT_FALSE(planned.ok()) << "tolerance " << tolerance;
        EXPECT_EQ(planned.error().code, katana::core::ErrorCode::InvalidArgument);
    }
    EXPECT_TRUE(polylinesOf(document).empty());
}

TEST(Linework, ARuleModelThatIsNotALayerNameIsRefusedNotDrawnOnAnotherLayer)
{
    // The survey map itself now refuses a rule whose model is not a layer
    // path (SurveyMap::add validates it), so such a rule can never reach
    // linework. processLinework's own check stays behind as a second guard;
    // what this pins is the property: the kerb is not drawn anywhere.
    Document document;
    katana::entity::SurveyMap map;
    SurveyRule kerb;
    kerb.key = "KB*";
    kerb.model = "a//b"; // an empty level: validateLayerPath refuses it
    kerb.breakline = SurveyBreakline::Line;
    const auto added = map.add(kerb);
    ASSERT_FALSE(added.ok()) << "the map takes no rule it could not draw on a layer";
    EXPECT_EQ(added.error().code, katana::core::ErrorCode::InvalidArgument);
    document.setSurveyMap(std::move(map));
    addPoint(document, Point2(0, 0), "KB1", "1");
    addPoint(document, Point2(10, 0), "KB1", "2");

    const auto result = run(document);
    EXPECT_EQ(result.command, nullptr) << "no rule, so no line on any layer";
    ASSERT_EQ(result.report.unplaced.size(), 2u);
    EXPECT_EQ(result.report.unplaced[0].reason, UnplacedReason::NoRule);
    EXPECT_EQ(result.report.unplaced[1].reason, UnplacedReason::NoRule);
    EXPECT_TRUE(polylinesOf(document).empty());
}

// ---- one command --------------------------------------------------------------------

TEST(Linework, OneUndoRemovesTheLinesTheirLayerTheirStyleAndRestoresThePoints)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    addPoint(document, Point2(0, 0), "WM01", "1");
    addPoint(document, Point2(10, 0), "WM01", "2");
    addPoint(document, Point2(20, 0), "WM01", "3");
    const auto before = document.model().entities.ids();

    LineworkOptions options;
    options.keepPoints = false;
    const auto report = run(document, options).report;
    EXPECT_EQ(report.pointsRemoved, 3u);
    ASSERT_EQ(document.model().entities.size(), 1u) << "the line replaced its points";
    const EntityId line = document.model().entities.ids().front();
    EXPECT_TRUE(document.model().layers.contains("SURVEY SERVICES"));
    EXPECT_TRUE(document.model().styles.contains("WATR Main"));

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().entities.ids(), before) << "the same points, the same ids";
    EXPECT_FALSE(document.model().layers.contains("SURVEY SERVICES"));
    EXPECT_FALSE(document.model().styles.contains("WATR Main"));
    EXPECT_FALSE(document.history().canUndo() &&
                 document.history().undoName() == "PROCESS_LINEWORK");

    ASSERT_TRUE(document.redo().ok());
    ASSERT_EQ(document.model().entities.size(), 1u);
    const Entity* redone = document.model().entities.find(line);
    ASSERT_NE(redone, nullptr) << "redo brings the line back under its id";
    EXPECT_EQ(redone->style, "WATR Main");
}

TEST(Linework, NothingToDoIsNoCommandAndNoError)
{
    Document empty;
    auto planned = katana::cad::processLinework(empty, {});
    ASSERT_TRUE(planned.ok());
    EXPECT_EQ(planned->command, nullptr);
    EXPECT_TRUE(planned->report.strings.empty());
    EXPECT_EQ(planned->report.considered, 0u);
}

TEST(Linework, TheCommandRefusesToRunOnAnotherDocument)
{
    Document planned;
    planned.setSurveyMap(lineAndPointMap());
    addPoint(planned, Point2(0, 0), "WM01", "1");
    addPoint(planned, Point2(10, 0), "WM01", "2");
    auto result = katana::cad::processLinework(planned, {});
    ASSERT_TRUE(result.ok());
    ASSERT_NE(result->command, nullptr);

    Document other;
    EXPECT_FALSE(other.execute(std::move(result->command)).ok());
    EXPECT_TRUE(polylinesOf(other).empty()) << "and leaves nothing behind";
    EXPECT_FALSE(other.model().layers.contains("SURVEY SERVICES"));
}

TEST(Linework, ALineOnlyTheFallbackRuleMakesIsReportedAsSuch)
{
    Document document;
    katana::entity::SurveyMap map;
    SurveyRule anything;
    anything.key = "*";
    anything.breakline = SurveyBreakline::Line;
    ASSERT_TRUE(map.add(anything).ok());
    document.setSurveyMap(std::move(map));
    addPoint(document, Point2(0, 0), "ZZ9", "1");
    addPoint(document, Point2(4, 0), "ZZ9", "2");

    const auto report = run(document).report;
    ASSERT_EQ(report.strings.size(), 1u);
    EXPECT_EQ(report.strings[0].key, "ZZ9");
    EXPECT_TRUE(hasNote(report, LineworkNoteKind::FallbackOnlyName, "ZZ9"));
}

TEST(Linework, PointsInOnePlaceMakeNoLineAndAreReported)
{
    Document document;
    document.setSurveyMap(lineAndPointMap());
    const EntityId a = addPoint(document, Point2(3, 3), "KB1", "1");
    const EntityId b = addPoint(document, Point2(3, 3), "KB1", "2");

    auto planned = katana::cad::processLinework(document, {});
    ASSERT_TRUE(planned.ok());
    EXPECT_EQ(planned->command, nullptr);
    ASSERT_EQ(planned->report.unplaced.size(), 2u);
    EXPECT_EQ(planned->report.unplaced[0].id, a);
    EXPECT_EQ(planned->report.unplaced[0].reason, UnplacedReason::Coincident);
    EXPECT_EQ(planned->report.unplaced[1].id, b);
    EXPECT_EQ(planned->report.unplaced[1].code, "KB1");
}
