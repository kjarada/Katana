// Labels made by hand and by rule (cad/annotation/auto_label.hpp,
// docs/annotation.md "Auto-labelling"): which entities a rule matches, one
// undo step per run, re-running as a no-op, keeping what a person dragged,
// and clearing only what the rules made.

#include <gtest/gtest.h>

#include "katana/cad/annotation/auto_label.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"

using namespace katana::cad;
using namespace katana::entity;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
namespace cmd = katana::commands;
namespace ann = katana::cad::annotation;

namespace {

// A drawing with the default label styles, three coded points on
// survey/points, a boundary on cadastre and a line on design.
struct Site {
    Document document;
    std::vector<EntityId> points;
    EntityId boundary = 0;
    EntityId design = 0;

    Site()
    {
        for (const LabelStyle& style : ann::defaultLabelStyles()) {
            EXPECT_TRUE(document.execute(cmd::createLabelStyle(style)).ok());
        }
        for (const char* name : {"survey/points", "cadastre", "design"}) {
            Layer layer;
            layer.name = name;
            EXPECT_TRUE(document.execute(cmd::createLayer(layer)).ok());
        }
        const char* codes[] = {"EP", "EP", "TREE"};
        for (int i = 0; i < 3; ++i) {
            Entity point{.geometry = PointGeometry{Point2(10.0 * i, 0)}, .layer = "survey/points"};
            point.properties["code"] = std::string(codes[i]);
            point.properties["point"] = std::int64_t{100 + i};
            point.properties["elevation"] = 20.0 + i;
            EXPECT_TRUE(document.execute(cmd::createEntities({point})).ok());
            points.push_back(document.lastCreatedEntities().front());
        }
        Entity lot{.geometry = Polyline2{{Point2(0, 0), Point2(30, 0), Point2(30, 20), Point2(0, 20)},
                                         true},
                   .layer = "cadastre"};
        EXPECT_TRUE(document.execute(cmd::createEntities({lot})).ok());
        boundary = document.lastCreatedEntities().front();
        Entity line{.geometry = Segment2{Point2(0, 50), Point2(40, 50)}, .layer = "design"};
        EXPECT_TRUE(document.execute(cmd::createEntities({line})).ok());
        design = document.lastCreatedEntities().front();
    }

    void rule(LabelRule r) { EXPECT_TRUE(document.execute(cmd::createLabelRule(std::move(r))).ok()); }

    [[nodiscard]] std::vector<const LabelGeometry*> labels() const
    {
        std::vector<const LabelGeometry*> found;
        document.model().entities.forEach([&](const Entity& entity) {
            if (const auto* label = std::get_if<LabelGeometry>(&entity.geometry)) {
                found.push_back(label);
            }
        });
        return found;
    }
};

} // namespace

TEST(AutoLabel, RulesMatchByLayerCodeAndType)
{
    Site site;
    const auto& model = site.document.model();
    const LabelStyle& spot = *model.labelStyles.find("Spot Level");
    LabelRule rule{.name = "ep", .labelStyle = "Spot Level", .layer = "survey", .code = "ep"};
    EXPECT_TRUE(ann::ruleMatches(rule, spot, *model.entities.find(site.points[0])))
        << "the layer's ancestor matches, and the code without case";
    EXPECT_FALSE(ann::ruleMatches(rule, spot, *model.entities.find(site.points[2]))) << "TREE";
    rule.code.clear();
    rule.layer = "design";
    EXPECT_FALSE(ann::ruleMatches(rule, spot, *model.entities.find(site.points[0])));
    rule.layer.clear();
    EXPECT_FALSE(ann::ruleMatches(rule, spot, *model.entities.find(site.design)))
        << "a point style cannot label a line";
    rule.enabled = false;
    EXPECT_FALSE(ann::ruleMatches(rule, spot, *model.entities.find(site.points[0])));
}

TEST(AutoLabel, ARunIsOneStepAndARerunChangesNothing)
{
    Site site;
    site.rule({.name = "levels", .labelStyle = "Spot Level", .code = "EP",
               .labelLayer = "labels/levels"});
    site.rule({.name = "bearings", .labelStyle = "Bearing Distance", .layer = "cadastre"});
    const std::size_t steps = site.document.history().undoCount();
    ann::AutoLabelReport report;
    auto run = ann::autoLabel(site.document.model(), {}, &report);
    ASSERT_TRUE(run.ok()) << run.error().describe();
    ASSERT_TRUE(*run);
    ASSERT_TRUE(site.document.execute(std::move(*run)).ok());
    EXPECT_EQ(report.created, 3u) << "two EP points and the boundary";
    EXPECT_EQ(report.perRule["levels"], 2u);
    EXPECT_EQ(site.document.history().undoCount(), steps + 1);
    EXPECT_TRUE(site.document.model().layers.contains("labels/levels"))
        << "the rule's layer made in the same step";

    // Again: nothing to do, and no empty undo step.
    auto again = ann::autoLabel(site.document.model(), {}, &report);
    ASSERT_TRUE(again.ok());
    EXPECT_FALSE(*again);
    EXPECT_EQ(report.kept, 3u);

    ASSERT_TRUE(site.document.undo().ok());
    EXPECT_TRUE(site.labels().empty());
    EXPECT_FALSE(site.document.model().layers.contains("labels/levels"));
}

TEST(AutoLabel, ARerunKeepsADraggedLabelAndDropsOnesNoLongerAskedFor)
{
    Site site;
    site.rule({.name = "levels", .labelStyle = "Spot Level", .code = "EP"});
    auto run = ann::autoLabel(site.document.model(), {});
    ASSERT_TRUE(site.document.execute(std::move(*run)).ok());
    // Drag one label, recode one point.
    EntityId draggedId = 0;
    site.document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<LabelGeometry>(entity.geometry) && draggedId == 0) {
            draggedId = entity.id;
        }
    });
    Entity dragged = *site.document.model().entities.find(draggedId);
    std::get<LabelGeometry>(dragged.geometry).position = Point2(-5, -5);
    const EntityId draggedTarget = std::get<LabelGeometry>(dragged.geometry).target;
    ASSERT_TRUE(site.document.execute(cmd::setEntityGeometry(draggedId, dragged.geometry)).ok());
    const EntityId other = draggedTarget == site.points[0] ? site.points[1] : site.points[0];
    ASSERT_TRUE(site.document.execute(cmd::setEntityProperty({other}, "code", std::string("KERB"))).ok());

    ann::AutoLabelReport report;
    auto rerun = ann::autoLabel(site.document.model(), {}, &report);
    ASSERT_TRUE(rerun.ok());
    ASSERT_TRUE(*rerun);
    ASSERT_TRUE(site.document.execute(std::move(*rerun)).ok());
    EXPECT_EQ(report.kept, 1u);
    EXPECT_EQ(report.removed, 1u);
    ASSERT_TRUE(site.document.model().entities.contains(draggedId)) << "the same label, same id";
    EXPECT_EQ(*std::get<LabelGeometry>(site.document.model().entities.find(draggedId)->geometry).position,
              Point2(-5, -5));
}

TEST(AutoLabel, ClearingRemovesOnlyWhatTheRulesMade)
{
    Site site;
    site.rule({.name = "levels", .labelStyle = "Spot Level", .code = "EP"});
    auto run = ann::autoLabel(site.document.model(), {});
    ASSERT_TRUE(run.ok());
    ASSERT_TRUE(site.document.execute(std::move(*run)).ok());
    // One by hand.
    ann::LabelRequest request{.target = site.points[2], .style = "Point Number"};
    auto byHand = ann::createLabel(site.document.model(), request);
    ASSERT_TRUE(byHand.ok()) << byHand.error().describe();
    ASSERT_TRUE(site.document.execute(std::move(*byHand)).ok());
    ASSERT_EQ(site.labels().size(), 3u);

    std::size_t removed = 0;
    auto clear = ann::clearAutoLabels(site.document.model(), {"levels"}, &removed);
    ASSERT_TRUE(clear.ok());
    ASSERT_TRUE(site.document.execute(std::move(*clear)).ok());
    EXPECT_EQ(removed, 2u);
    ASSERT_EQ(site.labels().size(), 1u);
    EXPECT_TRUE(site.labels().front()->rule.empty()) << "the hand-placed one stays";
    EXPECT_FALSE(ann::clearAutoLabels(site.document.model(), {"nothing"}).ok());
}

TEST(AutoLabel, ALabelThatWouldSayNothingIsNotMade)
{
    Site site;
    // A point with no level cannot carry a spot level.
    Entity bare{.geometry = PointGeometry{Point2(5, 5)}, .layer = "survey/points"};
    ASSERT_TRUE(site.document.execute(cmd::createEntities({bare})).ok());
    const EntityId id = site.document.lastCreatedEntities().front();
    const auto refused =
        ann::createLabel(site.document.model(), {.target = id, .style = "Spot Level"});
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("say nothing"), std::string::npos);
    EXPECT_FALSE(ann::createLabel(site.document.model(), {.target = id, .style = "Bearing Distance"})
                     .ok())
        << "a segment style on a point";
    EXPECT_FALSE(ann::createLabel(site.document.model(), {.target = id, .style = "Missing"}).ok());
}

TEST(AutoLabel, ChainageRulesLabelAlignments)
{
    Site site;
    Alignment road;
    road.name = "road";
    road.horizontal.pis = {katana::geometry::AlignmentPI{Point2(0, 0)},
                           katana::geometry::AlignmentPI{Point2(100, 0)}};
    ASSERT_TRUE(site.document.execute(cmd::createAlignment(road)).ok());
    site.rule({.name = "chainages", .labelStyle = "Chainage"});
    ann::AutoLabelReport report;
    auto run = ann::autoLabel(site.document.model(), {"chainages"}, &report);
    ASSERT_TRUE(run.ok());
    ASSERT_TRUE(site.document.execute(std::move(*run)).ok());
    EXPECT_EQ(report.created, 1u);
    EXPECT_EQ(site.labels().front()->alignment, "road");
}

TEST(AutoLabel, TheDefaultStylesAreAllValid)
{
    for (const LabelStyle& style : ann::defaultLabelStyles()) {
        EXPECT_TRUE(validate(style).ok()) << style.name;
    }
}
