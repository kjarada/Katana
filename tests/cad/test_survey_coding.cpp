// Applying a survey code to a drawing (PLAN.MD 20.3, slice 4).

#include <gtest/gtest.h>

#include "katana/cad/survey_coding.hpp"
#include "katana/commands/entity_commands.hpp"

namespace cmd = katana::commands;
using katana::cad::Document;
using katana::cad::SurveyCodingOptions;
using katana::cad::SurveyCodingReport;
using katana::entity::EntityId;
using katana::entity::SurveyRule;
using katana::geometry::Point2;

namespace {

// The rules the reference mapfile gives these codes, cut down.
katana::entity::SurveyMap waterAndBollards()
{
    katana::entity::SurveyMap map;
    SurveyRule main;
    main.key = "WM*";
    main.model = "SURVEY SERVICES";
    main.colour = "sui water potable";
    main.linestyle = "WATR Main";
    main.breakline = katana::entity::SurveyBreakline::Line;
    EXPECT_TRUE(map.add(main).ok());

    SurveyRule bollard;
    bollard.key = "AC*";
    bollard.model = "SURVEY DETAIL";
    bollard.section = katana::entity::SurveySection::VertexSymbol;
    bollard.symbol = katana::entity::SurveySymbol{"CULT Bollard", "white", 1.5, 0.0, 0.0, 0.0};
    EXPECT_TRUE(map.add(bollard).ok());

    SurveyRule everything;
    everything.key = "*";
    everything.section = katana::entity::SurveySection::StringAttribute;
    everything.attributes = {{"text", "DepthLocation", "Top of Pipe"},
                             {"text", "Diameter", "$PipeDiameter"}};
    EXPECT_TRUE(map.add(everything).ok());
    return map;
}

EntityId addCodedPoint(Document& document, const Point2& at, const std::string& code)
{
    cmd::EntityAttributes attributes;
    auto command = cmd::createPoint(at, attributes);
    EXPECT_TRUE(document.execute(std::move(command)).ok());
    const EntityId id = document.model().entities.ids().back();
    EXPECT_TRUE(document
                    .execute(cmd::setEntityProperty({id}, "code",
                                                    katana::entity::PropertyValue(code)))
                    .ok());
    return id;
}

} // namespace

TEST(SurveyCoding, ACodedPointGetsTheModelTheStyleAndTheAttributesTheMapfileGivesIt)
{
    Document document;
    document.setSurveyMap(waterAndBollards());
    const EntityId water = addCodedPoint(document, Point2(0, 0), "WM01");
    const EntityId bollard = addCodedPoint(document, Point2(10, 0), "AC07");

    SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(document, {}, &report);
    ASSERT_TRUE(command.ok()) << command.error().describe();
    ASSERT_NE(*command, nullptr) << "there is something to do";
    ASSERT_TRUE(document.execute(std::move(*command)).ok());

    EXPECT_EQ(report.coded, 2u);
    EXPECT_EQ(report.matched, 2u);
    EXPECT_TRUE(report.unmatchedCodes.empty());
    EXPECT_EQ(report.layersCreated, (std::vector<std::string>{"SURVEY DETAIL", "SURVEY SERVICES"}));
    EXPECT_EQ(report.stylesCreated, (std::vector<std::string>{"CULT Bollard", "WATR Main"}));

    const auto* first = document.model().entities.find(water);
    ASSERT_NE(first, nullptr);
    EXPECT_EQ(first->layer, "SURVEY SERVICES") << "12d's model is Katana's layer";
    EXPECT_EQ(first->style, "WATR Main");
    // The `*` rule's attribute reaches both, because attributes accumulate.
    EXPECT_EQ(std::get<std::string>(first->properties.at("DepthLocation")), "Top of Pipe");
    // ... but the one whose value names another attribute is left, and said so.
    EXPECT_FALSE(first->properties.contains("Diameter"));
    EXPECT_EQ(report.deferredAttributes, 2u) << "one per coded entity";

    const auto* second = document.model().entities.find(bollard);
    ASSERT_NE(second, nullptr);
    EXPECT_EQ(second->layer, "SURVEY DETAIL");
    EXPECT_EQ(second->style, "CULT Bollard");
    const auto* style = document.model().styles.find("CULT Bollard");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->symbol, "CULT Bollard") << "a symbol name resolved when it is drawn";
    EXPECT_EQ(style->symbolSize, 1.5);
    EXPECT_EQ(document.model().styles.find("WATR Main")->linetype, "WATR Main");
}

TEST(SurveyCoding, ItIsOneUndoRatherThanOnePerEntity)
{
    Document document;
    document.setSurveyMap(waterAndBollards());
    for (int i = 0; i < 20; ++i) {
        addCodedPoint(document, Point2(i, 0), "WM" + std::to_string(i));
    }
    const std::string layerBefore =
        document.model().entities.find(document.model().entities.ids().front())->layer;

    auto command = katana::cad::applySurveyCodes(document, {});
    ASSERT_TRUE(command.ok());
    ASSERT_TRUE(document.execute(std::move(*command)).ok());
    EXPECT_EQ(document.model().entities.find(document.model().entities.ids().front())->layer,
              "SURVEY SERVICES");

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().entities.find(document.model().entities.ids().front())->layer,
              layerBefore)
        << "one undo puts all twenty back";
    EXPECT_FALSE(document.model().layers.contains("SURVEY SERVICES"))
        << "and takes the layer it created with them";
}

TEST(SurveyCoding, ACodeOnlyTheStarRuleAnswersIsReportedAsFallbackOnlyRatherThanAsMatched)
{
    // Audit CAD-05. Every code meets the `*` attribute rule, so "some rule
    // matched" is true of a typo too. ZZ99 and ZZ98 get only the `*`
    // attributes - no model, no linestyle, no symbol - and must be named, not
    // counted with WM01 as codes the mapfile has a rule for.
    Document document;
    document.setSurveyMap(waterAndBollards());
    addCodedPoint(document, Point2(0, 0), "WM01");
    addCodedPoint(document, Point2(1, 0), "ZZ99");
    addCodedPoint(document, Point2(2, 0), "ZZ98");

    SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(document, {}, &report);
    ASSERT_TRUE(command.ok());
    EXPECT_EQ(report.coded, 3u);
    EXPECT_EQ(report.matched, 1u) << "only WM01 has a rule of its own";
    EXPECT_EQ(report.fallbackOnly, 2u);
    EXPECT_EQ(report.fallbackOnlyCodes, (std::vector<std::string>{"ZZ98", "ZZ99"}));
    EXPECT_TRUE(report.unmatchedCodes.empty()) << "a rule did match them: the bare `*`";
    EXPECT_EQ(report.layersCreated, (std::vector<std::string>{"SURVEY SERVICES"}));
}

TEST(SurveyCoding, ACodeNoRuleAtAllMatchesIsListedAsUnmatched)
{
    katana::entity::SurveyMap map;
    SurveyRule main;
    main.key = "WM*";
    main.model = "SURVEY SERVICES";
    ASSERT_TRUE(map.add(main).ok());
    Document document;
    document.setSurveyMap(std::move(map));
    addCodedPoint(document, Point2(0, 0), "WM01");
    addCodedPoint(document, Point2(1, 0), "ZZ99");

    SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(document, {}, &report);
    ASSERT_TRUE(command.ok());
    EXPECT_EQ(report.coded, 2u);
    EXPECT_EQ(report.matched, 1u);
    EXPECT_EQ(report.fallbackOnly, 0u);
    EXPECT_EQ(report.unmatchedCodes, (std::vector<std::string>{"ZZ99"}));
}

TEST(SurveyCoding, AnEntityWithNoCodeOrANumberForOneIsLeftAlone)
{
    Document document;
    document.setSurveyMap(waterAndBollards());
    ASSERT_TRUE(document.execute(cmd::createPoint(Point2(0, 0), {})).ok());
    const EntityId plain = document.model().entities.ids().back();
    // A number in the code property is a measurement someone named badly.
    ASSERT_TRUE(document
                    .execute(cmd::setEntityProperty({plain}, "code",
                                                    katana::entity::PropertyValue(1.5)))
                    .ok());
    SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(document, {}, &report);
    ASSERT_TRUE(command.ok());
    EXPECT_EQ(report.coded, 0u);
    EXPECT_EQ(*command, nullptr) << "nothing to do is not an error";
}

TEST(SurveyCoding, ANameTheLibraryDoesNotDefineIsStillRecordedAndIsReported)
{
    // A customisation need not be self-contained. The name is what 12d says
    // this is, so it is kept and draws plainly until a library defines it.
    Document document;
    document.setSurveyMap(waterAndBollards());
    addCodedPoint(document, Point2(0, 0), "WM01");

    SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(document, {}, &report);
    ASSERT_TRUE(command.ok());
    ASSERT_TRUE(document.execute(std::move(*command)).ok());
    EXPECT_EQ(report.missingDefinitions, (std::vector<std::string>{"WATR Main"}));
    EXPECT_EQ(document.model().styles.find("WATR Main")->linetype, "WATR Main");

    // With the library loaded there is nothing missing.
    katana::entity::StyleLibrary library;
    katana::entity::LineStyle main;
    main.name = "WATR Main";
    main.strokes.push_back({katana::entity::StrokeOp::Move, Point2(0, 0)});
    ASSERT_TRUE(library.add(main).ok());
    Document second;
    second.setSurveyMap(waterAndBollards());
    second.setStyleLibrary(std::move(library));
    addCodedPoint(second, Point2(0, 0), "WM01");
    SurveyCodingReport clean;
    auto again = katana::cad::applySurveyCodes(second, {}, &clean);
    ASSERT_TRUE(again.ok());
    EXPECT_TRUE(clean.missingDefinitions.empty());
}

TEST(SurveyCoding, TheColourCallbackDecidesTheColourAndWithoutOneNoneIsGuessed)
{
    Document document;
    document.setSurveyMap(waterAndBollards());
    addCodedPoint(document, Point2(0, 0), "WM01");

    SurveyCodingOptions options;
    auto plain = katana::cad::applySurveyCodes(document, options);
    ASSERT_TRUE(plain.ok());
    ASSERT_TRUE(document.execute(std::move(*plain)).ok());
    const auto* style = document.model().styles.find("WATR Main");
    ASSERT_NE(style, nullptr);
    EXPECT_FALSE(style->color.has_value()) << "no resolver, so the colour is left alone";
    EXPECT_EQ(style->description, "sui water potable")
        << "the 12d colour name is kept whatever Katana makes of it";

    Document second;
    second.setSurveyMap(waterAndBollards());
    addCodedPoint(second, Point2(0, 0), "WM01");
    options.colourOf = [](std::string_view name) -> std::optional<katana::entity::Color> {
        return name == "sui water potable" ? std::optional<katana::entity::Color>(
                                                 katana::entity::Color{0, 128, 255, 255})
                                           : std::nullopt;
    };
    auto coloured = katana::cad::applySurveyCodes(second, options);
    ASSERT_TRUE(coloured.ok());
    ASSERT_TRUE(second.execute(std::move(*coloured)).ok());
    const auto* resolved = second.model().styles.find("WATR Main");
    ASSERT_NE(resolved, nullptr);
    ASSERT_TRUE(resolved->color.has_value());
    EXPECT_EQ(resolved->color->b, 255);
}

TEST(SurveyCoding, OnlyTheEntitiesAskedForAreTouched)
{
    Document document;
    document.setSurveyMap(waterAndBollards());
    const EntityId first = addCodedPoint(document, Point2(0, 0), "WM01");
    addCodedPoint(document, Point2(1, 0), "WM02");

    SurveyCodingOptions options;
    options.ids = {first};
    SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(document, options, &report);
    ASSERT_TRUE(command.ok());
    ASSERT_TRUE(document.execute(std::move(*command)).ok());
    EXPECT_EQ(report.coded, 1u);
    EXPECT_EQ(document.model().entities.find(first)->layer, "SURVEY SERVICES");
    EXPECT_EQ(document.model().entities.find(document.model().entities.ids().back())->layer,
              katana::entity::kDefaultLayerName);
}

// ---- which style a code gets (decision D4) ----------------------------------

namespace {

// The shapes the reference mapfile gives these codes, cut down: every symbol
// code ALSO has a map_data rule saying linestyle "0"; the text codes 1* and
// 2* both say linestyle "0" in different colours; and one real linestyle is
// used in two colours.
katana::entity::SurveyMap realShapes()
{
    katana::entity::SurveyMap map;
    const auto mapData = [&map](std::string key, std::string model, std::string colour,
                                std::string linestyle) {
        SurveyRule rule;
        rule.key = std::move(key);
        rule.model = std::move(model);
        rule.colour = std::move(colour);
        rule.linestyle = std::move(linestyle);
        EXPECT_TRUE(map.add(rule).ok());
    };
    const auto symbol = [&map](std::string key, std::string style, double size) {
        SurveyRule rule;
        rule.key = std::move(key);
        rule.section = katana::entity::SurveySection::VertexSymbol;
        rule.symbol = katana::entity::SurveySymbol{std::move(style), "", size, 0.0, 0.0, 0.0};
        EXPECT_TRUE(map.add(rule).ok());
    };
    mapData("AC*", "SURVEY DETAIL", "white", "0");
    mapData("SV*", "SURVEY DETAIL", "red", "0");
    mapData("1*", "SURVEY TEXT", "yellow", "0");
    mapData("2*", "SURVEY TEXT", "cyan", "0");
    mapData("TS*", "SURVEY DETAIL", "Green", "TOPO Timber or Scrub Scattered");
    mapData("TD*", "SURVEY DETAIL", "Dark Green", "TOPO Timber or Scrub Scattered");
    mapData("WM*", "SURVEY SERVICES", "sui water potable", "WATR Main");
    symbol("AC*", "CULT Bollard", 1.5);
    symbol("SV*", "SEWR Manhole Cover", 1.0);
    return map;
}

// A colour table of the test's own, so that what is expected does not hang
// on the one in archive12d, which cad may not see anyway.
std::optional<katana::entity::Color> testColour(std::string_view name)
{
    using katana::entity::Color;
    if (name == "white") {
        return Color{255, 255, 255, 255};
    }
    if (name == "red") {
        return Color{255, 0, 0, 255};
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
    if (name == "Dark Green") {
        return Color{0, 100, 0, 255};
    }
    return std::nullopt; // "sui water potable" is unknown here, as it is to Katana
}

SurveyCodingOptions withColours()
{
    SurveyCodingOptions options;
    options.colourOf = testColour;
    return options;
}

const katana::entity::Style& styleOf(const Document& document, EntityId id)
{
    static const katana::entity::Style none;
    const auto* entity = document.model().entities.find(id);
    if (entity == nullptr) {
        ADD_FAILURE() << "no entity " << id;
        return none;
    }
    const auto* style = document.model().styles.find(entity->style);
    if (style == nullptr) {
        ADD_FAILURE() << "entity " << id << " has style \"" << entity->style
                      << "\", which does not exist";
        return none;
    }
    return *style;
}

void apply(Document& document, const SurveyCodingOptions& options,
           SurveyCodingReport* report = nullptr)
{
    auto command = katana::cad::applySurveyCodes(document, options, report);
    ASSERT_TRUE(command.ok()) << command.error().describe();
    if (*command != nullptr) {
        ASSERT_TRUE(document.execute(std::move(*command)).ok());
    }
}

} // namespace

TEST(SurveyCodingStyles, SymbolAndTextCodesOnLinestyleZeroEachKeepTheirOwnSymbolAndColour)
{
    // They all say linestyle "0". Named after the linestyle, they became ONE
    // style "0" built from whichever point came first: one symbol and one
    // colour for all four.
    Document document;
    document.setSurveyMap(realShapes());
    const EntityId bollard = addCodedPoint(document, Point2(0, 0), "AC01");
    const EntityId manhole = addCodedPoint(document, Point2(1, 0), "SV01");
    const EntityId yellowText = addCodedPoint(document, Point2(2, 0), "101");
    const EntityId cyanText = addCodedPoint(document, Point2(3, 0), "201");

    SurveyCodingReport report;
    apply(document, withColours(), &report);

    EXPECT_FALSE(document.model().styles.contains("0")) << "\"0\" is a plain line, not a name";
    const auto& first = styleOf(document, bollard);
    EXPECT_EQ(first.symbol, "CULT Bollard");
    EXPECT_EQ(first.symbolSize, 1.5);
    EXPECT_EQ(first.linetype, katana::entity::kContinuousLinetype);
    EXPECT_EQ(first.color, (katana::entity::Color{255, 255, 255, 255}));
    const auto& second = styleOf(document, manhole);
    EXPECT_EQ(second.symbol, "SEWR Manhole Cover");
    EXPECT_EQ(second.color, (katana::entity::Color{255, 0, 0, 255}));
    EXPECT_EQ(styleOf(document, yellowText).color, (katana::entity::Color{255, 255, 0, 255}));
    EXPECT_EQ(styleOf(document, cyanText).color, (katana::entity::Color{0, 255, 255, 255}));
    EXPECT_TRUE(styleOf(document, cyanText).symbol.empty());

    // Named after the symbol where there is one, else "Plain". The two text
    // codes are both plain lines with no symbol, so both want "Plain".
    // Appearances are named in a fixed order - linestyle, symbol, size, then
    // colour, whose identity is its RGB as "#RRGGBB" - and cyan "#00FFFF"
    // sorts before yellow "#FFFF00", so cyan takes "Plain" and yellow gets
    // its colour name added.
    EXPECT_EQ(report.stylesCreated, (std::vector<std::string>{"CULT Bollard", "Plain",
                                                              "Plain (yellow)",
                                                              "SEWR Manhole Cover"}));
    EXPECT_EQ(document.model().entities.find(cyanText)->style, "Plain");
    EXPECT_EQ(document.model().entities.find(yellowText)->style, "Plain (yellow)");
}

TEST(SurveyCodingStyles, ALinestyleUsedInTwoColoursGivesTwoStylesNamedTheSameWhicheverPointComesFirst)
{
    // "TOPO Timber or Scrub Scattered" comes in Green and in Dark Green in
    // each reference mapfile. In the fixed order Dark Green "#006400" sorts
    // before Green "#00FF00", so Dark Green takes the bare name.
    const std::string name = "TOPO Timber or Scrub Scattered";
    for (const bool greenFirst : {true, false}) {
        Document document;
        document.setSurveyMap(realShapes());
        EntityId green = 0;
        EntityId dark = 0;
        if (greenFirst) {
            green = addCodedPoint(document, Point2(0, 0), "TS01");
            dark = addCodedPoint(document, Point2(1, 0), "TD01");
        } else {
            dark = addCodedPoint(document, Point2(1, 0), "TD01");
            green = addCodedPoint(document, Point2(0, 0), "TS01");
        }
        SurveyCodingReport report;
        apply(document, withColours(), &report);

        EXPECT_EQ(report.stylesCreated, (std::vector<std::string>{name, name + " (Green)"}))
            << (greenFirst ? "green first" : "dark green first");
        EXPECT_EQ(document.model().entities.find(dark)->style, name);
        EXPECT_EQ(document.model().entities.find(green)->style, name + " (Green)");
        EXPECT_EQ(styleOf(document, green).color, (katana::entity::Color{0, 255, 0, 255}));
        EXPECT_EQ(styleOf(document, dark).color, (katana::entity::Color{0, 100, 0, 255}));
        EXPECT_EQ(styleOf(document, green).linetype, name);
    }
}

TEST(SurveyCodingStyles, RenamingACodedStyleThenApplyingCodesAgainReusesItInsteadOfMakingASecond)
{
    Document document;
    document.setSurveyMap(realShapes());
    addCodedPoint(document, Point2(0, 0), "WM01");
    apply(document, withColours());
    ASSERT_TRUE(document.model().styles.contains("WATR Main"));
    ASSERT_TRUE(document.execute(cmd::renameStyle("WATR Main", "Water main")).ok());
    const std::size_t styles = document.model().styles.size();

    const EntityId later = addCodedPoint(document, Point2(1, 0), "WM02");
    SurveyCodingReport report;
    apply(document, withColours(), &report);

    EXPECT_TRUE(report.stylesCreated.empty());
    EXPECT_EQ(report.stylesReused, (std::vector<std::string>{"Water main"}));
    EXPECT_EQ(document.model().styles.size(), styles) << "no duplicate of the renamed style";
    EXPECT_FALSE(document.model().styles.contains("WATR Main"));
    EXPECT_EQ(document.model().entities.find(later)->style, "Water main");
}

TEST(SurveyCodingStyles, AnImportedStringWhoseCodeIsOnlyInItsMetadataIsCodedAndKeepsItsSymbol)
{
    // A 12da import records a string's name - its code, in a coded survey -
    // as metadata "12d.name", and gives it a style named after its 12d
    // linestyle: here a symbol style, beside the style "0" the import makes
    // for plain strings. Reading the code from the metadata WITHOUT the
    // appearance rule would move this point onto "0" and lose its symbol.
    Document document;
    document.setSurveyMap(realShapes());
    katana::entity::Style plain;
    plain.name = "0";
    plain.linetype = "0";
    plain.description = "12d linestyle";
    ASSERT_TRUE(document.execute(cmd::createStyle(plain)).ok());
    katana::entity::Style imported;
    imported.name = "CULT Bollard";
    imported.linetype = "CULT Bollard";
    imported.symbol = "CULT Bollard";
    imported.symbolSize = 1.5;
    imported.description = "12d symbol";
    ASSERT_TRUE(document.execute(cmd::createStyle(imported)).ok());

    katana::entity::Entity point;
    point.geometry = katana::entity::PointGeometry{Point2(5, 5)};
    point.style = "CULT Bollard";
    point.metadata.insert_or_assign("12d.name", katana::entity::PropertyValue(std::string("AC01")));
    ASSERT_TRUE(document.execute(cmd::createEntities({point})).ok());
    const EntityId id = document.model().entities.ids().back();

    SurveyCodingReport report;
    apply(document, withColours(), &report);

    EXPECT_EQ(report.property, "12d.name") << "found in the metadata";
    EXPECT_EQ(report.coded, 1u);
    EXPECT_EQ(report.matched, 1u);
    const auto* entity = document.model().entities.find(id);
    ASSERT_NE(entity, nullptr);
    EXPECT_EQ(entity->layer, "SURVEY DETAIL");
    EXPECT_NE(entity->style, "0");
    EXPECT_EQ(styleOf(document, id).symbol, "CULT Bollard") << "the symbol is kept";
    // The imported style also runs the name as a linestyle and has no
    // colour, so it is not this appearance, and a new style is named for the
    // colour.
    EXPECT_EQ(entity->style, "CULT Bollard (white)");
}

TEST(SurveyCodingStyles, AStyleIsReusedOnlyWhenAllFourPartsOfTheAppearanceAgree)
{
    // An existing plain style in the wrong colour is not reused, and its name
    // is not taken over either.
    Document document;
    document.setSurveyMap(realShapes());
    katana::entity::Style red;
    red.name = "Plain";
    red.color = katana::entity::Color{255, 0, 0, 255};
    ASSERT_TRUE(document.execute(cmd::createStyle(red)).ok());
    katana::entity::Style yellow;
    yellow.name = "Signs";
    yellow.linetype = "Continuous"; // plain, in any case
    yellow.color = katana::entity::Color{255, 255, 0, 255};
    ASSERT_TRUE(document.execute(cmd::createStyle(yellow)).ok());
    const EntityId yellowText = addCodedPoint(document, Point2(0, 0), "101");
    const EntityId cyanText = addCodedPoint(document, Point2(0, 0), "201");

    SurveyCodingReport report;
    apply(document, withColours(), &report);
    EXPECT_EQ(document.model().entities.find(yellowText)->style, "Signs")
        << "plain and yellow: reused under its own name";
    EXPECT_EQ(document.model().entities.find(cyanText)->style, "Plain (cyan)")
        << "\"Plain\" is red, so cyan cannot have that name";
    EXPECT_EQ(report.stylesReused, (std::vector<std::string>{"Signs"}));
    EXPECT_EQ(report.stylesCreated, (std::vector<std::string>{"Plain (cyan)"}));
}

TEST(SurveyCodingStyles, AnImportedStyleWithNoColourIsReusedForACodeWhoseColourNameHasNoRgb)
{
    // The import-then-code path: a 12da import gives a string a style named
    // after its linestyle, with no colour and the description "12d
    // linestyle". WM* says "sui water potable", which testColour (like
    // Katana) has no RGB for, so the code draws as WATR Main with no colour -
    // exactly what the imported style draws. Matching the colour NAME against
    // the description made a second, identical "WATR Main (sui water
    // potable)" and moved the string onto it.
    Document document;
    document.setSurveyMap(realShapes());
    katana::entity::Style imported;
    imported.name = "WATR Main";
    imported.linetype = "WATR Main";
    imported.description = "12d linestyle";
    ASSERT_TRUE(document.execute(cmd::createStyle(imported)).ok());
    katana::entity::Entity point;
    point.geometry = katana::entity::PointGeometry{Point2(5, 5)};
    point.style = "WATR Main";
    point.metadata.insert_or_assign("12d.name", katana::entity::PropertyValue(std::string("WM01")));
    ASSERT_TRUE(document.execute(cmd::createEntities({point})).ok());
    const EntityId id = document.model().entities.ids().back();
    const std::size_t styles = document.model().styles.size();

    SurveyCodingReport report;
    apply(document, withColours(), &report);

    EXPECT_TRUE(report.stylesCreated.empty());
    EXPECT_EQ(report.stylesReused, (std::vector<std::string>{"WATR Main"}));
    EXPECT_EQ(document.model().styles.size(), styles) << "no duplicate of the imported style";
    EXPECT_EQ(document.model().entities.find(id)->style, "WATR Main");
    EXPECT_EQ(document.model().entities.find(id)->layer, "SURVEY SERVICES") << "still coded";
    ASSERT_EQ(report.codes.size(), 1u);
    EXPECT_EQ(report.codes[0].styleOutcome, katana::cad::SurveyStyleOutcome::Reused);
}

TEST(SurveyCodingStyles, RenamingACodedStyleAndRewritingItsDescriptionThenApplyingAgainMakesNoDuplicate)
{
    // Coding writes the colour name into the description because it has no
    // RGB for it. Someone renames the style AND describes it in their own
    // words; it still draws as the code says, so it is still the code's
    // style.
    Document document;
    document.setSurveyMap(realShapes());
    const EntityId first = addCodedPoint(document, Point2(0, 0), "WM01");
    apply(document, withColours());
    ASSERT_TRUE(document.model().styles.contains("WATR Main"));
    ASSERT_TRUE(document.execute(cmd::renameStyle("WATR Main", "Water")).ok());
    katana::entity::Style edited = *document.model().styles.find("Water");
    edited.description = "potable water main";
    ASSERT_TRUE(document.execute(cmd::updateStyle(edited)).ok());
    const std::size_t styles = document.model().styles.size();

    const EntityId later = addCodedPoint(document, Point2(1, 0), "WM02");
    SurveyCodingReport report;
    apply(document, withColours(), &report);

    EXPECT_TRUE(report.stylesCreated.empty());
    EXPECT_EQ(report.stylesReused, (std::vector<std::string>{"Water"}));
    EXPECT_EQ(document.model().styles.size(), styles) << "no second WATR Main";
    EXPECT_FALSE(document.model().styles.contains("WATR Main"));
    EXPECT_EQ(document.model().entities.find(first)->style, "Water") << "not moved off it";
    EXPECT_EQ(document.model().entities.find(later)->style, "Water");
}

TEST(SurveyCodingStyles, AStyleMadeForOneUnknownColourIsNotTakenByAnotherWhateverTheOrderOfCoding)
{
    // WM* and WR* draw alike in Katana - WATR Main with no RGB for either
    // colour - but they are different 12d colours, and a fresh drawing gives
    // each its own style. By hand: appearances are named in a fixed order,
    // and "sui water potable" sorts before "sui water recycled" ('p' < 'r'),
    // so potable takes the bare "WATR Main" and recycled, finding it taken,
    // gets " (sui water recycled)". Coding WM01 first and WR01 in a later
    // pass must end the same way, not put WR01 on potable's style.
    auto withRecycled = [] {
        katana::entity::SurveyMap map = realShapes();
        SurveyRule recycled;
        recycled.key = "WR*";
        recycled.model = "SURVEY SERVICES";
        recycled.colour = "sui water recycled";
        recycled.linestyle = "WATR Main";
        EXPECT_TRUE(map.add(recycled).ok());
        return map;
    };
    for (const bool inTwoPasses : {false, true}) {
        Document document;
        document.setSurveyMap(withRecycled());
        const EntityId potable = addCodedPoint(document, Point2(0, 0), "WM01");
        if (inTwoPasses) {
            apply(document, withColours());
        }
        const EntityId recycled = addCodedPoint(document, Point2(1, 0), "WR01");
        apply(document, withColours());

        const char* how = inTwoPasses ? "in two passes" : "in one";
        EXPECT_EQ(document.model().entities.find(potable)->style, "WATR Main") << how;
        EXPECT_EQ(document.model().entities.find(recycled)->style,
                  "WATR Main (sui water recycled)")
            << how;
        EXPECT_EQ(styleOf(document, recycled).description, "sui water recycled") << how;
    }
}

TEST(SurveyCodingStyles, OfTwoStylesThatDrawACodeAlikeTheOneDescribedByItsColourNameIsReused)
{
    // "A water" is first in name order and draws WM* just as well, but
    // "Water main" carries WM*'s colour name - it is the style an earlier
    // pass made for this code - so applying codes again keeps it there.
    Document document;
    document.setSurveyMap(realShapes());
    for (const auto& [name, description] :
         {std::pair{"A water", "12d linestyle"}, std::pair{"Water main", "sui water potable"}}) {
        katana::entity::Style style;
        style.name = name;
        style.linetype = "WATR Main";
        style.description = description;
        ASSERT_TRUE(document.execute(cmd::createStyle(style)).ok());
    }
    const EntityId id = addCodedPoint(document, Point2(0, 0), "WM01");
    SurveyCodingReport report;
    apply(document, withColours(), &report);
    EXPECT_EQ(document.model().entities.find(id)->style, "Water main");
    EXPECT_EQ(report.stylesReused, (std::vector<std::string>{"Water main"}));
}

// ---- what the report says -------------------------------------------------

TEST(SurveyCodingReport, WithoutCreatingLayersTheStyleAndAttributesAreStillApplied)
{
    // Audit CAD-18: a missing layer used to abandon the whole entity.
    Document document;
    document.setSurveyMap(waterAndBollards());
    const EntityId id = addCodedPoint(document, Point2(0, 0), "WM01");

    SurveyCodingOptions options;
    options.createLayers = false;
    SurveyCodingReport report;
    apply(document, options, &report);

    const auto* entity = document.model().entities.find(id);
    ASSERT_NE(entity, nullptr);
    EXPECT_EQ(entity->layer, katana::entity::kDefaultLayerName) << "no layer, so it stays";
    EXPECT_EQ(entity->style, "WATR Main");
    EXPECT_EQ(std::get<std::string>(entity->properties.at("DepthLocation")), "Top of Pipe");
    EXPECT_EQ(report.skippedNoLayer, 1u);
    EXPECT_TRUE(report.layersCreated.empty());
    EXPECT_EQ(report.changed, 1u) << "its style and a property changed";
    ASSERT_EQ(report.codes.size(), 1u);
    EXPECT_EQ(report.codes[0].layerKept, 1u) << "the row says so too";
}

TEST(SurveyCodingReport, ChangedCountsEachAlteredEntityOnceWhateverWasAltered)
{
    Document document;
    document.setSurveyMap(waterAndBollards());
    katana::entity::Layer services;
    services.name = "SURVEY SERVICES";
    ASSERT_TRUE(document.execute(cmd::createLayer(services)).ok());
    const EntityId onLayer = addCodedPoint(document, Point2(0, 0), "WM01");
    ASSERT_TRUE(document.execute(cmd::setEntityLayer({onLayer}, "SURVEY SERVICES")).ok());
    addCodedPoint(document, Point2(1, 0), "WM02");

    SurveyCodingReport report;
    apply(document, {}, &report);
    // Both change: the first only its style and a property (it is already on
    // the layer), the second its layer as well. Counting layer moves alone
    // gave 1.
    EXPECT_EQ(report.changed, 2u);

    // Everything is now as the map says, so a second pass has nothing to do.
    SurveyCodingReport again;
    auto second = katana::cad::applySurveyCodes(document, {}, &again);
    ASSERT_TRUE(second.ok());
    EXPECT_EQ(*second, nullptr);
    EXPECT_EQ(again.changed, 0u);
    EXPECT_EQ(again.matched, 2u) << "still matched; only nothing is left to change";
}

TEST(SurveyCodingReport, WithoutCreatingStylesAnAppearanceNoStyleHasIsCountedAndTheStyleKept)
{
    Document document;
    document.setSurveyMap(waterAndBollards());
    const EntityId id = addCodedPoint(document, Point2(0, 0), "WM01");
    SurveyCodingOptions options;
    options.createStyles = false;
    SurveyCodingReport report;
    apply(document, options, &report);
    EXPECT_EQ(document.model().entities.find(id)->style, "");
    EXPECT_EQ(document.model().entities.find(id)->layer, "SURVEY SERVICES");
    EXPECT_EQ(report.skippedNoStyle, 1u);
    EXPECT_TRUE(report.stylesCreated.empty());
}

TEST(SurveyCodingReport, AMissingDefinitionIsReportedEvenWhenTheCodeReusesAStyle)
{
    Document document;
    document.setSurveyMap(waterAndBollards());
    addCodedPoint(document, Point2(0, 0), "WM01");
    apply(document, {});
    addCodedPoint(document, Point2(1, 0), "WM02");
    SurveyCodingReport report;
    apply(document, {}, &report);
    EXPECT_EQ(report.stylesReused, (std::vector<std::string>{"WATR Main"}));
    EXPECT_EQ(report.missingDefinitions, (std::vector<std::string>{"WATR Main"}))
        << "the library still does not define it";
}

TEST(SurveyCodingReport, EachDistinctCodeHasARowSayingWhatHappenedToItsEntities)
{
    Document document;
    document.setSurveyMap(waterAndBollards());
    addCodedPoint(document, Point2(0, 0), "WM01");
    addCodedPoint(document, Point2(1, 0), "ZZ99");
    addCodedPoint(document, Point2(2, 0), "WM01");
    addCodedPoint(document, Point2(3, 0), "AC07");

    SurveyCodingReport report;
    apply(document, {}, &report);
    ASSERT_EQ(report.codes.size(), 3u);
    const auto& bollard = report.codes[0];
    const auto& water = report.codes[1];
    const auto& typo = report.codes[2];
    EXPECT_EQ(bollard.code, "AC07");
    EXPECT_EQ(water.code, "WM01");
    EXPECT_EQ(typo.code, "ZZ99");

    EXPECT_EQ(water.entities, 2u);
    EXPECT_EQ(water.kind, katana::entity::SurveyMatchKind::Prefix);
    EXPECT_TRUE(water.matched);
    EXPECT_EQ(water.layersFrom, (std::vector<std::string>{std::string(katana::entity::kDefaultLayerName)}));
    EXPECT_EQ(water.layer, "SURVEY SERVICES");
    EXPECT_EQ(water.style, "WATR Main");
    EXPECT_EQ(water.styleOutcome, katana::cad::SurveyStyleOutcome::Created);
    EXPECT_EQ(water.attributesSet, (std::vector<std::string>{"DepthLocation"}));
    EXPECT_EQ(water.attributesDeferred, (std::vector<std::string>{"Diameter"}));
    EXPECT_EQ(water.changed, 2u);

    EXPECT_EQ(typo.kind, katana::entity::SurveyMatchKind::FallbackOnly);
    EXPECT_FALSE(typo.matched);
    EXPECT_EQ(typo.layer, "");
    EXPECT_EQ(typo.styleOutcome, katana::cad::SurveyStyleOutcome::None);
    EXPECT_EQ(typo.attributesSet, (std::vector<std::string>{"DepthLocation"}))
        << "the `*` rule does say every code gets it";
    EXPECT_EQ(bollard.style, "CULT Bollard");
}

TEST(SurveyCodingReport, EveryEntityGetsWhatALookupOfItsOwnCodeGivesWhenCodesRepeat)
{
    // Decisions are made once per distinct code; this checks each entity
    // against a lookup of its own code alone, which is the per-entity path.
    Document document;
    const katana::entity::SurveyMap map = realShapes();
    document.setSurveyMap(map);
    const std::vector<std::string> codes = {"WM01", "AC01", "TS01", "WM01", "101", "AC01",
                                            "TD02", "WM03", "101",  "SV09", "TS01", "201"};
    std::vector<EntityId> ids;
    for (std::size_t i = 0; i < codes.size(); ++i) {
        ids.push_back(addCodedPoint(document, Point2(static_cast<double>(i), 0), codes[i]));
    }
    SurveyCodingReport report;
    apply(document, withColours(), &report);

    for (std::size_t i = 0; i < codes.size(); ++i) {
        const auto match = map.lookup(codes[i]);
        const auto& style = styleOf(document, ids[i]);
        EXPECT_EQ(document.model().entities.find(ids[i])->layer, match.resolved.model) << codes[i];
        const bool plain = katana::cad::isPlainLinestyle(match.resolved.linestyle);
        EXPECT_EQ(style.linetype, plain ? std::string(katana::entity::kContinuousLinetype)
                                        : match.resolved.linestyle)
            << codes[i];
        EXPECT_EQ(style.symbol, match.resolved.symbol ? match.resolved.symbol->style : "")
            << codes[i];
        EXPECT_EQ(style.color, testColour(match.resolved.colour)) << codes[i];
    }
    std::size_t rowsTotal = 0;
    for (const auto& row : report.codes) {
        rowsTotal += row.entities;
    }
    EXPECT_EQ(rowsTotal, codes.size());
    // WM01 AC01 TS01 101 TD02 WM03 SV09 201 are distinct; WM01, AC01, 101
    // and TS01 repeat: twelve entities, eight distinct codes.
    EXPECT_EQ(report.codes.size(), 8u);
}

TEST(SurveyCoding, APointsLineworkControlsAreLeftOutOfTheCodeItIsLookedUpBy)
{
    // "PABB ST" is the string PABB with a start control, as a controller
    // writes it. The exact key PABB must code it: the whole field, blank
    // included, matches no key. A prefix key hid this ("KB1 ST" still begins
    // with KB), which is why the key here is exact.
    katana::entity::SurveyMap map;
    SurveyRule bollard;
    bollard.key = "PABB";
    bollard.model = "SURVEY DETAIL";
    ASSERT_TRUE(map.add(bollard).ok());
    Document document;
    document.setSurveyMap(std::move(map));
    const EntityId started = addCodedPoint(document, Point2(0, 0), "PABB ST");
    const EntityId lowerCase = addCodedPoint(document, Point2(1, 0), "PABB st JPN 12");
    const EntityId plain = addCodedPoint(document, Point2(2, 0), "PABB");
    // A line is not a field point: its code is looked up as it is written.
    ASSERT_TRUE(document.execute(cmd::createLine(Point2(0, 5), Point2(9, 5), {})).ok());
    const EntityId line = document.model().entities.ids().back();
    ASSERT_TRUE(document
                    .execute(cmd::setEntityProperty({line}, "code",
                                                    katana::entity::PropertyValue("PABB ST")))
                    .ok());

    SurveyCodingReport report;
    apply(document, {}, &report);
    // By hand: three points reduce to PABB, which the exact key matches; the
    // line keeps "PABB ST", which nothing matches. Four coded, three matched,
    // two rows in name order ("PABB" < "PABB ST").
    EXPECT_EQ(report.coded, 4u);
    EXPECT_EQ(report.matched, 3u);
    EXPECT_EQ(report.unmatchedCodes, (std::vector<std::string>{"PABB ST"}));
    ASSERT_EQ(report.codes.size(), 2u);
    EXPECT_EQ(report.codes[0].code, "PABB") << "reported under the code it was looked up by";
    EXPECT_EQ(report.codes[0].entities, 3u);
    EXPECT_EQ(report.codes[1].code, "PABB ST");
    for (const EntityId id : {started, lowerCase, plain}) {
        EXPECT_EQ(document.model().entities.find(id)->layer, "SURVEY DETAIL") << id;
    }
    EXPECT_EQ(document.model().entities.find(line)->layer,
              std::string(katana::entity::kDefaultLayerName));
}

TEST(CustomisationCoverage, ABuiltInSymbolIsNeverListedAsUnresolved)
{
    // Audit CAD-17: "cross" draws through Katana's own shapes.
    Document document;
    katana::entity::Style style;
    style.name = "s";
    style.symbol = "cross";
    ASSERT_TRUE(document.execute(cmd::createStyle(style)).ok());
    const auto coverage = katana::cad::customisationCoverage(document);
    EXPECT_EQ(coverage.styles, 1u);
    EXPECT_TRUE(coverage.unresolved.empty());
    EXPECT_EQ(coverage.named, 0u);
    EXPECT_EQ(coverage.builtIn, 1u);

    // A name nothing draws is still reported.
    katana::entity::Style other;
    other.name = "t";
    other.symbol = "CULT Bollard";
    ASSERT_TRUE(document.execute(cmd::createStyle(other)).ok());
    EXPECT_EQ(katana::cad::customisationCoverage(document).unresolved,
              (std::vector<std::string>{"CULT Bollard"}));
}

TEST(CustomisationCoverage, AStyleThatTakesItsLinetypeFromItsLayerNamesNoDefinition)
{
    // "ByLayer" as a style's linetype means "whatever the layer draws with"
    // (decision D2), not a 12d linestyle called ByLayer: reporting it as in
    // no loaded library sent people looking for a definition that is not
    // meant to exist.
    Document document;
    katana::entity::Style style;
    style.name = "s";
    style.linetype = "ByLayer";
    ASSERT_TRUE(document.execute(cmd::createStyle(style)).ok());
    const auto coverage = katana::cad::customisationCoverage(document);
    EXPECT_EQ(coverage.styles, 1u);
    EXPECT_TRUE(coverage.unresolved.empty());
    EXPECT_EQ(coverage.named, 0u) << "it names nothing to resolve";
}
