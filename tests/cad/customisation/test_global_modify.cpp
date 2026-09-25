// Global Modify: what a scope and a filter take, and the one command that
// changes the entities, the layers they sit on and the styles they wear.
// Every expectation is worked out by hand from the drawing SetUp builds,
// which the comment on the fixture lays out.

#include <gtest/gtest.h>

#include <algorithm>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/global_modify.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"

using namespace katana::cad;
using katana::entity::Color;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::entity::Layer;
using katana::entity::Style;
using katana::entity::TextGeometry;
using katana::geometry::Box2;
using katana::geometry::Point2;
namespace cmd = katana::commands;

namespace {

constexpr Color kRed{255, 0, 0, 255};
constexpr Color kBlue{0, 0, 255, 255};

// The drawing:
//
//   layer             entity    where         style   colour  property
//   survey            pA point  (0,0)         Tree    -       code=TREE1
//   survey            pB point  (10,0)        -       red     code=POLE
//   survey/trees      pC point  (20,0)        Tree    -       code=TREE2
//   roads             ln line   (0,5)-(30,5)  Kerb    -       -
//   roads             tx text   at (0,10)     -       -       -       "CH 100"
//   locked            lk line   (0,20)-(5,20) -       -       -       (layer locked)
//
// Styles Tree (symbol "tree") and Kerb (linetype continuous); a line qn on
// "survey" wearing Tree too, so a Tree definition change reaches beyond
// three selected points.
struct GlobalModifyTest : ::testing::Test {
    Document document;
    EntityId pA = 0, pB = 0, pC = 0, ln = 0, tx = 0, lk = 0, qn = 0;

    void SetUp() override
    {
        addLayer("survey");
        addLayer("survey/trees");
        addLayer("roads");
        addLayer("locked");
        Style tree;
        tree.name = "Tree";
        tree.symbol = "tree";
        ASSERT_TRUE(document.execute(cmd::createStyle(tree)).ok());
        Style kerb;
        kerb.name = "Kerb";
        ASSERT_TRUE(document.execute(cmd::createStyle(kerb)).ok());

        pA = create(cmd::createPoint(Point2(0, 0), {"survey", "Tree", {}}));
        pB = create(cmd::createPoint(Point2(10, 0), {"survey", "", kRed}));
        pC = create(cmd::createPoint(Point2(20, 0), {"survey/trees", "Tree", {}}));
        ln = create(cmd::createLine(Point2(0, 5), Point2(30, 5), {"roads", "Kerb", {}}));
        TextGeometry text;
        text.position = Point2(0, 10);
        text.text = "CH 100";
        text.height = 2.5;
        tx = create(cmd::createText(text, {"roads", "", {}}));
        lk = create(cmd::createLine(Point2(0, 20), Point2(5, 20), {"locked", "", {}}));
        qn = create(cmd::createLine(Point2(0, -5), Point2(5, -5), {"survey", "Tree", {}}));
        ASSERT_TRUE(
            document.execute(cmd::setEntityProperty({pA}, "code", std::string("TREE1"))).ok());
        ASSERT_TRUE(
            document.execute(cmd::setEntityProperty({pB}, "code", std::string("POLE"))).ok());
        ASSERT_TRUE(
            document.execute(cmd::setEntityProperty({pC}, "code", std::string("TREE2"))).ok());

        Layer locked = *document.model().layers.find("locked");
        locked.locked = true;
        ASSERT_TRUE(document.execute(cmd::updateLayer(locked)).ok());
    }

    void addLayer(const char* name)
    {
        Layer layer;
        layer.name = name;
        ASSERT_TRUE(document.execute(cmd::createLayer(layer)).ok());
    }

    EntityId create(cmd::CommandPtr command)
    {
        EXPECT_TRUE(document.execute(std::move(command)).ok());
        const auto created = document.lastCreatedEntities();
        return created.empty() ? 0 : created.front();
    }

    const katana::entity::Entity& entity(EntityId id) const
    {
        return *document.model().entities.find(id);
    }
    const Layer& layer(const char* name) const { return *document.model().layers.find(name); }

    std::vector<EntityId> match(const ModifyScope& scope, const ModifyFilter& filter = {}) const
    {
        auto matched = matchEntities(document, scope, filter);
        EXPECT_TRUE(matched.ok());
        return matched.ok() ? *matched : std::vector<EntityId>{};
    }

    // Plans, checks the plan is good, and runs its command when it has one.
    GlobalModifyPlan apply(const ModifyScope& scope, const ModifyFilter& filter,
                           const GlobalModify& change)
    {
        auto plan = planGlobalModify(document, scope, filter, change);
        EXPECT_TRUE(plan.ok()) << (plan.ok() ? "" : plan.error().describe());
        if (!plan.ok()) {
            return {};
        }
        GlobalModifyPlan result = std::move(plan).value();
        if (result.command) {
            const auto status = document.execute(std::move(result.command));
            EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
        }
        return result;
    }

    void select(std::vector<EntityId> ids)
    {
        document.selection().set(std::move(ids));
        document.notifySelectionChanged();
    }
};

ModifyScope scopeOf(ScopeKind kind)
{
    ModifyScope scope;
    scope.kind = kind;
    return scope;
}

ModifyScope layersScope(std::vector<std::string> layers, bool sublayers)
{
    ModifyScope scope;
    scope.kind = ScopeKind::Layers;
    scope.layers = std::move(layers);
    scope.sublayers = sublayers;
    return scope;
}

} // namespace

// ---- patterns ------------------------------------------------------------------------------

TEST(GlobalModifyPattern, StarAndQuestionMarkMatchAsAFileNamePatternDoesIgnoringCase)
{
    EXPECT_TRUE(matchesPattern("TREE1", "tree*"));
    EXPECT_TRUE(matchesPattern("TREE1", "TREE?"));
    EXPECT_TRUE(matchesPattern("TREE1", "*1"));
    EXPECT_TRUE(matchesPattern("survey/trees", "survey/*"));
    EXPECT_TRUE(matchesPattern("", "*"));
    EXPECT_TRUE(matchesPattern("abc", "a*b*c"));
    EXPECT_TRUE(matchesPattern("abbbc", "a*b*c"));
    // Exact apart from case when there is no wildcard.
    EXPECT_TRUE(matchesPattern("Kerb", "KERB"));
    EXPECT_FALSE(matchesPattern("Kerbs", "Kerb"));
    EXPECT_FALSE(matchesPattern("TREE", "TREE?"));
    EXPECT_FALSE(matchesPattern("survey", "survey/*"));
    EXPECT_FALSE(matchesPattern("", "?"));
    // A back-tracking pattern that fails: the star must not stop at the
    // first 'b'.
    EXPECT_FALSE(matchesPattern("abcbd", "a*bc"));
    EXPECT_TRUE(matchesPattern("abcbc", "a*bc"));
}

// ---- what a scope takes ----------------------------------------------------------------------

TEST_F(GlobalModifyTest, TheSelectionScopeTakesTheSelectionAndTheDrawingScopeTakesEverything)
{
    select({pB, ln});
    EXPECT_EQ(match(scopeOf(ScopeKind::Selection)), (std::vector<EntityId>{pB, ln}));
    EXPECT_EQ(match(scopeOf(ScopeKind::Drawing)),
              (std::vector<EntityId>{pA, pB, pC, ln, tx, lk, qn}));
}

TEST_F(GlobalModifyTest, ALayerTakesItsSublayersOnlyWhenAsked)
{
    // "survey" holds pA, pB and qn; "survey/trees" holds pC.
    EXPECT_EQ(match(layersScope({"survey"}, true)), (std::vector<EntityId>{pA, pB, pC, qn}));
    EXPECT_EQ(match(layersScope({"survey"}, false)), (std::vector<EntityId>{pA, pB, qn}));
    EXPECT_EQ(match(layersScope({"survey/trees", "roads"}, false)),
              (std::vector<EntityId>{pC, ln, tx}));
}

TEST_F(GlobalModifyTest, AMissingLayerOrNoLayerAtAllIsRefused)
{
    EXPECT_EQ(matchEntities(document, layersScope({"nowhere"}, true), {}).error().code,
              katana::core::ErrorCode::NotFound);
    EXPECT_EQ(matchEntities(document, layersScope({}, true), {}).error().code,
              katana::core::ErrorCode::InvalidArgument);
}

TEST_F(GlobalModifyTest, AViewScopeTakesWhatThatViewDrawsAndOnlyWhatIsOnScreenWhenGivenAnArea)
{
    // The view hides "survey", and with it "survey/trees": left are the
    // roads' line and text and the locked layer's line (a locked layer still
    // draws).
    LayerOverrides hidden;
    hidden.hide("survey");
    ModifyScope scope = scopeOf(ScopeKind::View);
    scope.view = &hidden;
    EXPECT_EQ(match(scope), (std::vector<EntityId>{ln, tx, lk}));

    // On screen, the box x 0..8, y 4..6: the road line (0..30 at y 5)
    // crosses it; the text at y 10 and the line at y 20 lie above it.
    scope.area = Box2(Point2(0, 4), Point2(8, 6));
    EXPECT_EQ(match(scope), (std::vector<EntityId>{ln}));

    // A view hiding nothing of its own draws what the document draws; an
    // entity switched off is drawn by no view.
    ASSERT_TRUE(document.execute(cmd::setEntityVisible({pB}, false)).ok());
    EXPECT_EQ(match(scopeOf(ScopeKind::View)), (std::vector<EntityId>{pA, pC, ln, tx, lk, qn}));
}

// ---- which the filter keeps -------------------------------------------------------------------

TEST_F(GlobalModifyTest, TheFilterNarrowsByTypeStyleColourPropertyTextAndLayerPattern)
{
    const ModifyScope all = scopeOf(ScopeKind::Drawing);

    ModifyFilter points;
    points.types = {EntityType::Point};
    EXPECT_EQ(match(all, points), (std::vector<EntityId>{pA, pB, pC}));

    ModifyFilter byLayer;
    byLayer.style = "ByLayer";
    EXPECT_EQ(match(all, byLayer), (std::vector<EntityId>{pB, tx, lk}));

    ModifyFilter tree;
    tree.style = "tr*";
    EXPECT_EQ(match(all, tree), (std::vector<EntityId>{pA, pC, qn}));

    ModifyFilter red;
    red.colour = kRed;
    EXPECT_EQ(match(all, red), (std::vector<EntityId>{pB}));
    ModifyFilter colourByLayer;
    colourByLayer.colour = std::optional<Color>{};
    EXPECT_EQ(match(all, colourByLayer), (std::vector<EntityId>{pA, pC, ln, tx, lk, qn}));

    ModifyFilter treeCodes;
    treeCodes.property = "code";
    treeCodes.propertyValue = "TREE*";
    EXPECT_EQ(match(all, treeCodes), (std::vector<EntityId>{pA, pC}));
    ModifyFilter anyCode;
    anyCode.property = "code";
    EXPECT_EQ(match(all, anyCode), (std::vector<EntityId>{pA, pB, pC}));
    ModifyFilter anyPole;
    anyPole.propertyValue = "pole";
    EXPECT_EQ(match(all, anyPole), (std::vector<EntityId>{pB}));

    ModifyFilter chainage;
    chainage.text = "CH *";
    EXPECT_EQ(match(all, chainage), (std::vector<EntityId>{tx}));

    ModifyFilter underSurvey;
    underSurvey.layers = {"survey/*", "roads"};
    EXPECT_EQ(match(all, underSurvey), (std::vector<EntityId>{pC, ln, tx}));
}

TEST_F(GlobalModifyTest, DrawnOnlyLeavesOutWhatIsSwitchedOffOrOnAHiddenLayer)
{
    Layer roads = layer("roads");
    roads.visible = false;
    ASSERT_TRUE(document.execute(cmd::updateLayer(roads)).ok());
    ASSERT_TRUE(document.execute(cmd::setEntityVisible({pA}, false)).ok());
    ModifyFilter drawn;
    drawn.drawnOnly = true;
    EXPECT_EQ(match(scopeOf(ScopeKind::Drawing), drawn), (std::vector<EntityId>{pB, pC, lk, qn}));
}

// ---- changing the entities --------------------------------------------------------------------

TEST_F(GlobalModifyTest, TheSelectionIsRecolouredRestyledAndMovedInOneUndoStep)
{
    select({pA, pB, ln});
    GlobalModify change;
    change.entities.colour = kBlue;
    change.entities.style = "Kerb";
    change.entities.layer = "roads";
    const std::size_t undoBefore = document.history().undoCount();

    const GlobalModifyPlan plan = apply(scopeOf(ScopeKind::Selection), {}, change);

    // All three change: pA and pB in every field, ln in its colour only.
    EXPECT_EQ(plan.changed, (std::vector<EntityId>{pA, pB, ln}));
    for (const EntityId id : {pA, pB, ln}) {
        EXPECT_EQ(entity(id).color, kBlue);
        EXPECT_EQ(entity(id).style, "Kerb");
        EXPECT_EQ(entity(id).layer, "roads");
    }
    EXPECT_EQ(entity(pC).style, "Tree"); // not selected
    EXPECT_EQ(document.history().undoCount(), undoBefore + 1);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(entity(pA).color, std::nullopt);
    EXPECT_EQ(entity(pA).style, "Tree");
    EXPECT_EQ(entity(pA).layer, "survey");
    EXPECT_EQ(entity(pB).color, kRed);
    EXPECT_EQ(entity(ln).color, std::nullopt);
}

TEST_F(GlobalModifyTest, MovingOntoALayerTheDrawingLacksMakesItAndUndoTakesItAway)
{
    select({pA, pC});
    GlobalModify change;
    change.entities.layer = "asbuilt/trees";

    const GlobalModifyPlan plan = apply(scopeOf(ScopeKind::Selection), {}, change);

    EXPECT_EQ(plan.createsLayer, std::optional<std::string>("asbuilt/trees"));
    EXPECT_TRUE(document.model().layers.contains("asbuilt/trees"));
    EXPECT_EQ(entity(pA).layer, "asbuilt/trees");
    EXPECT_EQ(entity(pC).layer, "asbuilt/trees");
    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(document.model().layers.contains("asbuilt/trees"));
    EXPECT_EQ(entity(pC).layer, "survey/trees");
}

TEST_F(GlobalModifyTest, PropertiesAreSetAndRemovedAndTextHeightReachesOnlyText)
{
    GlobalModify change;
    change.entities.setProperties = {{"checked", true}};
    change.entities.removeProperties = {"code"};
    change.entities.textHeight = 5.0;

    const GlobalModifyPlan plan = apply(layersScope({"survey", "roads"}, false), {}, change);

    // pA, pB, qn (survey) and ln, tx (roads) all gain "checked", so all five
    // change; of them only tx is text, so four are counted as not text.
    EXPECT_EQ(plan.changed, (std::vector<EntityId>{pA, pB, ln, tx, qn}));
    EXPECT_EQ(plan.notText, 4u);
    EXPECT_EQ(std::get<TextGeometry>(entity(tx).geometry).height, 5.0);
    EXPECT_EQ(entity(pA).properties.count("code"), 0u);
    EXPECT_EQ(std::get<bool>(entity(pA).properties.at("checked")), true);
    EXPECT_EQ(entity(pC).properties.count("code"), 1u); // survey/trees: not in scope
}

TEST_F(GlobalModifyTest, AnEntityOnALockedLayerIsCountedAndLeftUnlessTheSameRequestUnlocksIt)
{
    GlobalModify change;
    change.entities.colour = kBlue;
    const GlobalModifyPlan left = apply(layersScope({"roads", "locked"}, false), {}, change);
    EXPECT_EQ(left.locked, 1u);
    EXPECT_EQ(left.changed, (std::vector<EntityId>{ln, tx}));
    EXPECT_EQ(entity(lk).color, std::nullopt);
    EXPECT_NE(left.summary().find("1 entity on a locked layer left as it is"), std::string::npos)
        << left.summary();

    // Unlocking the layer in the same request unlocks it FIRST.
    change.entities.colour = kRed;
    change.layers.locked = false;
    const GlobalModifyPlan unlocked = apply(layersScope({"locked"}, false), {}, change);
    EXPECT_EQ(unlocked.locked, 0u);
    EXPECT_EQ(entity(lk).color, kRed);
    EXPECT_FALSE(layer("locked").locked);
}

TEST_F(GlobalModifyTest, LockingALayerLocksItAfterItsEntitiesAreChanged)
{
    GlobalModify change;
    change.entities.colour = kBlue;
    change.layers.locked = true;
    const GlobalModifyPlan plan = apply(layersScope({"roads"}, false), {}, change);
    EXPECT_EQ(plan.changed, (std::vector<EntityId>{ln, tx}));
    EXPECT_EQ(plan.layersChanged, (std::vector<std::string>{"roads"}));
    EXPECT_EQ(entity(ln).color, kBlue);
    EXPECT_TRUE(layer("roads").locked);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(layer("roads").locked);
    EXPECT_EQ(entity(ln).color, std::nullopt);
}

TEST_F(GlobalModifyTest, MovingOntoALockedLayerIsRefusedBeforeAnythingRuns)
{
    select({pA});
    GlobalModify change;
    change.entities.layer = "locked";
    auto plan = planGlobalModify(document, scopeOf(ScopeKind::Selection), {}, change);
    ASSERT_FALSE(plan.ok());
    EXPECT_EQ(plan.error().code, katana::core::ErrorCode::InvalidState);
}

// ---- changing the layers and the styles --------------------------------------------------------

TEST_F(GlobalModifyTest, LayerFieldsChangeOnTheLayersTheMatchedEntitiesSitOn)
{
    // Points coded TREE*: pA on "survey", pC on "survey/trees". Those two
    // layers are recoloured and reweighted; "roads" is not.
    ModifyFilter trees;
    trees.property = "code";
    trees.propertyValue = "TREE*";
    GlobalModify change;
    change.layers.colour = kBlue;
    change.layers.lineWeight = 0.5;
    const GlobalModifyPlan plan = apply(scopeOf(ScopeKind::Drawing), trees, change);
    EXPECT_EQ(plan.layersChanged, (std::vector<std::string>{"survey", "survey/trees"}));
    EXPECT_TRUE(plan.changed.empty());
    EXPECT_EQ(layer("survey").color, kBlue);
    EXPECT_EQ(layer("survey/trees").lineWeight, 0.5);
    EXPECT_EQ(layer("roads").color, Color{});
}

TEST_F(GlobalModifyTest, TheLayersScopeChangesItsLayersEvenWithNothingOnThem)
{
    addLayer("empty");
    GlobalModify change;
    change.layers.visible = false;
    const GlobalModifyPlan plan = apply(layersScope({"empty"}, true), {}, change);
    EXPECT_TRUE(plan.matched.empty());
    EXPECT_EQ(plan.layersChanged, (std::vector<std::string>{"empty"}));
    EXPECT_FALSE(layer("empty").visible);
}

TEST_F(GlobalModifyTest, AStyleDefinitionChangeSaysWhoElseItRedraws)
{
    // The two TREE points wear Tree; so does the line qn, outside the
    // filter - one entity the change reaches beyond its scope.
    ModifyFilter points;
    points.types = {EntityType::Point};
    points.style = "Tree";
    GlobalModify change;
    change.styles.symbol = "cross";
    change.styles.symbolSize = 1.5;
    const GlobalModifyPlan plan = apply(scopeOf(ScopeKind::Drawing), points, change);
    EXPECT_EQ(plan.stylesChanged, (std::vector<std::string>{"Tree"}));
    EXPECT_EQ(plan.styleReachesOthers, 1u);
    EXPECT_EQ(document.model().styles.find("Tree")->symbol, "cross");
    EXPECT_EQ(document.model().styles.find("Tree")->symbolSize, 1.5);
    EXPECT_NE(plan.summary().find("also redraw 1 entity outside the scope"), std::string::npos)
        << plan.summary();
}

TEST_F(GlobalModifyTest, ASymbolGoesOnThePointsThroughAStyleAndOtherEntitiesAreCounted)
{
    // Survey layer and beneath: pA, pB, pC are points, qn is a line.
    GlobalModify change;
    change.entities.symbol = SymbolModify{"target", 0.0};
    const GlobalModifyPlan plan = apply(layersScope({"survey"}, true), {}, change);
    EXPECT_EQ(plan.notPoints, 1u);
    EXPECT_EQ(plan.changed, (std::vector<EntityId>{pA, pB, pC}));
    ASSERT_TRUE(plan.createsStyle.has_value());
    const std::string made = *plan.createsStyle;
    EXPECT_EQ(document.model().styles.find(made)->symbol, "target");
    for (const EntityId id : {pA, pB, pC}) {
        EXPECT_EQ(entity(id).style, made);
    }
    EXPECT_EQ(entity(qn).style, "Tree");
    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(document.model().styles.contains(made));
    EXPECT_EQ(entity(pA).style, "Tree");
    EXPECT_EQ(entity(pB).style, "");
}

// ---- what is refused, and what is no step at all ----------------------------------------------

TEST_F(GlobalModifyTest, ARequestTheDrawingWouldRefuseIsRefusedNamingTheField)
{
    select({pA});
    const ModifyScope selection = scopeOf(ScopeKind::Selection);
    const auto refused = [&](const GlobalModify& change) {
        auto plan = planGlobalModify(document, selection, {}, change);
        return plan.ok() ? std::string() : plan.error().describe();
    };
    EXPECT_NE(refused(GlobalModify{}).find("nothing to change"), std::string::npos);

    GlobalModify missingStyle;
    missingStyle.entities.style = "Nope";
    EXPECT_NE(refused(missingStyle).find("style does not exist"), std::string::npos);

    GlobalModify badLayer;
    badLayer.entities.layer = "a//b";
    EXPECT_FALSE(refused(badLayer).empty());

    GlobalModify flatText;
    flatText.entities.textHeight = 0.0;
    EXPECT_NE(refused(flatText).find("text height"), std::string::npos);

    GlobalModify missingLinetype;
    missingLinetype.layers.linetype = "no such dashes";
    EXPECT_NE(refused(missingLinetype).find("no linetype"), std::string::npos);

    GlobalModify missingSymbol;
    missingSymbol.styles.symbol = "no such symbol";
    EXPECT_NE(refused(missingSymbol).find("no symbol"), std::string::npos);

    GlobalModify missingHatch;
    missingHatch.layers.hatchPattern = "no such hatch";
    EXPECT_NE(refused(missingHatch).find("hatch pattern"), std::string::npos);
}

TEST_F(GlobalModifyTest, MatchingNothingOrChangingNothingPushesNoUndoStep)
{
    const std::size_t undoBefore = document.history().undoCount();
    GlobalModify change;
    change.entities.colour = kRed;

    ModifyFilter none;
    none.text = "no such text";
    auto nothing = planGlobalModify(document, scopeOf(ScopeKind::Drawing), none, change);
    ASSERT_TRUE(nothing.ok());
    EXPECT_EQ(nothing->command, nullptr);
    EXPECT_EQ(nothing->summary(),
              "Nothing matched: no entity is in the scope and passes the filter.");

    // pB is red already.
    select({pB});
    auto already = planGlobalModify(document, scopeOf(ScopeKind::Selection), {}, change);
    ASSERT_TRUE(already.ok());
    EXPECT_EQ(already->command, nullptr);
    EXPECT_NE(already->summary().find("nothing to change"), std::string::npos);
    EXPECT_EQ(document.history().undoCount(), undoBefore);
}

TEST_F(GlobalModifyTest, TheSummaryCountsWhatChanges)
{
    select({pA, pB});
    GlobalModify change;
    change.entities.colour = kBlue;
    change.layers.colour = kBlue;
    const GlobalModifyPlan plan = apply(scopeOf(ScopeKind::Selection), {}, change);
    EXPECT_EQ(plan.summary(), "2 entities matched; changing 2 entities and 1 layer.");
}

// ---- the command line ------------------------------------------------------------------------

TEST_F(GlobalModifyTest, ModifyOnTheCommandLineFiltersSetsAndIsOneUndoStep)
{
    CommandInterpreter interpreter(document);
    const std::size_t undoBefore = document.history().undoCount();

    // Every point on "survey" and beneath whose code starts TREE: pA and pC.
    auto reply =
        interpreter.run("MODIFY LAYERS survey WHERE TYPE=point PROP=code:tree* SET COLOUR=#0000FF "
                        "LAYER.WEIGHT=0.35 PROP=checked:true");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(*reply, "2 entities matched; changing 2 entities and 2 layers. One UNDO restores "
                      "it.");
    EXPECT_EQ(entity(pA).color, kBlue);
    EXPECT_EQ(entity(pC).color, kBlue);
    EXPECT_EQ(entity(pB).color, kRed); // coded POLE
    EXPECT_EQ(std::get<bool>(entity(pA).properties.at("checked")), true);
    EXPECT_EQ(layer("survey/trees").lineWeight, 0.35);
    EXPECT_EQ(document.history().undoCount(), undoBefore + 1);

    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_EQ(entity(pA).color, std::nullopt);
    EXPECT_EQ(layer("survey/trees").lineWeight, 0.25);
}

TEST_F(GlobalModifyTest, ModifyPreviewSaysWhatWouldChangeAndChangesNothing)
{
    CommandInterpreter interpreter(document);
    const std::size_t undoBefore = document.history().undoCount();
    auto reply = interpreter.run("MODIFY DRAWING WHERE STYLE=ByLayer SET STYLE=Kerb PREVIEW");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    // pB, tx and lk wear no style; lk is on the locked layer.
    EXPECT_EQ(*reply, "preview: 3 entities matched; changing 2 entities; 1 entity on a locked "
                      "layer left as it is.");
    EXPECT_EQ(entity(pB).style, "");
    EXPECT_EQ(document.history().undoCount(), undoBefore);
}

TEST_F(GlobalModifyTest, ModifyOnTheSelectionTakesAQuotedSymbolWithASize)
{
    CommandInterpreter interpreter(document);
    select({pA, ln});
    auto reply = interpreter.run("GM SET \"SYMBOL=manhole@2.5\"");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const katana::entity::Style* style = document.model().styles.find(entity(pA).style);
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->symbol, "manhole");
    EXPECT_EQ(style->symbolSize, 2.5);
    EXPECT_EQ(entity(ln).style, "Kerb");
    EXPECT_NE(reply->find("1 entity is not a point"), std::string::npos) << *reply;
}

TEST_F(GlobalModifyTest, ModifyRefusesWhatItCannotRead)
{
    CommandInterpreter interpreter(document);
    // No SET at all; an unknown key; nothing selected for the default scope.
    EXPECT_FALSE(interpreter.run("MODIFY DRAWING").ok());
    EXPECT_FALSE(interpreter.run("MODIFY DRAWING SET SHADE=blue").ok());
    EXPECT_FALSE(interpreter.run("MODIFY DRAWING WHERE TYPE=blob SET COLOUR=#FF0000").ok());
    EXPECT_FALSE(interpreter.run("MODIFY SET COLOUR=#FF0000").ok());
    EXPECT_FALSE(interpreter.run("MODIFY LAYERS nowhere SET COLOUR=#FF0000").ok());
}
