// What a colour NAME means in a drawing (colour_lookup.hpp): the Document's
// own table, then the standard names - and the same answer as the function
// survey coding takes.

#include <gtest/gtest.h>

#include <optional>
#include <string>

#include "katana/cad/code_table.hpp"
#include "katana/cad/colour_lookup.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/commands/entity_commands.hpp"

namespace cmd = katana::commands;
using katana::cad::Document;
using katana::entity::Color;
using katana::entity::EntityId;
using katana::geometry::Point2;

namespace {

// A table with one name of its own, in a colour no standard name has.
katana::entity::ColourTable siteColours()
{
    katana::entity::ColourTable colours;
    EXPECT_TRUE(colours.add("sui gas", Color{12, 34, 56, 255}).ok());
    return colours;
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

// One rule: gas mains go on GAS, in the customisation's own colour.
katana::entity::SurveyMap gasMains()
{
    katana::entity::SurveyMap map;
    katana::entity::SurveyRule rule;
    rule.key = "GM*";
    rule.model = "GAS";
    rule.colour = "sui gas";
    rule.linestyle = "GAS Main";
    EXPECT_TRUE(map.add(rule).ok());
    return map;
}

} // namespace

TEST(ColourLookup, AStandardNameResolvesInADocumentWithNoCustomisation)
{
    const Document document;
    // Red is full red and nothing else, however the name is written.
    for (const char* name : {"red", "RED", " Red "}) {
        const auto red = katana::cad::resolveColour(document, name);
        ASSERT_TRUE(red.has_value()) << name;
        EXPECT_EQ(red->r, 255) << name;
        EXPECT_EQ(red->g, 0) << name;
        EXPECT_EQ(red->b, 0) << name;
    }
    // A name nothing knows leaves the colour alone: no guess is made.
    EXPECT_FALSE(katana::cad::resolveColour(document, "sui gas").has_value());
    EXPECT_FALSE(katana::cad::resolveColour(document, "pen 025").has_value());
    EXPECT_FALSE(katana::cad::resolveColour(document, "").has_value());
}

TEST(ColourLookup, TheCustomisationsOwnNamesResolveBesideTheStandardOnes)
{
    Document document;
    document.setColourTable(siteColours());
    // Its own name, compared as colour names are.
    EXPECT_EQ(katana::cad::resolveColour(document, "sui gas"),
              (std::optional<Color>{Color{12, 34, 56, 255}}));
    EXPECT_EQ(katana::cad::resolveColour(document, "SUI_Gas"),
              (std::optional<Color>{Color{12, 34, 56, 255}}));
    // The standard names are still there.
    const auto red = katana::cad::resolveColour(document, "red");
    ASSERT_TRUE(red.has_value());
    EXPECT_EQ(red->r, 255);
    EXPECT_EQ(red->g, 0);
    // And a name neither has is still unknown.
    EXPECT_FALSE(katana::cad::resolveColour(document, "sui water").has_value());
}

TEST(ColourLookup, TheLookupFunctionAsksTheDocumentAtEachCall)
{
    Document document;
    // Made BEFORE the table is there, and of both types that take one.
    const katana::cad::ColourLookup lookup = katana::cad::colourLookup(document);
    katana::cad::SurveyCodingOptions options;
    options.colourOf = katana::cad::colourLookup(document);

    EXPECT_FALSE(lookup("sui gas").has_value());
    document.setColourTable(siteColours());
    EXPECT_EQ(lookup("sui gas"), (std::optional<Color>{Color{12, 34, 56, 255}}));
    EXPECT_EQ(options.colourOf("sui gas"), (std::optional<Color>{Color{12, 34, 56, 255}}));
    // A table installed later is the one it answers from.
    document.setColourTable({});
    EXPECT_FALSE(lookup("sui gas").has_value());
    EXPECT_TRUE(lookup("red").has_value());
}

TEST(ColourLookup, SurveyCodingGivenTheLookupColoursByTheCustomisationsNames)
{
    // A rule naming a colour only the customisation defines. With the
    // Document's lookup the style it makes has that colour.
    Document document;
    document.setSurveyMap(gasMains());
    document.setColourTable(siteColours());
    addCodedPoint(document, Point2(0, 0), "GM01");

    katana::cad::SurveyCodingOptions options;
    options.colourOf = katana::cad::colourLookup(document);
    auto coded = katana::cad::applySurveyCodes(document, options);
    ASSERT_TRUE(coded.ok()) << coded.error().describe();
    ASSERT_NE(*coded, nullptr);
    ASSERT_TRUE(document.execute(std::move(*coded)).ok());

    const auto* style = document.model().styles.find("GAS Main");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->color, (std::optional<Color>{Color{12, 34, 56, 255}}));
}

TEST(ColourLookup, SurveyCodingGivenNoLookupStillLeavesColoursAloneWhateverTheDocumentKnows)
{
    // Offered, not applied behind a caller's back: with no lookup passed, no
    // colour is set, although the Document could have resolved the name.
    Document document;
    document.setSurveyMap(gasMains());
    document.setColourTable(siteColours());
    addCodedPoint(document, Point2(0, 0), "GM01");

    auto coded = katana::cad::applySurveyCodes(document, {});
    ASSERT_TRUE(coded.ok()) << coded.error().describe();
    ASSERT_NE(*coded, nullptr);
    ASSERT_TRUE(document.execute(std::move(*coded)).ok());

    const auto* style = document.model().styles.find("GAS Main");
    ASSERT_NE(style, nullptr);
    EXPECT_FALSE(style->color.has_value());
}
