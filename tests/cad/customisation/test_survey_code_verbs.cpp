// CODE and MAPFILE (survey_code_verbs.hpp) through CommandInterpreter::run, as
// the window's command line, katana_cli and katana_mcp type them. They were
// katana_cli's own until 2026-09-26, and the window answered "unknown
// command"; in the interpreter every front end has them.
//
// The map is the cli.* tests' own, built rule by rule, by index:
//   0  KT*  map_data: model KATANA TEST, colour red, linestyle "0"
//   1  KX*  map_data with no model - a warning in the lint
//   2  KT*  vertex_symbol_data: the built-in "cross", size 2
// The texts are code_table.hpp's formatters', which tests/cad/customisation/
// test_code_table.cpp pins; what is pinned here is that the verbs reach them,
// what they refuse, and that CODE is one undo step.

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

// The one colour name the tests' codes use, as a front end's table gives it:
// red is #FF0000.
std::optional<Color> red(std::string_view name)
{
    if (name == "red") {
        return Color::fromHex("#FF0000").valueOr(Color{});
    }
    return std::nullopt;
}

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    explicit Session(bool loaded = true, bool colours = true)
    {
        if (loaded) {
            document.setSurveyMap(testCodes());
        }
        if (colours) {
            interpreter.setColourLookup(red);
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
};

bool contains(const std::string& text, std::string_view part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

TEST(SurveyCodeVerbs, AreRefusedByNameUntilSurveyCodesAreLoaded)
{
    Session session(false);
    session.codedPoint();
    const std::size_t steps = session.document.history().undoCount();
    for (const char* line : {"CODE", "code census", "CODE EXPLAIN KT01", "MAPFILE LIST",
                             "MAPFILE CHECK"}) {
        const katana::core::Error error = session.refused(line);
        EXPECT_EQ(error.code, ErrorCode::InvalidState) << line;
        EXPECT_EQ(error.message, "no survey codes are loaded; use CUSTOMISE <file> first") << line;
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
    EXPECT_TRUE(reply.starts_with("1 entity carries a code in \"code\": 1 matched, 0 fallback-only"))
        << reply;
    EXPECT_TRUE(contains(reply, "Layers created: KATANA TEST\nStyles created: cross\n")) << reply;
    EXPECT_TRUE(reply.ends_with("Applied as one command. UNDO puts it all back.")) << reply;
    EXPECT_EQ(session.document.model().entities.find(1)->layer, "KATANA TEST");
    EXPECT_EQ(session.document.history().undoCount(), steps + 1);
    // The colour came through the front end's lookup: red is #FF0000.
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

TEST(SurveyCodeVerbs, WithoutAColourLookupAColourIsLeftAloneRatherThanGuessed)
{
    Session session(true, false);
    session.codedPoint();
    session.ok("CODE");
    const katana::entity::Style* style = session.document.model().styles.find("cross");
    ASSERT_NE(style, nullptr);
    EXPECT_FALSE(style->color.has_value());
    EXPECT_TRUE(contains(session.ok("CODE EXPLAIN KT01"),
                         "colour \"red\": unknown name, so the entity keeps its own"));
}

TEST(SurveyCodeVerbs, ExplainCensusAndListAnswerWithTheManagersWords)
{
    Session session;
    session.codedPoint();
    // Rule #0 gives the model and #2 the symbol; the lookup gives the colour.
    const std::string explained = session.ok("code explain KT01");
    EXPECT_TRUE(explained.starts_with("Code \"KT01\": prefix match, matched\n")) << explained;
    EXPECT_TRUE(contains(explained, "  model: KATANA TEST  <- rule #0 KT* (map_data)\n"));
    EXPECT_TRUE(contains(explained, "  symbol: cross, size 2  <- rule #2 KT* (vertex_symbol_data)"));
    EXPECT_TRUE(explained.ends_with("  colour \"red\": #FF0000")) << explained;
    // A code of several words is the rest of the line.
    EXPECT_TRUE(session.ok("CODE EXPLAIN KT 01").starts_with("Code \"KT 01\""));

    EXPECT_EQ(session.ok("CODE CENSUS"),
              "1 entity carries a code in \"code\", 1 distinct code\n"
              "  KT01: 1, prefix, matched, model KATANA TEST");
    EXPECT_EQ(session.ok("CODE CENSUS elsewhere"),
              "0 entities carry a code in \"elsewhere\", 0 distinct codes");

    const std::string listed = session.ok("MAPFILE LIST katana");
    EXPECT_TRUE(listed.starts_with("KT*  [prefix; 2 rules: map_data, vertex_symbol_data]  model "
                                   "KATANA TEST"))
        << listed;
    EXPECT_TRUE(listed.ends_with("1 of 2 codes match \"katana\"")) << listed;
    // Every reply is a line a front end ends itself: no trailing newline.
    for (const std::string& reply : {explained, listed}) {
        EXPECT_FALSE(reply.ends_with('\n'));
    }
}

TEST(SurveyCodeVerbs, MapfileCheckWithOnlyWarningsIsAReply)
{
    Session session;
    EXPECT_EQ(session.ok("MAPFILE CHECK"),
              "3 rules checked: 0 errors, 1 warning\n"
              "  by kind: 1 no model\n"
              "  rule #1 KX* (map_data): warning, no model: a map_data rule with no model: its "
              "codes stay on whatever layer they are on");
}

TEST(SurveyCodeVerbs, MapfileCheckRefusesALintWithAnErrorAndCarriesTheWholeLint)
{
    // SurveyMap::add refuses the rules that lint as errors, so the refusal is
    // shown on issues made by hand - what lintSurveyRule reports for a key
    // with a blank before it.
    const LintIssue blank{0, " WM*", SurveySection::Map, LintSeverity::Error,
                          LintKind::KeyWhitespace, "key \" WM*\" has blanks around it"};
    const LintIssue colour{1, "AC*", SurveySection::Map, LintSeverity::Warning,
                           LintKind::UnknownColour, "colour \"sui\" is not a colour name"};
    const auto refused = katana::cad::mapfileCheckReply({blank, colour}, 2);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(refused.error().message,
              "the loaded survey codes have 1 error: a rule cannot be applied as written\n"
              "2 rules checked: 1 error, 1 warning\n"
              "  by kind: 1 unknown colour, 1 key whitespace\n"
              "  rule #0  WM* (map_data): error, key whitespace: key \" WM*\" has blanks around it\n"
              "  rule #1 AC* (map_data): warning, unknown colour: colour \"sui\" is not a colour "
              "name");
    // Warnings alone, and nothing at all, are replies.
    EXPECT_TRUE(katana::cad::mapfileCheckReply({colour}, 2).ok());
    EXPECT_EQ(katana::cad::mapfileCheckReply({}, 0).valueOr({}), "0 rules checked: no problems found");
}

TEST(SurveyCodeVerbs, AWordThatIsNoSubcommandIsAUsageError)
{
    Session session;
    for (const char* line : {"MAPFILE", "MAPFILE LINT", "MAPFILE CHECK everything", "CODE EXPLAIN"}) {
        const katana::core::Error error = session.refused(line);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << line;
        EXPECT_TRUE(error.message.starts_with("usage: ")) << line << ": " << error.message;
    }
}

TEST(SurveyCodeVerbs, TheHelpNamesThem)
{
    const std::string help = CommandInterpreter::helpText();
    for (const char* verb : {"CODE [property]", "CODE EXPLAIN code", "CODE CENSUS [property]",
                             "MAPFILE LIST [filter] | CHECK"}) {
        EXPECT_TRUE(contains(help, verb)) << verb;
    }
}
