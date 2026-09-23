// The survey code library questioned: why a code gets what it gets, the map
// as a table of codes, the codes a drawing carries, and what is wrong with a
// map - and the one report text every front end prints.

#include <gtest/gtest.h>

#include "katana/cad/code_table.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"

namespace cmd = katana::commands;
using katana::cad::CodeExplanation;
using katana::cad::Document;
using katana::cad::LintIssue;
using katana::cad::LintKind;
using katana::cad::LintSeverity;
using katana::cad::NearMissKind;
using katana::entity::Color;
using katana::entity::EntityId;
using katana::entity::LineStyle;
using katana::entity::SurveyAttribute;
using katana::entity::SurveyMap;
using katana::entity::SurveyMatchKind;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;
using katana::geometry::Point2;

namespace {

SurveyRule mapData(std::string key, std::string model, std::string colour, std::string linestyle)
{
    SurveyRule rule;
    rule.key = std::move(key);
    rule.model = std::move(model);
    rule.colour = std::move(colour);
    rule.linestyle = std::move(linestyle);
    return rule;
}

SurveyRule vertexSymbol(std::string key, std::string style, double size, std::string colour = {})
{
    SurveyRule rule;
    rule.key = std::move(key);
    rule.section = SurveySection::VertexSymbol;
    rule.symbol = katana::entity::SurveySymbol{std::move(style), std::move(colour), size,
                                               0.0, 0.0, 0.0};
    return rule;
}

// The shapes of the reference mapfile, cut down, by index:
//   0  AC*  map_data: linestyle "0" - every symbol code says so
//   1  AC*  vertex_symbol_data: the bollard
//   2  1*   map_data: a text code on linestyle "0", yellow
//   3  2*   map_data: another, cyan
//   4  WM*  map_data: a real linestyle, a colour Katana does not know
//   5  WM*  string_attribute_data: an attribute naming another ($)
//   6  PABB map_data: an exact key, a linestyle no library here defines
//   7  *    string_attribute_data: every code gets it
//   8-11 *  pipe_data: the reference files carry sixteen of these in each
//           pipe section, identical but for the DepthLocation they give
SurveyMap referenceShapes()
{
    SurveyMap map;
    SurveyRule bollard = mapData("AC*", "SURVEY DETAIL", "white", "0");
    bollard.group = "SURVEY - CULT";
    bollard.comment = "[AC*] Bollard";
    EXPECT_TRUE(map.add(bollard).ok());
    EXPECT_TRUE(map.add(vertexSymbol("AC*", "CULT Bollard", 1.5)).ok());
    EXPECT_TRUE(map.add(mapData("1*", "SURVEY TEXT", "yellow", "0")).ok());
    EXPECT_TRUE(map.add(mapData("2*", "SURVEY TEXT", "cyan", "0")).ok());
    SurveyRule main = mapData("WM*", "SURVEY SERVICES", "sui water potable", "WATR Main");
    main.breakline = katana::entity::SurveyBreakline::Line;
    main.group = "SURVEY - WATR";
    main.comment = "[WM*] Main";
    EXPECT_TRUE(map.add(main).ok());
    SurveyRule diameter;
    diameter.key = "WM*";
    diameter.section = SurveySection::StringAttribute;
    diameter.attributes = {{"text", "Diameter", "$PipeDiameter"}};
    EXPECT_TRUE(map.add(diameter).ok());
    EXPECT_TRUE(
        map.add(mapData("PABB", "SURVEY DETAIL", "Green", "TOPO Timber or Scrub Scattered")).ok());
    SurveyRule zone;
    zone.key = "*";
    zone.section = SurveySection::StringAttribute;
    zone.attributes = {{"integer", "Model Validation Zone", "1"}};
    EXPECT_TRUE(map.add(zone).ok());
    for (const char* depth : {"Top of Pipe", "Obvert", "Cable", "Trace Wire"}) {
        SurveyRule pipe;
        pipe.key = "*";
        pipe.section = SurveySection::Pipe;
        pipe.attributes = {{"text", "DepthLocation", depth}};
        pipe.pipe = katana::entity::SurveyPipe{"Obvert", "diameter", "$PipeDiameter", "", true};
        EXPECT_TRUE(map.add(pipe).ok());
    }
    return map;
}

LineStyle definition(std::string name, bool atVertices, std::string source)
{
    LineStyle style;
    style.name = std::move(name);
    style.atVertices = atVertices;
    style.source = std::move(source);
    return style;
}

katana::entity::StyleLibrary library()
{
    katana::entity::StyleLibrary library;
    for (LineStyle style : {definition("CULT Bollard", true, "user_symbols_test.4d"),
                            definition("WATR Main", false, "user_linestyl_test.4d"),
                            // Not `mode vertex`, but from the symbol file: a
                            // symbol all the same (decision D3).
                            definition("SEWR Manhole Cover", false, "user_symbols_test.4d"),
                            // Read from nowhere known.
                            definition("MYST Thing", false, "")}) {
        EXPECT_TRUE(katana::entity::addOrReplace(library, std::move(style)).ok());
    }
    return library;
}

// A colour table of the test's own; "sui water potable" is unknown to it, as
// it is to Katana.
std::optional<Color> testColour(std::string_view name)
{
    if (name == "white") {
        return Color{255, 255, 255, 255};
    }
    if (name == "yellow") {
        return Color{255, 255, 0, 255};
    }
    if (name == "cyan") {
        return Color{0, 255, 255, 255};
    }
    if (name == "Green") {
        return Color{0, 255, 0, 255};
    }
    if (name == "red") {
        return Color{255, 0, 0, 255};
    }
    return std::nullopt;
}

CodeExplanation explain(const SurveyMap& map, std::string_view code)
{
    static const katana::entity::StyleLibrary defined = library();
    return katana::cad::explainCode(
        map, code, [](std::string_view name) { return defined.find(name); }, testColour);
}

EntityId addCodedPoint(Document& document, const Point2& at, const std::string& code)
{
    EXPECT_TRUE(document.execute(cmd::createPoint(at)).ok());
    const EntityId id = document.model().entities.ids().back();
    EXPECT_TRUE(
        document.execute(cmd::setEntityProperty({id}, "code", katana::entity::PropertyValue(code)))
            .ok());
    return id;
}

} // namespace

// ---- explaining one code ------------------------------------------------------

TEST(ExplainCode, EachFieldNamesTheRuleItCameFromMostSpecificFirst)
{
    const SurveyMap map = referenceShapes();
    const CodeExplanation why = explain(map, "AC01");

    EXPECT_EQ(why.kind, SurveyMatchKind::Prefix);
    EXPECT_TRUE(why.matched);
    // AC* (0, 1), then the bare `*` rules in read order (7 to 11).
    EXPECT_EQ(why.rules, (std::vector<std::size_t>{0, 1, 7, 8, 9, 10, 11}));
    EXPECT_EQ(why.resolved, map.lookup("AC01").resolved);

    // In CodeFieldSource's field order: rule 0 gives five, the vertex_symbol
    // rule the symbol, and the first `*` pipe row the pipe.
    ASSERT_EQ(why.fields.size(), 7u);
    const std::vector<std::tuple<std::string, std::string, std::size_t, SurveySection>> expected = {
        {"model", "SURVEY DETAIL", 0, SurveySection::Map},
        {"colour", "white", 0, SurveySection::Map},
        {"linestyle", "0", 0, SurveySection::Map},
        {"group", "SURVEY - CULT", 0, SurveySection::Map},
        {"comment", "[AC*] Bollard", 0, SurveySection::Map},
        {"symbol", "CULT Bollard, size 1.5", 1, SurveySection::VertexSymbol},
        {"pipe", "diameter $PipeDiameter, Obvert", 8, SurveySection::Pipe},
    };
    for (std::size_t i = 0; i < expected.size(); ++i) {
        EXPECT_EQ(why.fields[i].field, std::get<0>(expected[i]));
        EXPECT_EQ(why.fields[i].value, std::get<1>(expected[i])) << why.fields[i].field;
        EXPECT_EQ(why.fields[i].rule, std::get<2>(expected[i])) << why.fields[i].field;
        EXPECT_EQ(why.fields[i].section, std::get<3>(expected[i])) << why.fields[i].field;
        EXPECT_EQ(why.fields[i].key, map.rules()[std::get<2>(expected[i])].key);
    }

    EXPECT_TRUE(why.linestyle.plain) << "\"0\" is 12d's plain line";
    EXPECT_FALSE(why.linestyle.defined);
    EXPECT_EQ(why.symbol.name, "CULT Bollard");
    EXPECT_TRUE(why.symbol.defined);
    EXPECT_TRUE(why.symbol.vertexMode);
    ASSERT_TRUE(why.colour.rgb.has_value());
    EXPECT_EQ(why.colour.rgb->toHex(), "#FFFFFF");

    // Accumulated in rule order: the zone from rule 7, DepthLocation from the
    // FIRST pipe row (8) - rows 9 to 11 name it again and are not taken.
    ASSERT_EQ(why.attributes.size(), 2u);
    EXPECT_EQ(why.attributes[0].attribute.name, "Model Validation Zone");
    EXPECT_EQ(why.attributes[0].rule, 7u);
    EXPECT_EQ(why.attributes[0].scope, "string");
    EXPECT_EQ(why.attributes[1].attribute.value, "Top of Pipe");
    EXPECT_EQ(why.attributes[1].rule, 8u);
    EXPECT_FALSE(why.attributes[1].deferred);
    EXPECT_TRUE(why.nearMisses.empty());
}

TEST(ExplainCode, ALinestyleIsResolvedAColourUnknownByNameAndADollarValueIsDeferred)
{
    const SurveyMap map = referenceShapes();
    const CodeExplanation why = explain(map, "WM01");
    EXPECT_EQ(why.rules, (std::vector<std::size_t>{4, 5, 7, 8, 9, 10, 11}));
    EXPECT_EQ(why.linestyle.name, "WATR Main");
    EXPECT_FALSE(why.linestyle.plain);
    EXPECT_TRUE(why.linestyle.defined);
    EXPECT_FALSE(why.linestyle.vertexMode);
    EXPECT_EQ(why.colour.name, "sui water potable");
    EXPECT_FALSE(why.colour.rgb.has_value()) << "an unknown name, not a guessed colour";
    ASSERT_EQ(why.attributes.size(), 3u);
    EXPECT_EQ(why.attributes[0].attribute.name, "Diameter");
    EXPECT_TRUE(why.attributes[0].deferred) << "$PipeDiameter names another attribute";
    EXPECT_EQ(why.attributes[0].rule, 5u);
    // model, colour, linestyle, group, comment, breakline from rule 4; pipe
    // from rule 8.
    ASSERT_EQ(why.fields.size(), 7u);
    EXPECT_EQ(why.fields[5].field, "breakline");
    EXPECT_EQ(why.fields[5].value, "Line");
}

TEST(ExplainCode, TwoTextCodesOnLinestyleZeroDifferOnlyInColour)
{
    const SurveyMap map = referenceShapes();
    const CodeExplanation yellow = explain(map, "101");
    const CodeExplanation cyan = explain(map, "201");
    EXPECT_TRUE(yellow.matched);
    EXPECT_TRUE(yellow.linestyle.plain);
    EXPECT_TRUE(cyan.linestyle.plain);
    EXPECT_TRUE(yellow.symbol.name.empty());
    EXPECT_EQ(yellow.resolved.model, cyan.resolved.model);
    ASSERT_TRUE(yellow.colour.rgb && cyan.colour.rgb);
    EXPECT_EQ(yellow.colour.rgb->toHex(), "#FFFF00");
    EXPECT_EQ(cyan.colour.rgb->toHex(), "#00FFFF");
}

TEST(ExplainCode, AnExactKeyIsExactAndACodeOnlyTheStarAnswersIsFallbackOnly)
{
    const SurveyMap map = referenceShapes();
    const CodeExplanation exact = explain(map, "PABB");
    EXPECT_EQ(exact.kind, SurveyMatchKind::Exact);
    EXPECT_TRUE(exact.matched);
    EXPECT_FALSE(exact.linestyle.defined) << "no library here defines it";

    // D5: the `*` rules give a pipe and attributes, but no model, linestyle
    // or symbol, so this is not a match.
    const CodeExplanation typo = explain(map, "ZZ99");
    EXPECT_EQ(typo.kind, SurveyMatchKind::FallbackOnly);
    EXPECT_FALSE(typo.matched);
    EXPECT_EQ(typo.rules, (std::vector<std::size_t>{7, 8, 9, 10, 11}));

    const CodeExplanation nothing = explain(SurveyMap{}, "ZZ99");
    EXPECT_EQ(nothing.kind, SurveyMatchKind::None);
    EXPECT_FALSE(nothing.matched);
    EXPECT_TRUE(nothing.rules.empty());
    EXPECT_TRUE(nothing.fields.empty());
    EXPECT_TRUE(nothing.attributes.empty());
}

TEST(ExplainCode, AKeyMissedOnlyByCaseOrByBlanksIsNamedAsANearMiss)
{
    const SurveyMap map = referenceShapes();
    const CodeExplanation lower = explain(map, "wm01");
    EXPECT_EQ(lower.kind, SurveyMatchKind::FallbackOnly) << "matching is byte for byte";
    ASSERT_EQ(lower.nearMisses.size(), 1u);
    EXPECT_EQ(lower.nearMisses[0].key, "WM*");
    EXPECT_EQ(lower.nearMisses[0].kind, NearMissKind::Case);

    const CodeExplanation padded = explain(map, " WM01");
    ASSERT_EQ(padded.nearMisses.size(), 1u);
    EXPECT_EQ(padded.nearMisses[0].key, "WM*");
    EXPECT_EQ(padded.nearMisses[0].kind, NearMissKind::Whitespace);

    const CodeExplanation exact = explain(map, "Pabb");
    ASSERT_EQ(exact.nearMisses.size(), 1u);
    EXPECT_EQ(exact.nearMisses[0].key, "PABB");
    EXPECT_EQ(exact.nearMisses[0].kind, NearMissKind::Case);
}

TEST(ExplainCode, IsPrintedAsOneTextNamingEachRule)
{
    // Worked by hand from referenceShapes(): only the `*` rules catch "wm01";
    // the pipe comes from rule 8, the zone from 7, DepthLocation from 8.
    const std::string expected =
        "Code \"wm01\": only the bare * rule answers it: fallback-only, not matched\n"
        "  pipe: diameter $PipeDiameter, Obvert  <- rule #8 * (pipe_data)\n"
        "  string attribute Model Validation Zone = 1  <- rule #7 * (string_attribute_data)\n"
        "  string attribute DepthLocation = Top of Pipe  <- rule #8 * (pipe_data)\n"
        "  near miss: key \"WM*\" differs only in letter case\n";
    EXPECT_EQ(katana::cad::formatCodeExplanation(explain(referenceShapes(), "wm01")), expected);
}

TEST(ExplainCode, ALaterRuleSayingSomethingElseIsShownAsOverruled)
{
    // The two built-in mapfiles disagree on `hide` for 190 keys; a shorter
    // prefix can disagree with a longer one. A later rule saying the SAME is
    // not shown: nothing was decided against it.
    SurveyMap map;
    SurveyRule shown = vertexSymbol("AC*", "CULT Bollard", 1.5);
    shown.hide = false;
    SurveyRule hidden = shown;
    hidden.hide = true;
    ASSERT_TRUE(map.add(shown).ok());                                          // 0
    ASSERT_TRUE(map.add(hidden).ok());                                         // 1
    ASSERT_TRUE(map.add(mapData("A*", "SURVEY DETAIL", "red", "")).ok());      // 2
    ASSERT_TRUE(map.add(mapData("AC*", "SURVEY DETAIL", "white", "")).ok());   // 3

    // By hand: AC* (0, 1, 3) before A* (2). model: 3, and 2 agrees. colour:
    // 3 white, 2 red. hide: 0 no, 1 yes. symbol: 0, and 1 agrees.
    EXPECT_EQ(katana::cad::formatCodeExplanation(explain(map, "AC01")),
              "Code \"AC01\": prefix match, matched\n"
              "  model: SURVEY DETAIL  <- rule #3 AC* (map_data)\n"
              "  colour: white  <- rule #3 AC* (map_data)\n"
              "    overruled: red  <- rule #2 A* (map_data)\n"
              "  hide: no  <- rule #0 AC* (vertex_symbol_data)\n"
              "    overruled: yes  <- rule #1 AC* (vertex_symbol_data)\n"
              "  symbol: CULT Bollard, size 1.5  <- rule #0 AC* (vertex_symbol_data)\n"
              "  symbol \"CULT Bollard\": defined, mode vertex\n"
              "  colour \"white\": #FFFFFF\n");
}

// ---- the map as a table of codes ------------------------------------------------

TEST(CodeTable, HasOneRowPerDistinctKeyInKeyOrderWithItsRulesAndSections)
{
    const SurveyMap map = referenceShapes();
    const auto rows = katana::cad::codeTable(map);
    // By byte: '*' 42 < '1' 49 < '2' 50 < 'A' 65 < 'P' 80 < 'W' 87.
    ASSERT_EQ(rows.size(), 6u);
    EXPECT_EQ(rows[0].key, "*");
    EXPECT_EQ(rows[0].kind, SurveyMatchKind::FallbackOnly);
    EXPECT_EQ(rows[0].rules, (std::vector<std::size_t>{7, 8, 9, 10, 11}));
    EXPECT_EQ(rows[0].sections,
              (std::vector<SurveySection>{SurveySection::Pipe, SurveySection::StringAttribute}))
        << "distinct, in section order rather than read order";
    EXPECT_EQ(rows[1].key, "1*");
    EXPECT_EQ(rows[2].key, "2*");

    const auto& bollard = rows[3];
    EXPECT_EQ(bollard.key, "AC*");
    EXPECT_EQ(bollard.kind, SurveyMatchKind::Prefix);
    EXPECT_EQ(bollard.rules, (std::vector<std::size_t>{0, 1}));
    EXPECT_EQ(bollard.sections,
              (std::vector<SurveySection>{SurveySection::Map, SurveySection::VertexSymbol}));
    // Combined as a code the key catches: its own rules, then `*`.
    EXPECT_EQ(bollard.combined.model, "SURVEY DETAIL");
    ASSERT_TRUE(bollard.combined.symbol.has_value());
    EXPECT_EQ(bollard.combined.symbol->style, "CULT Bollard");
    EXPECT_TRUE(bollard.combined.pipe.has_value()) << "the `*` pipe row reaches it too";

    EXPECT_EQ(rows[4].key, "PABB");
    EXPECT_EQ(rows[4].kind, SurveyMatchKind::Exact);
    EXPECT_EQ(rows[5].key, "WM*");
    EXPECT_EQ(rows[5].rules, (std::vector<std::size_t>{4, 5}));
}

TEST(CodeTable, AFilterFoldsCaseAndSearchesKeyCommentGroupModelColourLinestyleAndSymbol)
{
    const auto rows = katana::cad::codeTable(referenceShapes());
    const auto keysMatching = [&rows](std::string_view filter) {
        std::vector<std::string> keys;
        for (const auto& row : rows) {
            if (katana::cad::codeTableRowMatches(row, filter)) {
                keys.push_back(row.key);
            }
        }
        return keys;
    };
    EXPECT_EQ(keysMatching("bollard"), (std::vector<std::string>{"AC*"}));
    EXPECT_EQ(keysMatching("survey text"), (std::vector<std::string>{"1*", "2*"}));
    // Every row but `*`, which has no model, group or comment of its own.
    EXPECT_EQ(keysMatching("SURVEY"), (std::vector<std::string>{"1*", "2*", "AC*", "PABB", "WM*"}));
    EXPECT_EQ(keysMatching("pabb"), (std::vector<std::string>{"PABB"}));
    EXPECT_EQ(keysMatching("cyan"), (std::vector<std::string>{"2*"}));
    EXPECT_EQ(keysMatching("watr main"), (std::vector<std::string>{"WM*"}));
    EXPECT_EQ(keysMatching("").size(), 6u);
}

// ---- the codes a drawing carries ------------------------------------------------

TEST(CodeCensus, CountsEachDistinctCodeAndClassesItAgainstTheMap)
{
    Document document;
    document.setSurveyMap(referenceShapes());
    addCodedPoint(document, Point2(0, 0), "AC01");
    addCodedPoint(document, Point2(1, 0), "ZZ99");
    addCodedPoint(document, Point2(2, 0), "AC01");
    addCodedPoint(document, Point2(3, 0), "101");
    ASSERT_TRUE(document.execute(cmd::createPoint(Point2(4, 0))).ok()); // no code

    const auto census = katana::cad::codeCensus(document);
    EXPECT_EQ(census.property, "code");
    EXPECT_EQ(census.coded, 4u);
    ASSERT_EQ(census.codes.size(), 3u);
    // "101" < "AC01" < "ZZ99".
    EXPECT_EQ(census.codes[0].code, "101");
    EXPECT_EQ(census.codes[0].entities, 1u);
    EXPECT_EQ(census.codes[0].model, "SURVEY TEXT");
    EXPECT_EQ(census.codes[1].code, "AC01");
    EXPECT_EQ(census.codes[1].entities, 2u);
    EXPECT_EQ(census.codes[1].kind, SurveyMatchKind::Prefix);
    EXPECT_TRUE(census.codes[1].matched);
    EXPECT_EQ(census.codes[2].code, "ZZ99");
    EXPECT_EQ(census.codes[2].kind, SurveyMatchKind::FallbackOnly);
    EXPECT_FALSE(census.codes[2].matched);

    const std::string expected = "4 entities carry a code in \"code\", 3 distinct codes\n"
                                 "  101: 1, prefix, matched, model SURVEY TEXT\n"
                                 "  AC01: 2, prefix, matched, model SURVEY DETAIL\n"
                                 "  ZZ99: 1, fallback only, not matched\n";
    EXPECT_EQ(katana::cad::formatCodeCensus(census), expected);
}

TEST(CodeCensus, FindsCodesAnImportLeftInMetadata)
{
    Document document;
    document.setSurveyMap(referenceShapes());
    katana::entity::Entity point;
    point.geometry = katana::entity::PointGeometry{Point2(5, 5)};
    point.metadata.insert_or_assign("12d.name", katana::entity::PropertyValue(std::string("WM01")));
    ASSERT_TRUE(document.execute(cmd::createEntities({point})).ok());
    const auto census = katana::cad::codeCensus(document);
    EXPECT_EQ(census.property, "12d.name");
    ASSERT_EQ(census.codes.size(), 1u);
    EXPECT_EQ(census.codes[0].code, "WM01");
    EXPECT_EQ(census.codes[0].model, "SURVEY SERVICES");
}

// ---- what is wrong with a map -----------------------------------------------------

namespace {

std::vector<std::pair<std::size_t, LintKind>> kinds(const std::vector<LintIssue>& issues)
{
    std::vector<std::pair<std::size_t, LintKind>> out;
    for (const LintIssue& issue : issues) {
        out.emplace_back(issue.rule, issue.kind);
    }
    return out;
}

bool builtIn(std::string_view name)
{
    return katana::entity::isBuiltInSymbolName(name);
}

} // namespace

TEST(LintSurveyMap, FindsTheUnknownColourTheMissingLinestyleAndTheStarPipeRowsThatNeverContribute)
{
    const auto issues =
        katana::cad::lintSurveyMap(referenceShapes(), library(), testColour, builtIn);
    // By hand, rule by rule: 0-3 are clean (linestyle "0" is plain, the
    // bollard is defined). 4: "sui water potable" is not in the colour table.
    // 5 adds an attribute rule 4 does not give. 6: no library defines "TOPO
    // Timber or Scrub Scattered". 7 is the first `*`; 8 is the first to give
    // a pipe. 9, 10, 11 give a pipe and a DepthLocation, both of which 8
    // already gives, so a lookup never takes anything from them.
    const std::vector<std::pair<std::size_t, LintKind>> expected = {
        {4, LintKind::UnknownColour},
        {6, LintKind::UnresolvedLinestyle},
        {9, LintKind::ShadowedRule},
        {10, LintKind::ShadowedRule},
        {11, LintKind::ShadowedRule},
    };
    ASSERT_EQ(kinds(issues), expected);
    for (const LintIssue& issue : issues) {
        EXPECT_EQ(issue.severity, LintSeverity::Warning);
    }
    EXPECT_EQ(issues[2].key, "*");
    EXPECT_EQ(issues[2].section, SurveySection::Pipe);
}

TEST(LintSurveyMap, JudgesSymbolsAndLinestylesByWhatTheDefinitionIs)
{
    SurveyMap map;
    SurveyRule noModel;
    noModel.key = "XX*";
    noModel.colour = "red";
    ASSERT_TRUE(map.add(noModel).ok());                                                     // 0
    ASSERT_TRUE(map.add(mapData("BL*", "SURVEY DETAIL", "", "CULT Bollard")).ok());         // 1
    ASSERT_TRUE(map.add(vertexSymbol("WV*", "WATR Main", 1.0)).ok());                       // 2
    ASSERT_TRUE(map.add(vertexSymbol("SV*", "SEWR Manhole Cover", 1.0)).ok());              // 3
    ASSERT_TRUE(map.add(vertexSymbol("CR*", "cross", 1.0)).ok());                           // 4
    ASSERT_TRUE(map.add(vertexSymbol("NS*", "NOPE Symbol", 1.0, "not a colour")).ok());     // 5
    ASSERT_TRUE(map.add(mapData("CL*", "SURVEY DETAIL", "", "Continuous")).ok());           // 6
    ASSERT_TRUE(map.add(mapData("ON*", "SURVEY DETAIL", "", "1")).ok());                    // 7
    ASSERT_TRUE(map.add(noModel).ok());                                                     // 8
    ASSERT_TRUE(map.add(vertexSymbol("MY*", "MYST Thing", 1.0)).ok());                      // 9

    // 0 and 8 put their code nowhere; 8 is also rule 0 again. 1 draws a
    // vertex definition along a line. 2 uses a linestyle from the linestyle
    // file as a symbol. 3 is from the symbol file and 9 from nowhere known:
    // both are taken as symbols. 4 is a shape Katana draws. 5 names nothing
    // and its symbol colour is unknown (UnresolvedSymbol sorts first). 6 and
    // 7 are plain lines.
    const std::vector<std::pair<std::size_t, LintKind>> expected = {
        {0, LintKind::NoModel},
        {1, LintKind::LinestyleIsVertex},
        {2, LintKind::SymbolNotSymbolCapable},
        {5, LintKind::UnresolvedSymbol},
        {5, LintKind::UnknownColour},
        {8, LintKind::NoModel},
        {8, LintKind::DuplicateRule},
    };
    EXPECT_EQ(kinds(katana::cad::lintSurveyMap(map, library(), testColour, builtIn)), expected);

    // With no built-in test, "cross" is unresolved too; with no colour table
    // no colour is judged.
    const auto bare = katana::cad::lintSurveyMap(map, library(), {}, {});
    const std::vector<std::pair<std::size_t, LintKind>> withoutHelpers = {
        {0, LintKind::NoModel},
        {1, LintKind::LinestyleIsVertex},
        {2, LintKind::SymbolNotSymbolCapable},
        {4, LintKind::UnresolvedSymbol},
        {5, LintKind::UnresolvedSymbol},
        {8, LintKind::NoModel},
        {8, LintKind::DuplicateRule},
    };
    EXPECT_EQ(kinds(bare), withoutHelpers);
}

TEST(LintSurveyRule, CatchesAKeyWithBlanksAndAModelThatCannotBeALayerBeforeTheMapRefusesThem)
{
    SurveyRule rule = mapData(" WM*", "SURVEY//SERVICES", "", "");
    SurveyMap map;
    EXPECT_FALSE(map.add(rule).ok()) << "a map never holds such a rule";

    const auto issues = katana::cad::lintSurveyRule(rule, 42, library(), testColour, builtIn);
    const std::vector<std::pair<std::size_t, LintKind>> expected = {
        {42, LintKind::InvalidLayerPath},
        {42, LintKind::KeyWhitespace},
    };
    EXPECT_EQ(kinds(issues), expected);
    for (const LintIssue& issue : issues) {
        EXPECT_EQ(issue.severity, LintSeverity::Error);
        EXPECT_EQ(issue.key, " WM*");
    }
}

TEST(LintSurveyMap, IsPrintedAsOneTextWithTheCounts)
{
    SurveyMap map;
    ASSERT_TRUE(map.add(mapData("WM*", "SURVEY SERVICES", "sui water potable", "WATR Main")).ok());
    ASSERT_TRUE(map.add(mapData("AC*", "SURVEY DETAIL", "white", "0")).ok());
    const auto issues = katana::cad::lintSurveyMap(map, library(), testColour, builtIn);
    EXPECT_EQ(katana::cad::formatLint(issues, map.size()),
              "2 rules checked: 0 errors, 1 warning\n"
              "  by kind: 1 unknown colour\n"
              "  rule #0 WM* (map_data): warning, unknown colour: colour \"sui water potable\" "
              "is not a colour name Katana knows, so the entity keeps its own\n");
    EXPECT_EQ(katana::cad::formatLint({}, 3), "3 rules checked: no problems found\n");
}

// ---- the coding report, as text ---------------------------------------------------

TEST(FormatCodingReport, SaysWhatHappenedOverallAndCodeByCode)
{
    SurveyMap map;
    ASSERT_TRUE(map.add(mapData("WM*", "SURVEY SERVICES", "", "WATR Main")).ok());
    Document document;
    document.setSurveyMap(std::move(map));
    addCodedPoint(document, Point2(0, 0), "WM01");
    addCodedPoint(document, Point2(1, 0), "ZZ99");
    addCodedPoint(document, Point2(2, 0), "WM01");

    katana::cad::SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(document, {}, &report);
    ASSERT_TRUE(command.ok());
    // By hand: three coded, the two WM01 matched by WM*, ZZ99 by nothing (no
    // `*` here). Both WM01 points move from layer "0" and get the new style;
    // no library is loaded, so WATR Main is missing.
    EXPECT_EQ(katana::cad::formatCodingReport(report),
              "3 entities carry a code in \"code\": 2 matched, 0 fallback-only (only the bare * "
              "rule answers), 1 with no rule\n"
              "2 entities changed\n"
              "Layers created: SURVEY SERVICES\n"
              "Styles created: WATR Main\n"
              "Codes with no rule: ZZ99\n"
              "Named but in no loaded library: WATR Main\n"
              "By code:\n"
              "  WM01: 2 entities, prefix, matched; layer 0 -> SURVEY SERVICES; style WATR Main "
              "(created); 2 changed\n"
              "  ZZ99: 1 entity, none, not matched; 0 changed\n");
}

TEST(FormatCoverage, NamesBuiltInShapesApartFromMissingNames)
{
    katana::cad::CustomisationCoverage coverage;
    coverage.styles = 3;
    coverage.named = 2;
    coverage.resolved = 1;
    coverage.builtIn = 1;
    coverage.unresolved = {"X"};
    EXPECT_EQ(katana::cad::formatCoverage(coverage),
              "1 of this drawing's 3 styles are drawn with a loaded definition (2 name one, 1 use "
              "a shape Katana draws itself; the rest are plain lines)\n"
              "  1 name is in no loaded library: \"X\"\n");
}
