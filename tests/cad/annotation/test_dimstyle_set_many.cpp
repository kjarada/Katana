// DIMSTYLE with several fields on one line (annotation/dimension_style_verbs.hpp,
// docs/annotation.md "Dimension styles"): what the window's Dimension Styles
// manager sends when Apply or Duplicate is pressed, so each is ONE undo step,
// and a line with one bad pair changes nothing. Also the words a dialog puts
// on a line (command_words.hpp), which must read back as the value shown.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/cad/annotation/command_words.hpp"
#include "katana/cad/annotation/dimension_style_verbs.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::entity::ArrowHead;
using katana::entity::DimensionStyle;
namespace ann = katana::cad::annotation;

namespace {

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> " << reply.error().describe();
        return reply.ok() ? *reply : std::string();
    }
    // The refusal's code; Internal, which no DIMSTYLE refusal uses, when
    // the line was carried out after all.
    ErrorCode refused(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was carried out: " << *reply;
        return reply.ok() ? ErrorCode::Internal : reply.error().code;
    }
    [[nodiscard]] std::size_t steps() const { return document.history().undoCount(); }
    [[nodiscard]] const DimensionStyle& style(const std::string& name) const
    {
        return *document.model().dimensionStyles.find(name);
    }
};

} // namespace

TEST(DimstyleSetMany, SeveralPairsOnOneLineAreOneUndoStep)
{
    Session s;
    s.run("DIMSTYLE NEW site");
    const std::size_t before = s.steps();
    s.run("DIMSTYLE SET site TEXT 3.5 GAP 0.8 HEAD Tick PREFIX \"L= \" PAPER on DECIMALS 2");
    EXPECT_EQ(s.steps(), before + 1) << "six fields, one step";
    const DimensionStyle& site = s.style("site");
    EXPECT_DOUBLE_EQ(site.textHeight, 3.5);
    EXPECT_DOUBLE_EQ(site.textGap, 0.8);
    EXPECT_EQ(site.arrowHead, ArrowHead::Tick);
    EXPECT_EQ(site.prefix, "L= ");
    EXPECT_TRUE(site.paperSized);
    EXPECT_EQ(site.decimals, 2);

    // One UNDO takes back every field the line set.
    s.run("UNDO");
    EXPECT_EQ(s.style("site"), DimensionStyle{.name = "site"});
}

TEST(DimstyleSetMany, OneBadPairRefusesTheWholeLineAndChangesNothing)
{
    Session s;
    s.run("DIMSTYLE NEW site");
    const std::size_t before = s.steps();
    EXPECT_EQ(s.refused("DIMSTYLE SET site TEXT 3.5 HEAD Diamond"), ErrorCode::ParseFailure);
    EXPECT_EQ(s.refused("DIMSTYLE SET site TEXT 3.5 NOSUCH 1"), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.refused("DIMSTYLE SET site TEXT 3.5 GAP"), ErrorCode::InvalidArgument)
        << "a field with no value";
    EXPECT_EQ(s.refused("DIMSTYLE SET site TEXT 3.5 ARROW 0"), ErrorCode::InvalidArgument)
        << "every pair reads, but the style it makes is invalid";
    EXPECT_EQ(s.refused("DIMSTYLE SET site TEXT"), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.refused("DIMSTYLE SET site"), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.style("site"), DimensionStyle{.name = "site"}) << "the TEXT before each was kept";
    EXPECT_EQ(s.steps(), before);
}

TEST(DimstyleSetMany, SwitchesAndPlacesAreReadStrictly)
{
    // "PAPER of" once turned paper sizing off without a word, and "DECIMALS
    // 2.7" gave two places.
    Session s;
    s.run("DIMSTYLE NEW site PAPER on");
    EXPECT_EQ(s.refused("DIMSTYLE SET site PAPER of"), ErrorCode::ParseFailure);
    EXPECT_TRUE(s.style("site").paperSized);
    EXPECT_EQ(s.refused("DIMSTYLE SET site DECIMALS 2.7"), ErrorCode::ParseFailure);
    s.run("DIMSTYLE SET site TRIM yes PAPER no");
    EXPECT_TRUE(s.style("site").suppressTrailingZeros);
    EXPECT_FALSE(s.style("site").paperSized);
}

TEST(DimstyleSetMany, NewWithFieldsMakesTheStyleInOneStep)
{
    Session s;
    const std::size_t before = s.steps();
    s.run("DIMSTYLE NEW copy TEXT 5 SUFFIX mm SCALE 1000");
    EXPECT_EQ(s.steps(), before + 1);
    EXPECT_DOUBLE_EQ(s.style("copy").textHeight, 5.0);
    EXPECT_EQ(s.style("copy").suffix, "mm");
    s.run("UNDO");
    EXPECT_FALSE(s.document.model().dimensionStyles.contains("copy"));
    EXPECT_EQ(s.refused("DIMSTYLE NEW bad TEXT 5 HEAD Diamond"), ErrorCode::ParseFailure);
    EXPECT_FALSE(s.document.model().dimensionStyles.contains("bad"));
}

TEST(DimstyleSetMany, InfoIsOneRecordOfEveryField)
{
    // The defaults are DimensionStyle's (tables.hpp: DIMTXT 2.5, DIMGAP and
    // DIMEXO 0.625, DIMEXE 1.25, DIMASZ 2.5, three places), and ten units at
    // three places read "10.000".
    Session s;
    EXPECT_EQ(s.run("DIMSTYLE INFO Standard"),
              "name=Standard text=2.5 gap=0.625 extoff=0.625 extbeyond=1.25 arrow=2.5 "
              "head=ClosedFilled scale=1 decimals=3 round=0 prefix=\"\" suffix=\"\" trim=off "
              "paper=off layers=0 reads=10.000");
    s.run("DIMSTYLE NEW site SCALE 1000 DECIMALS 1 SUFFIX mm");
    s.run("LAYER NEW dims");
    s.run("LAYER DIMSTYLE dims site");
    const std::string info = s.run("DIMSTYLE INFO site");
    EXPECT_NE(info.find(" layers=1 "), std::string::npos) << info;
    EXPECT_NE(info.find(" reads=10000.0mm"), std::string::npos) << info;
    EXPECT_EQ(s.refused("DIMSTYLE INFO missing"), ErrorCode::NotFound);
    EXPECT_EQ(ann::layersUsingDimensionStyle(s.document.model(), "site"),
              std::vector<std::string>{"dims"});
}

TEST(DimstyleSetMany, TheChangesBetweenTwoStylesReadBackAsTheSecond)
{
    Session s;
    s.run("DIMSTYLE NEW site");
    const DimensionStyle from = s.style("site");
    DimensionStyle to = from;
    to.textHeight = 1.8;
    to.textGap = 0.1 + 0.2; // not a short decimal: it must come back exactly
    to.arrowHead = ArrowHead::Open;
    to.decimals = 1;
    to.roundTo = 0.05;
    to.prefix = "R ";
    to.suffix = "m";
    to.paperSized = true;

    const auto changes = ann::dimensionStyleChanges(from, to);
    ASSERT_TRUE(changes.ok()) << changes.error().describe();
    EXPECT_EQ(changes->find("EXTOFF"), std::string::npos) << "an unchanged field is not written";
    EXPECT_NE(changes->find("PREFIX \"R \""), std::string::npos) << *changes;
    s.run("DIMSTYLE SET site " + *changes);
    EXPECT_EQ(s.style("site"), to);

    // Back again, the suffix emptied: an empty word is "".
    const auto back = ann::dimensionStyleChanges(to, from);
    ASSERT_TRUE(back.ok());
    EXPECT_NE(back->find("SUFFIX \"\""), std::string::npos) << *back;
    s.run("DIMSTYLE SET site " + *back);
    EXPECT_EQ(s.style("site"), from);

    EXPECT_EQ(*ann::dimensionStyleChanges(from, from), "") << "nothing to set";
    DimensionStyle quoted = from;
    quoted.suffix = "\"";
    EXPECT_FALSE(ann::dimensionStyleChanges(from, quoted).ok())
        << "a double quote cannot be written on a line, so it is refused, not cut short";
}

TEST(DimstyleSetMany, EveryFieldHasAValueThatSetsItBack)
{
    // Each field written by dimensionStyleFieldValue and read by
    // setDimensionStyleField gives the same style: the record and the line
    // speak one language.
    DimensionStyle style{.name = "x"};
    style.textHeight = 3.25;
    style.arrowHead = ArrowHead::Dot;
    style.suppressTrailingZeros = true;
    style.prefix = "~";
    DimensionStyle copy{.name = "x"};
    for (const std::string_view field : ann::dimensionStyleFields()) {
        ASSERT_TRUE(
            ann::setDimensionStyleField(copy, field, ann::dimensionStyleFieldValue(style, field))
                .ok())
            << field;
    }
    EXPECT_EQ(copy, style);
    EXPECT_EQ(ann::dimensionStyleFields().size(), 13u)
        << "TEXT GAP EXTOFF EXTBEYOND ARROW HEAD SCALE DECIMALS ROUND PREFIX SUFFIX TRIM PAPER";
    EXPECT_EQ(ann::setDimensionStyleField(style, "colour", "red").error().code,
              ErrorCode::InvalidArgument);
}

TEST(DimstyleSetMany, TheHelpGivesTheManyPairSet)
{
    const std::string help = CommandInterpreter::helpText();
    EXPECT_NE(help.find("DIMSTYLE SET name field value [field value ...]"), std::string::npos);
    EXPECT_NE(help.find("INFO name"), std::string::npos);
}

TEST(CommandWords, AValueIsQuotedOnlyWhenItMustBeAndRefusedWhenItCannotBe)
{
    EXPECT_EQ(*ann::commandWord("Notes"), "Notes");
    EXPECT_EQ(*ann::commandWord("Road names"), "\"Road names\"");
    EXPECT_EQ(*ann::commandWord(""), "\"\"");
    EXPECT_EQ(*ann::commandWord("a\tb"), "\"a\tb\"");
    EXPECT_EQ(ann::commandWord("5\" pipe").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(ann::commandWord("two\nlines").error().code, ErrorCode::InvalidArgument);

    // What the tokenizer reads back is the value.
    for (const char* value : {"Notes", "Road names", "", "L= "}) {
        const auto words = CommandInterpreter::tokenize("X " + *ann::commandWord(value));
        ASSERT_TRUE(words.ok());
        ASSERT_EQ(words->size(), 2u) << value;
        EXPECT_EQ((*words)[1], value);
    }
}

TEST(CommandWords, AnAnnotationTextWritesItsLineBreaksAsBackslashN)
{
    EXPECT_EQ(*ann::annotationTextWord("PIT 12\nIL 10.50"), "\"PIT 12\\nIL 10.50\"");
    EXPECT_EQ(*ann::annotationTextWord("A\r\nB"), "A\\nB") << "a Windows line end is one break";
    EXPECT_EQ(ann::annotationTextWord("C:\\new").error().code, ErrorCode::InvalidArgument)
        << "the verb would read the backslash and n as a break";
    EXPECT_EQ(ann::annotationTextWord("say \"hi\"").error().code, ErrorCode::InvalidArgument);

    // Through the verb: the text it makes has the breaks.
    Session s;
    s.run("TEXT 0,0 2.5 " + *ann::annotationTextWord("first line\nsecond") + " justify=TL");
    const auto* text = std::get_if<katana::entity::TextGeometry>(
        &s.document.model().entities.find(1)->geometry);
    ASSERT_NE(text, nullptr);
    EXPECT_EQ(text->text, "first line\nsecond");
    EXPECT_EQ(ann::recordValue("a b"), "\"a b\"");
    EXPECT_EQ(ann::recordValue("x=1"), "\"x=1\"");
    EXPECT_EQ(ann::recordValue("plain"), "plain");
}
