// The Katana customisation format (docs/customisation.md): a customisation
// that is written reads back as the same value, the bytes written are the
// layout the document states, and the degenerate files read as what they say.
//
// Every expectation here is written by hand from the format's stated rules -
// the layout, the member names, the defaults - and none is a copy of what a
// run printed.

#include <gtest/gtest.h>

#include <bit>
#include <cmath>
#include <cstdint>
#include <limits>
#include <string>
#include <vector>

#include "customisation_fixture.hpp"
#include "katana/entity/customisation.hpp"

using katana::entity::Color;
using katana::entity::Customisation;
using katana::entity::CustomisationAutomation;
using katana::entity::CustomisationBase;
using katana::entity::CustomisationSourceNote;
using katana::entity::CustomisationWriteOptions;
using katana::entity::LineStyle;
using katana::entity::LineworkCodes;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StrokeText;
using katana::entity::StyleUnits;
using katana::entity::SurveyAttribute;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyPipe;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;
using katana::entity::SurveySymbol;
using katana::entity::SurveyTextStyle;
using katana::testing::customisationWith;
using katana::testing::readCustomisation;
using katana::testing::writeCustomisation;

namespace {

Stroke moveTo(double x, double y)
{
    Stroke stroke;
    stroke.op = StrokeOp::Move;
    stroke.point = {x, y};
    return stroke;
}

Stroke drawTo(double x, double y)
{
    Stroke stroke;
    stroke.op = StrokeOp::Draw;
    stroke.point = {x, y};
    return stroke;
}

Stroke arc(double radius, double start, double end)
{
    Stroke stroke;
    stroke.op = StrokeOp::Arc;
    stroke.radius = radius;
    stroke.startAngle = start;
    stroke.endAngle = end;
    return stroke;
}

Stroke circle(double radius)
{
    Stroke stroke;
    stroke.op = StrokeOp::Circle;
    stroke.radius = radius;
    return stroke;
}

Stroke dot(double radius)
{
    Stroke stroke;
    stroke.op = StrokeOp::Dot;
    stroke.radius = radius;
    return stroke;
}

Stroke pen(std::string name)
{
    Stroke stroke;
    stroke.op = StrokeOp::Pen;
    stroke.pen = std::move(name);
    return stroke;
}

// A text stroke and the text it places, kept in step as the model requires.
void addText(LineStyle& definition, StrokeText text)
{
    Stroke stroke;
    stroke.op = StrokeOp::Text;
    stroke.text = definition.texts.size();
    definition.texts.push_back(std::move(text));
    definition.strokes.push_back(stroke);
}

void add(Customisation& customisation, LineStyle definition)
{
    const auto added = customisation.library.add(std::move(definition));
    ASSERT_TRUE(added.ok()) << added.error().describe();
}

void add(Customisation& customisation, SurveyRule rule)
{
    const auto added = customisation.map.add(std::move(rule));
    ASSERT_TRUE(added.ok()) << added.error().describe();
}

// ---- the small customisation whose text is written out below --------------------------------

LineStyle fence()
{
    LineStyle definition;
    definition.name = "Fence";
    definition.group = "Site/Lines";
    definition.length = 2.5;
    definition.source = "Site";
    definition.strokes = {moveTo(0, 0),       drawTo(1.5, 0), pen("red"),
                          arc(-0.25, 0, 180), circle(0.5),    dot(0)};
    StrokeText label;
    label.text = "F";
    label.height = 1.5;
    label.justify = "middle-centre";
    addText(definition, label);
    return definition;
}

Customisation smallCustomisation()
{
    Customisation customisation;
    customisation.name = "Site";
    customisation.description = "A small site set.";
    customisation.notice = {"Line one.", "Line \"two\"."};
    customisation.sources = {CustomisationSourceNote{"Site", true, true, {"Made by hand."}},
                             CustomisationSourceNote{"Shared", false, false, {}}};
    customisation.basedOn = CustomisationBase{"NSW", "0123456789abcdef"};
    EXPECT_TRUE(customisation.colours.add("Site Orange", Color{255, 127, 0, 255}).ok());
    EXPECT_TRUE(customisation.colours.add("site_grey", Color{128, 128, 128, 255}).ok());
    LineworkCodes codes;
    codes.close = "C";
    customisation.linework = codes;
    customisation.automation = CustomisationAutomation{true, false};

    LineStyle empty;
    empty.name = "Empty";
    empty.source = "Shared";
    add(customisation, empty);

    add(customisation, fence());

    LineStyle gate;
    gate.name = "Gate";
    gate.units = StyleUnits::TwoPoint;
    gate.anchor1 = {0, 0.75};
    gate.anchor2 = {14, 0.75};
    gate.stretchMode = 2;
    gate.cycleMode = 1;
    gate.source = "Site";
    gate.strokes = {moveTo(0, 0), drawTo(14, 0)};
    add(customisation, gate);

    LineStyle peg;
    peg.name = "Peg";
    peg.units = StyleUnits::Paper;
    peg.atVertices = true;
    peg.factor = 2;
    peg.origin = {1, -2};
    peg.source = "Site";
    peg.symbol = true;
    peg.strokes = {circle(0.5)};
    add(customisation, peg);

    SurveyRule fences;
    fences.key = "FN*";
    fences.section = SurveySection::Map;
    fences.model = "SITE/FENCES";
    fences.colour = "Site Orange";
    fences.breakline = SurveyBreakline::Line;
    fences.linestyle = "Fence";
    fences.weight = "0";
    fences.group = "Site";
    fences.comment = "a fence";
    add(customisation, fences);

    SurveyRule pegs;
    pegs.key = "PG";
    pegs.section = SurveySection::VertexSymbol;
    pegs.hide = false;
    pegs.symbol = SurveySymbol{"Peg", "", 1.5, 0, 0, 0};
    add(customisation, pegs);

    SurveyRule texts;
    texts.key = "*";
    texts.section = SurveySection::VertexTextStyle;
    texts.textStyle = SurveyTextStyle{};
    add(customisation, texts);

    SurveyRule surfaces;
    surfaces.key = "TN*";
    surfaces.section = SurveySection::Tinable;
    surfaces.tinable = false;
    add(customisation, surfaces);

    SurveyRule pipes;
    pipes.key = "PP*";
    pipes.section = SurveySection::Pipe;
    pipes.pipe = SurveyPipe{"Obvert", "", "$PipeDiameter", "", true};
    pipes.attributes = {SurveyAttribute{"text", "Owner", "Council"},
                        SurveyAttribute{"integer", "Zone", ""}};
    add(customisation, pipes);
    return customisation;
}

// By hand, from "Layout" in docs/customisation.md: two blanks a level; the
// top-level members one a line in their fixed order; the entries of notice,
// sources, colours, linestyles, symbols and codes one a line, colours in the
// order of their folded names ("site grey" before "site orange") and
// definitions in name order; a definition's strokes one a line; a member at
// its default left out, except those always written and a present optional
// ("hide": false, "text": {}); every member of linework and automation.
constexpr const char* kSmallCustomisationText = R"json({
  "format": "katana-customisation",
  "version": 1,
  "name": "Site",
  "description": "A small site set.",
  "notice": [
    "Line one.",
    "Line \"two\"."
  ],
  "sources": [
    {"name": "Site", "definitions": true, "rules": true, "notice": ["Made by hand."]},
    {"name": "Shared"}
  ],
  "basedOn": {"name": "NSW", "digest": "0123456789abcdef"},
  "colours": {
    "site_grey": "#808080",
    "Site Orange": "#FF7F00"
  },
  "linework": {"start": "ST", "end": "END", "close": "C", "arcStart": "BC", "arcEnd": "EC", "join": "JPN", "rectangle": "RECT"},
  "automation": {"codesOnSurveyImport": true, "lineworkOnSurveyImport": false},
  "linestyles": [
    {"name": "Empty", "from": "Shared"},
    {"name": "Fence", "group": "Site/Lines", "length": 2.5, "strokes": [
      ["move", 0, 0],
      ["draw", 1.5, 0],
      ["pen", "red"],
      ["arc", -0.25, 0, 180],
      ["circle", 0.5],
      ["dot", 0],
      ["text", {"text": "F", "height": 1.5, "justify": "middle-centre"}]
    ]},
    {"name": "Gate", "units": "twoPoint", "anchors": [[0, 0.75], [14, 0.75]], "stretchMode": 2, "cycleMode": 1, "strokes": [
      ["move", 0, 0],
      ["draw", 14, 0]
    ]}
  ],
  "symbols": [
    {"name": "Peg", "units": "paper", "atVertices": true, "factor": 2, "origin": [1, -2], "strokes": [
      ["circle", 0.5]
    ]}
  ],
  "codes": [
    {"key": "FN*", "sets": "feature", "layer": "SITE/FENCES", "colour": "Site Orange", "draw": "line", "linestyle": "Fence", "weight": "0", "group": "Site", "comment": "a fence"},
    {"key": "PG", "sets": "symbol", "hide": false, "symbol": {"name": "Peg", "size": 1.5}},
    {"key": "*", "sets": "text", "text": {}},
    {"key": "TN*", "sets": "surface", "surface": false},
    {"key": "PP*", "sets": "pipe", "pipe": {"justify": "Obvert", "size1": "$PipeDiameter", "active": true}, "attributes": [{"type": "text", "name": "Owner", "value": "Council"}, {"type": "integer", "name": "Zone"}]}
  ]
}
)json";

// ---- the field-complete customisation -------------------------------------------------------

// A rule with every field a rule has, each at a value no other field of its
// kind has: two members that held the same value could be crossed in the
// reader - "underline" read into `strikeout` - and the rule would still come
// back equal. The model lets any field sit on a rule of any section, and so
// does the format, so one of these is made for each section.
//
// Five members are true-or-false, and with every one of them true no two
// could be told apart. `variant` (0, 1 or 2) settles them so that each pair
// differs in at least one variant:
//
//   variant        surface  hide   underline  strikeout  italic
//      0            true    false    false      true      true
//      1            false   true     true       false     true
//      2            true    true     true       true      false
SurveyRule ruleWithEveryField(std::string key, SurveySection section, SurveyBreakline breakline,
                              int variant)
{
    SurveyRule rule;
    rule.key = std::move(key);
    rule.section = section;
    rule.model = "SURVEY/SERVICES";
    rule.colour = "sui water";
    rule.linestyle = "L Everything";
    rule.weight = "Normal";
    rule.group = "G - SERVICES";
    rule.comment = "every field, with a \"quote\" and a \\ backslash";
    rule.breakline = breakline;
    rule.tinable = variant != 1;
    rule.hide = variant != 0;
    rule.symbol = SurveySymbol{"S Peg", "white", 1.5, 45, 0.2, -0.1};
    rule.textStyle =
        SurveyTextStyle{"ISO", "red", "paper",      2.5,          "left",       "bottom", 0.5, 0.25,
                        30,    10,    0.8,          variant != 0, variant != 1, variant != 2, "0"};
    rule.pipe = SurveyPipe{"Obvert", "diameter", "$PipeDiameter", "0.3", true};
    rule.vertexPipe = SurveyPipe{"Invert", "culvert", "0.375", "0.6", true};
    rule.segmentPipe = SurveyPipe{"Centre", "diameter", "1", "2", true};
    rule.attributes = {SurveyAttribute{"text", "Owner", "Council & Co"},
                       SurveyAttribute{"integer", "Zone", "1"}};
    rule.vertexAttributes = {SurveyAttribute{"integer", "N", "2"},
                             SurveyAttribute{"text", "Old", ""}};
    rule.segmentAttributes = {SurveyAttribute{"text", "S", "$Other"}};
    return rule;
}

Customisation fieldCompleteCustomisation()
{
    Customisation customisation;
    customisation.name = "Everything";
    customisation.description = "Every member of the format.\nOn two lines.";
    customisation.notice = {"First line of the notice.", "", "A third, after an empty one."};
    customisation.sources = {
        CustomisationSourceNote{"Everything", true, true, {"Its own notice.", "In two lines."}},
        CustomisationSourceNote{"Rules Only", false, true, {}},
        CustomisationSourceNote{"Other Set", true, false, {"The other set's notice."}}};
    customisation.basedOn = CustomisationBase{"Base Set", "00ff00ff00ff00ff"};
    EXPECT_TRUE(customisation.colours.add("sui water", Color{0, 112, 255, 255}).ok());
    EXPECT_TRUE(customisation.colours.add("Half Clear", Color{1, 2, 3, 128}).ok());
    // Every spelling changed from its default, and one control switched off.
    customisation.linework = LineworkCodes{"S", "E", "C", "PC", "PT", "", "BOX"};
    // Both off, where both are on when left out. (Which is which is pinned by
    // the small customisation, where one is on and the other off.)
    customisation.automation = CustomisationAutomation{false, false};

    // A symbol with every member of a definition and every kind of stroke,
    // and no number at the value it has when left out: a dot of radius 0, an
    // arc from 0 or an anchor at x = 0 would come back the same from a reader
    // that never read that number.
    LineStyle everything;
    everything.name = "S Everything";
    everything.group = "Survey/S";
    everything.units = StyleUnits::Paper;
    everything.atVertices = true;
    everything.length = 2.5;
    everything.factor = 3;
    everything.origin = {1, -2};
    everything.anchor1 = {0.5, 0.75};
    everything.anchor2 = {14, -0.25};
    everything.stretchMode = 2;
    everything.cycleMode = -1;
    everything.source = "Other Set";
    everything.symbol = true;
    everything.strokes = {pen("pen 035"),      moveTo(0, 0), drawTo(3, 0.1),
                          arc(-1.75, 30, 180), circle(0.5),  dot(0.25)};
    addText(everything, StrokeText{"W \"M\" \\ 1", 90, 1.5, "middle-centre", "Arial", 0.85,
                                   {0.5, -0.3, 0.035}});
    addText(everything, StrokeText{}); // a text with nothing said: ["text", {}]
    everything.strokes.push_back(pen("view_colour"));
    everything.strokes.push_back(moveTo(0.00001, 12345678.125));
    add(customisation, everything);

    LineStyle world;
    world.name = "L Everything";
    world.group = "Survey/L";
    world.source = "Everything";
    world.strokes = {moveTo(0, 0), drawTo(1, 0)};
    add(customisation, world);

    // Drawn at vertices and NOT listed as a symbol: the two are said
    // separately, and neither may be worked out from the other.
    LineStyle marks;
    marks.name = "V Marks";
    marks.atVertices = true;
    marks.source = "Everything";
    marks.strokes = {dot(0)};
    add(customisation, marks);

    LineStyle gate;
    gate.name = "T Gate";
    gate.units = StyleUnits::TwoPoint;
    gate.anchor2 = {14, 0}; // one anchor moved is enough for both to be written
    gate.source = "";       // made in a session: of no customisation
    gate.strokes = {moveTo(0, 0), drawTo(14, 0)};
    add(customisation, gate);

    LineStyle nothing;
    nothing.name = "Empty";
    nothing.source = "Everything";
    nothing.symbol = true;
    add(customisation, nothing);

    // One rule a section, each with every field, the three variants in turn.
    add(customisation, ruleWithEveryField("WM*", SurveySection::Map, SurveyBreakline::Line, 0));
    add(customisation,
        ruleWithEveryField("AC*", SurveySection::VertexSymbol, SurveyBreakline::Point, 1));
    add(customisation,
        ruleWithEveryField("1", SurveySection::VertexTextStyle, SurveyBreakline::Line, 2));
    add(customisation, ruleWithEveryField("*", SurveySection::Pipe, SurveyBreakline::Line, 0));
    add(customisation,
        ruleWithEveryField("VP*", SurveySection::VertexPipe, SurveyBreakline::Line, 1));
    add(customisation,
        ruleWithEveryField("SP*", SurveySection::SegmentPipe, SurveyBreakline::Line, 2));
    add(customisation,
        ruleWithEveryField("LP*", SurveySection::StringAttribute, SurveyBreakline::Line, 0));
    add(customisation,
        ruleWithEveryField("PNAL", SurveySection::VertexAttribute, SurveyBreakline::Line, 1));
    add(customisation, ruleWithEveryField("TN*", SurveySection::Tinable, SurveyBreakline::Line, 2));

    // Optionals that are present and say nothing: not the same as absent.
    SurveyRule present;
    present.key = "WM*";
    present.section = SurveySection::Map;
    present.tinable = false;
    present.hide = false;
    present.symbol = SurveySymbol{};
    present.textStyle = SurveyTextStyle{};
    present.pipe = SurveyPipe{};
    present.vertexPipe = SurveyPipe{};
    present.segmentPipe = SurveyPipe{};
    add(customisation, present);

    // A rule that says nothing but what it must.
    SurveyRule bare;
    bare.key = "WM*";
    bare.section = SurveySection::Map;
    add(customisation, bare);
    return customisation;
}

std::size_t count(const std::string& text, std::string_view piece)
{
    std::size_t found = 0;
    for (std::size_t at = text.find(piece); at != std::string::npos;
         at = text.find(piece, at + piece.size())) {
        ++found;
    }
    return found;
}

} // namespace

// ---- round trip -----------------------------------------------------------------------------

TEST(CustomisationFormat, EveryFieldOfTheModelComesBackAsItWasWritten)
{
    const Customisation original = fieldCompleteCustomisation();
    const std::string text = writeCustomisation(original);
    const Customisation again = readCustomisation(text);

    EXPECT_TRUE(again == original) << text;
    // Said member by member as well, so that a failure says where.
    EXPECT_EQ(again.name, original.name);
    EXPECT_EQ(again.description, original.description);
    EXPECT_EQ(again.notice, original.notice);
    EXPECT_EQ(again.sources, original.sources);
    EXPECT_EQ(again.basedOn, original.basedOn);
    EXPECT_EQ(again.colours, original.colours);
    EXPECT_EQ(again.linework, original.linework);
    EXPECT_EQ(again.automation, original.automation);
    EXPECT_EQ(again.library.all(), original.library.all());
    EXPECT_EQ(again.map.rules(), original.map.rules()) << "the rules, in their order";
}

// The round trip above proves nothing about a member the fixture never
// writes. Every member name and every word of the format, listed here by hand
// from docs/customisation.md, is in the fixture's text.
TEST(CustomisationFormat, TheFieldCompleteFixtureWritesEveryMemberAndWordTheFormatHas)
{
    const std::string text = writeCustomisation(fieldCompleteCustomisation());
    for (const char* member :
         {// the file
          "format", "version", "name", "description", "notice", "sources", "basedOn", "colours",
          "linework", "automation", "linestyles", "symbols", "codes",
          // a source and the base
          "definitions", "rules", "digest",
          // linework and automation
          "start", "end", "close", "arcStart", "arcEnd", "join", "rectangle", "codesOnSurveyImport",
          "lineworkOnSurveyImport",
          // a definition
          "group", "units", "atVertices", "length", "factor", "origin", "anchors", "stretchMode",
          "cycleMode", "from", "strokes",
          // a text stroke
          "text", "angle", "height", "justify", "font", "widthFactor", "extra",
          // a rule
          "key", "sets", "layer", "colour", "draw", "linestyle", "weight", "comment", "surface",
          "hide", "symbol", "pipe", "vertexPipe", "segmentPipe", "attributes", "vertexAttributes",
          "segmentAttributes",
          // a rule's symbol, text and pipes, and an attribute
          "size", "rotation", "offset", "raise", "style", "justifyX", "justifyY", "slant",
          "underline", "strikeout", "italic", "shape", "size1", "size2", "active", "type",
          "value"}) {
        EXPECT_NE(text.find("\"" + std::string(member) + "\": "), std::string::npos)
            << "the fixture never writes \"" << member << "\"";
    }
    for (const char* stroke : {"move", "draw", "arc", "circle", "dot", "pen", "text"}) {
        EXPECT_NE(text.find("[\"" + std::string(stroke) + "\", "), std::string::npos) << stroke;
    }
    for (const char* sets : {"feature", "symbol", "text", "pipe", "vertexPipe", "segmentPipe",
                             "attributes", "vertexAttributes", "surface"}) {
        EXPECT_NE(text.find("\"sets\": \"" + std::string(sets) + "\""), std::string::npos) << sets;
    }
    // "world" is the default and so is never written; the two others are.
    EXPECT_NE(text.find("\"units\": \"paper\""), std::string::npos);
    EXPECT_NE(text.find("\"units\": \"twoPoint\""), std::string::npos);
    EXPECT_EQ(text.find("\"units\": \"world\""), std::string::npos);
    EXPECT_NE(text.find("\"draw\": \"line\""), std::string::npos);
    EXPECT_NE(text.find("\"draw\": \"point\""), std::string::npos);
    EXPECT_NE(text.find("\"type\": \"text\""), std::string::npos);
    EXPECT_NE(text.find("\"type\": \"integer\""), std::string::npos);
    // The optionals that are present and say nothing, each written as {}:
    // the symbol, the text and the three pipes of the eleventh rule.
    EXPECT_NE(text.find(R"("surface": false, "hide": false, "symbol": {}, "text": {}, )"
                        R"("pipe": {}, "vertexPipe": {}, "segmentPipe": {}})"),
              std::string::npos)
        << text;
    // A definition made in a session keeps "no source" under a named file.
    EXPECT_NE(text.find("\"from\": \"\""), std::string::npos);
}

// The members of a definition and of each kind of stroke, in the order the
// format fixes, for the symbol of the fixture that has every one of them. By
// hand: the file's four blanks before the definition, six before a stroke;
// 0.00001 in its shorter form with an exponent; "from" because its source is
// not the file's name; a text with nothing said as {}.
TEST(CustomisationFormat, ADefinitionWithEveryMemberIsWrittenInTheOrderTheFormatFixes)
{
    const std::string text = writeCustomisation(fieldCompleteCustomisation());
    const std::string expected = R"json(
    {"name": "S Everything", "group": "Survey/S", "units": "paper", "atVertices": true, "length": 2.5, "factor": 3, "origin": [1, -2], "anchors": [[0.5, 0.75], [14, -0.25]], "stretchMode": 2, "cycleMode": -1, "from": "Other Set", "strokes": [
      ["pen", "pen 035"],
      ["move", 0, 0],
      ["draw", 3, 0.1],
      ["arc", -1.75, 30, 180],
      ["circle", 0.5],
      ["dot", 0.25],
      ["text", {"text": "W \"M\" \\ 1", "angle": 90, "height": 1.5, "justify": "middle-centre", "font": "Arial", "widthFactor": 0.85, "extra": [0.5, -0.3, 0.035]}],
      ["text", {}],
      ["pen", "view_colour"],
      ["move", 1e-05, 12345678.125]
    ]}
  ],)json";
    EXPECT_NE(text.find(expected), std::string::npos) << text;

    // The linestyle drawn at vertices says so, and sits in "linestyles": the
    // last of the three there, by name. Its source is the file's own name, so
    // it has no "from"; its dot of radius 0 is written with its 0, since a
    // stroke's values are never left out.
    const std::string atVertices = R"json(
    {"name": "V Marks", "atVertices": true, "strokes": [
      ["dot", 0]
    ]}
  ],
  "symbols": [)json";
    EXPECT_NE(text.find(atVertices), std::string::npos) << text;
}

// The same for a rule: every member a rule has, and every member of its
// symbol, its text, its three pipes and its three attribute lists, on the one
// line a rule is written on, for each of the fixture's three variants (its
// first three rules). A member that is false where false is the value left
// out - a text's flags - is not written; "surface" and "hide" are optionals,
// written whenever present. An attribute's empty value is left out.
TEST(CustomisationFormat, ARuleWithEveryFieldIsWrittenInTheOrderTheFormatFixes)
{
    const std::string text = writeCustomisation(fieldCompleteCustomisation());
    // What the three have in common, by hand, in the three stretches the
    // varying members sit between.
    const std::string upToSurface =
        R"json("layer": "SURVEY/SERVICES", "colour": "sui water", "draw": "DRAW", "linestyle": "L Everything", "weight": "Normal", "group": "G - SERVICES", "comment": "every field, with a \"quote\" and a \\ backslash", )json";
    const std::string upToFlags =
        R"json("symbol": {"name": "S Peg", "colour": "white", "size": 1.5, "rotation": 45, "offset": 0.2, "raise": -0.1}, )json"
        R"json("text": {"style": "ISO", "colour": "red", "units": "paper", "size": 2.5, "justifyX": "left", "justifyY": "bottom", "offset": 0.5, "raise": 0.25, "angle": 30, "slant": 10, "widthFactor": 0.8, )json";
    const std::string fromWeight =
        R"json("weight": "0"}, )json"
        R"json("pipe": {"justify": "Obvert", "shape": "diameter", "size1": "$PipeDiameter", "size2": "0.3", "active": true}, )json"
        R"json("vertexPipe": {"justify": "Invert", "shape": "culvert", "size1": "0.375", "size2": "0.6", "active": true}, )json"
        R"json("segmentPipe": {"justify": "Centre", "shape": "diameter", "size1": "1", "size2": "2", "active": true}, )json"
        R"json("attributes": [{"type": "text", "name": "Owner", "value": "Council & Co"}, {"type": "integer", "name": "Zone", "value": "1"}], )json"
        R"json("vertexAttributes": [{"type": "integer", "name": "N", "value": "2"}, {"type": "text", "name": "Old"}], )json"
        R"json("segmentAttributes": [{"type": "text", "name": "S", "value": "$Other"}]},)json";
    struct Variant {
        const char* keyAndSets;
        const char* draw;
        const char* switches;
        const char* flags;
    };
    for (const Variant& variant :
         {Variant{R"("key": "WM*", "sets": "feature", )", "line",
                  R"("surface": true, "hide": false, )", R"("strikeout": true, "italic": true, )"},
          Variant{R"("key": "AC*", "sets": "symbol", )", "point",
                  R"("surface": false, "hide": true, )", R"("underline": true, "italic": true, )"},
          Variant{R"("key": "1", "sets": "text", )", "line", R"("surface": true, "hide": true, )",
                  R"("underline": true, "strikeout": true, )"}}) {
        std::string common = upToSurface;
        common.replace(common.find("DRAW"), 4, variant.draw);
        const std::string expected = std::string("\n    {") + variant.keyAndSets + common +
                                     variant.switches + upToFlags + variant.flags + fromWeight;
        EXPECT_NE(text.find(expected), std::string::npos) << expected << "\n\n" << text;
    }
}

TEST(CustomisationFormat, RulesKeepTheirOrderBecauseOrderIsPrecedence)
{
    // Three rules of one key and one section, told apart by their layers:
    // which one a code takes its layer from is decided by their order alone.
    Customisation customisation;
    customisation.name = "Order";
    for (const char* layer : {"SECOND", "FIRST", "THIRD"}) {
        SurveyRule rule;
        rule.key = "WM*";
        rule.section = SurveySection::Map;
        rule.model = layer;
        add(customisation, rule);
    }
    const Customisation again = readCustomisation(writeCustomisation(customisation));
    ASSERT_EQ(again.map.size(), 3u);
    EXPECT_EQ(again.map.rules()[0].model, "SECOND");
    EXPECT_EQ(again.map.rules()[1].model, "FIRST");
    EXPECT_EQ(again.map.rules()[2].model, "THIRD");
    EXPECT_EQ(again.map.lookup("WM01").resolved.model, "SECOND") << "the earliest rule wins";
}

TEST(CustomisationFormat, WritingWhatWasReadGivesTheSameBytes)
{
    for (const Customisation& customisation :
         {fieldCompleteCustomisation(), smallCustomisation(), Customisation{"Bare"}}) {
        const std::string first = writeCustomisation(customisation);
        EXPECT_EQ(writeCustomisation(customisation), first) << "one value, two writes";
        EXPECT_EQ(writeCustomisation(readCustomisation(first)), first) << customisation.name;
    }
}

// ---- the layout -----------------------------------------------------------------------------

TEST(CustomisationFormat, ASmallCustomisationIsWrittenByteForByteAsTheLayoutSays)
{
    EXPECT_EQ(writeCustomisation(smallCustomisation()), kSmallCustomisationText);
}

TEST(CustomisationFormat, TheTextOfTheSmallCustomisationReadsAsTheValueItWasWrittenFrom)
{
    EXPECT_TRUE(readCustomisation(kSmallCustomisationText) == smallCustomisation());
}

// The example docs/customisation.md prints under "A worked example", character
// for character: a file a person may well copy to start their own. It reads
// as what it says, and it is laid out as the writer lays a file out. This is
// a COPY of the document's text, and the `docs` test (tools/check_docs.py,
// check_worked_example) fails when the document's block is not this raw
// string - so the document's example and the format cannot drift apart
// unseen. Change one and the other must change with it.
TEST(CustomisationFormat, TheWorkedExampleOfTheDocumentIsWrittenAsItIsPrinted)
{
    const std::string example = R"json({
  "format": "katana-customisation",
  "version": 1,
  "name": "Site",
  "colours": {
    "sui water potable": "#0070FF"
  },
  "linestyles": [
    {"name": "WATR Main", "group": "Survey/WATR", "units": "paper", "length": 12, "strokes": [
      ["move", 0, 0],
      ["draw", 8, 0],
      ["move", 10, -0.75],
      ["text", {"text": "W", "height": 1.5, "justify": "middle-centre"}]
    ]}
  ],
  "symbols": [
    {"name": "CULT Bollard", "group": "Survey/CULT", "units": "paper", "strokes": [
      ["circle", 0.75],
      ["dot", 0]
    ]}
  ],
  "codes": [
    {"key": "WM*", "sets": "feature", "layer": "SURVEY SERVICES", "colour": "sui water potable", "draw": "line", "linestyle": "WATR Main"},
    {"key": "AC*", "sets": "symbol", "symbol": {"name": "CULT Bollard"}},
    {"key": "*", "sets": "attributes", "attributes": [{"type": "text", "name": "Surveyed by"}]}
  ]
}
)json";
    const Customisation read = readCustomisation(example);
    EXPECT_EQ(writeCustomisation(read), example);

    // What the example says, in the model's terms.
    EXPECT_EQ(read.name, "Site");
    EXPECT_EQ(read.colours.find("SUI Water Potable"), (Color{0x00, 0x70, 0xFF, 255}));
    ASSERT_NE(read.library.find("WATR Main"), nullptr);
    EXPECT_FALSE(read.library.find("WATR Main")->symbol);
    EXPECT_EQ(read.library.find("WATR Main")->units, StyleUnits::Paper);
    EXPECT_EQ(read.library.find("WATR Main")->length, 12.0);
    ASSERT_EQ(read.library.find("WATR Main")->texts.size(), 1u);
    EXPECT_EQ(read.library.find("WATR Main")->texts[0].text, "W");
    ASSERT_NE(read.library.find("CULT Bollard"), nullptr);
    EXPECT_TRUE(read.library.find("CULT Bollard")->symbol);
    // A water main: WM01 goes on its layer, as a line, in its linestyle; and
    // the `*` rule's attribute reaches it too.
    const auto waterMain = read.map.lookup("WM01");
    EXPECT_EQ(waterMain.resolved.model, "SURVEY SERVICES");
    EXPECT_EQ(waterMain.resolved.breakline, SurveyBreakline::Line);
    EXPECT_EQ(waterMain.resolved.linestyle, "WATR Main");
    ASSERT_EQ(waterMain.resolved.attributes.size(), 1u);
    EXPECT_EQ(waterMain.resolved.attributes[0].name, "Surveyed by");
    ASSERT_TRUE(read.map.lookup("AC3").resolved.symbol.has_value());
    EXPECT_EQ(read.map.lookup("AC3").resolved.symbol->style, "CULT Bollard");
}

TEST(CustomisationFormat, TextIsEscapedAsJsonAndNothingElseIs)
{
    // A quote and a backslash take a backslash; a control character below
    // U+0020 is \n, \t or \u00XX with lower-case digits (U+001F here); '/' and
    // what is not ASCII are written as they are (the micro sign is C2 B5 in
    // UTF-8).
    Customisation customisation;
    customisation.name = "E";
    customisation.description = "a \"b\" c\\d\ne\tf\x1Fg/h \xC2\xB5m";
    const std::string text = writeCustomisation(customisation);
    EXPECT_NE(text.find("\"description\": \"a \\\"b\\\" c\\\\d\\ne\\tf\\u001fg/h \xC2\xB5m\""),
              std::string::npos)
        << text;
    EXPECT_EQ(readCustomisation(text).description, customisation.description);
}

TEST(CustomisationFormat, NumbersAreTheShortestTextThatReadsBackAndNeverPadded)
{
    Customisation customisation;
    customisation.name = "N";
    LineStyle definition;
    definition.name = "N";
    definition.source = "N";
    definition.strokes = {moveTo(0.1, 100), drawTo(0.00001, 1e21), drawTo(12345678.125, -2.5),
                          arc(1.0 / 3.0, 0, 360)};
    add(customisation, definition);
    const std::string text = writeCustomisation(customisation);
    // 0.00001 and 1e21 in the form with an exponent, which is the shorter;
    // a third is the 16 digits it takes to say which double it is.
    EXPECT_NE(text.find("[\"move\", 0.1, 100]"), std::string::npos) << text;
    EXPECT_NE(text.find("[\"draw\", 1e-05, 1e+21]"), std::string::npos) << text;
    EXPECT_NE(text.find("[\"draw\", 12345678.125, -2.5]"), std::string::npos) << text;
    EXPECT_NE(text.find("[\"arc\", 0.3333333333333333, 0, 360]"), std::string::npos) << text;
}

// ---- degenerate files -----------------------------------------------------------------------

TEST(CustomisationFormat, ACustomisationOfItsThreeRequiredMembersIsEmptyAndSaysNothingOfTheRest)
{
    const Customisation read =
        readCustomisation(R"({"format": "katana-customisation", "version": 1, "name": "T"})");
    EXPECT_EQ(read.name, "T");
    EXPECT_TRUE(read.description.empty());
    EXPECT_TRUE(read.notice.empty());
    EXPECT_TRUE(read.sources.empty());
    EXPECT_FALSE(read.basedOn.has_value());
    EXPECT_TRUE(read.colours.empty());
    EXPECT_FALSE(read.linework.has_value()) << "absent: it says nothing about the control codes";
    EXPECT_FALSE(read.automation.has_value());
    EXPECT_TRUE(read.library.empty());
    EXPECT_TRUE(read.map.empty());

    // And it is written back as exactly those three, one a line.
    EXPECT_EQ(writeCustomisation(read), "{\n"
                                        "  \"format\": \"katana-customisation\",\n"
                                        "  \"version\": 1,\n"
                                        "  \"name\": \"T\"\n"
                                        "}\n");
}

TEST(CustomisationFormat, EmptyListsAreTheSameAsListsLeftOut)
{
    const Customisation empty = readCustomisation(customisationWith(
        R"("description": "", "notice": [], "sources": [], "colours": {}, "linestyles": [],
           "symbols": [], "codes": [])"));
    EXPECT_TRUE(empty == readCustomisation(customisationWith("")));
}

TEST(CustomisationFormat, LineworkAndAutomationPresentButEmptyAreTheDefaultsAndNotAbsent)
{
    const Customisation read =
        readCustomisation(customisationWith(R"("linework": {}, "automation": {})"));
    ASSERT_TRUE(read.linework.has_value());
    EXPECT_EQ(*read.linework, LineworkCodes{}) << "ST, END, CL, BC, EC, JPN and RECT";
    EXPECT_EQ(read.linework->start, "ST");
    EXPECT_EQ(read.linework->rectangle, "RECT");
    ASSERT_TRUE(read.automation.has_value());
    EXPECT_TRUE(read.automation->codesOnSurveyImport);
    EXPECT_TRUE(read.automation->lineworkOnSurveyImport);

    // A member left out of one of them is its default; the others are as given.
    const Customisation partial = readCustomisation(customisationWith(
        R"("linework": {"close": "C", "join": ""}, "automation": {"lineworkOnSurveyImport": false})"));
    EXPECT_EQ(partial.linework->close, "C");
    EXPECT_EQ(partial.linework->join, "") << "an empty spelling switches the control off";
    EXPECT_EQ(partial.linework->end, "END");
    EXPECT_TRUE(partial.automation->codesOnSurveyImport);
    EXPECT_FALSE(partial.automation->lineworkOnSurveyImport);
    // And the other switch on its own, so that neither is read as the other.
    const Customisation other =
        readCustomisation(customisationWith(R"("automation": {"codesOnSurveyImport": false})"));
    EXPECT_FALSE(other.automation->codesOnSurveyImport);
    EXPECT_TRUE(other.automation->lineworkOnSurveyImport);

    // Each of the seven spellings is the one its own member gives: seven
    // different words, one a control.
    const Customisation seven = readCustomisation(customisationWith(
        R"("linework": {"start": "a", "end": "b", "close": "c", "arcStart": "d", "arcEnd": "e",
                        "join": "f", "rectangle": "g"})"));
    ASSERT_TRUE(seven.linework.has_value());
    EXPECT_EQ(seven.linework->start, "a");
    EXPECT_EQ(seven.linework->end, "b");
    EXPECT_EQ(seven.linework->close, "c");
    EXPECT_EQ(seven.linework->arcStart, "d");
    EXPECT_EQ(seven.linework->arcEnd, "e");
    EXPECT_EQ(seven.linework->join, "f");
    EXPECT_EQ(seven.linework->rectangle, "g");
}

TEST(CustomisationFormat, ADefinitionWithNoStrokesReadsWithOrWithoutAnEmptyList)
{
    const Customisation read = readCustomisation(customisationWith(
        R"("linestyles": [{"name": "Bare"}, {"name": "Listed", "strokes": []}])"));
    ASSERT_EQ(read.library.size(), 2u);
    for (const char* name : {"Bare", "Listed"}) {
        const LineStyle* definition = read.library.find(name);
        ASSERT_NE(definition, nullptr) << name;
        EXPECT_TRUE(definition->strokes.empty());
        EXPECT_TRUE(definition->texts.empty());
        EXPECT_FALSE(definition->symbol);
        EXPECT_EQ(definition->source, "T") << "the file's name, when it says no \"from\"";
    }
}

// Every member a file may leave out, written out at the value it has when left
// out - the defaults docs/customisation.md lists - reads as if it were not
// there. This is the reader's half of "a member at its default is omitted",
// for the words the writer never writes ("units": "world").
TEST(CustomisationFormat, EveryMemberWrittenOutAtItsDefaultReadsAsIfLeftOut)
{
    const Customisation spelled = readCustomisation(customisationWith(R"(
        "linestyles": [{"name": "D", "group": "", "units": "world", "atVertices": false,
                        "length": 0, "factor": 1, "origin": [0, 0], "anchors": [[0, 0], [0, 0]],
                        "stretchMode": 0, "cycleMode": 0, "from": "T",
                        "strokes": [["text", {"text": "", "angle": 0, "height": 0, "justify": "",
                                              "font": "", "widthFactor": 1, "extra": [0, 0, 0]}]]}],
        "codes": [{"key": "A", "sets": "feature", "layer": "", "colour": "", "linestyle": "",
                   "weight": "", "group": "", "comment": "",
                   "symbol": {"name": "", "colour": "", "size": 0, "rotation": 0, "offset": 0,
                              "raise": 0},
                   "text": {"style": "", "colour": "", "units": "", "size": 0, "justifyX": "",
                            "justifyY": "", "offset": 0, "raise": 0, "angle": 0, "slant": 0,
                            "widthFactor": 1, "underline": false, "strikeout": false,
                            "italic": false, "weight": ""},
                   "pipe": {"justify": "", "shape": "", "size1": "", "size2": "",
                            "active": false},
                   "attributes": [], "vertexAttributes": [], "segmentAttributes": []}],
        "sources": [{"name": "S", "definitions": false, "rules": false, "notice": []}])"));
    const Customisation left = readCustomisation(customisationWith(R"(
        "linestyles": [{"name": "D", "strokes": [["text", {}]]}],
        "codes": [{"key": "A", "sets": "feature", "symbol": {}, "text": {}, "pipe": {}}],
        "sources": [{"name": "S"}])"));
    EXPECT_TRUE(spelled == left);

    // And those defaults are the model's own: a definition and a rule made in
    // code with nothing set but what the file gives.
    LineStyle definition;
    definition.name = "D";
    definition.source = "T";
    addText(definition, StrokeText{});
    ASSERT_EQ(left.library.size(), 1u);
    EXPECT_EQ(*left.library.find("D"), definition);
    SurveyRule rule;
    rule.key = "A";
    rule.symbol = SurveySymbol{};
    rule.textStyle = SurveyTextStyle{};
    rule.pipe = SurveyPipe{};
    ASSERT_EQ(left.map.size(), 1u);
    EXPECT_EQ(left.map.rules()[0], rule);
    EXPECT_EQ(left.library.find("D")->factor, 1.0);
    EXPECT_EQ(left.library.find("D")->texts[0].widthFactor, 1.0);
    EXPECT_EQ(left.map.rules()[0].textStyle->widthFactor, 1.0);
}

TEST(CustomisationFormat, TheArrayADefinitionSitsInSaysWhetherItIsASymbol)
{
    const Customisation read = readCustomisation(
        customisationWith(R"("linestyles": [{"name": "Line"}], "symbols": [{"name": "Mark"}])"));
    EXPECT_FALSE(read.library.find("Line")->symbol);
    EXPECT_TRUE(read.library.find("Mark")->symbol);
    // Not whether it is drawn at vertices: that is said separately, and a
    // symbol need not be.
    EXPECT_FALSE(read.library.find("Mark")->atVertices);

    // Nor the other way about: a definition drawn at vertices that sits in
    // "linestyles" is not a symbol for that. All four combinations exist.
    const Customisation vertex = readCustomisation(customisationWith(
        R"("linestyles": [{"name": "V", "atVertices": true}],
           "symbols": [{"name": "S", "atVertices": true}])"));
    ASSERT_NE(vertex.library.find("V"), nullptr);
    EXPECT_TRUE(vertex.library.find("V")->atVertices);
    EXPECT_FALSE(vertex.library.find("V")->symbol);
    ASSERT_NE(vertex.library.find("S"), nullptr);
    EXPECT_TRUE(vertex.library.find("S")->atVertices);
    EXPECT_TRUE(vertex.library.find("S")->symbol);
    // Written back, each is in the list it was read from and says for itself
    // that it is drawn at vertices.
    EXPECT_EQ(writeCustomisation(vertex), "{\n"
                                          "  \"format\": \"katana-customisation\",\n"
                                          "  \"version\": 1,\n"
                                          "  \"name\": \"T\",\n"
                                          "  \"linestyles\": [\n"
                                          "    {\"name\": \"V\", \"atVertices\": true}\n"
                                          "  ],\n"
                                          "  \"symbols\": [\n"
                                          "    {\"name\": \"S\", \"atVertices\": true}\n"
                                          "  ]\n"
                                          "}\n");
}

// Each of the nine words a rule's "sets" may be is one section of the model,
// and that section is written as that word. Taken from the format's own list
// (docs/customisation.md, "A rule"), which gives them in the order of the
// sections and says what each is about: "feature" is where a code goes and how
// it is drawn - the layer, colour, line or point and linestyle the model's Map
// section holds - "attributes" are those on the string and "surface" is
// whether the code goes into one, which the model calls tinable. The round
// trip alone cannot see two of these words exchanged, because the reader and
// the writer share one table of them.
TEST(CustomisationFormat, EachWordARuleSetsIsOneSectionOfTheModelAndIsWrittenAsThatWord)
{
    const std::vector<std::pair<std::string, SurveySection>> words = {
        {"feature", SurveySection::Map},
        {"symbol", SurveySection::VertexSymbol},
        {"text", SurveySection::VertexTextStyle},
        {"pipe", SurveySection::Pipe},
        {"vertexPipe", SurveySection::VertexPipe},
        {"segmentPipe", SurveySection::SegmentPipe},
        {"attributes", SurveySection::StringAttribute},
        {"vertexAttributes", SurveySection::VertexAttribute},
        {"surface", SurveySection::Tinable},
    };
    ASSERT_EQ(words.size(), 9u);
    for (const auto& [word, section] : words) {
        const Customisation read = readCustomisation(
            customisationWith("\"codes\": [{\"key\": \"A\", \"sets\": \"" + word + "\"}]"));
        ASSERT_EQ(read.map.size(), 1u) << word;
        EXPECT_EQ(read.map.rules()[0].section, section) << word;

        Customisation made;
        made.name = "T";
        SurveyRule rule;
        rule.key = "A";
        rule.section = section;
        add(made, rule);
        EXPECT_EQ(writeCustomisation(made), "{\n"
                                            "  \"format\": \"katana-customisation\",\n"
                                            "  \"version\": 1,\n"
                                            "  \"name\": \"T\",\n"
                                            "  \"codes\": [\n"
                                            "    {\"key\": \"A\", \"sets\": \"" +
                                                word +
                                                "\"}\n"
                                                "  ]\n"
                                                "}\n")
            << word;
    }
}

// The reference rules repeat themselves - 446 of the 1,624 say again, field
// for field, what an earlier rule said - and the format keeps a customisation
// as it is: two rules alike are two rules, each on its own line, and the
// second keeps its place in the order.
TEST(CustomisationFormat, TwoRulesAlikeInEveryFieldAreBothKeptAndBothWritten)
{
    const Customisation read = readCustomisation(customisationWith(
        R"("codes": [{"key": "WM*", "sets": "feature", "layer": "SURVEY"},
                     {"key": "AC*", "sets": "symbol"},
                     {"key": "WM*", "sets": "feature", "layer": "SURVEY"}])"));
    ASSERT_EQ(read.map.size(), 3u);
    EXPECT_EQ(read.map.rules()[0], read.map.rules()[2]);
    EXPECT_EQ(read.map.rules()[1].key, "AC*");
    EXPECT_EQ(writeCustomisation(read),
              "{\n"
              "  \"format\": \"katana-customisation\",\n"
              "  \"version\": 1,\n"
              "  \"name\": \"T\",\n"
              "  \"codes\": [\n"
              "    {\"key\": \"WM*\", \"sets\": \"feature\", \"layer\": \"SURVEY\"},\n"
              "    {\"key\": \"AC*\", \"sets\": \"symbol\"},\n"
              "    {\"key\": \"WM*\", \"sets\": \"feature\", \"layer\": \"SURVEY\"}\n"
              "  ]\n"
              "}\n");
}

// "In name order" is the order of the names' bytes, as the library keeps
// them: every upper-case letter is before every lower-case one ('Z' is 5A,
// 'a' is 61), so "Zinc" is before "apple".
TEST(CustomisationFormat, DefinitionsAreWrittenInTheOrderOfTheirBytesSoUpperCaseComesFirst)
{
    Customisation customisation;
    customisation.name = "T";
    for (const char* name : {"apple", "Zinc", "zinc", "Apple"}) {
        LineStyle definition;
        definition.name = name;
        definition.source = "T";
        add(customisation, definition);
    }
    LineStyle symbol;
    symbol.name = "Mark";
    symbol.source = "T";
    symbol.symbol = true;
    add(customisation, symbol);
    EXPECT_EQ(writeCustomisation(customisation), "{\n"
                                                 "  \"format\": \"katana-customisation\",\n"
                                                 "  \"version\": 1,\n"
                                                 "  \"name\": \"T\",\n"
                                                 "  \"linestyles\": [\n"
                                                 "    {\"name\": \"Apple\"},\n"
                                                 "    {\"name\": \"Zinc\"},\n"
                                                 "    {\"name\": \"apple\"},\n"
                                                 "    {\"name\": \"zinc\"}\n"
                                                 "  ],\n"
                                                 "  \"symbols\": [\n"
                                                 "    {\"name\": \"Mark\"}\n"
                                                 "  ]\n"
                                                 "}\n");
}

TEST(CustomisationFormat, ADefinitionsSourceIsItsFromOrElseTheFilesName)
{
    const Customisation read = readCustomisation(customisationWith(
        R"("linestyles": [{"name": "Own"}, {"name": "Other", "from": "Site"},
                          {"name": "Session", "from": ""}])"));
    EXPECT_EQ(read.library.find("Own")->source, "T");
    EXPECT_EQ(read.library.find("Other")->source, "Site");
    EXPECT_EQ(read.library.find("Session")->source, "") << "made in a session: of no customisation";

    // Written back, only the two that differ from the name say so.
    const std::string text = writeCustomisation(read);
    EXPECT_EQ(count(text, "\"from\": "), 2u) << text;
}

// ---- negative zero and the reach of the numbers ---------------------------------------------

TEST(CustomisationFormat, NegativeZeroSurvivesWhereverANumberIs)
{
    Customisation customisation;
    customisation.name = "Z";
    LineStyle definition;
    definition.name = "Z";
    definition.source = "Z";
    definition.length = -0.0; // equal to the default 0 by ==, and still not left out
    definition.origin = {-0.0, 0.0};
    definition.anchor1 = {0.0, -0.0};
    definition.anchor2 = {-0.0, 0.0};
    definition.strokes = {moveTo(-0.0, 0.0), arc(-0.0, -0.0, 0.0), circle(-0.0), dot(-0.0)};
    addText(definition, StrokeText{"", -0.0, -0.0, "", "", -0.0, {-0.0, 0.0, -0.0}});
    add(customisation, definition);
    SurveyRule rule;
    rule.key = "Z";
    rule.symbol = SurveySymbol{"", "", -0.0, -0.0, -0.0, -0.0};
    SurveyTextStyle style;
    style.size = -0.0;
    style.offset = -0.0;
    style.raise = -0.0;
    style.angle = -0.0;
    style.slant = -0.0;
    style.widthFactor = -0.0;
    rule.textStyle = style;
    add(customisation, rule);

    // Every number a definition and a rule have, each written -0.0 where it
    // is negative zero and 0 where it is zero: by hand from the layout, with
    // nothing left out for being "equal to" a default of 0 or 1.
    const std::string text = writeCustomisation(customisation);
    EXPECT_EQ(
        text,
        R"json({
  "format": "katana-customisation",
  "version": 1,
  "name": "Z",
  "linestyles": [
    {"name": "Z", "length": -0.0, "origin": [-0.0, 0], "anchors": [[0, -0.0], [-0.0, 0]], "strokes": [
      ["move", -0.0, 0],
      ["arc", -0.0, -0.0, 0],
      ["circle", -0.0],
      ["dot", -0.0],
      ["text", {"angle": -0.0, "height": -0.0, "widthFactor": -0.0, "extra": [-0.0, 0, -0.0]}]
    ]}
  ],
  "codes": [
    {"key": "Z", "sets": "feature", "symbol": {"size": -0.0, "rotation": -0.0, "offset": -0.0, "raise": -0.0}, "text": {"size": -0.0, "offset": -0.0, "raise": -0.0, "angle": -0.0, "slant": -0.0, "widthFactor": -0.0}}
  ]
}
)json");

    const Customisation again = readCustomisation(text);
    const LineStyle* read = again.library.find("Z");
    ASSERT_NE(read, nullptr);
    EXPECT_TRUE(std::signbit(read->length));
    EXPECT_TRUE(std::signbit(read->origin.x));
    EXPECT_FALSE(std::signbit(read->origin.y));
    EXPECT_FALSE(std::signbit(read->anchor1.x));
    EXPECT_TRUE(std::signbit(read->anchor1.y));
    EXPECT_TRUE(std::signbit(read->anchor2.x));
    EXPECT_FALSE(std::signbit(read->anchor2.y));
    ASSERT_EQ(read->strokes.size(), 5u);
    EXPECT_TRUE(std::signbit(read->strokes[0].point.x));
    EXPECT_FALSE(std::signbit(read->strokes[0].point.y));
    EXPECT_TRUE(std::signbit(read->strokes[1].radius));
    EXPECT_TRUE(std::signbit(read->strokes[1].startAngle));
    EXPECT_FALSE(std::signbit(read->strokes[1].endAngle));
    EXPECT_TRUE(std::signbit(read->strokes[2].radius));
    EXPECT_TRUE(std::signbit(read->strokes[3].radius));
    ASSERT_EQ(read->texts.size(), 1u);
    EXPECT_TRUE(std::signbit(read->texts[0].angle));
    EXPECT_TRUE(std::signbit(read->texts[0].height));
    EXPECT_TRUE(std::signbit(read->texts[0].widthFactor));
    EXPECT_TRUE(std::signbit(read->texts[0].unnamed[0]));
    EXPECT_FALSE(std::signbit(read->texts[0].unnamed[1]));
    EXPECT_TRUE(std::signbit(read->texts[0].unnamed[2]));
    ASSERT_EQ(again.map.size(), 1u);
    const SurveyRule& back = again.map.rules()[0];
    ASSERT_TRUE(back.symbol.has_value());
    EXPECT_TRUE(std::signbit(back.symbol->size));
    EXPECT_TRUE(std::signbit(back.symbol->rotation));
    EXPECT_TRUE(std::signbit(back.symbol->offset));
    EXPECT_TRUE(std::signbit(back.symbol->raise));
    ASSERT_TRUE(back.textStyle.has_value());
    EXPECT_TRUE(std::signbit(back.textStyle->size));
    EXPECT_TRUE(std::signbit(back.textStyle->offset));
    EXPECT_TRUE(std::signbit(back.textStyle->raise));
    EXPECT_TRUE(std::signbit(back.textStyle->angle));
    EXPECT_TRUE(std::signbit(back.textStyle->slant));
    EXPECT_TRUE(std::signbit(back.textStyle->widthFactor));
}

// A property over doubles: whatever finite number a customisation holds comes
// back with the same BITS, by every way the reader takes a number in. The
// classes it claims to reach are counted and each count is asserted, because
// a generator that never made a subnormal would pass this without having
// tried one.
TEST(CustomisationFormat, EveryFiniteDoubleComesBackBitForBit)
{
    // The cases a text form is most likely to get wrong, named.
    std::vector<double> values = {
        0.0,
        -0.0,
        0.1,
        1.0 / 3.0,
        5e-324,                             // the smallest subnormal
        2.2250738585072009e-308,            // the largest subnormal
        2.2250738585072014e-308,            // the smallest normal
        std::numeric_limits<double>::max(), // 1.7976931348623157e308
        -std::numeric_limits<double>::max(),
        9007199254740992.0,      // 2^53: the last gap of one
        9007199254740994.0,      // 2^53 + 2
        9223372036854775808.0,   // 2^63: one past a signed 64-bit integer
        18446744073709551616.0,  // 2^64: one past an unsigned one
        123456789012345680000.0, // written in full, 21 digits
        1e21,
        1e22,
        1e23,
        0.000001,
        12345678.125,
        -0.47350000000000003,
    };
    // Then any bit pattern at all: SplitMix64 (Steele, Lea and Flood, 2014)
    // from a fixed seed, so the run is the same every time.
    std::uint64_t state = 0x9E3779B97F4A7C15ULL;
    const auto next = [&state] {
        std::uint64_t z = (state += 0x9E3779B97F4A7C15ULL);
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
        return z ^ (z >> 31);
    };
    while (values.size() < 4000) {
        const double value = std::bit_cast<double>(next());
        if (std::isfinite(value)) {
            values.push_back(value);
        }
    }
    // And subnormals made on purpose: a random pattern has one in 2,047.
    for (int i = 0; i < 50; ++i) {
        values.push_back(std::bit_cast<double>(next() & 0x800FFFFFFFFFFFFFULL));
    }
    // Taken two at a time and three at a time below, so a multiple of six.
    while (values.size() % 6 != 0) {
        values.push_back(1.5);
    }

    // Every value goes down each of the three ways the reader takes a number
    // in, which share no code past the JSON library:
    //   * a stroke's values, made into doubles as the text is parsed;
    //   * a point read from the tree: a definition's origin and two anchors;
    //   * a member read from the tree: the angle and width factor of a text
    //     stroke's object and its three "extra" numbers, a rule's symbol
    //     (rotation, offset, raise) and its text (offset, raise, angle, slant,
    //     width factor).
    // No member is used that the model restricts (a length, a factor, a
    // size), so any finite double is a value each of these may hold.
    Customisation customisation;
    customisation.name = "D";
    LineStyle definition;
    definition.name = "D";
    definition.source = "D";
    for (std::size_t i = 0; i < values.size(); i += 2) {
        definition.strokes.push_back(moveTo(values[i], values[i + 1]));
    }
    for (std::size_t i = 0; i < values.size(); i += 3) {
        StrokeText text;
        text.angle = values[i];
        text.widthFactor = values[i + 1];
        text.unnamed = {values[i], values[i + 1], values[i + 2]};
        addText(definition, text);
    }
    add(customisation, definition);
    for (std::size_t i = 0; i < values.size(); i += 2) {
        LineStyle points;
        points.name = "P" + std::to_string(i / 2);
        points.source = "D";
        points.origin = {values[i], values[i + 1]};
        points.anchor1 = {values[i + 1], values[i]};
        points.anchor2 = {values[i], values[i]};
        add(customisation, points);
    }
    for (std::size_t i = 0; i < values.size(); i += 3) {
        SurveyRule rule;
        rule.key = "R";
        rule.symbol = SurveySymbol{"", "", 0, values[i], values[i + 1], values[i + 2]};
        SurveyTextStyle style;
        style.offset = values[i];
        style.raise = values[i + 1];
        style.angle = values[i + 2];
        style.slant = values[i];
        style.widthFactor = values[i + 1];
        rule.textStyle = style;
        add(customisation, rule);
    }

    const std::string text = writeCustomisation(customisation);
    const Customisation again = readCustomisation(text);
    const LineStyle* read = again.library.find("D");
    ASSERT_NE(read, nullptr);
    ASSERT_EQ(read->strokes.size(), values.size() / 2 + values.size() / 3);
    ASSERT_EQ(read->texts.size(), values.size() / 3);
    ASSERT_EQ(again.map.size(), values.size() / 3);

    const auto same = [](double back, double value) {
        return std::bit_cast<std::uint64_t>(back) == std::bit_cast<std::uint64_t>(value);
    };
    for (std::size_t i = 0; i < values.size(); i += 2) {
        const double x = values[i];
        const double y = values[i + 1];
        const LineStyle* points = again.library.find("P" + std::to_string(i / 2));
        ASSERT_NE(points, nullptr) << i;
        ASSERT_TRUE(same(points->origin.x, x) && same(points->origin.y, y))
            << "an origin, values " << i << ": wrote " << x << " and " << y << ", read "
            << points->origin.x << " and " << points->origin.y;
        ASSERT_TRUE(same(points->anchor1.x, y) && same(points->anchor1.y, x) &&
                    same(points->anchor2.x, x) && same(points->anchor2.y, x))
            << "the anchors, values " << i << ": " << x << " and " << y;
    }
    for (std::size_t i = 0; i < values.size(); i += 3) {
        const double a = values[i];
        const double b = values[i + 1];
        const double c = values[i + 2];
        const StrokeText& stroke = read->texts[i / 3];
        ASSERT_TRUE(same(stroke.angle, a) && same(stroke.widthFactor, b))
            << "a text stroke's angle and width factor, values " << i << ": " << a << " and " << b;
        ASSERT_TRUE(same(stroke.unnamed[0], a) && same(stroke.unnamed[1], b) &&
                    same(stroke.unnamed[2], c))
            << "a text stroke's extra, values " << i << ": " << a << ", " << b << " and " << c;
        const SurveyRule& rule = again.map.rules()[i / 3];
        ASSERT_TRUE(rule.symbol.has_value() && rule.textStyle.has_value()) << i;
        ASSERT_TRUE(same(rule.symbol->rotation, a) && same(rule.symbol->offset, b) &&
                    same(rule.symbol->raise, c))
            << "a rule's symbol, values " << i << ": " << a << ", " << b << " and " << c;
        ASSERT_TRUE(same(rule.textStyle->offset, a) && same(rule.textStyle->raise, b) &&
                    same(rule.textStyle->angle, c) && same(rule.textStyle->slant, a) &&
                    same(rule.textStyle->widthFactor, b))
            << "a rule's text, values " << i << ": " << a << ", " << b << " and " << c;
    }

    std::size_t negative = 0;
    std::size_t subnormal = 0;
    std::size_t beyondInteger = 0; // at or above 2^64: no integer type holds it
    std::size_t tiny = 0;          // below 1e-5: written with a negative exponent
    for (std::size_t i = 0; i < values.size(); ++i) {
        const Stroke& stroke = read->strokes[i / 2];
        const double back = i % 2 == 0 ? stroke.point.x : stroke.point.y;
        ASSERT_EQ(std::bit_cast<std::uint64_t>(back), std::bit_cast<std::uint64_t>(values[i]))
            << "a stroke, value " << i << ": wrote " << values[i] << ", read " << back;
        negative += std::signbit(values[i]) ? 1 : 0;
        subnormal += std::fpclassify(values[i]) == FP_SUBNORMAL ? 1 : 0;
        beyondInteger += std::abs(values[i]) >= 18446744073709551616.0 ? 1 : 0;
        tiny += values[i] != 0.0 && std::abs(values[i]) < 1e-5 ? 1 : 0;
    }
    // By hand: half of all bit patterns are negative, and half have an
    // exponent above 2^64 or below 1e-5 each, give or take; 50 subnormals
    // were added outright, and two more are named above.
    EXPECT_GT(negative, 1500u);
    EXPECT_GE(subnormal, 52u);
    EXPECT_GT(beyondInteger, 1000u);
    EXPECT_GT(tiny, 1000u);
    EXPECT_NE(text.find("e+"), std::string::npos) << "no number was written with an exponent";
    EXPECT_NE(text.find("e-"), std::string::npos);
}

// ---- what a write holds ---------------------------------------------------------------------

TEST(CustomisationFormat, TheOptionsLeaveOutLinestylesSymbolsOrCodesAndNothingElse)
{
    const Customisation whole = smallCustomisation();

    CustomisationWriteOptions codesOnly;
    codesOnly.linestyles = false;
    codesOnly.symbols = false;
    const Customisation rules = readCustomisation(writeCustomisation(whole, codesOnly));
    EXPECT_TRUE(rules.library.empty());
    EXPECT_EQ(rules.map, whole.map);
    EXPECT_EQ(rules.colours, whole.colours) << "the colours go with whatever is written";
    EXPECT_EQ(rules.linework, whole.linework);
    EXPECT_EQ(rules.name, "Site");
    // "And nothing else": what was read is the whole value with only its
    // definitions gone - the description, the notice, the sources, the base,
    // the colours, the linework and the automation are all as they were.
    Customisation withoutDefinitions = whole;
    withoutDefinitions.library = katana::entity::StyleLibrary{};
    EXPECT_TRUE(rules == withoutDefinitions);

    CustomisationWriteOptions symbolsOnly;
    symbolsOnly.linestyles = false;
    symbolsOnly.codes = false;
    const Customisation symbols = readCustomisation(writeCustomisation(whole, symbolsOnly));
    EXPECT_EQ(symbols.library.names(), (std::vector<std::string>{"Peg"}));
    EXPECT_TRUE(symbols.map.empty());
    Customisation onlySymbols = whole;
    onlySymbols.map = katana::entity::SurveyMap{};
    for (const char* linestyle : {"Empty", "Fence", "Gate"}) {
        ASSERT_TRUE(onlySymbols.library.remove(linestyle).ok()) << linestyle;
    }
    EXPECT_TRUE(symbols == onlySymbols);

    CustomisationWriteOptions linestylesOnly;
    linestylesOnly.symbols = false;
    linestylesOnly.codes = false;
    const Customisation linestyles = readCustomisation(writeCustomisation(whole, linestylesOnly));
    EXPECT_EQ(linestyles.library.names(), (std::vector<std::string>{"Empty", "Fence", "Gate"}));
    EXPECT_TRUE(linestyles.map.empty());
    Customisation onlyLinestyles = whole;
    onlyLinestyles.map = katana::entity::SurveyMap{};
    ASSERT_TRUE(onlyLinestyles.library.remove("Peg").ok());
    EXPECT_TRUE(linestyles == onlyLinestyles);

    // With every option on, the whole value.
    EXPECT_TRUE(readCustomisation(writeCustomisation(whole, CustomisationWriteOptions{})) == whole);
}

TEST(CustomisationFormat, OnlyTheNamedDefinitionsAreWrittenInNameOrderWhateverOrderTheyAreNamedIn)
{
    CustomisationWriteOptions options;
    options.only = {"Peg", "Gate", "Empty", "Gate"}; // one of them twice
    const std::string text = writeCustomisation(smallCustomisation(), options);
    const Customisation read = readCustomisation(text);
    EXPECT_EQ(read.library.names(), (std::vector<std::string>{"Empty", "Gate", "Peg"}));
    EXPECT_LT(text.find("\"name\": \"Empty\""), text.find("\"name\": \"Gate\""));
    EXPECT_EQ(read.map, smallCustomisation().map) << "the names choose definitions, not rules";
}

TEST(CustomisationFormat, ADefinitionAskedForByNameIsNeverLeftOutSilently)
{
    CustomisationWriteOptions missing;
    missing.only = {"Fence", "No Such Style"};
    const auto unknown = katana::entity::customisationToJson(smallCustomisation(), missing);
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, katana::core::ErrorCode::NotFound);
    EXPECT_NE(unknown.error().describe().find("No Such Style"), std::string::npos)
        << unknown.error().describe();

    // Peg is a symbol; asking for it by name while leaving symbols out is two
    // instructions that cannot both be followed.
    CustomisationWriteOptions excluded;
    excluded.symbols = false;
    excluded.only = {"Fence", "Peg"};
    const auto dropped = katana::entity::customisationToJson(smallCustomisation(), excluded);
    ASSERT_FALSE(dropped.ok());
    EXPECT_EQ(dropped.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_NE(dropped.error().describe().find("Peg"), std::string::npos)
        << dropped.error().describe();
}

// ---- one definition as text -----------------------------------------------------------------

// The object a file holds for "Fence" above, with the file's four blanks of
// indent taken off and no line break after it.
constexpr const char* kFenceText =
    R"json({"name": "Fence", "group": "Site/Lines", "length": 2.5, "strokes": [
  ["move", 0, 0],
  ["draw", 1.5, 0],
  ["pen", "red"],
  ["arc", -0.25, 0, 180],
  ["circle", 0.5],
  ["dot", 0],
  ["text", {"text": "F", "height": 1.5, "justify": "middle-centre"}]
]})json";

TEST(CustomisationFormat, ADefinitionOnItsOwnIsTheObjectAFileHolds)
{
    const auto text = katana::entity::definitionToJson(fence(), "Site");
    ASSERT_TRUE(text.ok()) << text.error().describe();
    EXPECT_EQ(*text, kFenceText);

    const auto read = katana::entity::definitionFromJson(kFenceText, false, "Site");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(*read, fence());
}

TEST(CustomisationFormat, ADefinitionOnItsOwnIsToldItsArrayAndTheNameItSitsUnder)
{
    // Under no name, the source is said; under another's, it is said too.
    const auto alone = katana::entity::definitionToJson(fence());
    ASSERT_TRUE(alone.ok()) << alone.error().describe();
    EXPECT_NE(alone->find("\"length\": 2.5, \"from\": \"Site\", \"strokes\": ["), std::string::npos)
        << *alone;
    const auto back = katana::entity::definitionFromJson(*alone, false);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_EQ(back->source, "Site");

    // The text says neither which array nor which name: the caller does.
    const auto symbol = katana::entity::definitionFromJson(R"({"name": "Mark"})", true, "Mine");
    ASSERT_TRUE(symbol.ok()) << symbol.error().describe();
    EXPECT_TRUE(symbol->symbol);
    EXPECT_EQ(symbol->source, "Mine");
    const auto unnamed = katana::entity::definitionFromJson(R"({"name": "Mark"})", false);
    ASSERT_TRUE(unnamed.ok()) << unnamed.error().describe();
    EXPECT_FALSE(unnamed->symbol);
    EXPECT_EQ(unnamed->source, "");
}

TEST(CustomisationFormat, ADefinitionOnItsOwnIsRefusedAsOneInAFileIsAndNamedAsADefinition)
{
    using katana::core::ErrorCode;
    const auto notJson = katana::entity::definitionFromJson("move 0 0", false);
    ASSERT_FALSE(notJson.ok());
    EXPECT_EQ(notJson.error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(notJson.error().message, "not a definition in the Katana customisation format");

    const auto list = katana::entity::definitionFromJson("[]", false);
    ASSERT_FALSE(list.ok());
    EXPECT_EQ(list.error().message, "not a definition in the Katana customisation format");

    const auto unknown =
        katana::entity::definitionFromJson(R"({"name": "Fence", "colour": "red"})", false);
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::ParseFailure);
    katana::testing::expectNames(unknown.error(), "definition \"Fence\"", "colour");

    const auto twice = katana::entity::definitionFromJson(
        R"({"name": "Fence", "group": "a", "group": "b"})", false);
    ASSERT_FALSE(twice.ok());
    EXPECT_EQ(twice.error().code, ErrorCode::ParseFailure);
    katana::testing::expectNames(twice.error(), "definition \"Fence\"", "group");

    // No library is there to validate it, so reading does: a factor of 0
    // would collapse every stroke onto the origin.
    const auto invalid =
        katana::entity::definitionFromJson(R"({"name": "Fence", "factor": 0})", false);
    ASSERT_FALSE(invalid.ok());
    EXPECT_EQ(invalid.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(invalid.error().message.rfind("definition \"Fence\": ", 0), 0u)
        << invalid.error().describe();

    // And writing one that validate refuses fails the same way.
    LineStyle unbounded = fence();
    unbounded.length = std::numeric_limits<double>::infinity();
    const auto written = katana::entity::definitionToJson(unbounded);
    ASSERT_FALSE(written.ok());
    EXPECT_EQ(written.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(written.error().message.rfind("definition \"Fence\": ", 0), 0u)
        << written.error().describe();
}

// ---- comparing, naming, the digest ----------------------------------------------------------

TEST(Customisation, TwoAreEqualOnlyWhenEveryMemberIs)
{
    const Customisation base = smallCustomisation();
    EXPECT_TRUE(base == smallCustomisation());

    std::vector<Customisation> changed(10, base);
    changed[0].name = "Other";
    changed[1].description += ".";
    changed[2].notice.push_back("one more line");
    changed[3].sources.pop_back();
    changed[4].basedOn.reset();
    EXPECT_TRUE(changed[5].colours.add("one more", Color{1, 2, 3, 255}).ok());
    changed[6].linework->start = "S";
    changed[7].automation.reset();
    {
        LineStyle fenceAgain = fence();
        fenceAgain.symbol = true; // one flag of one definition
        ASSERT_TRUE(changed[8].library.update(fenceAgain).ok());
    }
    ASSERT_TRUE(changed[9].map.move(0, 1).ok()); // the same rules in another order
    for (std::size_t i = 0; i < changed.size(); ++i) {
        EXPECT_FALSE(changed[i] == base) << "change " << i << " went unnoticed";
        EXPECT_TRUE(changed[i] != base);
    }

    // A definition more, with every other one equal, is a difference too.
    Customisation larger = base;
    LineStyle extra;
    extra.name = "Extra";
    ASSERT_TRUE(larger.library.add(extra).ok());
    EXPECT_FALSE(larger == base);
    EXPECT_FALSE(base == larger);
}

TEST(Customisation, ANameIsOneLineOfTextAndNotAPath)
{
    using katana::core::ErrorCode;
    using katana::entity::validateCustomisationName;
    EXPECT_TRUE(validateCustomisationName("NSW").ok());
    EXPECT_TRUE(validateCustomisationName("Site set 2026 (draft)").ok());
    EXPECT_TRUE(validateCustomisationName("Z\xC3\xBCrich").ok()) << "any UTF-8";
    EXPECT_TRUE(validateCustomisationName("linestyles.4d").ok()) << "a file's name is a name";

    // What a project's record of names cannot hold: it stores one a line, and
    // refuses a path separator (storage's validateMetadata).
    for (const char* name : {"", "Roads/2026", "Roads\\2026", "two\nlines", "two\rlines"}) {
        const auto status = validateCustomisationName(name);
        ASSERT_FALSE(status.ok()) << '"' << name << '"';
        EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    }
    const auto notText = validateCustomisationName("\xFF\xFE");
    ASSERT_FALSE(notText.ok());
    EXPECT_EQ(notText.error().code, ErrorCode::InvalidArgument);
}

// A name is an identity - a project records it, and a drawing is matched to
// its customisation by it - so two names that look alike must BE alike, and a
// name must be something a person can see. The project's store would take
// every one of these; they are refused here.
TEST(Customisation, ANameHasNoBlankAtEitherEndAndNoCharacterThatCannotBeSeen)
{
    using katana::core::ErrorCode;
    using katana::entity::validateCustomisationName;
    // A blank before or after, and blanks alone.
    for (const char* name : {"NSW ", " NSW", " ", "   ", " Site set "}) {
        const auto status = validateCustomisationName(name);
        ASSERT_FALSE(status.ok()) << '"' << name << '"';
        EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
        EXPECT_NE(status.error().message.find("blank"), std::string::npos)
            << status.error().describe();
    }
    // The control characters: U+0000 to U+001F and U+007F. A tab, an escape
    // sequence that would colour a terminal, a NUL in the middle, a delete, a
    // form feed; and a tab at an end is a control character before it is a
    // blank.
    for (const std::string& name :
         {std::string("a\tb"), std::string("a\x1B[31mb"), std::string("a\0b", 3),
          std::string("a\x7F"), std::string("\fNSW"), std::string("NSW\t"), std::string("\x01")}) {
        const auto status = validateCustomisationName(name);
        ASSERT_FALSE(status.ok()) << name.size() << " bytes";
        EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
        EXPECT_NE(status.error().message.find("control character"), std::string::npos)
            << status.error().describe();
    }
    // What is refused is shown with the character made visible, as JSON
    // escapes it, so that the refusal itself can be read.
    const auto escape = validateCustomisationName("a\x1B[31mb");
    ASSERT_FALSE(escape.ok());
    EXPECT_EQ(escape.error().context, "\"a\\u001b[31mb\"");

    // A blank inside a name, two of them, and what is not ASCII (a no-break
    // space, C2 A0, included: only the plain blank is looked for) are a name.
    for (const char* name : {"Site set 2026", "a  b", "N S W", "Z\xC3\xBCrich S\xC3\xBC"
                                                              "d",
                             "NSW\xC2\xA0"}) {
        EXPECT_TRUE(validateCustomisationName(name).ok()) << '"' << name << '"';
    }
}

// FNV-1a, 64-bit, against the algorithm's published test vectors (Fowler,
// Noll and Vo: the empty string, "a" and "foobar") - the vectors the cad
// layer's sourceNameHash is pinned to, here in the 16 digits a file holds.
TEST(Customisation, TheDigestIsFnv1a64AsSixteenLowerCaseHexDigits)
{
    using katana::entity::customisationDigest;
    EXPECT_EQ(customisationDigest(""), "cbf29ce484222325");
    EXPECT_EQ(customisationDigest("a"), "af63dc4c8601ec8c");
    EXPECT_EQ(customisationDigest("foobar"), "85944171f73967e8");
    // Sixteen digits ALWAYS: none of the three above begins with 0, so a
    // digest written without its leading zeros would pass them. The same
    // published list gives 0x08985907b541d342 for "fo".
    EXPECT_EQ(customisationDigest("fo"), "08985907b541d342");
    // Every byte counts, a zero byte and one above 0x7F included.
    EXPECT_NE(customisationDigest(std::string("a\0", 2)), customisationDigest("a"));
    EXPECT_NE(customisationDigest("\xC3\xBC"), customisationDigest("\xC3\xBD"));
    // A digest it gives is one a file's "basedOn" accepts.
    Customisation customisation;
    customisation.name = "Kept";
    customisation.basedOn = CustomisationBase{"NSW", customisationDigest("anything")};
    EXPECT_TRUE(readCustomisation(writeCustomisation(customisation)) == customisation);
}
