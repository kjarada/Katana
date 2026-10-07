// CODE (survey_code_verbs.hpp) through CommandInterpreter::run, as the
// window's command line, katana_cli and katana_mcp type it. It was
// katana_cli's own until 2026-09-26, and the window answered "unknown
// command"; in the interpreter every front end has it.
//
// The map is the cli.* tests' own, built rule by rule, by index:
//   0  KT*  feature: layer KATANA TEST, colour red, linestyle "0"
//   1  KX*  feature with no layer - a warning in the lint
//   2  KT*  symbol: the built-in "cross", size 2
// The texts are code_table.hpp's formatters', which tests/cad/customisation/
// test_code_table.cpp pins; what is pinned here is that the verbs reach them,
// what they refuse, what a scope takes, and that CODE is one undo step.
//
// A rule's section and fields are named here by the Katana customisation
// format's words ("feature", "layer", "surface", "at vertices"), which the
// survey code tools show since the survey code file's own (`map_data`,
// `model`, `tinable`, `mode vertex`) left the window. Each expectation holding
// such a word was changed old to new from the table "The words a rule is
// shown by" in docs/customisation.md, not from what a run printed.
//
// CODE LIST and CODE CHECK were a verb of their own, named after that file,
// and a reply of CODE and CODE CENSUS now begins with what its scope took.
// Each expectation those two changed was changed from docs/customisation.md,
// "The verbs" - the same replies under the new words, and one line more,
// whose count is the number of entities the test itself drew.

#include <gtest/gtest.h>

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_code_verbs.hpp"
#include "katana/entity/survey_map.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::LintIssue;
using katana::cad::LintKind;
using katana::cad::LintSeverity;
using katana::core::ErrorCode;
using katana::entity::Color;
using katana::entity::EntityId;
using katana::entity::SurveyMap;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

namespace {

SurveyMap testCodes()
{
    SurveyMap map;
    SurveyRule katana;
    katana.key = "KT*";
    katana.model = "KATANA TEST";
    katana.colour = "red";
    katana.linestyle = "0";
    katana.comment = "[KT*] Katana test";
    EXPECT_TRUE(map.add(katana).ok());
    SurveyRule nowhere;
    nowhere.key = "KX*";
    nowhere.colour = "red";
    EXPECT_TRUE(map.add(nowhere).ok());
    SurveyRule cross;
    cross.key = "KT*";
    cross.section = SurveySection::VertexSymbol;
    cross.symbol = katana::entity::SurveySymbol{"cross", "", 2.0, 0.0, 0.0, 0.0};
    EXPECT_TRUE(map.add(cross).ok());
    return map;
}

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    explicit Session(bool loaded = true)
    {
        if (loaded) {
            document.setSurveyMap(testCodes());
        }
    }

    std::string ok(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> "
                                << (reply.ok() ? std::string{} : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    katana::core::Error refused(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was accepted:\n" << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }
    // One coded point, as a survey import leaves it.
    void codedPoint()
    {
        ok("POINT 1,1");
        ok("SELECT ALL");
        ok("PROP SET code KT01");
    }
    // What `draw` makes, carrying `code` under `property`; its id.
    EntityId coded(const std::string& draw, const std::string& code,
                   const std::string& property = "code")
    {
        ok(draw);
        const std::vector<EntityId> made = document.lastCreatedEntities();
        EXPECT_EQ(made.size(), 1u) << draw;
        const EntityId id = made.empty() ? 0 : made.front();
        ok("SELECT " + std::to_string(id));
        ok("PROP SET " + property + " " + code);
        ok("SELECT NONE");
        return id;
    }
    std::string layerOf(EntityId id) const
    {
        const katana::entity::Entity* entity = document.model().entities.find(id);
        return entity == nullptr ? std::string("(no such entity)") : entity->layer;
    }
};

bool contains(const std::string& text, std::string_view part)
{
    return text.find(part) != std::string::npos;
}

// Three coded entities for the scope tests, by hand:
//   point (1,1)          on layer 0, code KT01
//   point (2,2)          on layer a, code KT02
//   line (10,10)-(15,15) on layer a, code KX01
// so the drawing holds 3, layer a holds 2, and 2 are points. KT* gives a
// layer (KATANA TEST); KX* is a rule and gives none.
struct Scoped : Session {
    EntityId onZero = 0;
    EntityId pointOnA = 0;
    EntityId lineOnA = 0;

    Scoped()
    {
        onZero = coded("POINT 1,1", "KT01");
        ok("LAYER NEW a");
        ok("LAYER SET a");
        pointOnA = coded("POINT 2,2", "KT02");
        lineOnA = coded("LINE 10,10 15,15", "KX01");
        ok("LAYER SET 0");
    }
};

const char* const kCensusOfLayerA = "2 entities carry a code in \"code\", 2 distinct codes\n"
                                    "  KT02: 1, prefix, matched, layer KATANA TEST\n"
                                    "  KX01: 1, prefix, matched";

} // namespace

TEST(SurveyCodeVerbs, AreRefusedByNameUntilSurveyCodesAreLoaded)
{
    Session session(false);
    session.codedPoint();
    const std::size_t steps = session.document.history().undoCount();
    for (const char* line : {"CODE", "code census", "CODE EXPLAIN KT01", "CODE LIST", "CODE CHECK",
                             "CODE DRAWING PREVIEW"}) {
        const katana::core::Error error = session.refused(line);
        EXPECT_EQ(error.code, ErrorCode::InvalidState) << line;
        // The remedy names the file that loads: "use CUSTOMISE <file> first"
        // once meant a survey code file, which is a kind of file that is gone.
        EXPECT_EQ(error.message,
                  "no survey codes are loaded; CUSTOMISE <file> loads a Katana customisation file")
            << line;
    }
    EXPECT_EQ(session.document.history().undoCount(), steps);
    EXPECT_EQ(session.document.model().entities.find(1)->layer, "0");
}

TEST(SurveyCodeVerbs, CodeAppliesTheLoadedCodesAsOneUndoStep)
{
    Session session;
    session.codedPoint();
    const std::size_t steps = session.document.history().undoCount();
    const std::string reply = session.ok("CODE");
    // What the scope took first: no scope word is the drawing, which holds
    // the one point.
    EXPECT_TRUE(reply.starts_with("scope=drawing matched=1\n"
                                  "1 entity carries a code in \"code\": 1 matched, 0 fallback-only"))
        << reply;
    EXPECT_TRUE(contains(reply, "Layers created: KATANA TEST\nStyles created: cross\n")) << reply;
    EXPECT_TRUE(reply.ends_with("Applied as one command. UNDO puts it all back.")) << reply;
    EXPECT_EQ(session.document.model().entities.find(1)->layer, "KATANA TEST");
    EXPECT_EQ(session.document.history().undoCount(), steps + 1);
    // No front end passed a colour: red is a standard name, #FF0000, and the
    // Document knows the standard names itself.
    const katana::entity::Style* style = session.document.model().styles.find("cross");
    ASSERT_NE(style, nullptr);
    ASSERT_TRUE(style->color.has_value());
    EXPECT_EQ(style->color->toHex(), "#FF0000");

    // Again: everything already has what its rule gives.
    EXPECT_TRUE(session.ok("CODE").ends_with("Nothing to change."));
    EXPECT_EQ(session.document.history().undoCount(), steps + 1);

    session.ok("UNDO");
    EXPECT_EQ(session.document.model().entities.find(1)->layer, "0");
    EXPECT_FALSE(session.document.model().layers.contains("KATANA TEST"));
    EXPECT_FALSE(session.document.model().styles.contains("cross"));
}

TEST(SurveyCodeVerbs, AColourNameNothingKnowsIsLeftAloneRatherThanGuessedAt)
{
    // Two names: "red" is standard; "pen 025" is a plot pen, which nothing
    // resolves (docs/customisation.md, "Colour names"). The verb asks the
    // Document and nobody else - it took a lookup of a caller's own, asked
    // last, until 2026-10-07, and this test had a half that passed one.
    SurveyMap map;
    const auto feature = [&map](const char* key, const char* layer, const char* colour) {
        SurveyRule rule;
        rule.key = key;
        rule.model = layer;
        rule.colour = colour;
        rule.linestyle = "0";
        EXPECT_TRUE(map.add(rule).ok());
    };
    feature("RD*", "REDS", "red");
    feature("PN*", "PENS", "pen 025");

    Session session(false);
    session.document.setSurveyMap(map);
    // The standard name resolves; the other is left alone.
    EXPECT_TRUE(contains(session.ok("CODE EXPLAIN RD1"), "colour \"red\": #FF0000"));
    EXPECT_TRUE(contains(session.ok("CODE EXPLAIN PN1"),
                         "colour \"pen 025\": unknown name, so the entity keeps its own"));

    // And a coded entity whose colour nothing knows gets a style with none.
    session.coded("POINT 1,1", "PN1");
    session.ok("CODE");
    bool found = false;
    session.document.model().styles.forEach([&found](const katana::entity::Style& style) {
        if (style.description == "pen 025") {
            found = true;
            EXPECT_FALSE(style.color.has_value()) << style.name;
        }
    });
    EXPECT_TRUE(found) << "the style made for the code keeps the colour's name";
}

TEST(SurveyCodeVerbs, AColourTheCustomisationNamesItselfIsResolvedFromItsOwnTable)
{
    // "site orange" is no standard name. Until the session's own table says
    // what it is, it is a name nothing knows; once the table says #112233 the
    // verb says so too - it asks the Document at each line, so it follows a
    // table installed since.
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("site orange", Color::fromHex("#112233").valueOr(Color{})).ok());
    SurveyMap map;
    SurveyRule rule;
    rule.key = "SO*";
    rule.model = "ORANGES";
    rule.colour = "site orange";
    ASSERT_TRUE(map.add(rule).ok());

    Session session(false);
    session.document.setSurveyMap(map);
    EXPECT_TRUE(contains(session.ok("CODE EXPLAIN SO1"),
                         "colour \"site orange\": unknown name, so the entity keeps its own"));
    session.document.setColourTable(colours);
    EXPECT_TRUE(contains(session.ok("CODE EXPLAIN SO1"), "colour \"site orange\": #112233"));
}

TEST(SurveyCodeVerbs, ExplainCensusAndListAnswerWithTheManagersWords)
{
    Session session;
    session.codedPoint();
    // Rule #0 gives the layer and #2 the symbol; red is a standard colour.
    const std::string explained = session.ok("code explain KT01");
    EXPECT_TRUE(explained.starts_with("Code \"KT01\": prefix match, matched\n")) << explained;
    EXPECT_TRUE(contains(explained, "  layer: KATANA TEST  <- rule #0 KT* (feature)\n"));
    EXPECT_TRUE(contains(explained, "  symbol: cross, size 2  <- rule #2 KT* (symbol)"));
    EXPECT_TRUE(explained.ends_with("  colour \"red\": #FF0000")) << explained;
    // A code of several words is the rest of the line.
    EXPECT_TRUE(session.ok("CODE EXPLAIN KT 01").starts_with("Code \"KT 01\""));

    EXPECT_EQ(session.ok("CODE CENSUS"),
              "scope=drawing matched=1\n"
              "1 entity carries a code in \"code\", 1 distinct code\n"
              "  KT01: 1, prefix, matched, layer KATANA TEST");
    // A word that is no scope word is the property, as it always was.
    EXPECT_EQ(session.ok("CODE CENSUS elsewhere"),
              "scope=drawing matched=1\n"
              "0 entities carry a code in \"elsewhere\", 0 distinct codes");

    // One line a code. KT* is the only key of the two whose combined rule
    // holds "katana" (its layer); its two rules are a feature and a symbol
    // rule, and what they give is listed layer, linestyle, symbol, colour,
    // comment.
    const std::string listed = session.ok("CODE LIST katana");
    EXPECT_EQ(listed, "KT*  [prefix; 2 rules: feature, symbol]  layer KATANA TEST; linestyle 0; "
                      "symbol cross; colour red; \"[KT*] Katana test\"\n"
                      "1 of 2 codes match \"katana\"");
    EXPECT_TRUE(session.ok("code list").ends_with("2 of 2 codes")) << session.ok("code list");
    // Every reply is a line a front end ends itself: no trailing newline.
    for (const std::string& reply : {explained, listed}) {
        EXPECT_FALSE(reply.ends_with('\n'));
    }
}

TEST(SurveyCodeVerbs, CodeCheckWithOnlyWarningsIsAReply)
{
    Session session;
    EXPECT_EQ(session.ok("CODE CHECK"),
              "3 rules checked: 0 errors, 1 warning\n"
              "  by kind: 1 no layer\n"
              "  rule #1 KX* (feature): warning, no layer: a feature rule with no layer: its "
              "codes stay on whatever layer they are on");
}

TEST(SurveyCodeVerbs, CodeCheckShowsAFieldItsSectionDoesNotUse)
{
    // A symbol rule that also names a layer: a lookup applies it, so it is no
    // error, but a load that replaces this key's symbol rules takes the layer
    // with them. One rule, one warning, naming the field by the word CODE
    // EXPLAIN names it by.
    SurveyMap map;
    SurveyRule rule;
    rule.key = "BL*";
    rule.section = SurveySection::VertexSymbol;
    rule.symbol = katana::entity::SurveySymbol{"cross", "", 1.0, 0.0, 0.0, 0.0};
    rule.model = "SURVEY DETAIL";
    ASSERT_TRUE(map.add(rule).ok());
    Session session(false);
    session.document.setSurveyMap(map);
    EXPECT_EQ(session.ok("CODE CHECK"),
              "1 rule checked: 0 errors, 1 warning\n"
              "  by kind: 1 field outside section\n"
              "  rule #0 BL* (symbol): warning, field outside section: holds layer, which a "
              "symbol rule does not use: a load that replaces this key's symbol rules replaces "
              "it too");
}

TEST(SurveyCodeVerbs, CodeCheckRefusesALintWithAnErrorAndCarriesTheWholeLint)
{
    // SurveyMap::add refuses the rules that lint as errors, so the refusal is
    // shown on issues made by hand - what lintSurveyRule reports for a key
    // with a blank before it.
    const LintIssue blank{0, " WM*", SurveySection::Map, LintSeverity::Error,
                          LintKind::KeyWhitespace, "key \" WM*\" has blanks around it"};
    const LintIssue colour{1, "AC*", SurveySection::Map, LintSeverity::Warning,
                           LintKind::UnknownColour, "colour \"sui\" is not a colour name"};
    const auto refused = katana::cad::codeCheckReply({blank, colour}, 2);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(refused.error().message,
              "the loaded survey codes have 1 error: a rule cannot be applied as written\n"
              "2 rules checked: 1 error, 1 warning\n"
              "  by kind: 1 unknown colour, 1 key whitespace\n"
              "  rule #0  WM* (feature): error, key whitespace: key \" WM*\" has blanks around it\n"
              "  rule #1 AC* (feature): warning, unknown colour: colour \"sui\" is not a colour "
              "name");
    // Warnings alone, and nothing at all, are replies.
    EXPECT_TRUE(katana::cad::codeCheckReply({colour}, 2).ok());
    EXPECT_EQ(katana::cad::codeCheckReply({}, 0).valueOr({}), "0 rules checked: no problems found");
}

TEST(SurveyCodeVerbs, ASubcommandGivenWronglyIsAUsageError)
{
    Session session;
    for (const char* line : {"CODE CHECK everything", "CODE EXPLAIN"}) {
        const katana::core::Error error = session.refused(line);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << line;
        EXPECT_TRUE(error.message.starts_with("usage: ")) << line << ": " << error.message;
    }
    EXPECT_EQ(session.refused("CODE CHECK everything").message, "usage: CODE CHECK");
    // Malformed is malformed whatever is loaded.
    Session empty(false);
    EXPECT_EQ(empty.refused("CODE CHECK everything").message, "usage: CODE CHECK");
}

TEST(SurveyCodeVerbs, MapfileIsNoLongerAVerb)
{
    // Its two subcommands are CODE LIST and CODE CHECK. The word itself is
    // answered as any word that is no verb is, loaded or not.
    for (const bool loaded : {true, false}) {
        Session session(loaded);
        for (const char* line : {"MAPFILE", "MAPFILE LIST", "MAPFILE CHECK", "mapfile check"}) {
            const katana::core::Error error = session.refused(line);
            EXPECT_EQ(error.code, ErrorCode::ParseFailure) << line;
            EXPECT_EQ(error.message, "unknown command; type HELP") << line;
        }
    }
}

TEST(SurveyCodeVerbs, TheHelpNamesThem)
{
    const std::string help = CommandInterpreter::helpText();
    for (const char* verb : {"CODE [scope] [WHERE k=v ...] [PROPERTY name] [PREVIEW]",
                             "CODE EXPLAIN code",
                             "CODE CENSUS [scope] [WHERE k=v ...] [PROPERTY name]",
                             "CODE LIST [filter]", "CODE CHECK"}) {
        EXPECT_TRUE(contains(help, verb)) << verb;
    }
    // Why its default is not the other verbs': said where the scope is said.
    EXPECT_TRUE(contains(help, "With no scope word it is the whole DRAWING, under a bare WHERE "
                               "too"));
    EXPECT_FALSE(contains(help, "MAPFILE"));
}

// ---- the scope ------------------------------------------------------------------------------

TEST(SurveyCodeScope, NoScopeWordIsTheWholeDrawingWhateverIsSelected)
{
    Scoped session;
    session.ok("SELECT " + std::to_string(session.lineOnA));
    EXPECT_EQ(session.ok("CODE CENSUS"),
              "scope=drawing matched=3\n"
              "3 entities carry a code in \"code\", 3 distinct codes\n"
              "  KT01: 1, prefix, matched, layer KATANA TEST\n"
              "  KT02: 1, prefix, matched, layer KATANA TEST\n"
              "  KX01: 1, prefix, matched");
    // Under a bare WHERE too: the two points of the drawing, not the line
    // that is selected. (MODIFY ... WHERE with no scope word is the selection.)
    EXPECT_EQ(session.ok("CODE CENSUS WHERE TYPE=point"),
              "scope=drawing where=\"TYPE=point\" matched=2\n"
              "2 entities carry a code in \"code\", 2 distinct codes\n"
              "  KT01: 1, prefix, matched, layer KATANA TEST\n"
              "  KT02: 1, prefix, matched, layer KATANA TEST");
    // ALL and DRAWING say the same in so many words.
    EXPECT_TRUE(session.ok("CODE CENSUS ALL").starts_with("scope=drawing matched=3\n"));
    EXPECT_TRUE(session.ok("CODE CENSUS DRAWING").starts_with("scope=drawing matched=3\n"));
    // The selection is a scope when it is asked for.
    EXPECT_EQ(session.ok("CODE CENSUS SELECTION"),
              "scope=selection matched=1\n"
              "1 entity carries a code in \"code\", 1 distinct code\n"
              "  KX01: 1, prefix, matched");
}

TEST(SurveyCodeScope, LayersAndAreaTakeWhatTheySay)
{
    Scoped session;
    EXPECT_EQ(session.ok("CODE CENSUS LAYERS a"),
              std::string("scope=layers layers=a sublayers=yes matched=2\n") + kCensusOfLayerA);
    // LAYER is LAYERS, as the shared grammar has it - when a layer follows.
    EXPECT_EQ(session.ok("CODE CENSUS LAYER a ONLY"),
              std::string("scope=layers layers=a sublayers=no matched=2\n") + kCensusOfLayerA);
    // A window holding the first point alone: (1,1) is inside 0,0 - 1.5,1.5,
    // (2,2) and the line from (10,10) are not.
    EXPECT_EQ(session.ok("CODE CENSUS AREA 0,0,1.5,1.5"),
              "scope=area area=0,0,1.5,1.5 matched=1\n"
              "1 entity carries a code in \"code\", 1 distinct code\n"
              "  KT01: 1, prefix, matched, layer KATANA TEST");
    // The scope and its filter together, and the property named after them.
    EXPECT_EQ(session.ok("CODE CENSUS LAYERS a WHERE TYPE=point PROPERTY code"),
              "scope=layers layers=a sublayers=yes where=\"TYPE=point\" matched=1\n"
              "1 entity carries a code in \"code\", 1 distinct code\n"
              "  KT02: 1, prefix, matched, layer KATANA TEST");
}

TEST(SurveyCodeScope, CodeOnAScopeChangesOnlyWhatTheScopeTook)
{
    Scoped session;
    const std::size_t steps = session.document.history().undoCount();
    const std::string reply = session.ok("CODE LAYERS a");
    EXPECT_TRUE(reply.starts_with("scope=layers layers=a sublayers=yes matched=2\n"
                                  "2 entities carry a code in \"code\": 2 matched, 0 "
                                  "fallback-only"))
        << reply;
    EXPECT_TRUE(reply.ends_with("Applied as one command. UNDO puts it all back.")) << reply;
    EXPECT_EQ(session.document.history().undoCount(), steps + 1);
    // KT02 on layer a went to its rule's layer; KX01's rule gives none, so
    // the line stays; KT01 on layer 0 was outside the scope and is untouched.
    EXPECT_EQ(session.layerOf(session.pointOnA), "KATANA TEST");
    EXPECT_EQ(session.layerOf(session.lineOnA), "a");
    EXPECT_EQ(session.layerOf(session.onZero), "0");

    session.ok("UNDO");
    EXPECT_EQ(session.layerOf(session.pointOnA), "a");
}

TEST(SurveyCodeScope, PreviewReportsAndChangesNothing)
{
    Scoped session;
    const std::size_t steps = session.document.history().undoCount();
    // PREVIEW alone is a preview of the whole drawing - never a property
    // called PREVIEW, which would have coded the drawing for real.
    for (const char* line : {"CODE PREVIEW", "code preview", "CODE AREA 0,0,1.5,1.5 PREVIEW",
                             "CODE PREVIEW AREA 0,0,1.5,1.5", "CODE PROPERTY code PREVIEW"}) {
        const std::string reply = session.ok(line);
        EXPECT_TRUE(reply.starts_with("scope=")) << line << ": " << reply;
        EXPECT_TRUE(contains(reply, "Layers created: KATANA TEST\n")) << line << ": " << reply;
        EXPECT_TRUE(reply.ends_with("Preview: nothing was changed.")) << line << ": " << reply;
        EXPECT_EQ(session.document.history().undoCount(), steps) << line;
        EXPECT_EQ(session.layerOf(session.onZero), "0") << line;
        EXPECT_FALSE(session.document.model().layers.contains("KATANA TEST")) << line;
    }
    // By hand, for the window holding the first point alone.
    EXPECT_EQ(session.ok("CODE AREA 0,0,1.5,1.5 PREVIEW"),
              "scope=area area=0,0,1.5,1.5 matched=1\n"
              "1 entity carries a code in \"code\": 1 matched, 0 fallback-only (only the bare * "
              "rule answers), 0 with no rule\n"
              "1 entity changed\n"
              "Layers created: KATANA TEST\n"
              "Styles created: cross\n"
              "By code:\n"
              "  KT01: 1 entity, prefix, matched; layer 0 -> KATANA TEST; style cross (created); "
              "1 changed\n"
              "Preview: nothing was changed.");
}

TEST(SurveyCodeScope, ACensusTakesPreviewAndIsTheSameCensus)
{
    // CODE and CODE CENSUS are one grammar, PREVIEW included. A census
    // changes nothing, so the word asks nothing more of it: wherever it
    // stands the reply is the census of the same scope, never a usage error
    // and never a property called PREVIEW.
    Scoped session;
    const std::string drawing = "scope=drawing matched=3\n"
                                "3 entities carry a code in \"code\", 3 distinct codes\n"
                                "  KT01: 1, prefix, matched, layer KATANA TEST\n"
                                "  KT02: 1, prefix, matched, layer KATANA TEST\n"
                                "  KX01: 1, prefix, matched";
    for (const char* line : {"CODE CENSUS DRAWING PREVIEW", "CODE CENSUS PREVIEW",
                             "code census preview drawing", "CODE CENSUS PREVIEW PROPERTY code"}) {
        EXPECT_EQ(session.ok(line), drawing) << line;
    }
    EXPECT_EQ(session.ok("CODE CENSUS LAYERS a PREVIEW"),
              std::string("scope=layers layers=a sublayers=yes matched=2\n") + kCensusOfLayerA);
    // A property that IS called as the word is named with PROPERTY, as for
    // CODE.
    EXPECT_EQ(session.ok("CODE CENSUS PROPERTY preview"),
              "scope=drawing matched=3\n0 entities carry a code in \"preview\", 0 distinct codes");
}

TEST(SurveyCodeScope, ViewIsTheWindowsViewWhereAFrontEndAnswersForOne)
{
    // A view showing the window 0,0 - 1.5,1.5 and hiding nothing of its own,
    // as the window hands one to the interpreter: of the three coded
    // entities it shows the first point, (1,1), alone.
    Scoped session;
    session.interpreter.setScopeContext(
        [](std::optional<std::uint32_t>) -> katana::core::Result<katana::cad::ScopeView> {
            katana::cad::ScopeView view;
            view.id = 3;
            view.area = katana::geometry::Box2(katana::geometry::Point2(0, 0),
                                               katana::geometry::Point2(1.5, 1.5));
            return view;
        });
    EXPECT_EQ(session.ok("CODE CENSUS VIEW"),
              "scope=view view=3 area=0,0,1.5,1.5 matched=1\n"
              "1 entity carries a code in \"code\", 1 distinct code\n"
              "  KT01: 1, prefix, matched, layer KATANA TEST");
    // CODE on it codes that point and leaves the two the view does not show.
    const std::string reply = session.ok("CODE VIEW");
    EXPECT_TRUE(reply.starts_with("scope=view view=3 area=0,0,1.5,1.5 matched=1\n"
                                  "1 entity carries a code in \"code\": 1 matched"))
        << reply;
    EXPECT_EQ(session.layerOf(session.onZero), "KATANA TEST");
    EXPECT_EQ(session.layerOf(session.pointOnA), "a");
    EXPECT_EQ(session.layerOf(session.lineOnA), "a");
}

TEST(SurveyCodeScope, AScopeThatTakesNothingIsReportedAndCodesNothing)
{
    // Nothing is selected. An empty list of ids means EVERY entity to
    // applySurveyCodes, so a scope that took nothing must never be handed on:
    // it would code the whole drawing.
    Scoped session;
    const std::size_t steps = session.document.history().undoCount();
    EXPECT_EQ(session.ok("CODE SELECTION"),
              "scope=selection matched=0\n"
              "0 entities carry a code in \"code\": 0 matched, 0 fallback-only (only the bare * "
              "rule answers), 0 with no rule\n"
              "0 entities changed\n"
              "Nothing to change.");
    EXPECT_EQ(session.document.history().undoCount(), steps);
    EXPECT_EQ(session.layerOf(session.onZero), "0");
    EXPECT_EQ(session.layerOf(session.pointOnA), "a");
    EXPECT_EQ(session.ok("CODE CENSUS SELECTION PROPERTY feature_code"),
              "scope=selection matched=0\n"
              "0 entities carry a code in \"feature_code\", 0 distinct codes");
    // An empty window, and a filter nothing meets.
    EXPECT_TRUE(session.ok("CODE AREA 100,100,101,101").starts_with("scope=area "
                                                                     "area=100,100,101,101 "
                                                                     "matched=0\n"));
    EXPECT_TRUE(session.ok("CODE WHERE TYPE=circle").ends_with("Nothing to change."));
    EXPECT_EQ(session.document.history().undoCount(), steps);
}

TEST(SurveyCodeScope, AWordThatCannotBeginAScopeIsStillTheProperty)
{
    // "Layer" is an attribute many drawings from GIS data carry. LAYER needs
    // the layers after it, so alone it is the property it always was - and
    // AREA, which needs its window, and PROPERTY, which needs its name, are
    // read the same way.
    Scoped session;
    session.ok("SELECT " + std::to_string(session.onZero));
    session.ok("PROP SET Layer KT09");
    session.ok("SELECT NONE");
    EXPECT_EQ(session.ok("CODE CENSUS Layer"),
              "scope=drawing matched=3\n"
              "1 entity carries a code in \"Layer\", 1 distinct code\n"
              "  KT09: 1, prefix, matched, layer KATANA TEST");
    EXPECT_EQ(session.ok("CODE CENSUS AREA"),
              "scope=drawing matched=3\n0 entities carry a code in \"AREA\", 0 distinct codes");
    EXPECT_EQ(session.ok("CODE CENSUS PROPERTY"),
              "scope=drawing matched=3\n0 entities carry a code in \"PROPERTY\", 0 distinct codes");
    // A name of two words is still the rest of the line.
    EXPECT_EQ(session.ok("CODE CENSUS field code"),
              "scope=drawing matched=3\n0 entities carry a code in \"field code\", 0 distinct codes");
    // CODE itself reads it so too: coded under "Layer", the one entity that
    // carries it goes to KT*'s layer.
    const std::string reply = session.ok("CODE Layer");
    EXPECT_TRUE(reply.starts_with("scope=drawing matched=3\n"
                                  "1 entity carries a code in \"Layer\": 1 matched"))
        << reply;
    EXPECT_EQ(session.layerOf(session.onZero), "KATANA TEST");
    EXPECT_EQ(session.layerOf(session.pointOnA), "a") << "coded under \"code\", not \"Layer\"";

    // A property that IS called as a scope word that stands alone, or as a
    // subcommand, is named with PROPERTY.
    EXPECT_EQ(session.ok("CODE CENSUS PROPERTY ALL"),
              "scope=drawing matched=3\n0 entities carry a code in \"ALL\", 0 distinct codes");
    EXPECT_EQ(session.ok("CODE CENSUS PROPERTY LIST"),
              "scope=drawing matched=3\n0 entities carry a code in \"LIST\", 0 distinct codes");
    EXPECT_EQ(session.ok("CODE CENSUS PROPERTY Layer LAYERS a"),
              "scope=layers layers=a sublayers=yes matched=2\n"
              "0 entities carry a code in \"Layer\", 0 distinct codes");
}

TEST(SurveyCodeScope, APropertyOfSeveralWordsWhoseFirstIsAScopeWordIsNamedWithProperty)
{
    // With a word after it LAYER begins the scope form, and nothing can tell
    // "Layer a", a property of two words, from the layer a: the line is the
    // layer. "Area m2" is then a window that is not four numbers. Each is
    // still a property when it is named as one, quoted to be one word.
    Scoped session;
    EXPECT_EQ(session.ok("CODE CENSUS Layer a"),
              std::string("scope=layers layers=a sublayers=yes matched=2\n") + kCensusOfLayerA);
    const katana::core::Error area = session.refused("CODE CENSUS Area m2");
    EXPECT_EQ(area.code, ErrorCode::ParseFailure);
    EXPECT_EQ(area.context, "m2");
    EXPECT_EQ(session.ok("CODE CENSUS PROPERTY \"Layer a\""),
              "scope=drawing matched=3\n0 entities carry a code in \"Layer a\", 0 distinct codes");
    EXPECT_EQ(session.ok("CODE CENSUS PROPERTY \"Area m2\""),
              "scope=drawing matched=3\n0 entities carry a code in \"Area m2\", 0 distinct codes");
    // A property of several words whose first is none of those words is the
    // rest of the line, quotes or none.
    EXPECT_EQ(session.ok("CODE CENSUS Area_m2 of lot"),
              "scope=drawing matched=3\n"
              "0 entities carry a code in \"Area_m2 of lot\", 0 distinct codes");
}

TEST(SurveyCodeScope, WhatTheGrammarRefuses)
{
    Scoped session;
    const std::size_t steps = session.document.history().undoCount();
    // A word that is neither the scope's nor the verb's own.
    for (const char* line : {"CODE DRAWING everything", "CODE LAYERS a b",
                             "CODE PROPERTY a PROPERTY b", "CODE DRAWING PROPERTY",
                             "CODE DRAWING PREVIEW WHERE TYPE=point"}) {
        const katana::core::Error error = session.refused(line);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << line;
        EXPECT_EQ(error.message,
                  "usage: CODE [<scope>] [WHERE key=value ...] [PROPERTY <name>] [PREVIEW]")
            << line;
    }
    // The census has the same grammar and its own usage line.
    for (const char* line : {"CODE CENSUS DRAWING everything", "CODE CENSUS PROPERTY a PROPERTY b",
                             "CODE CENSUS DRAWING PROPERTY"}) {
        EXPECT_EQ(session.refused(line).message,
                  "usage: CODE CENSUS [<scope>] [WHERE key=value ...] [PROPERTY <name>] [PREVIEW]")
            << line;
    }
    // The shared parser's own refusals come through as they are.
    EXPECT_EQ(session.refused("CODE DRAWING SELECTION").message,
              "give one scope: SELECTION, DRAWING, VIEW, AREA or LAYERS");
    EXPECT_EQ(session.refused("CODE WHERE TYPE=blob").code, ErrorCode::ParseFailure);
    EXPECT_EQ(session.refused("CODE AREA 1,2,3").code, ErrorCode::ParseFailure);
    // A layer the drawing lacks, and the window's view where there is none.
    EXPECT_EQ(session.refused("CODE LAYERS nowhere").code, ErrorCode::NotFound);
    const katana::core::Error view = session.refused("CODE VIEW");
    EXPECT_EQ(view.code, ErrorCode::InvalidState);
    EXPECT_TRUE(contains(view.message, "AREA x0,y0,x1,y1")) << view.message;
    EXPECT_EQ(session.document.history().undoCount(), steps);
    EXPECT_EQ(session.layerOf(session.onZero), "0");
}
