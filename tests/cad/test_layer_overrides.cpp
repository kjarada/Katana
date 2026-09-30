// Per-view layer visibility (PLAN.MD 47; include/katana/cad/layer_overrides.hpp).
//
// Two halves. The first pins LayerOverrides itself: what hide, show, isolate
// and pruneMissing return and leave behind, worked out by hand from the
// header's contract. The second puts two views on ONE document - a plan that
// hides a layer and a 3D view that does not - and asserts that every consumer
// of the visibility rule (drawing, picking, box selection, snapping, the 3D
// scene, framing) answers per view, while the document itself never learns
// that a view hid anything.
//
// Every expected value below is derived from the stated contract, not
// captured from a run: the layer names are chosen so that the whole-segment
// rule, the ancestor rule and the "subtractive only" rule each decide at least
// one assertion.

#include <gtest/gtest.h>

#include <filesystem>
#include <functional>
#include <numbers>
#include <set>
#include <string>
#include <system_error>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/drawing/grips.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/scene.hpp"
#include "katana/cad/section.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"

using katana::cad::BoxSelectionMode;
using katana::cad::Document;
using katana::cad::kNoLayerOverrides;
using katana::cad::LayerOverrides;
using katana::cad::SelectionFilter;
using katana::cad::SnapMode;
using katana::cad::SnapRequest;
using katana::core::ErrorCode;
using katana::entity::EntityId;
using katana::entity::Layer;
using katana::entity::LayerDatabase;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Point2;

namespace {

std::set<std::string, std::less<>> entries(std::initializer_list<const char*> names)
{
    std::set<std::string, std::less<>> out;
    for (const char* name : names) {
        out.emplace(name);
    }
    return out;
}

} // namespace

// ---- LayerOverrides on its own ------------------------------------------------------

TEST(LayerOverrides, AnUntouchedViewHidesNothing)
{
    const LayerOverrides view;
    EXPECT_TRUE(view.empty());
    EXPECT_EQ(view.size(), 0u);
    EXPECT_FALSE(view.hides("0"));
    EXPECT_FALSE(view.hides("design/surface/tin1"));
    EXPECT_FALSE(view.hides(""));
    EXPECT_FALSE(view.hidesDirectly("design"));
    EXPECT_EQ(view, kNoLayerOverrides);
}

TEST(LayerOverrides, HideReportsTrueOnlyWhenThePathWasNotAlreadyAnEntry)
{
    LayerOverrides view;
    EXPECT_TRUE(view.hide("design"));
    EXPECT_FALSE(view.hide("design")) << "already hidden directly: nothing to repaint";
    EXPECT_EQ(view.size(), 1u);

    // A child of a hidden parent is hidden already, but not DIRECTLY - the
    // contract counts entries, so this is a new one.
    EXPECT_TRUE(view.hide("design/surface"));
    EXPECT_EQ(view.size(), 2u);
}

TEST(LayerOverrides, ShowReportsTrueOnlyWhenItRemovedAnEntry)
{
    LayerOverrides view;
    EXPECT_FALSE(view.show("design")) << "nothing to show in an untouched view";
    ASSERT_TRUE(view.hide("design"));
    EXPECT_TRUE(view.show("design"));
    EXPECT_FALSE(view.show("design"));
    EXPECT_TRUE(view.empty());
}

TEST(LayerOverrides, HidingAPathHidesEverythingBeneathItButNotItsParent)
{
    LayerOverrides view;
    ASSERT_TRUE(view.hide("design/surface"));

    EXPECT_TRUE(view.hides("design/surface"));
    EXPECT_TRUE(view.hides("design/surface/tin1"));
    EXPECT_TRUE(view.hides("design/surface/tin1/edges"));
    EXPECT_FALSE(view.hides("design")) << "hiding a child must not hide its parent";
    EXPECT_FALSE(view.hides("design/road")) << "nor its sibling";

    EXPECT_TRUE(view.hidesDirectly("design/surface"));
    EXPECT_FALSE(view.hidesDirectly("design/surface/tin1"))
        << "hidden by its parent, which a check box shows as greyed, not unticked";
}

TEST(LayerOverrides, AncestorsAreComparedByWholeSegmentsNotByText)
{
    // The same bug tests/entity/test_layer_tree.cpp guards the document rule
    // against: "designs" and "design 2" start with "design" as TEXT but are
    // other trees.
    LayerOverrides view;
    ASSERT_TRUE(view.hide("design"));

    EXPECT_TRUE(view.hides("design"));
    EXPECT_TRUE(view.hides("design/surface/tin1"));
    EXPECT_FALSE(view.hides("designs"));
    EXPECT_FALSE(view.hides("designs/x"));
    EXPECT_FALSE(view.hides("design 2"));
    EXPECT_FALSE(view.hides("design-old/x"));
    EXPECT_FALSE(view.hides("desig"));
}

TEST(LayerOverrides, ShowingAChildOfAHiddenParentStillLeavesItHidden)
{
    LayerOverrides view;
    ASSERT_TRUE(view.hide("design"));
    ASSERT_TRUE(view.hide("design/surface/tin1"));

    EXPECT_TRUE(view.show("design/surface/tin1")) << "its own entry does go";
    EXPECT_FALSE(view.hidesDirectly("design/surface/tin1"));
    EXPECT_TRUE(view.hides("design/surface/tin1")) << "the parent still hides it";

    EXPECT_FALSE(view.show("design/surface")) << "never an entry, so nothing to remove";
    EXPECT_TRUE(view.hides("design/surface"));

    ASSERT_TRUE(view.show("design"));
    EXPECT_FALSE(view.hides("design/surface/tin1"));
}

TEST(LayerOverrides, TheEmptyPathIsNeverAnEntry)
{
    // No layer has the empty name, and hides() never asks about it (the walk
    // up the ancestors stops before it), so an entry for it would be dead.
    LayerOverrides view;
    EXPECT_FALSE(view.hide(""));
    EXPECT_TRUE(view.empty());
    EXPECT_FALSE(view.hides("0"));
    EXPECT_FALSE(view.show(""));
}

TEST(LayerOverrides, ClearShowsEverythingTheDocumentShows)
{
    LayerOverrides view;
    ASSERT_TRUE(view.hide("design"));
    ASSERT_TRUE(view.hide("survey/points"));
    view.clear();
    EXPECT_TRUE(view.empty());
    EXPECT_FALSE(view.hides("design/surface"));
    EXPECT_FALSE(view.hides("survey/points"));
}

TEST(LayerOverrides, TwoViewsAreEqualExactlyWhenTheyHideTheSameEntries)
{
    LayerOverrides a;
    LayerOverrides b;
    ASSERT_TRUE(a.hide("design"));
    ASSERT_TRUE(a.hide("survey"));
    ASSERT_TRUE(b.hide("survey"));
    ASSERT_TRUE(b.hide("design"));
    EXPECT_EQ(a, b) << "the order things were hidden in is not state";

    ASSERT_TRUE(b.hide("design/surface"));
    EXPECT_NE(a, b) << "an entry under a hidden parent draws nothing different but is still state";

    ASSERT_TRUE(b.show("design/surface"));
    EXPECT_EQ(a, b);
    a.clear();
    EXPECT_EQ(a, kNoLayerOverrides);
}

// ---- isolate ------------------------------------------------------------------------

TEST(LayerOverrides, IsolatingHidesTheFewestEntriesThatLeaveOnlyThatBranch)
{
    // Worked by hand. The tree of {a/b/c, a/x, a/b/y, k/y, m} is
    //
    //   a ── b ── c        k ── y        m
    //   │    └─── y
    //   └─ x
    //
    // Isolating a/b keeps a/b, everything beneath it and its ancestor a. At
    // the root the siblings of "a" are k and m; under "a" the sibling of "b"
    // is x. Hiding k covers k/y. So exactly {a/x, k, m}.
    const std::vector<std::string> names = {"a/b/c", "a/x", "a/b/y", "k/y", "m"};
    LayerOverrides view;
    const auto status = view.isolate("a/b", names);
    ASSERT_TRUE(status.ok()) << status.error().describe();

    EXPECT_EQ(view.hidden(), entries({"a/x", "k", "m"}));
    EXPECT_FALSE(view.hides("a"));
    EXPECT_FALSE(view.hides("a/b"));
    EXPECT_FALSE(view.hides("a/b/c"));
    EXPECT_FALSE(view.hides("a/b/y"));
    EXPECT_TRUE(view.hides("a/x"));
    EXPECT_TRUE(view.hides("k/y"));
    EXPECT_TRUE(view.hides("m"));
}

TEST(LayerOverrides, IsolatingALeafHidesItsSiblingsAtEveryLevel)
{
    // Root level: siblings of "design" are "0" and "asbuilt". Under design:
    // the sibling of "surface" is "road". Under design/surface: "tin2".
    const std::vector<std::string> names = {"0", "asbuilt/road/kerb", "design/road/kerb",
                                            "design/surface/tin1", "design/surface/tin2"};
    LayerOverrides view;
    ASSERT_TRUE(view.isolate("design/surface/tin1", names).ok());
    EXPECT_EQ(view.hidden(), entries({"0", "asbuilt", "design/road", "design/surface/tin2"}));
}

TEST(LayerOverrides, IsolatingComparesWholeSegments)
{
    // "designs" and "design 2" are roots of their own, so they are siblings of
    // "design" to hide - not part of it to keep.
    const std::vector<std::string> names = {"design/x", "designs/y", "design 2"};
    LayerOverrides view;
    ASSERT_TRUE(view.isolate("design", names).ok());
    EXPECT_EQ(view.hidden(), entries({"design 2", "designs"}));
    EXPECT_FALSE(view.hides("design/x"));
}

TEST(LayerOverrides, ANodeThatExistsOnlyAsAPrefixCanBeIsolated)
{
    // No name in the list IS "design/surface"; it is a node of the derived
    // tree because "design/surface/tin1" lies beneath it (layer_path.hpp).
    const std::vector<std::string> names = {"design/surface/tin1", "design/road", "survey"};
    LayerOverrides view;
    ASSERT_TRUE(view.isolate("design/surface", names).ok());
    EXPECT_EQ(view.hidden(), entries({"design/road", "survey"}));
}

TEST(LayerOverrides, IsolatingTheOnlyTreeHidesNothing)
{
    LayerOverrides view;
    ASSERT_TRUE(view.isolate("a", {"a", "a/b", "a/c/d"}).ok());
    EXPECT_TRUE(view.empty());
}

TEST(LayerOverrides, IsolatingSomethingThatIsNotThereFailsAndChangesNothing)
{
    const std::vector<std::string> names = {"design/surface/tin1", "survey"};
    LayerOverrides view;
    ASSERT_TRUE(view.hide("survey"));
    const LayerOverrides before = view;

    for (const char* missing : {"nothing", "design/surface/tin1/deeper", "desig", ""}) {
        const auto status = view.isolate(missing, names);
        ASSERT_FALSE(status.ok()) << "'" << missing << "'";
        EXPECT_EQ(status.error().code, ErrorCode::NotFound) << "'" << missing << "'";
        // Isolating nothing would hide the whole drawing; a failure must not
        // leave half of that behind either (PLAN.MD 36).
        EXPECT_EQ(view, before) << "'" << missing << "'";
    }
}

TEST(LayerOverrides, IsolatingReplacesWhateverTheViewHidBefore)
{
    const std::vector<std::string> names = {"a/b/c", "a/x", "a/b/y", "k/y", "m"};
    LayerOverrides view;
    ASSERT_TRUE(view.hide("a/b/c")); // inside the branch about to be isolated
    ASSERT_TRUE(view.hide("zzz"));   // names no layer at all
    ASSERT_TRUE(view.isolate("a/b", names).ok());
    EXPECT_EQ(view.hidden(), entries({"a/x", "k", "m"}));
    EXPECT_FALSE(view.hides("a/b/c"));
}

// ---- pruneMissing --------------------------------------------------------------------

namespace {

// design, design/surface and design/surface/tin1 (the ancestors are created by
// add), survey, and the default layer 0.
LayerDatabase jobLayers()
{
    LayerDatabase layers;
    for (const char* name : {"design/surface/tin1", "survey"}) {
        Layer layer;
        layer.name = name;
        EXPECT_TRUE(layers.add(layer).ok()) << name;
    }
    return layers;
}

} // namespace

TEST(LayerOverrides, PruningDropsOnlyEntriesThatNameNoLayer)
{
    const LayerDatabase layers = jobLayers();
    LayerOverrides view;
    for (const char* name : {"design", "design/surface/tin1", "survey", "gone", "design/gone",
                             "designs", "survey/points"}) {
        ASSERT_TRUE(view.hide(name)) << name;
    }

    // gone, design/gone, designs (whole segments: not "design") and
    // survey/points (survey has no children) name nothing: four.
    EXPECT_EQ(view.pruneMissing(layers), 4u);
    EXPECT_EQ(view.hidden(), entries({"design", "design/surface/tin1", "survey"}));

    EXPECT_EQ(view.pruneMissing(layers), 0u) << "a second prune has nothing left to drop";
}

TEST(LayerOverrides, PruningKeepsAnEntryForAParentWhoseChildrenExist)
{
    // "design/surface" is kept because a layer lies beneath it. (Through the
    // LayerDatabase API the parent always has a record of its own too - add()
    // creates missing ancestors - so the "only a prefix" case of isolate
    // cannot be built here; the rule that keeps it is the same isLayerUnder.)
    const LayerDatabase layers = jobLayers();
    LayerOverrides view;
    ASSERT_TRUE(view.hide("design/surface"));
    EXPECT_EQ(view.pruneMissing(layers), 0u);
    EXPECT_TRUE(view.hides("design/surface/tin1"));
}

TEST(LayerOverrides, PruningAnUntouchedViewDropsNothing)
{
    LayerOverrides view;
    EXPECT_EQ(view.pruneMissing(jobLayers()), 0u);
    EXPECT_EQ(view.pruneMissing(LayerDatabase{}), 0u);
}

TEST(LayerOverrides, PruningAfterARenameStopsTheOldNameHidingALaterLayer)
{
    // The reason pruneMissing exists: a view hid design/surface, the branch was
    // renamed away, and a new layer that later takes the old name must not be
    // born hidden in this view.
    LayerDatabase layers = jobLayers();
    LayerOverrides view;
    ASSERT_TRUE(view.hide("design/surface"));

    ASSERT_TRUE(layers.renameSubtree("design/surface", "archive/surface").ok());
    EXPECT_EQ(view.pruneMissing(layers), 1u);
    EXPECT_TRUE(view.empty());

    Layer reborn;
    reborn.name = "design/surface/new";
    ASSERT_TRUE(layers.add(reborn).ok());
    EXPECT_FALSE(view.hides(reborn.name));
}

// ---- the rule with a view, on one document ------------------------------------------

namespace {

constexpr const char* kTin = "design/surface/tin1";
constexpr const char* kSibling = "design 2"; // sorts between design and design/...
constexpr const char* kTextPrefix = "designs/x";
constexpr const char* kSurvey = "survey/points";

// One document, four horizontal lines ten units apart, each on its own layer:
//
//   y = 30   survey/points
//   y = 20   designs/x
//   y = 10   design 2
//   y =  0   design/surface/tin1
//
// and two views of it: `plan` hides the PARENT "design", `model3d` hides
// nothing. A Document is neither copied nor moved, so each test owns one.
struct TwoViews {
    Document document;
    EntityId tin = katana::entity::kInvalidEntityId;
    EntityId sibling = katana::entity::kInvalidEntityId;
    EntityId textPrefix = katana::entity::kInvalidEntityId;
    EntityId survey = katana::entity::kInvalidEntityId;
    LayerOverrides plan;
    LayerOverrides model3d;

    TwoViews()
    {
        tin = line(kTin, 0.0);
        sibling = line(kSibling, 10.0);
        textPrefix = line(kTextPrefix, 20.0);
        survey = line(kSurvey, 30.0);
        EXPECT_TRUE(plan.hide("design"));
    }

    EntityId line(const char* layerName, double y)
    {
        if (!document.model().layers.contains(layerName)) {
            Layer layer;
            layer.name = layerName;
            EXPECT_TRUE(document.execute(katana::commands::createLayer(layer)).ok()) << layerName;
        }
        katana::commands::EntityAttributes on;
        on.layer = layerName;
        EXPECT_TRUE(document
                        .execute(katana::commands::createLine(Point2(0.0, y), Point2(10.0, y), on))
                        .ok());
        return document.model().entities.ids().back();
    }

    [[nodiscard]] const katana::entity::Entity& entity(EntityId id) const
    {
        return *document.model().entities.find(id);
    }

    // Through a command, as the Layers panel does. An update that changes
    // nothing is refused by the command, so it is not sent.
    void setLayer(const char* name, bool visible, bool locked)
    {
        Layer layer = *document.model().layers.find(name);
        if (layer.visible == visible && layer.locked == locked) {
            return;
        }
        layer.visible = visible;
        layer.locked = locked;
        ASSERT_TRUE(document.execute(katana::commands::updateLayer(layer)).ok()) << name;
    }
};

bool drawn(const TwoViews& views, EntityId id, const LayerOverrides& view)
{
    return katana::cad::isDrawn(views.document.model(), views.entity(id), view);
}

bool selectable(const TwoViews& views, EntityId id, const LayerOverrides& view)
{
    return katana::cad::isSelectable(views.document.model(), views.entity(id), view);
}

} // namespace

TEST(ViewLayerRule, AViewHidingTheParentHidesTheChildsEntityThereAndNowhereElse)
{
    const TwoViews views;

    EXPECT_FALSE(drawn(views, views.tin, views.plan)) << "hidden by the parent, in the plan";
    EXPECT_FALSE(selectable(views, views.tin, views.plan));
    EXPECT_TRUE(drawn(views, views.tin, views.model3d)) << "the other view is untouched";
    EXPECT_TRUE(selectable(views, views.tin, views.model3d));
    EXPECT_TRUE(drawn(views, views.tin, kNoLayerOverrides)) << "and so is the document";

    // Whole segments: the plan hid "design", not these.
    EXPECT_TRUE(drawn(views, views.sibling, views.plan));
    EXPECT_TRUE(drawn(views, views.textPrefix, views.plan));
    EXPECT_TRUE(drawn(views, views.survey, views.plan));
}

TEST(ViewLayerRule, HidingInAViewLeavesTheDocumentUntouched)
{
    // A view's layers are view state: not saved, not undoable, not a change
    // (layer_overrides.hpp). Measured on a saved document so "modified" has a
    // baseline of false.
    // Its own directory, removed by name, as test_cad.cpp does, because ctest
    // runs these cases in parallel; removed after the Document has closed the
    // project database, which Windows will not delete while it is open.
    namespace fs = std::filesystem;
    const fs::path directory =
        fs::temp_directory_path() / "katana-cad-tests-view-layers" / "views.katana";
    fs::remove_all(directory.parent_path());
    {
        TwoViews views;
        const auto saved = views.document.saveAs(directory);
        ASSERT_TRUE(saved.ok()) << saved.error().describe();
        ASSERT_FALSE(views.document.isModified());

        const std::vector<Layer> layersBefore = views.document.model().layers.all();
        const std::size_t undoBefore = views.document.history().undoCount();
        int notified = 0;
        const auto listening = views.document.addListener([&] { ++notified; });

        ASSERT_TRUE(views.model3d.hide(kSurvey));
        ASSERT_TRUE(views.plan.isolate(kSurvey, views.document.model().layers.names()).ok());
        ASSERT_TRUE(views.plan.show("design"));
        EXPECT_EQ(views.plan.pruneMissing(views.document.model().layers), 0u);
        views.model3d.clear();

        EXPECT_EQ(views.document.model().layers.all(), layersBefore)
            << "no Layer::visible flag may be flipped to implement a view";
        EXPECT_EQ(views.document.history().undoCount(), undoBefore);
        EXPECT_FALSE(views.document.isModified());
        EXPECT_EQ(notified, 0) << "the document heard nothing, so nothing else repaints";
    }
    std::error_code ignored;
    fs::remove_all(directory.parent_path(), ignored);
}

TEST(ViewLayerRule, AViewCannotShowWhatTheDocumentHides)
{
    TwoViews views;
    views.setLayer("survey", false, false);

    EXPECT_FALSE(drawn(views, views.survey, views.plan));
    EXPECT_FALSE(drawn(views, views.survey, views.model3d));

    // There is nothing a view can do to bring it back: show() removes only a
    // hide of the view's own, and an untouched view is the most it can show.
    EXPECT_FALSE(views.model3d.show("survey"));
    EXPECT_FALSE(views.model3d.show(kSurvey));
    views.plan.clear();
    EXPECT_FALSE(drawn(views, views.survey, views.plan));
    EXPECT_FALSE(drawn(views, views.survey, views.model3d));
}

TEST(ViewLayerRule, ALockedLayerIsDrawnButNotSelectableInAViewThatShowsIt)
{
    TwoViews views;
    views.setLayer("survey", true, true);

    EXPECT_TRUE(drawn(views, views.survey, views.model3d));
    EXPECT_FALSE(selectable(views, views.survey, views.model3d));

    // And a view that hides it neither draws nor selects it.
    ASSERT_TRUE(views.model3d.hide("survey"));
    EXPECT_FALSE(drawn(views, views.survey, views.model3d));
    EXPECT_FALSE(selectable(views, views.survey, views.model3d));
}

TEST(ViewLayerRule, NoOverridesIsExactlyTheDocumentRuleItReplaced)
{
    // The rule before per-view layers (commit 584b439) was
    //   drawn      = entity.visible && layer effectively visible
    //   selectable = drawn && !layer effectively locked
    // kNoLayerOverrides and an untouched view must give exactly that, in every
    // combination of the document's own switches.
    TwoViews views;
    const auto ids = std::vector<EntityId>{views.tin, views.sibling, views.textPrefix, views.survey};
    int combinations = 0;
    for (const bool parentVisible : {true, false}) {
        for (const bool parentLocked : {false, true}) {
            for (const bool surveyVisible : {true, false}) {
                views.setLayer("design", parentVisible, parentLocked);
                views.setLayer("survey", surveyVisible, !surveyVisible);
                ASSERT_TRUE(views.document
                                .execute(katana::commands::setEntityVisible(
                                    {views.textPrefix}, parentVisible != surveyVisible))
                                .ok());
                const auto& model = views.document.model();
                const LayerOverrides untouched;
                for (const EntityId id : ids) {
                    const auto& entity = views.entity(id);
                    const bool oldDrawn =
                        entity.visible && model.layers.effectivelyVisible(entity.layer);
                    const bool oldSelectable =
                        oldDrawn && !model.layers.effectivelyLocked(entity.layer);
                    EXPECT_EQ(katana::cad::isDrawn(model, entity, kNoLayerOverrides), oldDrawn)
                        << entity.layer;
                    EXPECT_EQ(katana::cad::isDrawn(model.layers.resolve(entity.layer), entity,
                                                   kNoLayerOverrides),
                              oldDrawn)
                        << entity.layer;
                    EXPECT_EQ(katana::cad::isSelectable(model, entity, kNoLayerOverrides),
                              oldSelectable)
                        << entity.layer;
                    EXPECT_EQ(katana::cad::isDrawn(model, entity, untouched), oldDrawn);
                    EXPECT_EQ(katana::cad::isSelectable(model, entity, untouched), oldSelectable);
                }
                ++combinations;
            }
        }
    }
    EXPECT_EQ(combinations, 8);
}

TEST(ViewLayerRule, AnEntityOnALayerThatDoesNotExistIsDrawnInNoView)
{
    // ResolvedLayer's contract: a missing layer is neither shown nor editable.
    // A view adds nothing to that - it can only subtract.
    katana::entity::Model model;
    katana::entity::Entity orphan;
    orphan.layer = "no/such/layer";
    orphan.geometry = katana::geometry::Segment2{Point2(0, 0), Point2(1, 0)};
    const LayerOverrides untouched;
    EXPECT_FALSE(katana::cad::isDrawn(model, orphan, untouched));
    EXPECT_FALSE(katana::cad::isSelectable(model, orphan, untouched));
}

// ---- every consumer honours the view --------------------------------------------------

TEST(ViewLayerRule, PickingHonoursTheViewItIsMadeIn)
{
    const TwoViews views;
    const auto& model = views.document.model();
    const Point2 onTheTin(5.0, 0.0);

    SelectionFilter inPlan;
    inPlan.view = &views.plan;
    SelectionFilter in3d;
    in3d.view = &views.model3d;

    for (const auto* index : {static_cast<const katana::geometry::SpatialIndex*>(nullptr),
                              &views.document.spatialIndex()}) {
        EXPECT_FALSE(katana::cad::pickEntity(model, onTheTin, 1.0, inPlan, index).has_value())
            << "a line the plan does not show cannot be picked there";
        EXPECT_EQ(katana::cad::pickEntity(model, onTheTin, 1.0, in3d, index), views.tin);
        EXPECT_EQ(katana::cad::pickEntity(model, onTheTin, 1.0, {}, index), views.tin)
            << "no view: the document rule";
        // The plan still picks what it shows.
        EXPECT_EQ(katana::cad::pickEntity(model, Point2(5.0, 10.0), 1.0, inPlan, index),
                  views.sibling);
    }
}

TEST(ViewLayerRule, BoxSelectionHonoursTheViewItIsMadeIn)
{
    const TwoViews views;
    const auto& model = views.document.model();
    const Box2 everything(Point2(-1.0, -1.0), Point2(11.0, 31.0));

    SelectionFilter inPlan;
    inPlan.view = &views.plan;
    SelectionFilter in3d;
    in3d.view = &views.model3d;

    const std::vector<EntityId> all = {views.tin, views.sibling, views.textPrefix, views.survey};
    const std::vector<EntityId> allButTin = {views.sibling, views.textPrefix, views.survey};
    for (const auto mode : {BoxSelectionMode::Window, BoxSelectionMode::Crossing}) {
        for (const auto* index : {static_cast<const katana::geometry::SpatialIndex*>(nullptr),
                                  &views.document.spatialIndex()}) {
            EXPECT_EQ(katana::cad::pickInBox(model, everything, mode, inPlan, index), allButTin);
            EXPECT_EQ(katana::cad::pickInBox(model, everything, mode, in3d, index), all);
            EXPECT_EQ(katana::cad::pickInBox(model, everything, mode, {}, index), all);
        }
    }
}

TEST(ViewLayerRule, SnappingHonoursTheViewTheCursorIsIn)
{
    const TwoViews views;
    const auto& model = views.document.model();

    SnapRequest request;
    request.cursor = Point2(10.0, 0.0); // the tin line's end; the next line is 10 away
    request.aperture = 1.0;
    request.modes = static_cast<katana::cad::SnapModes>(SnapMode::Endpoint);

    for (const auto* index : {static_cast<const katana::geometry::SpatialIndex*>(nullptr),
                              &views.document.spatialIndex()}) {
        request.view = &views.plan;
        EXPECT_FALSE(katana::cad::snap(model, request, index).has_value())
            << "a snap to a line the view does not show";

        request.view = &views.model3d;
        const auto snapped = katana::cad::snap(model, request, index);
        ASSERT_TRUE(snapped.has_value());
        EXPECT_EQ(snapped->mode, SnapMode::Endpoint);
        EXPECT_EQ(snapped->entity, views.tin);
        EXPECT_EQ(snapped->point, Point2(10.0, 0.0));
    }
}

TEST(ViewLayerRule, AGhostIsNeitherPickedNorSnappedToNorGivenGrips)
{
    // Selected, the tin line is drawn in the plan view as a ghost
    // (cad/selection_style.hpp) - drawn, but still on a layer that view
    // hides: a click, a snap or a grip there must not reach what the view
    // hides, or Delete would erase, and a drag move, something the user
    // cannot see.
    TwoViews views;
    views.document.selection().add(views.tin);
    const auto& model = views.document.model();
    // A ghost there by the one rule, and drawn in the 3D view, which hides
    // nothing: it is the plan view's rule that keeps it out of reach.
    ASSERT_TRUE(katana::cad::isGhost(model, views.entity(views.tin), views.plan));
    ASSERT_FALSE(katana::cad::isGhost(model, views.entity(views.tin), views.model3d));
    EXPECT_TRUE(
        katana::cad::gripsOfSelection(views.document, views.document.selection().ids(), views.plan)
            .empty());
    EXPECT_EQ(katana::cad::gripsOfSelection(views.document, views.document.selection().ids(),
                                            views.model3d)
                  .size(),
              3U) << "a line's two ends and middle, where the view shows it";
    SelectionFilter inPlan;
    inPlan.view = &views.plan;
    SnapRequest request;
    request.cursor = Point2(10.0, 0.0);
    request.aperture = 1.0;
    request.modes = static_cast<katana::cad::SnapModes>(SnapMode::Endpoint);
    request.view = &views.plan;
    const Box2 around(Point2(-1.0, -1.0), Point2(11.0, 1.0));
    for (const auto* index : {static_cast<const katana::geometry::SpatialIndex*>(nullptr),
                              &views.document.spatialIndex()}) {
        EXPECT_FALSE(
            katana::cad::pickEntity(model, Point2(5.0, 0.0), 1.0, inPlan, index).has_value());
        EXPECT_FALSE(katana::cad::snap(model, request, index).has_value());
        EXPECT_TRUE(
            katana::cad::pickInBox(model, around, BoxSelectionMode::Crossing, inPlan, index)
                .empty());
    }
}

TEST(ViewLayerRule, AHiddenLineIsNotHalfOfAnIntersectionSnap)
{
    // An intersection needs two drawn curves. A vertical survey line crosses
    // the tin at (3, 0); in the plan the tin is hidden, so there is nothing to
    // intersect with.
    TwoViews views;
    const EntityId crossing = [&] {
        katana::commands::EntityAttributes on;
        on.layer = kSurvey;
        EXPECT_TRUE(views.document
                        .execute(katana::commands::createLine(Point2(3.0, -5.0), Point2(3.0, 5.0),
                                                              on))
                        .ok());
        return views.document.model().entities.ids().back();
    }();
    ASSERT_NE(crossing, katana::entity::kInvalidEntityId);

    SnapRequest request;
    request.cursor = Point2(3.2, 0.1);
    request.aperture = 1.0;
    request.modes = static_cast<katana::cad::SnapModes>(SnapMode::Intersection);

    request.view = &views.model3d;
    const auto shown = katana::cad::snap(views.document.model(), request);
    ASSERT_TRUE(shown.has_value());
    EXPECT_EQ(shown->mode, SnapMode::Intersection);
    EXPECT_NEAR(shown->point.x, 3.0, 1e-12);
    EXPECT_NEAR(shown->point.y, 0.0, 1e-12);

    request.view = &views.plan;
    EXPECT_FALSE(katana::cad::snap(views.document.model(), request).has_value());
}

TEST(ViewLayerRule, TheThreeDSceneHonoursTheViewItIsBuiltFor)
{
    // Each straight line on a continuous linetype is one DrawLine
    // (tests/cad/test_layer_inheritance.cpp counts the same way).
    const TwoViews views;
    katana::cad::SceneBuilder builder;
    katana::cad::SceneOptions options;
    options.drawGrid = false;

    katana::render::DrawList list;
    options.layers = &views.model3d;
    builder.build(views.document, {}, options, list);
    EXPECT_EQ(list.lines.size(), 4u);

    options.layers = &views.plan;
    builder.build(views.document, {}, options, list);
    EXPECT_EQ(list.lines.size(), 3u) << "the tin line is hidden in this view";

    options.layers = nullptr;
    builder.build(views.document, {}, options, list);
    EXPECT_EQ(list.lines.size(), 4u) << "no view: the document rule";
}

TEST(ViewLayerRule, SceneBoundsFrameOnlyWhatTheViewDraws)
{
    TwoViews views;
    katana::cad::SceneOptions options;
    options.drawGrid = false;

    options.layers = &views.model3d;
    auto box = katana::cad::sceneBounds(views.document, {}, options);
    ASSERT_FALSE(box.empty());
    EXPECT_EQ(box.min.y, 0.0);
    EXPECT_EQ(box.max.y, 30.0);

    options.layers = &views.plan; // the line at y = 0 is hidden here
    box = katana::cad::sceneBounds(views.document, {}, options);
    ASSERT_FALSE(box.empty());
    EXPECT_EQ(box.min.y, 10.0);
    EXPECT_EQ(box.max.y, 30.0);
    EXPECT_EQ(box.min.x, 0.0);
    EXPECT_EQ(box.max.x, 10.0);

    ASSERT_TRUE(views.plan.hide("survey"));
    box = katana::cad::sceneBounds(views.document, {}, options);
    EXPECT_EQ(box.max.y, 20.0);

    views.plan.clear();
    for (const char* name : {"design", kSibling, "designs", "survey"}) {
        ASSERT_TRUE(views.plan.hide(name));
    }
    EXPECT_TRUE(katana::cad::sceneBounds(views.document, {}, options).empty())
        << "a view that hides everything frames nothing";
}

TEST(ViewLayerRule, ASectionCrossingCarriesTheLayerAViewFiltersItBy)
{
    // The section is cut once with the document rule and shared between views
    // (section.cpp); a view that hides a layer drops that layer's crossings
    // when it paints them. That needs the crossing to name the layer of the
    // entity it came from.
    const TwoViews views;
    katana::geometry::Polyline2 across;
    across.vertices = {Point2(5.0, -5.0), Point2(5.0, 35.0)}; // crosses all four lines
    const auto section = katana::cad::extractSection(across, {}, &views.document.model());
    ASSERT_TRUE(section.ok()) << section.error().describe();
    ASSERT_EQ(section->crossings.size(), 4u);

    std::size_t shownInPlan = 0;
    for (const auto& crossing : section->crossings) {
        EXPECT_EQ(crossing.layer, views.entity(crossing.entity).layer);
        if (!views.plan.hides(crossing.layer)) {
            ++shownInPlan;
        }
    }
    EXPECT_EQ(shownInPlan, 3u) << "the plan hides the one on design/surface/tin1";
}

// ---- drawnExtent ------------------------------------------------------------------------

TEST(DrawnExtent, AnEmptyModelHasAnEmptyExtent)
{
    const katana::entity::Model model;
    EXPECT_TRUE(katana::cad::drawnExtent(model, kNoLayerOverrides).empty());
}

TEST(DrawnExtent, TheExtentLeavesOutEverythingTheViewDoesNotDraw)
{
    // Strays far outside the drawing, each hidden a different way; only the
    // locked one counts, because a locked layer still draws.
    TwoViews views;
    const EntityId hiddenEntity = views.line("strays/entity", 1000.0);
    views.line("strays/layer", -1000.0);
    views.line("strays/view", 500.0);
    const EntityId locked = views.line("locked", 40.0);
    ASSERT_TRUE(
        views.document.execute(katana::commands::setEntityVisible({hiddenEntity}, false)).ok());
    views.setLayer("strays/layer", false, false);
    views.setLayer("locked", true, true);
    ASSERT_TRUE(views.model3d.hide("strays/view"));
    ASSERT_NE(locked, katana::entity::kInvalidEntityId);

    // model3d draws y = 0, 10, 20, 30 and the locked line at 40.
    const Box2 in3d = katana::cad::drawnExtent(views.document.model(), views.model3d);
    EXPECT_EQ(in3d, Box2(Point2(0.0, 0.0), Point2(10.0, 40.0)));

    // The plan also hides the tin at y = 0, but shows strays/view at 500.
    const Box2 inPlan = katana::cad::drawnExtent(views.document.model(), views.plan);
    EXPECT_EQ(inPlan, Box2(Point2(0.0, 10.0), Point2(10.0, 500.0)));

    // A view hiding every layer that is drawn frames nothing.
    LayerOverrides nothing;
    for (const auto& name : views.document.model().layers.roots()) {
        nothing.hide(name);
    }
    EXPECT_TRUE(katana::cad::drawnExtent(views.document.model(), nothing).empty());
}

TEST(DrawnExtent, AnArcContributesItsOwnExtentNotItsCircles)
{
    // The upper half of a circle of radius 10 about the origin: from (10, 0)
    // through (0, 10) to (-10, 0). Its own box is [-10, 10] x [0, 10]; its
    // circle's would reach down to y = -10.
    Document document;
    const Arc2 upper{Point2(0.0, 0.0), 10.0, 0.0, std::numbers::pi};
    ASSERT_TRUE(
        document.execute(katana::commands::createArc(upper, document.currentAttributes())).ok());

    const Box2 box = katana::cad::drawnExtent(document.model(), kNoLayerOverrides);
    ASSERT_FALSE(box.empty());
    // sin(pi) is 1.2e-16 in doubles, not 0: the tolerance is that rounding.
    EXPECT_NEAR(box.min.x, -10.0, 1e-12);
    EXPECT_NEAR(box.max.x, 10.0, 1e-12);
    EXPECT_NEAR(box.min.y, 0.0, 1e-12) << "an arc framed by its circle";
    EXPECT_NEAR(box.max.y, 10.0, 1e-12);
}
