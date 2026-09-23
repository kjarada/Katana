// The survey-code manager's helpers below Qt: the code list as CSV, the key a
// new rule for a drawing's code starts from, and attribute lists as text.
// Every expected string is written out by hand from the rules the test
// builds, with the working beside it - never captured from a run.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/cad/code_edit.hpp"
#include "katana/entity/survey_map.hpp"

using katana::cad::codeListCsv;
using katana::cad::csvField;
using katana::cad::formatAttributeLines;
using katana::cad::parseAttributeLines;
using katana::cad::suggestedKey;
using katana::entity::SurveyAttribute;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyMap;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;
using katana::entity::SurveySymbol;

TEST(CodeListCsv, AFieldIsQuotedOnlyWhenItHoldsACommaAQuoteOrALineBreakAndItsQuotesAreDoubled)
{
    EXPECT_EQ(csvField("Water main"), "Water main");
    EXPECT_EQ(csvField(""), "");
    // Blanks are data: RFC 4180 does not quote for them, and nor does this.
    EXPECT_EQ(csvField(" padded "), " padded ");
    EXPECT_EQ(csvField("Kerb, barrier"), "\"Kerb, barrier\"");
    // 12" pipe -> the quote doubled, the whole enclosed: "12"" pipe"
    EXPECT_EQ(csvField("12\" pipe"), "\"12\"\" pipe\"");
    EXPECT_EQ(csvField("two\nlines"), "\"two\nlines\"");
    EXPECT_EQ(csvField("cr\rhere"), "\"cr\rhere\"");
}

TEST(CodeListCsv, EachKeyIsOneRecordOfWhatItResolvesToWithUnsetFieldsLeftEmpty)
{
    SurveyMap map;
    // WM*: a line code, whose comment holds a comma and a quote.
    SurveyRule water;
    water.key = "WM*";
    water.model = "SURVEY SERVICES";
    water.colour = "blue";
    water.breakline = SurveyBreakline::Line;
    water.linestyle = "WATR Main";
    water.comment = "Water main, 6\" cast iron";
    water.group = "SERVICES";
    ASSERT_TRUE(map.add(water).ok());
    // AC*: a point code, in two sections - map_data and vertex_symbol_data.
    SurveyRule chamber;
    chamber.key = "AC*";
    chamber.model = "FURNITURE";
    chamber.colour = "white";
    chamber.breakline = SurveyBreakline::Point;
    chamber.linestyle = "0";
    chamber.comment = "Access chamber";
    ASSERT_TRUE(map.add(chamber).ok());
    SurveyRule chamberSymbol;
    chamberSymbol.key = "AC*";
    chamberSymbol.section = SurveySection::VertexSymbol;
    chamberSymbol.symbol = SurveySymbol{.style = "Survey Mark", .size = 1.5};
    ASSERT_TRUE(map.add(chamberSymbol).ok());
    // TR*: a symbol whose size is 0, the map's "the definition's own size".
    SurveyRule tree;
    tree.key = "TR*";
    tree.section = SurveySection::VertexSymbol;
    tree.symbol = SurveySymbol{.style = "Tree", .size = 0.0};
    ASSERT_TRUE(map.add(tree).ok());
    // GS: tinable_data only.
    SurveyRule ground;
    ground.key = "GS";
    ground.section = SurveySection::Tinable;
    ground.tinable = true;
    ASSERT_TRUE(map.add(ground).ok());

    // Key order (byte order): "AC*" < "GS" < "TR*" < "WM*".
    //   AC*: comment, no group, model, colour, point, "0", symbol, 1.5, no tinable
    //   GS:  nothing but tinable yes
    //   TR*: symbol "Tree", and its size 0 written as nothing
    //   WM*: the comment quoted, its quote doubled
    const std::string expected =
        "code,description,group,layer,colour,line/point,linestyle,symbol,size,tinable\r\n"
        "AC*,Access chamber,,FURNITURE,white,point,0,Survey Mark,1.5,\r\n"
        "GS,,,,,,,,,yes\r\n"
        "TR*,,,,,,,Tree,,\r\n"
        "WM*,\"Water main, 6\"\" cast iron\",SERVICES,SURVEY SERVICES,blue,line,WATR Main,,,\r\n";
    EXPECT_EQ(codeListCsv(map), expected);
}

TEST(CodeListCsv, AKeyCaughtByALessSpecificKeyTakesWhatThatKeySaysAndItDoesNotSayItself)
{
    SurveyMap map;
    SurveyRule broad;
    broad.key = "W*";
    broad.model = "WATER";
    broad.group = "SERVICES";
    ASSERT_TRUE(map.add(broad).ok());
    SurveyRule narrow;
    narrow.key = "WM*";
    narrow.comment = "Water main";
    ASSERT_TRUE(map.add(narrow).ok());
    // W*: its own model and group. WM*: its own comment, and W*'s model and
    // group, because lookup("WM*") meets both keys - WM* first.
    EXPECT_EQ(codeListCsv(map),
              "code,description,group,layer,colour,line/point,linestyle,symbol,size,tinable\r\n"
              "W*,,SERVICES,WATER,,,,,,\r\n"
              "WM*,Water main,SERVICES,WATER,,,,,,\r\n");
}

TEST(CodeListCsv, AnEmptyMapIsTheHeaderAlone)
{
    EXPECT_EQ(codeListCsv(SurveyMap{}),
              "code,description,group,layer,colour,line/point,linestyle,symbol,size,tinable\r\n");
}

TEST(SuggestedKey, ACodeEndingInAStringNumberGetsAPrefixKeyOnWhatComesBeforeIt)
{
    EXPECT_EQ(suggestedKey("WM01"), "WM*");
    EXPECT_EQ(suggestedKey("KB12"), "KB*");
    // The blank between the code and its number belongs to neither.
    EXPECT_EQ(suggestedKey("KB 12"), "KB*");
    // Only the TRAILING number is the string number.
    EXPECT_EQ(suggestedKey("P2X7"), "P2X*");
}

TEST(SuggestedKey, ACodeWithNoNumberOrNothingButANumberStaysExactAndIsTrimmed)
{
    EXPECT_EQ(suggestedKey("PABB"), "PABB");
    EXPECT_EQ(suggestedKey("  PABB "), "PABB");
    // "*" alone would catch every code.
    EXPECT_EQ(suggestedKey("105"), "105");
    EXPECT_EQ(suggestedKey("   "), "");
}

TEST(AttributeLines, AttributesBecomeOneLineEachAndReadBackAsThemselves)
{
    const std::vector<SurveyAttribute> attributes{{"text", "Source", "Field survey"},
                                                  {"text", "Diameter", "$PipeDiameter"},
                                                  {"integer", "Pit Number", "12"}};
    const std::string text = formatAttributeLines(attributes);
    EXPECT_EQ(text, "text Source = Field survey\n"
                    "text Diameter = $PipeDiameter\n"
                    "integer Pit Number = 12\n");
    const auto back = parseAttributeLines(text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_EQ(*back, attributes);
}

TEST(AttributeLines, BlankLinesAreSkippedAndTheBlanksAroundTheEqualsSignAreNotData)
{
    const auto parsed = parseAttributeLines("\n  text   Source=Survey = 2 \r\n\n");
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    // The FIRST '=' ends the name; the value keeps its own '='.
    ASSERT_EQ(parsed->size(), 1u);
    EXPECT_EQ((*parsed)[0], (SurveyAttribute{"text", "Source", "Survey = 2"}));
    const auto none = parseAttributeLines("");
    ASSERT_TRUE(none.ok());
    EXPECT_TRUE(none->empty());
}

TEST(AttributeLines, ALineWithNoEqualsNoNameOrAnUnknownTypeIsRefusedNamingTheLine)
{
    const auto noEquals = parseAttributeLines("text A = 1\ntext B 2\n");
    ASSERT_FALSE(noEquals.ok());
    EXPECT_NE(noEquals.error().describe().find("attribute line 2"), std::string::npos)
        << noEquals.error().describe();
    EXPECT_FALSE(parseAttributeLines("text = 1").ok());
    EXPECT_FALSE(parseAttributeLines("real Height = 1.5").ok());
    EXPECT_FALSE(parseAttributeLines("Source = Survey").ok());
}
