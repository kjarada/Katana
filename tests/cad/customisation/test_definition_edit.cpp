// cad/definition_edit.hpp: a definition's strokes as the text an editor shows
// and takes back, the name a copy starts under, and the line that removes one.
//
// Every definition expected here is built by hand, and every text is written
// out by the rules of docs/customisation.md ("Layout"): one stroke a line,
// `["move", x, y]` with a blank after each comma, a number as its shortest
// text, a comma ending every line but the last.

#include <gtest/gtest.h>

#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/definition_edit.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/tables.hpp"

using katana::cad::Document;
using katana::cad::freeDefinitionName;
using katana::cad::readStrokeText;
using katana::cad::removeDefinitionLine;
using katana::cad::StrokeTextRead;
using katana::cad::strokeText;
using katana::core::ErrorCode;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StrokeText;
using katana::entity::StyleUnits;

namespace {

Stroke move(double x, double y) { return Stroke{.op = StrokeOp::Move, .point = {x, y}}; }
Stroke draw(double x, double y) { return Stroke{.op = StrokeOp::Draw, .point = {x, y}}; }

// A paper linestyle of length 12 with no strokes: the members a text is read
// under.
LineStyle kerbMembers()
{
    LineStyle members;
    members.name = "TEST Kerb";
    members.group = "Test/Lines";
    members.units = StyleUnits::Paper;
    members.length = 12.0;
    members.source = "Site";
    return members;
}

bool contains(const std::string& text, std::string_view part)
{
    return text.find(part) != std::string::npos;
}

TEST(DefinitionEdit, TheStrokesOfADefinitionAreTheLinesAFileHoldsOneStrokeALine)
{
    LineStyle kerb = kerbMembers();
    StrokeText letter;
    letter.text = "W";
    letter.height = 1.5;
    kerb.texts = {letter};
    kerb.strokes = {move(0.0, 0.0), draw(8.0, 0.0), Stroke{.op = StrokeOp::Circle, .radius = 0.5},
                    Stroke{.op = StrokeOp::Text, .text = 0}};
    const auto text = strokeText(kerb);
    ASSERT_TRUE(text.ok()) << text.error().describe();
    EXPECT_EQ(*text, "[\"move\", 0, 0],\n"
                     "[\"draw\", 8, 0],\n"
                     "[\"circle\", 0.5],\n"
                     "[\"text\", {\"text\": \"W\", \"height\": 1.5}]");

    // No strokes, no lines - not the members' own line.
    const auto none = strokeText(kerbMembers());
    ASSERT_TRUE(none.ok());
    EXPECT_EQ(*none, "");

    // What the format cannot write has no text: a move that carries a radius.
    LineStyle stray = kerbMembers();
    stray.strokes = {Stroke{.op = StrokeOp::Move, .point = {0.0, 0.0}, .radius = 2.0}};
    const auto refused = strokeText(stray);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
}

TEST(DefinitionEdit, TextReadsAsTheDefinitionItsMembersAndItsLinesDescribe)
{
    // The second line without the comma a file would end it with, a blank
    // line passed over, Windows line ends, and blanks round a line.
    const StrokeTextRead read = readStrokeText(kerbMembers(), "[\"move\", 0, 0],\r\n"
                                                              "  [\"draw\", 8, 0]  \r\n"
                                                              "\r\n"
                                                              "[\"dot\", 0.25],\r\n");
    ASSERT_TRUE(read.definition.has_value()) << read.problem;
    LineStyle expected = kerbMembers();
    expected.strokes = {move(0.0, 0.0), draw(8.0, 0.0),
                        Stroke{.op = StrokeOp::Dot, .radius = 0.25}};
    EXPECT_TRUE(*read.definition == expected);
    EXPECT_EQ(read.line, 0);
    EXPECT_TRUE(read.problem.empty());

    // The members' own strokes and texts are not looked at: the text is all
    // the strokes there are, and no text is no strokes.
    LineStyle withOld = kerbMembers();
    withOld.strokes = {move(1.0, 1.0), draw(2.0, 2.0)};
    const StrokeTextRead none = readStrokeText(withOld, "\n  \n");
    ASSERT_TRUE(none.definition.has_value()) << none.problem;
    EXPECT_TRUE(*none.definition == kerbMembers());

    // Which list holds it is the members' to say.
    LineStyle symbol = kerbMembers();
    symbol.symbol = true;
    symbol.atVertices = true;
    const StrokeTextRead asSymbol = readStrokeText(symbol, "[\"move\", 0, 0]");
    ASSERT_TRUE(asSymbol.definition.has_value()) << asSymbol.problem;
    EXPECT_TRUE(asSymbol.definition->symbol);
    EXPECT_TRUE(asSymbol.definition->atVertices);
}

TEST(DefinitionEdit, ATextOfAStrokeIsKeptCharacterForCharacter)
{
    // A no-break space (U+00A0) and a line separator (U+2028) are characters
    // of a text like any other; neither is a blank or a line end of the
    // lines themselves.
    const std::string label = "A\xC2\xA0" "B\xE2\x80\xA8" "C";
    const StrokeTextRead read =
        readStrokeText(kerbMembers(), "[\"move\", 0, 0],\n[\"text\", {\"text\": \"" + label +
                                          "\", \"height\": 1}]");
    ASSERT_TRUE(read.definition.has_value()) << read.problem;
    ASSERT_EQ(read.definition->texts.size(), 1u);
    EXPECT_EQ(read.definition->texts.front().text, label);
    // And written back, the same bytes.
    const auto text = strokeText(*read.definition);
    ASSERT_TRUE(text.ok());
    EXPECT_TRUE(contains(*text, label)) << *text;
}

TEST(DefinitionEdit, AStrokeTheReaderRefusesIsNamedByItsLineInTheReadersOwnWords)
{
    // A word that is no kind of stroke on the third line: the reader names
    // the stroke by its place counted from 0, and the line is put in front.
    const StrokeTextRead typo = readStrokeText(kerbMembers(), "[\"move\", -1, 0],\n"
                                                              "[\"draw\", 1, 0],\n"
                                                              "[\"drow\", 0, -1],\n"
                                                              "[\"draw\", 0, 1]");
    EXPECT_FALSE(typo.definition.has_value());
    EXPECT_EQ(typo.line, 3);
    EXPECT_TRUE(typo.problem.starts_with(
        "Line 3: definition \"TEST Kerb\" strokes[2]: \"drow\" is not a kind of stroke"))
        << typo.problem;
    EXPECT_EQ(typo.error.code, ErrorCode::ParseFailure);

    // The line is the text's own, not the stroke's place: a blank line above
    // moves the one and not the other.
    const StrokeTextRead short1 = readStrokeText(kerbMembers(), "[\"move\", 0, 0],\n"
                                                                "\n"
                                                                "[\"arc\", 1, 90]");
    EXPECT_EQ(short1.line, 3);
    EXPECT_TRUE(short1.problem.starts_with(
        "Line 3: definition \"TEST Kerb\" strokes[1]: \"arc\" takes a radius, a start angle "
        "and an end angle (3 after the word) and has 2"))
        << short1.problem;

    // What entity::validate refuses of a stroke is named by its line too.
    const StrokeTextRead below = readStrokeText(
        kerbMembers(), "[\"move\", 0, 0],\n[\"text\", {\"text\": \"W\", \"height\": -1}]");
    EXPECT_EQ(below.line, 2);
    EXPECT_TRUE(contains(below.problem, "linestyle text height is negative")) << below.problem;
}

TEST(DefinitionEdit, ARefusalNeverCitesAPlaceInTextNobodyTyped)
{
    // The lines are read under a first line of this function's making, so
    // the reader's "line 2, column 14" is of a text the person never saw.

    // A number too large to hold. The reader's words are `this stroke has a
    // number too large to hold`, then what was written, then its place - and
    // the place is what is left out.
    const StrokeTextRead large = readStrokeText(kerbMembers(), "[\"move\", 1e999, 0]");
    EXPECT_EQ(large.line, 1);
    EXPECT_EQ(large.problem, "Line 1: definition \"TEST Kerb\" strokes[0]: this stroke has a "
                             "number too large to hold - 1e999");

    // A line missing its closing bracket is short of a "]" - not, as the
    // text it is read in would have it, met by a "}" nobody typed.
    const StrokeTextRead open = readStrokeText(kerbMembers(), "[\"move\", 0, 0");
    EXPECT_EQ(open.line, 1);
    EXPECT_TRUE(open.problem.starts_with("Line 1 is not a stroke: ")) << open.problem;
    EXPECT_TRUE(contains(open.problem, "expected ']'")) << open.problem;
    EXPECT_FALSE(contains(open.problem, "}")) << open.problem;

    // A word cut short - which every word of a stroke is while it is being
    // typed - is a string with no closing quote, and is said of what was
    // typed: no bracket of the text it is read in runs on into it.
    const StrokeTextRead cut = readStrokeText(kerbMembers(), "[\"move\", 0, 0],\n[\"dr");
    EXPECT_EQ(cut.line, 2);
    EXPECT_TRUE(cut.problem.starts_with("Line 2 is not a stroke: ")) << cut.problem;
    EXPECT_TRUE(contains(cut.problem, "\"dr")) << cut.problem;
    EXPECT_FALSE(contains(cut.problem, "]")) << cut.problem;
    EXPECT_FALSE(contains(cut.problem, "}")) << cut.problem;

    // Two strokes on one line are joined like any two, so a fault in the
    // second is said of it - not of there being something after the first.
    const StrokeTextRead two =
        readStrokeText(kerbMembers(), "[\"move\", 0, 0], [\"draw\", 1 0]");
    EXPECT_EQ(two.line, 1);
    EXPECT_TRUE(two.problem.starts_with("Line 1 is not a stroke: ")) << two.problem;
    EXPECT_FALSE(contains(two.problem, "end of input")) << two.problem;
    const StrokeTextRead twoSound =
        readStrokeText(kerbMembers(), "[\"move\", 0, 0], [\"draw\", 1, 0]");
    ASSERT_TRUE(twoSound.definition.has_value()) << twoSound.problem;
    EXPECT_EQ(twoSound.definition->strokes.size(), 2u);

    // Text that is not JSON, on the fourth line of the text.
    const StrokeTextRead comma = readStrokeText(kerbMembers(), "[\"move\", -1, 0],\n"
                                                               "\n"
                                                               "[\"draw\", 1, 0],\n"
                                                               "[\"draw\", 0 1]");
    EXPECT_EQ(comma.line, 4);
    EXPECT_TRUE(comma.problem.starts_with("Line 4 is not a stroke: ")) << comma.problem;

    // A line that is nothing but a comma is not a stroke either, and is not
    // passed over as a blank one is.
    const StrokeTextRead lone =
        readStrokeText(kerbMembers(), "[\"move\", 0, 0],\n,\n[\"draw\", 1, 0]");
    EXPECT_EQ(lone.line, 2);

    // JSON has no word for a number that is not one.
    for (const char* notANumber : {"inf", "nan", "NaN", "Infinity", "-inf"}) {
        const StrokeTextRead read = readStrokeText(
            kerbMembers(), std::string("[\"move\", 0, 0],\n[\"draw\", ") + notANumber + ", 0]");
        EXPECT_FALSE(read.definition.has_value()) << notANumber;
        EXPECT_EQ(read.line, 2) << notANumber;
        EXPECT_TRUE(read.problem.starts_with("Line 2 is not a stroke: ")) << read.problem;
    }

    for (const StrokeTextRead* each : {&large, &open, &cut, &two, &comma, &lone}) {
        EXPECT_FALSE(contains(each->problem, "column")) << each->problem;
        EXPECT_FALSE(contains(each->problem, " at line ")) << each->problem;
        EXPECT_FALSE(each->definition.has_value());
    }
}

TEST(DefinitionEdit, TextThatIsMoreThanStrokesAndMembersThatAreNoDefinitionAreRefusedOfNoLine)
{
    // A line that ends the list of strokes and goes on to give members: sound
    // JSON, and a definition at vertices under members that say it is not.
    const StrokeTextRead more = readStrokeText(
        kerbMembers(), "[\"move\", 0, 0]], \"atVertices\": true, \"anchors\": [[0, 0], [1, 1]");
    EXPECT_FALSE(more.definition.has_value());
    EXPECT_EQ(more.line, 0);
    EXPECT_TRUE(more.problem.starts_with("The strokes hold more than strokes")) << more.problem;
    EXPECT_EQ(more.error.code, ErrorCode::InvalidArgument);

    // entity::validate's two rules for a definition's own numbers, in its
    // words; they are a member's, so no line is named.
    LineStyle negative = kerbMembers();
    negative.length = -1.0;
    const StrokeTextRead length = readStrokeText(negative, "[\"move\", 0, 0]");
    EXPECT_FALSE(length.definition.has_value());
    EXPECT_EQ(length.line, 0);
    EXPECT_TRUE(contains(length.problem, "linestyle length must be finite and not negative"))
        << length.problem;
    LineStyle flat = kerbMembers();
    flat.factor = 0.0;
    const StrokeTextRead factor = readStrokeText(flat, "");
    EXPECT_FALSE(factor.definition.has_value());
    EXPECT_TRUE(contains(factor.problem, "linestyle factor must be finite and greater than zero"))
        << factor.problem;
}

TEST(DefinitionEdit, ACopyStartsUnderTheFirstNameNeitherTheLibraryNorTheDrawingHolds)
{
    Document document;
    katana::entity::StyleLibrary library;
    for (const char* name : {"TEST Valve", "TEST Valve 2"}) {
        LineStyle definition;
        definition.name = name;
        ASSERT_TRUE(library.add(definition).ok());
    }
    document.setStyleLibrary(std::move(library));
    // The drawing's own linetype table holds the next: a library linestyle of
    // that name would be drawn in its place (decision D2).
    katana::entity::Linetype dashes;
    dashes.name = "TEST Valve 3";
    dashes.pattern = {katana::entity::LinetypeElement{2.0}, katana::entity::LinetypeElement{-1.0}};
    const auto made = document.execute(katana::commands::createLinetype(dashes));
    ASSERT_TRUE(made.ok()) << made.error().describe();

    EXPECT_EQ(freeDefinitionName(document, "TEST Valve"), "TEST Valve 4");
    EXPECT_EQ(freeDefinitionName(document, "TEST Valve 2"), "TEST Valve 2 2");
    // A name that is free is itself the answer.
    EXPECT_EQ(freeDefinitionName(document, "TEST Hydrant"), "TEST Hydrant");
    EXPECT_EQ(freeDefinitionName(Document{}, "TEST Valve"), "TEST Valve");
}

TEST(DefinitionEdit, TheRemoveLineNamesTheDefinitionInQuotesAndReadsBackAsItsWords)
{
    const auto plain = removeDefinitionLine("TEST Valve", false);
    ASSERT_TRUE(plain.ok()) << plain.error().describe();
    EXPECT_EQ(*plain, "CUSTOMISE REMOVE \"TEST Valve\"");
    const auto forced = removeDefinitionLine("TEST Valve", true);
    ASSERT_TRUE(forced.ok());
    EXPECT_EQ(*forced, "CUSTOMISE REMOVE \"TEST Valve\" FORCE");
    // A name with no blank is quoted all the same, as the line is documented.
    const auto oneWord = removeDefinitionLine("Kerb", false);
    ASSERT_TRUE(oneWord.ok());
    EXPECT_EQ(*oneWord, "CUSTOMISE REMOVE \"Kerb\"");

    // What the interpreter makes of the line is the three or four words meant.
    using katana::cad::CommandInterpreter;
    const auto words = CommandInterpreter::tokenize(*forced);
    ASSERT_TRUE(words.ok()) << words.error().describe();
    EXPECT_EQ(*words, (std::vector<std::string>{"CUSTOMISE", "REMOVE", "TEST Valve", "FORCE"}));
    // A name that only begins with one of the line's words is a name.
    const auto main = removeDefinitionLine("Force main", true);
    ASSERT_TRUE(main.ok());
    const auto mainWords = CommandInterpreter::tokenize(*main);
    ASSERT_TRUE(mainWords.ok()) << mainWords.error().describe();
    EXPECT_EQ(*mainWords,
              (std::vector<std::string>{"CUSTOMISE", "REMOVE", "Force main", "FORCE"}));
    EXPECT_TRUE(removeDefinitionLine("CODES", false).ok());
}

TEST(DefinitionEdit, ANameNoLineCanNameIsRefusedRatherThanWrittenAsSomethingElse)
{
    // A word of a command line has no escape for a double quote: the line
    // would run on another name, cut short at the quote.
    const auto quoted = removeDefinitionLine("TEST 6\" Pipe", false);
    ASSERT_FALSE(quoted.ok());
    EXPECT_EQ(quoted.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(quoted.error().message, "a double quote cannot be written on a command line");
    EXPECT_FALSE(removeDefinitionLine("two\nlines", false).ok());

    // The quotes do not survive the tokenizer, so a definition named as one
    // of the line's own words is read as that word: `CUSTOMISE REMOVE "CODE"
    // FORCE` would remove the survey code FORCE.
    for (const char* word : {"CODE", "code", "Code", "FORCE", "force"}) {
        for (const bool force : {false, true}) {
            const auto refused = removeDefinitionLine(word, force);
            ASSERT_FALSE(refused.ok()) << word;
            EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
            EXPECT_TRUE(contains(refused.error().message, "CUSTOMISE REMOVE"))
                << refused.error().message;
        }
    }
    // No name is no definition.
    EXPECT_FALSE(removeDefinitionLine("", false).ok());
}

} // namespace
