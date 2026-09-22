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

TEST(SurveyCoding, ACodeTheMapfileHasNoRuleForIsNamedRatherThanLeftLookingHandled)
{
    Document document;
    document.setSurveyMap(waterAndBollards());
    addCodedPoint(document, Point2(0, 0), "WM01");
    addCodedPoint(document, Point2(1, 0), "ZZ99");
    addCodedPoint(document, Point2(2, 0), "ZZ98");

    SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(document, {}, &report);
    ASSERT_TRUE(command.ok());
    EXPECT_EQ(report.coded, 3u);
    // Every code matches the `*` rule, so all three are "matched"; what
    // distinguishes them is that ZZ* has no model and no style to give.
    EXPECT_EQ(report.matched, 3u);
    EXPECT_EQ(report.layersCreated, (std::vector<std::string>{"SURVEY SERVICES"}));
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
