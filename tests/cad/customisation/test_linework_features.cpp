// Drawing the coded strings a field file strings itself (survey::SurveyFeature).
//
// Every expected value here is worked by hand from the project the test
// builds; the working is in the comment beside it.

#include <gtest/gtest.h>

#include <optional>
#include <string>
#include <vector>

#include "katana/cad/linework.hpp"
#include "katana/commands/entity_commands.hpp"

namespace cmd = katana::commands;
using katana::cad::Document;
using katana::cad::LineworkNoteKind;
using katana::cad::SurveyFeatureOptions;
using katana::cad::SurveyFeatureResult;
using katana::cad::UnplacedFeatureReason;
using katana::entity::Entity;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyRule;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::survey::SurveyFeature;
using katana::survey::SurveyPoint;
using katana::survey::SurveyProject;

namespace {

// Shaped like the reference mapfile's rules, with invented names: a water
// main and a kerb are lines.
katana::entity::SurveyMap lineMap()
{
    katana::entity::SurveyMap map;
    SurveyRule water;
    water.key = "WM*";
    water.model = "SURVEY SERVICES";
    water.colour = "blue";
    water.linestyle = "WATR Main";
    water.breakline = SurveyBreakline::Line;
    EXPECT_TRUE(map.add(water).ok());

    SurveyRule kerb;
    kerb.key = "KB*";
    kerb.model = "SURVEY KERB";
    kerb.breakline = SurveyBreakline::Line;
    EXPECT_TRUE(map.add(kerb).ok());
    return map;
}

SurveyPoint point(const std::string& id, double northing, double easting,
                  std::optional<double> elevation = std::nullopt, const std::string& code = {})
{
    SurveyPoint p;
    p.id = id;
    p.northing = northing;
    p.easting = easting;
    p.elevation = elevation;
    p.code = code;
    return p;
}

SurveyFeature feature(const std::string& name, const std::string& code,
                      std::vector<std::string> ids, bool closed = false)
{
    SurveyFeature f;
    f.name = name;
    f.code = code;
    f.pointIds = std::move(ids);
    f.closed = closed;
    return f;
}

std::vector<const Entity*> polylinesOf(const Document& document)
{
    std::vector<const Entity*> found;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<Polyline2>(entity.geometry)) {
            found.push_back(&entity);
        }
    });
    return found;
}

// Plans and executes, failing the test on an error.
SurveyFeatureResult run(Document& document, const SurveyProject& project,
                        const SurveyFeatureOptions& options = {})
{
    auto planned = katana::cad::drawSurveyFeatures(document, project, options);
    EXPECT_TRUE(planned.ok()) << (planned.ok() ? "" : planned.error().describe());
    if (!planned.ok()) {
        return {};
    }
    SurveyFeatureResult result = std::move(*planned);
    if (result.command) {
        EXPECT_TRUE(document.execute(std::move(result.command)).ok());
    }
    return result;
}

std::string textProperty(const Entity& entity, const std::string& key)
{
    const auto found = entity.properties.find(key);
    if (found == entity.properties.end()) {
        return "<absent>";
    }
    const auto* text = std::get_if<std::string>(&found->second);
    return text != nullptr ? *text : "<not text>";
}

} // namespace

TEST(SurveyFeatures, AFeatureIsDrawnThroughTheProjectsPointsInTheFilesOrderOnTheRulesLayerInItsStyle)
{
    Document document;
    document.setSurveyMap(lineMap());
    SurveyProject project;
    project.points = {point("1", 100.0, 200.0, 10.0), point("2", 100.0, 210.0),
                      point("3", 110.0, 210.0, 12.0)};
    SurveyFeature main = feature("WM01", "WM", {"3", "1", "2"});
    main.description = "water main";
    project.features = {main};

    const SurveyFeatureResult result = run(document, project);

    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    const Entity& line = *lines[0];
    // In the file's order 3, 1, 2, each at (easting, northing):
    // 3 = (210, 110), 1 = (200, 100), 2 = (210, 100).
    const Polyline2& shape = std::get<Polyline2>(line.geometry);
    EXPECT_FALSE(shape.closed);
    ASSERT_EQ(shape.vertices.size(), 3u);
    EXPECT_EQ(shape.vertices[0], Point2(210.0, 110.0));
    EXPECT_EQ(shape.vertices[1], Point2(200.0, 100.0));
    EXPECT_EQ(shape.vertices[2], Point2(210.0, 100.0));
    // Heights in the same order: 3 has 12, 1 has 10, 2 has none.
    const auto heights = katana::entity::heightsOf(line.properties, 3);
    ASSERT_EQ(heights.size(), 3u);
    EXPECT_EQ(heights[0], std::optional<double>(12.0));
    EXPECT_EQ(heights[1], std::optional<double>(10.0));
    EXPECT_EQ(heights[2], std::nullopt) << "absent is not zero";

    // WM -> rule WM*: model SURVEY SERVICES, linestyle WATR Main.
    EXPECT_EQ(line.layer, "SURVEY SERVICES");
    EXPECT_EQ(line.style, "WATR Main");
    EXPECT_EQ(textProperty(line, "code"), "WM") << "the line carries the code the rules key on";
    EXPECT_EQ(textProperty(line, "description"), "water main");

    ASSERT_EQ(result.report.strings.size(), 1u);
    const auto& built = result.report.strings[0];
    EXPECT_EQ(built.name, "WM01");
    // The CODE is split: "WM" under "WM*" is the bare prefix, so no number.
    EXPECT_EQ(built.key, "WM*");
    EXPECT_EQ(built.number, "");
    EXPECT_TRUE(built.points.empty()) << "the points are the project's, not entities";
    EXPECT_EQ(built.pointNumbers, (std::vector<std::string>{"3", "1", "2"}));
    EXPECT_EQ(built.vertices, 3u);
    EXPECT_EQ(result.report.layersCreated, (std::vector<std::string>{"SURVEY SERVICES"}));
    EXPECT_TRUE(result.unplaced.empty());
    ASSERT_NE(result.report.styling, nullptr) << "styled when the command ran";
}

TEST(SurveyFeatures, AClosedFeatureClosesAndAPointWithNoPositionIsLeftOutAndSaidSo)
{
    Document document;
    document.setSurveyMap(lineMap());
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0), point("2", 0.0, 10.0), point("3", 10.0, 10.0)};
    katana::survey::UnpositionedPoint nowhere;
    nowhere.id = "U";
    project.unpositionedPoints = {nowhere};
    project.features = {feature("", "KB", {"1", "U", "2", "3"}, true)};

    const SurveyFeatureResult result = run(document, project);

    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    const Polyline2& shape = std::get<Polyline2>(lines[0]->geometry);
    // U has nowhere to be, so 1, 2, 3 at (easting, northing), closed.
    EXPECT_TRUE(shape.closed);
    EXPECT_EQ(shape.vertices, (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(10, 10)}));
    EXPECT_EQ(lines[0]->layer, "SURVEY KERB");

    ASSERT_EQ(result.report.strings.size(), 1u);
    EXPECT_EQ(result.report.strings[0].name, "KB") << "no name: the code names it";
    EXPECT_TRUE(result.report.strings[0].closed);
    EXPECT_EQ(result.report.strings[0].pointNumbers, (std::vector<std::string>{"1", "2", "3"}));

    ASSERT_EQ(result.report.notes.size(), 1u);
    EXPECT_EQ(result.report.notes[0].kind, LineworkNoteKind::UnpositionedPoint);
    EXPECT_EQ(result.report.notes[0].pointNumber, "U");
    EXPECT_EQ(result.report.notes[0].detail, "KB");
}

TEST(SurveyFeatures, AFeatureThatCannotMakeALineIsReportedWithWhyAndNoCommandIsMade)
{
    Document document;
    document.setSurveyMap(lineMap());
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0), point("4", 5.0, 5.0), point("5", 5.0, 5.0)};
    katana::survey::UnpositionedPoint nowhere;
    nowhere.id = "U";
    project.unpositionedPoints = {nowhere};
    // [0]: 1 and U - one positioned point, no line.
    // [1]: 4 and 5 - both at (5, 5), no length.
    project.features = {feature("K1", "KB", {"1", "U"}), feature("K2", "KB", {"4", "5"})};

    auto planned = katana::cad::drawSurveyFeatures(document, project, {});
    ASSERT_TRUE(planned.ok()) << planned.error().describe();
    EXPECT_EQ(planned->command, nullptr);
    EXPECT_TRUE(planned->report.strings.empty());
    EXPECT_TRUE(planned->report.layersCreated.empty()) << "no line, so no layer for one";
    ASSERT_EQ(planned->unplaced.size(), 2u);
    EXPECT_EQ(planned->unplaced[0].index, 0u);
    EXPECT_EQ(planned->unplaced[0].name, "K1");
    EXPECT_EQ(planned->unplaced[0].reason, UnplacedFeatureReason::TooFewPoints);
    EXPECT_EQ(planned->unplaced[1].index, 1u);
    EXPECT_EQ(planned->unplaced[1].code, "KB");
    EXPECT_EQ(planned->unplaced[1].reason, UnplacedFeatureReason::Coincident);
}

TEST(SurveyFeatures, ACodeNoRuleKnowsGoesOnItsPointsLayerAndIsSaidToBeUnstyled)
{
    Document document;
    document.setSurveyMap(lineMap());
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0, std::nullopt, "ZZ"),
                      point("2", 0.0, 10.0, std::nullopt, "ZZ")};
    project.features = {feature("fence", "ZZ", {"1", "2"})};
    SurveyFeatureOptions options;
    options.import.layerPerCode = true;

    const SurveyFeatureResult result = run(document, project, options);

    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    // No rule, so no model: the layer the import gives point 1, which with a
    // layer per code is survey/points + its code ZZ.
    EXPECT_EQ(lines[0]->layer, "survey/points/ZZ");
    EXPECT_EQ(result.report.layersCreated, (std::vector<std::string>{"survey/points/ZZ"}));
    ASSERT_EQ(result.report.notes.size(), 1u);
    EXPECT_EQ(result.report.notes[0].kind, LineworkNoteKind::NoRuleForName);
    EXPECT_EQ(result.report.notes[0].detail, "ZZ");
    EXPECT_EQ(result.report.strings[0].key, "") << "no rule matched, so no key";
}

TEST(SurveyFeatures, AnImportThatWroteNoCodeStillGivesTheLineItsCodeUnderTheFirstCandidateName)
{
    Document document;
    document.setSurveyMap(lineMap());
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0), point("2", 0.0, 10.0)};
    project.features = {feature("WM01", "WM", {"1", "2"})};
    SurveyFeatureOptions options;
    // The import wrote no code onto its points; the line still needs one,
    // because the rules style it by the code it carries.
    options.import.codeProperty = "";

    const SurveyFeatureResult result = run(document, project, options);

    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    const Entity& line = *lines[0];
    EXPECT_FALSE(line.properties.contains("")) << "a property with no name is not a property";
    // codePropertyCandidates().front() is "code", as processLinework reads
    // when no point carries a candidate.
    EXPECT_EQ(result.report.property, "code");
    EXPECT_EQ(textProperty(line, "code"), "WM");
    // So the rules reach it: WM -> WM*, linestyle WATR Main.
    EXPECT_EQ(line.style, "WATR Main");
    ASSERT_NE(result.report.styling, nullptr);
    EXPECT_EQ(result.report.styling->matched, 1u);
}

TEST(SurveyFeatures, WhenLayersMayNotBeCreatedAMissingOneIsRefusedAndAnExistingOneUsed)
{
    Document document;
    document.setSurveyMap(lineMap());
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0), point("2", 0.0, 10.0)};
    project.features = {feature("WM01", "WM", {"1", "2"})};
    SurveyFeatureOptions options;
    options.coding.createLayers = false;

    // Neither SURVEY SERVICES (the rule's) nor survey/points (the points') exists.
    auto refused = katana::cad::drawSurveyFeatures(document, project, options);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::NotFound);

    // With the points' layer there, the line goes on it rather than being refused.
    katana::entity::Layer points;
    points.name = "survey/points";
    ASSERT_TRUE(document.execute(cmd::createLayer(points)).ok());
    const SurveyFeatureResult result = run(document, project, options);
    const auto lines = polylinesOf(document);
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(lines[0]->layer, "survey/points");
    EXPECT_TRUE(result.report.layersCreated.empty());
    EXPECT_FALSE(document.model().layers.contains("SURVEY SERVICES"));
}

TEST(SurveyFeatures, ARuleModelThatIsNotALayerNameIsRefused)
{
    // Refused where it enters: SurveyMap::add validates the model as a layer
    // path, so drawSurveyFeatures can never be handed a rule naming "a//b".
    // Its own check of the rule's layer stays as a second guard.
    katana::entity::SurveyMap map;
    SurveyRule kerb;
    kerb.key = "KB*";
    kerb.model = "a//b"; // an empty level: validateLayerPath refuses it
    kerb.breakline = SurveyBreakline::Line;
    const auto added = map.add(kerb);
    ASSERT_FALSE(added.ok());
    EXPECT_EQ(added.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(map.empty());
}

TEST(SurveyFeatures, AnInconsistentProjectIsRefusedWithTheValidatorsError)
{
    Document document;
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0)};
    project.features = {feature("K1", "KB", {"1", "9"})}; // 9 is in neither list

    const auto expected = katana::survey::validateProject(project);
    ASSERT_FALSE(expected.ok());
    auto planned = katana::cad::drawSurveyFeatures(document, project, {});
    ASSERT_FALSE(planned.ok());
    EXPECT_EQ(planned.error().code, expected.error().code);
    EXPECT_TRUE(document.model().entities.empty());
}

TEST(SurveyFeatures, OneUndoRemovesEveryFeatureLineAndTheLayersTheyMade)
{
    Document document;
    document.setSurveyMap(lineMap());
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0), point("2", 0.0, 10.0), point("3", 5.0, 0.0),
                      point("4", 5.0, 10.0)};
    project.features = {feature("WM01", "WM", {"1", "2"}), feature("KB1", "KB", {"3", "4"})};

    const SurveyFeatureResult result = run(document, project);
    ASSERT_EQ(polylinesOf(document).size(), 2u);
    EXPECT_EQ(result.report.layersCreated,
              (std::vector<std::string>{"SURVEY KERB", "SURVEY SERVICES"}));

    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.model().entities.empty());
    EXPECT_FALSE(document.model().layers.contains("SURVEY KERB"));
    EXPECT_FALSE(document.model().layers.contains("SURVEY SERVICES"));
    EXPECT_FALSE(document.model().styles.contains("WATR Main"));
}

TEST(SurveyFeatures, NoFeaturesIsNoCommandAndNoError)
{
    Document document;
    SurveyProject project;
    project.points = {point("1", 0.0, 0.0)};
    auto planned = katana::cad::drawSurveyFeatures(document, project, {});
    ASSERT_TRUE(planned.ok());
    EXPECT_EQ(planned->command, nullptr);
    EXPECT_TRUE(planned->report.strings.empty());
    EXPECT_TRUE(planned->unplaced.empty());
}
