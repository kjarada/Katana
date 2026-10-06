// cad::customisationPart (customisation_part.hpp): what a PART of a
// customisation is written as - the one rule CUSTOMISE EXPORT with a kind word
// or ONLY, the Survey Code Manager's Export Codes and the Symbol Library's
// Export Selected all cut by.
//
// Nothing is read from a file and no verb or dialog is run: the session is
// the value built below, and every expectation is worked out by hand from it
// and from the rule as the header states it.
//
//   name "Works", description "The works set.", notice "Works: all rights
//   reserved."; based on Works 0123456789abcdef; linework and automation said.
//
//   sources   Works   definitions, rules   no notice of its own (the
//                                          session's is its)
//             Client  rules                "Client codes, for this job only."
//             Marks   definitions          "Marks drawn by hand."
//             Tints   neither              "Tints: free to use."
//
//   colours   tint teal, tint rose, tint lime, tint plum
//
//   library   WORKS Peg    symbol, from Works   a pen "tint teal", a circle
//             WORKS Fence  linestyle, from Works   a move and a draw: no pen
//             MARK Cross   symbol, from Marks   a move and a draw: no pen
//             LOOSE Dot    symbol, made in a session (no source)
//                                               a pen "Tint_Rose", a dot
//
//   rules     #0 PG* feature   colour "tint teal"
//             #1 FN* feature   linestyle WORKS Fence, no colour
//             #2 MK* symbol    MARK Cross, its colour "TINT-LIME"
//             #3 TX* text      its colour "tint plum"
//
// A colour name is one name by its fold (entity::foldColourName: lower case,
// `_` and `-` as a blank), so "Tint_Rose" is tint rose and "TINT-LIME" tint
// lime; a table lists its entries in the order of the folded names.

#include <gtest/gtest.h>

#include <string>
#include <utility>
#include <vector>

#include "katana/cad/customisation_part.hpp"
#include "katana/entity/customisation.hpp"

using katana::cad::customisationPart;
using katana::entity::Color;
using katana::entity::Customisation;
using katana::entity::CustomisationSourceNote;
using katana::entity::CustomisationWriteOptions;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

namespace {

using Sources = std::vector<CustomisationSourceNote>;
using Lines = std::vector<std::string>;

const char* const kOwnNotice = "Works: all rights reserved.";
const char* const kClientNotice = "Client codes, for this job only.";
const char* const kMarksNotice = "Marks drawn by hand.";
const char* const kTintsNotice = "Tints: free to use.";

Stroke move(double x, double y) { return Stroke{.op = StrokeOp::Move, .point = {x, y}}; }
Stroke draw(double x, double y) { return Stroke{.op = StrokeOp::Draw, .point = {x, y}}; }
Stroke pen(const char* name) { return Stroke{.op = StrokeOp::Pen, .pen = name}; }

LineStyle definition(const char* name, bool symbol, const char* source,
                     std::vector<Stroke> strokes)
{
    LineStyle made;
    made.name = name;
    made.symbol = symbol;
    made.atVertices = symbol;
    made.source = source;
    made.strokes = std::move(strokes);
    return made;
}

Customisation works()
{
    Customisation session;
    session.name = "Works";
    session.description = "The works set.";
    session.notice = {kOwnNotice};
    session.sources = {{"Works", true, true, {}},
                       {"Client", false, true, {kClientNotice}},
                       {"Marks", true, false, {kMarksNotice}},
                       {"Tints", false, false, {kTintsNotice}}};
    session.basedOn = katana::entity::CustomisationBase{"Works", "0123456789abcdef"};
    EXPECT_TRUE(session.colours.add("tint teal", Color{0, 128, 128, 255}).ok());
    EXPECT_TRUE(session.colours.add("tint rose", Color{255, 0, 127, 255}).ok());
    EXPECT_TRUE(session.colours.add("tint lime", Color{0, 255, 0, 255}).ok());
    EXPECT_TRUE(session.colours.add("tint plum", Color{128, 0, 128, 255}).ok());
    session.linework = katana::entity::LineworkCodes{};
    session.automation = katana::entity::CustomisationAutomation{};

    for (const LineStyle& each :
         {definition("WORKS Peg", true, "Works",
                     {pen("tint teal"), Stroke{.op = StrokeOp::Circle, .radius = 0.5}}),
          definition("WORKS Fence", false, "Works", {move(0.0, 0.0), draw(2.0, 0.0)}),
          definition("MARK Cross", true, "Marks", {move(-1.0, 0.0), draw(1.0, 0.0)}),
          definition("LOOSE Dot", true, "", {pen("Tint_Rose"), Stroke{.op = StrokeOp::Dot}})}) {
        EXPECT_TRUE(session.library.add(each).ok()) << each.name;
    }

    SurveyRule pegs;
    pegs.key = "PG*";
    pegs.model = "PEGS";
    pegs.colour = "tint teal";
    SurveyRule fences;
    fences.key = "FN*";
    fences.model = "FENCES";
    fences.linestyle = "WORKS Fence";
    SurveyRule marks;
    marks.key = "MK*";
    marks.section = SurveySection::VertexSymbol;
    marks.symbol = katana::entity::SurveySymbol{.style = "MARK Cross", .colour = "TINT-LIME"};
    SurveyRule texts;
    texts.key = "TX*";
    texts.section = SurveySection::VertexTextStyle;
    texts.textStyle = katana::entity::SurveyTextStyle{.colour = "tint plum"};
    for (const SurveyRule& each : {pegs, fences, marks, texts}) {
        EXPECT_TRUE(session.map.add(each).ok()) << each.key;
    }
    return session;
}

// The names of a table's colours, in the order it lists them.
Lines colourNames(const Customisation& of)
{
    Lines names;
    for (const katana::entity::ColourTable::Entry& entry : of.colours.entries()) {
        names.push_back(entry.name);
    }
    return names;
}

CustomisationWriteOptions codesAlone()
{
    CustomisationWriteOptions options;
    options.linestyles = false;
    options.symbols = false;
    return options;
}

CustomisationWriteOptions definitionsNamed(std::vector<std::string> names)
{
    CustomisationWriteOptions options;
    options.codes = false;
    options.only = std::move(names);
    return options;
}

// What the writer makes of the part with the SAME options, read back: the
// part is cut to be handed to it so.
Customisation writtenAndRead(const Customisation& part, const CustomisationWriteOptions& options)
{
    const auto text = katana::entity::customisationToJson(part, options);
    EXPECT_TRUE(text.ok()) << (text.ok() ? std::string{} : text.error().describe());
    if (!text.ok()) {
        return {};
    }
    auto read = katana::entity::customisationFromJson(*text);
    EXPECT_TRUE(read.ok()) << (read.ok() ? std::string{} : read.error().describe());
    return read.ok() ? std::move(*read) : Customisation{};
}

} // namespace

TEST(CustomisationPart, APartSaysNothingOfTheSettingsAndIsNoEditionOfTheBuiltIn)
{
    const Customisation session = works();
    ASSERT_TRUE(session.linework.has_value());
    ASSERT_TRUE(session.automation.has_value());
    ASSERT_TRUE(session.basedOn.has_value());

    for (const CustomisationWriteOptions& options :
         {codesAlone(), definitionsNamed({"MARK Cross"}), CustomisationWriteOptions{}}) {
        const Customisation part = customisationPart(session, options);
        // Merged into a colleague's session a part must not reset their
        // control codes or switches, and it is not a copy of the built-in to
        // be told from another edition - whichever part it is, the whole
        // asked for AS a part included.
        EXPECT_FALSE(part.linework.has_value());
        EXPECT_FALSE(part.automation.has_value());
        EXPECT_FALSE(part.basedOn.has_value());
        // Whose it is travels with every part.
        EXPECT_EQ(part.name, "Works");
        EXPECT_EQ(part.description, "The works set.");
        ASSERT_FALSE(part.notice.empty());
        EXPECT_EQ(part.notice.front(), kOwnNotice);
        // The definitions and rules themselves are the writer's to choose
        // among, by the same options.
        EXPECT_EQ(part.library.size(), 4u);
        EXPECT_EQ(part.map.size(), 4u);
    }
}

TEST(CustomisationPart, TheCodesAloneListTheSourcesThatBroughtRulesAndCarryTheColoursTheRulesName)
{
    const Customisation session = works();
    const CustomisationWriteOptions options = codesAlone();
    const Customisation part = customisationPart(session, options);

    // Works and Client brought rules, and are said to have brought those
    // alone: no definition is written. Marks brought definitions, none of
    // which is written, so it is no source of this part - and its notice is
    // written with the part's own. Tints, a table of colours, is a source
    // because the part carries a colour.
    EXPECT_EQ(part.sources, (Sources{{"Works", false, true, {}},
                                     {"Client", false, true, {kClientNotice}},
                                     {"Tints", false, false, {kTintsNotice}}}));
    EXPECT_EQ(part.notice, (Lines{kOwnNotice, kMarksNotice}));
    // The three places a rule names a colour: PG*'s own, MK*'s symbol's and
    // TX*'s text's, as the TABLE spells them and in its order. Not tint
    // rose, which only a pen names - and no pen is written.
    EXPECT_EQ(colourNames(part), (Lines{"tint lime", "tint plum", "tint teal"}));

    const Customisation read = writtenAndRead(part, options);
    EXPECT_TRUE(read.library.empty());
    EXPECT_TRUE(read.map == session.map);
    EXPECT_EQ(read.sources, part.sources);
    EXPECT_EQ(read.notice, part.notice);
    EXPECT_FALSE(read.linework.has_value());
}

TEST(CustomisationPart, NamedDefinitionsListTheSourcesTheyCameFromAndNoOther)
{
    const Customisation session = works();

    // The cross alone, which came from Marks and has no pen. Works brought
    // definitions too, but none of ITS is written: listed, it would pass for
    // loaded wherever the file went (a session's sources, and a project's
    // record of them, go by name). No colour is carried, so the table of
    // colours is no source either. Every notice left out is written after
    // the part's own, in the order of the sources.
    const CustomisationWriteOptions cross = definitionsNamed({"MARK Cross"});
    const Customisation one = customisationPart(session, cross);
    EXPECT_EQ(one.sources, (Sources{{"Marks", true, false, {kMarksNotice}}}));
    EXPECT_EQ(one.notice, (Lines{kOwnNotice, kClientNotice, kTintsNotice}));
    EXPECT_TRUE(one.colours.empty());
    const Customisation read = writtenAndRead(one, cross);
    EXPECT_EQ(read.library.names(), Lines{"MARK Cross"});
    EXPECT_TRUE(read.map.empty());

    // With the peg, whose pen is tint teal: Works is a source now, of
    // definitions alone - its rules are not written - and Tints because a
    // colour is carried. That one colour and no other.
    const Customisation two =
        customisationPart(session, definitionsNamed({"MARK Cross", "WORKS Peg"}));
    EXPECT_EQ(two.sources, (Sources{{"Works", true, false, {}},
                                    {"Marks", true, false, {kMarksNotice}},
                                    {"Tints", false, false, {kTintsNotice}}}));
    EXPECT_EQ(two.notice, (Lines{kOwnNotice, kClientNotice}));
    EXPECT_EQ(colourNames(two), Lines{"tint teal"});
}

TEST(CustomisationPart, AKindOfDefinitionIsWrittenWithTheColoursItsPensNameByTheFold)
{
    const Customisation session = works();

    // The symbols: the peg (a pen of tint teal), the cross, and the dot made
    // in a session, whose pen is spelled Tint_Rose - one name with the
    // table's tint rose. The dot has no source, so it adds none.
    CustomisationWriteOptions symbols;
    symbols.linestyles = false;
    symbols.codes = false;
    const Customisation marks = customisationPart(session, symbols);
    EXPECT_EQ(marks.sources, (Sources{{"Works", true, false, {}},
                                      {"Marks", true, false, {kMarksNotice}},
                                      {"Tints", false, false, {kTintsNotice}}}));
    EXPECT_EQ(marks.notice, (Lines{kOwnNotice, kClientNotice}));
    EXPECT_EQ(colourNames(marks), (Lines{"tint rose", "tint teal"}));

    // The linestyles: the fence, which has no pen. Nothing of Client, Marks
    // or Tints is written, and each notice is.
    CustomisationWriteOptions linestyles;
    linestyles.symbols = false;
    linestyles.codes = false;
    const Customisation lines = customisationPart(session, linestyles);
    EXPECT_EQ(lines.sources, (Sources{{"Works", true, false, {}}}));
    EXPECT_EQ(lines.notice, (Lines{kOwnNotice, kClientNotice, kMarksNotice, kTintsNotice}));
    EXPECT_TRUE(lines.colours.empty());
}

TEST(CustomisationPart, ANoticeIsWrittenOnceHoweverManySourcesLeftOutCarryIt)
{
    // Client's notice begins with the session's own line, and Marks carries
    // Client's: three sources left out of a part of linestyles, five lines
    // among them and the part, three of them distinct besides the part's.
    Customisation session = works();
    session.sources[1].notice = {kOwnNotice, kClientNotice};
    session.sources[2].notice = {kClientNotice, kMarksNotice};
    CustomisationWriteOptions linestyles;
    linestyles.symbols = false;
    linestyles.codes = false;

    const Customisation part = customisationPart(session, linestyles);
    EXPECT_EQ(part.notice, (Lines{kOwnNotice, kClientNotice, kMarksNotice, kTintsNotice}));
}

TEST(CustomisationPart, EveryKindAskedForAsAPartHoldsEverythingButTheSettings)
{
    // CUSTOMISE EXPORT <file> CODES LINESTYLES SYMBOLS: every definition and
    // rule, and so every source as it stands and every colour something
    // names - all four here - with no notice to carry over.
    const Customisation session = works();
    const Customisation part = customisationPart(session, CustomisationWriteOptions{});
    EXPECT_EQ(part.sources, session.sources);
    EXPECT_EQ(part.notice, Lines{kOwnNotice});
    EXPECT_EQ(colourNames(part), (Lines{"tint lime", "tint plum", "tint rose", "tint teal"}));
    EXPECT_EQ(part.library.names(), session.library.names());
    EXPECT_TRUE(part.map == session.map);
}

TEST(CustomisationPart, WhatTheWriterWouldRefuseIsNotCountedAsWritten)
{
    const Customisation session = works();

    // A linestyle asked for by name where only symbols are written: the
    // writer refuses the write, and it is not counted here either - so the
    // part lists no source for a definition it does not hold.
    CustomisationWriteOptions wrongKind;
    wrongKind.linestyles = false;
    wrongKind.codes = false;
    wrongKind.only = {"WORKS Fence"};
    const Customisation none = customisationPart(session, wrongKind);
    EXPECT_TRUE(none.sources.empty());
    EXPECT_TRUE(none.colours.empty());
    EXPECT_EQ(none.notice, (Lines{kOwnNotice, kClientNotice, kMarksNotice, kTintsNotice}));
    const auto refused = katana::entity::customisationToJson(none, wrongKind);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::InvalidArgument);

    // A name the library does not hold is no definition at all.
    const Customisation missing = customisationPart(session, definitionsNamed({"NOT There"}));
    EXPECT_TRUE(missing.sources.empty());
    const auto notFound =
        katana::entity::customisationToJson(missing, definitionsNamed({"NOT There"}));
    ASSERT_FALSE(notFound.ok());
    EXPECT_EQ(notFound.error().code, katana::core::ErrorCode::NotFound);
}

TEST(CustomisationPart, WithNoRulesToWriteNoSourceIsSaidToHaveBroughtAny)
{
    // The codes asked for of a session whose rules are all gone - the Survey
    // Code Manager's buffer, emptied: nothing is written, so nothing came
    // from anywhere, no colour is named, and every notice is carried.
    Customisation session = works();
    session.map = {};
    const Customisation part = customisationPart(session, codesAlone());
    EXPECT_TRUE(part.sources.empty());
    EXPECT_TRUE(part.colours.empty());
    EXPECT_EQ(part.notice, (Lines{kOwnNotice, kClientNotice, kMarksNotice, kTintsNotice}));
}

TEST(CustomisationPart, ACustomisationWithNothingInItIsAPartWithNothingInIt)
{
    // Nothing to cut, nothing to carry: a Document nothing was loaded into.
    EXPECT_TRUE(customisationPart(Customisation{}, codesAlone()) == Customisation{});
    EXPECT_TRUE(customisationPart(Customisation{}, definitionsNamed({"Anything"})) ==
                Customisation{});

    // Rules set by hand, with no source to say where they came from: the
    // rules, the colour they name, and no source.
    Customisation loose;
    loose.name = "Loose";
    ASSERT_TRUE(loose.colours.add("tint teal", Color{0, 128, 128, 255}).ok());
    ASSERT_TRUE(loose.colours.add("tint rose", Color{255, 0, 127, 255}).ok());
    SurveyRule rule;
    rule.key = "KT*";
    rule.colour = "tint rose";
    ASSERT_TRUE(loose.map.add(rule).ok());
    const Customisation part = customisationPart(loose, codesAlone());
    EXPECT_TRUE(part.sources.empty());
    EXPECT_TRUE(part.notice.empty());
    EXPECT_EQ(colourNames(part), Lines{"tint rose"});
    EXPECT_EQ(part.map.size(), 1u);
}
