// Nested layers (PLAN.MD Phase 05 / Phase 09).
//
// The properties worth pinning are the ones a tree gets wrong quietly: a
// "starts with" test that treats "designs/x" as a child of "design", an
// inheritance rule that stops at the first parent instead of walking to the
// root, and a rename that half-applies and leaves entities pointing at layers
// that are no longer there.

#include <gtest/gtest.h>

#include <algorithm>

#include "katana/entity/layer_path.hpp"
#include "katana/entity/tables.hpp"

using katana::core::ErrorCode;
using katana::entity::isLayerUnder;
using katana::entity::Layer;
using katana::entity::LayerDatabase;
using katana::entity::layerAncestors;
using katana::entity::layerDepth;
using katana::entity::layerLeaf;
using katana::entity::layerParent;
using katana::entity::rewriteLayerPrefix;
using katana::entity::validateLayerPath;

namespace {

LayerDatabase jobTree()
{
    LayerDatabase layers;
    for (const char* name : {"design/surface/tin1", "design/surface/tin2", "design/road/kerb",
                             "asbuilt/road/kerb", "asbuilt/services/water"}) {
        Layer layer;
        layer.name = name;
        EXPECT_TRUE(layers.add(layer).ok()) << name;
    }
    return layers;
}

} // namespace

// ---- path arithmetic -------------------------------------------------------------

TEST(LayerPath, SplitsIntoLeafParentAndAncestors)
{
    EXPECT_EQ(layerLeaf("design/surface/tin1"), "tin1");
    EXPECT_EQ(layerLeaf("0"), "0");
    EXPECT_EQ(layerParent("design/surface/tin1"), "design/surface");
    EXPECT_EQ(layerParent("0"), "");
    EXPECT_EQ(layerDepth("design/surface/tin1"), 3u);
    EXPECT_EQ(layerDepth("0"), 1u);

    const auto ancestors = layerAncestors("design/surface/tin1");
    ASSERT_EQ(ancestors.size(), 2u);
    EXPECT_EQ(ancestors[0], "design");
    EXPECT_EQ(ancestors[1], "design/surface");
    EXPECT_TRUE(layerAncestors("0").empty()) << "a layer is not its own ancestor";
}

TEST(LayerPath, DescendantTestComparesWholeSegmentsNotTextPrefixes)
{
    // THE bug this exists to prevent: hiding "design" must not hide "designs".
    EXPECT_TRUE(isLayerUnder("design/surface/tin1", "design"));
    EXPECT_TRUE(isLayerUnder("design/surface", "design"));
    EXPECT_TRUE(isLayerUnder("design", "design")) << "a layer is under itself";
    EXPECT_FALSE(isLayerUnder("designs/surface", "design"));
    EXPECT_FALSE(isLayerUnder("design", "design/surface"));
    EXPECT_FALSE(isLayerUnder("asbuilt/road", "design"));
    EXPECT_TRUE(isLayerUnder("anything", "")) << "everything is under the root";
}

TEST(LayerPath, RejectsNamesThatWouldBreakTheTreeOrAFileName)
{
    EXPECT_TRUE(validateLayerPath("0").ok());
    EXPECT_TRUE(validateLayerPath("design/surface/tin1").ok());
    EXPECT_TRUE(validateLayerPath("As-Built 2024/Road (E)").ok()) << "ordinary names still work";

    EXPECT_FALSE(validateLayerPath("").ok());
    EXPECT_FALSE(validateLayerPath("/design").ok());
    EXPECT_FALSE(validateLayerPath("design/").ok());
    EXPECT_FALSE(validateLayerPath("design//surface").ok());
    EXPECT_FALSE(validateLayerPath("design/ /surface").ok());
    EXPECT_FALSE(validateLayerPath("design/ surface").ok()) << "leading space";
    EXPECT_FALSE(validateLayerPath("design/surface ").ok()) << "trailing space";
    // A layer name reaches a file name whenever something is exported per
    // layer, and ".." there is a path traversal.
    EXPECT_FALSE(validateLayerPath("design/../etc").ok());
    EXPECT_FALSE(validateLayerPath(".").ok());
    EXPECT_FALSE(validateLayerPath(std::string("a\nb")).ok()) << "control characters";

    std::string tooDeep = "a";
    for (int i = 0; i < 20; ++i) {
        tooDeep += "/a";
    }
    EXPECT_FALSE(validateLayerPath(tooDeep).ok());
    EXPECT_FALSE(validateLayerPath(std::string(600, 'x')).ok());
}

TEST(LayerPath, RewritingAPrefixMovesOnlyRealDescendants)
{
    EXPECT_EQ(rewriteLayerPrefix("design/surface/tin1", "design", "asbuilt"),
              "asbuilt/surface/tin1");
    EXPECT_EQ(rewriteLayerPrefix("design", "design", "asbuilt"), "asbuilt");
    EXPECT_EQ(rewriteLayerPrefix("designs/x", "design", "asbuilt"), "designs/x")
        << "a text prefix that is not a segment must be left alone";
}

// ---- the database ----------------------------------------------------------------

TEST(LayerTree, AddingANestedLayerCreatesItsAncestors)
{
    LayerDatabase layers;
    Layer layer;
    layer.name = "design/surface/tin1";
    ASSERT_TRUE(layers.add(layer).ok());

    // Real layers, not placeholders: each must be findable and settable.
    ASSERT_NE(layers.find("design"), nullptr);
    ASSERT_NE(layers.find("design/surface"), nullptr);
    ASSERT_NE(layers.find("design/surface/tin1"), nullptr);
    EXPECT_EQ(layers.find("design")->name, "design");
    EXPECT_EQ(layers.size(), 4u) << "three levels plus the default layer 0";
}

TEST(LayerTree, AddingASiblingReusesTheExistingAncestors)
{
    LayerDatabase layers = jobTree();
    // 0, design, design/surface, .../tin1, .../tin2, design/road, .../kerb,
    // asbuilt, asbuilt/road, .../kerb, asbuilt/services, .../water.
    EXPECT_EQ(layers.size(), 12u);
}

TEST(LayerTree, AddingALayerThatAlreadyExistsIsRefusedButEnsureIsNot)
{
    LayerDatabase layers;
    Layer layer;
    layer.name = "design/surface";
    ASSERT_TRUE(layers.add(layer).ok());

    const auto again = layers.add(layer);
    ASSERT_FALSE(again.ok());
    EXPECT_EQ(again.error().code, ErrorCode::AlreadyExists);

    // ensure() is what an importer wants: make the path exist, do not complain.
    EXPECT_TRUE(layers.ensure("design/surface").ok());
    EXPECT_TRUE(layers.ensure("design/surface/tin9").ok());
    EXPECT_TRUE(layers.contains("design/surface/tin9"));
    EXPECT_FALSE(layers.ensure("design//bad").ok()) << "ensure still validates";
}

TEST(LayerTree, ChildrenAndRootsListOnlyTheImmediateLevel)
{
    const LayerDatabase layers = jobTree();

    const auto roots = layers.roots();
    ASSERT_EQ(roots.size(), 3u);
    EXPECT_EQ(roots[0], "0");
    EXPECT_EQ(roots[1], "asbuilt");
    EXPECT_EQ(roots[2], "design");

    const auto designChildren = layers.children("design");
    ASSERT_EQ(designChildren.size(), 2u);
    EXPECT_EQ(designChildren[0], "design/road");
    EXPECT_EQ(designChildren[1], "design/surface");

    const auto surfaceChildren = layers.children("design/surface");
    ASSERT_EQ(surfaceChildren.size(), 2u);
    EXPECT_EQ(surfaceChildren[0], "design/surface/tin1");

    EXPECT_TRUE(layers.children("design/surface/tin1").empty());
    EXPECT_TRUE(layers.children("nope").empty());
    EXPECT_TRUE(layers.hasChildren("design"));
    EXPECT_FALSE(layers.hasChildren("design/surface/tin1"));
    EXPECT_FALSE(layers.hasChildren("0")) << "the default layer has no children";
}

TEST(LayerTree, SubtreeIsPreOrderAndStopsAtTheBranch)
{
    const LayerDatabase layers = jobTree();
    const auto under = layers.subtree("design");
    ASSERT_EQ(under.size(), 6u);
    EXPECT_EQ(under[0], "design") << "the branch includes itself, first";
    EXPECT_EQ(under[1], "design/road");
    EXPECT_EQ(under[2], "design/road/kerb");
    EXPECT_EQ(under[3], "design/surface");
    EXPECT_EQ(under[4], "design/surface/tin1");
    EXPECT_EQ(under[5], "design/surface/tin2");

    EXPECT_TRUE(layers.subtree("missing").empty());
    // Nothing from the other branch leaks in even though both have road/kerb.
    for (const auto& name : under) {
        EXPECT_EQ(name.rfind("asbuilt", 0), std::string::npos);
    }
}

TEST(LayerTree, VisibilityInheritsAllTheWayToTheRoot)
{
    LayerDatabase layers = jobTree();
    ASSERT_TRUE(layers.effectivelyVisible("design/surface/tin1"));

    // Switch off the GRANDparent: a rule that only checked the immediate
    // parent would still call the leaf visible.
    Layer design = *layers.find("design");
    design.visible = false;
    ASSERT_TRUE(layers.update(design).ok());

    EXPECT_FALSE(layers.effectivelyVisible("design"));
    EXPECT_FALSE(layers.effectivelyVisible("design/surface"));
    EXPECT_FALSE(layers.effectivelyVisible("design/surface/tin1"));
    // And the other branch is untouched.
    EXPECT_TRUE(layers.effectivelyVisible("asbuilt/road/kerb"));
    // The layer's OWN flag is unchanged; only the effective answer differs.
    EXPECT_TRUE(layers.find("design/surface/tin1")->visible);
}

TEST(LayerTree, LockInheritsAndAMissingLayerIsNeitherVisibleNorEditable)
{
    LayerDatabase layers = jobTree();
    EXPECT_FALSE(layers.effectivelyLocked("design/surface/tin1"));

    Layer surface = *layers.find("design/surface");
    surface.locked = true;
    ASSERT_TRUE(layers.update(surface).ok());
    EXPECT_TRUE(layers.effectivelyLocked("design/surface/tin1"));
    EXPECT_FALSE(layers.effectivelyLocked("design/road/kerb"));

    // A layer that is gone: not drawn, not editable. Either answer the other
    // way round would let an orphaned entity be quietly shown or changed.
    EXPECT_FALSE(layers.effectivelyVisible("design/gone"));
    EXPECT_TRUE(layers.effectivelyLocked("design/gone"));
}

TEST(LayerTree, RemovingABranchNodeIsRefusedRatherThanOrphaningIt)
{
    LayerDatabase layers = jobTree();
    const auto refused = layers.remove("design/surface");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(layers.contains("design/surface/tin1")) << "nothing may have been deleted";

    // A leaf is fine.
    EXPECT_TRUE(layers.remove("design/surface/tin2").ok());
    EXPECT_FALSE(layers.contains("design/surface/tin2"));
}

TEST(LayerTree, RemovingASubtreeReturnsItDeepestFirstSoUndoCanReplayIt)
{
    LayerDatabase layers = jobTree();
    const auto removed = layers.removeSubtree("design");
    ASSERT_TRUE(removed.ok()) << removed.error().describe();
    ASSERT_EQ(removed->size(), 6u);
    // Deepest first: replaying the list backwards recreates parents before
    // children, which is exactly what an undo has to do.
    EXPECT_EQ(removed->front().name, "design/surface/tin2") << "deepest and last, first";
    EXPECT_EQ(removed->back().name, "design");

    EXPECT_FALSE(layers.contains("design"));
    EXPECT_FALSE(layers.contains("design/surface/tin1"));
    EXPECT_TRUE(layers.contains("asbuilt/road/kerb")) << "the other branch survives";

    // Replaying it backwards restores the branch exactly.
    for (auto it = removed->rbegin(); it != removed->rend(); ++it) {
        ASSERT_TRUE(layers.add(*it).ok()) << it->name;
    }
    EXPECT_TRUE(layers.contains("design/surface/tin1"));
}

TEST(LayerTree, TheDefaultLayerCannotBeRemovedOrRenamed)
{
    LayerDatabase layers = jobTree();
    EXPECT_FALSE(layers.remove("0").ok());
    EXPECT_FALSE(layers.removeSubtree("0").ok());
    EXPECT_FALSE(layers.renameSubtree("0", "zero").ok());
    EXPECT_TRUE(layers.contains("0"));
}

TEST(LayerTree, RenamingABranchMovesEveryDescendantAndReportsTheMapping)
{
    LayerDatabase layers = jobTree();
    const auto mapping = layers.renameSubtree("design", "proposed");
    ASSERT_TRUE(mapping.ok()) << mapping.error().describe();
    ASSERT_EQ(mapping->size(), 6u);

    for (const auto& [before, after] : *mapping) {
        EXPECT_EQ(after, "proposed" + before.substr(std::string("design").size()));
        EXPECT_FALSE(layers.contains(before)) << before << " should have moved";
        EXPECT_TRUE(layers.contains(after)) << after << " should exist";
    }
    EXPECT_TRUE(layers.contains("proposed/surface/tin1"));
    EXPECT_TRUE(layers.contains("asbuilt/road/kerb")) << "the other branch is untouched";
}

TEST(LayerTree, RenamingIntoAnotherBranchCreatesTheNewParents)
{
    LayerDatabase layers = jobTree();
    const auto mapping = layers.renameSubtree("design/surface", "archive/2024/surface");
    ASSERT_TRUE(mapping.ok()) << mapping.error().describe();

    EXPECT_TRUE(layers.contains("archive"));
    EXPECT_TRUE(layers.contains("archive/2024"));
    EXPECT_TRUE(layers.contains("archive/2024/surface/tin1"));
    EXPECT_FALSE(layers.contains("design/surface"));
    EXPECT_TRUE(layers.contains("design/road/kerb")) << "the rest of design stays";
}

TEST(LayerTree, ACollidingRenameChangesNothingAtAll)
{
    LayerDatabase layers = jobTree();
    // asbuilt/road already exists, so moving design/road onto it collides.
    const auto refused = layers.renameSubtree("design/road", "asbuilt/road");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::AlreadyExists);

    // The whole point: a failure must leave the tree exactly as it was, not
    // half moved.
    EXPECT_TRUE(layers.contains("design/road"));
    EXPECT_TRUE(layers.contains("design/road/kerb"));
    EXPECT_TRUE(layers.contains("asbuilt/road"));
    EXPECT_TRUE(layers.contains("asbuilt/road/kerb"));
    EXPECT_EQ(layers.size(), 12u);
}

TEST(LayerTree, ALayerCannotBeMovedInsideItself)
{
    LayerDatabase layers = jobTree();
    const auto refused = layers.renameSubtree("design", "design/old");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(layers.contains("design/surface/tin1"));

    EXPECT_FALSE(layers.renameSubtree("design", "design//bad").ok()) << "target is validated";
    EXPECT_FALSE(layers.renameSubtree("missing", "elsewhere").ok());
}

TEST(LayerTree, RenamingToTheSameNameIsANoOpRatherThanACollision)
{
    LayerDatabase layers = jobTree();
    const auto mapping = layers.renameSubtree("design", "design");
    ASSERT_TRUE(mapping.ok()) << mapping.error().describe();
    EXPECT_TRUE(mapping->empty());
    EXPECT_TRUE(layers.contains("design/surface/tin1"));
}

TEST(LayerTree, NamesAreOrderedSoThatListingThemIsAlreadyATreeWalk)
{
    const LayerDatabase layers = jobTree();
    const auto names = layers.names();
    ASSERT_TRUE(std::is_sorted(names.begin(), names.end()));
    // A parent always immediately precedes its first child, which is what lets
    // a tree be built in one pass instead of by repeated lookup.
    for (std::size_t i = 1; i < names.size(); ++i) {
        const auto parent = layerParent(names[i]);
        if (!parent.empty()) {
            const auto found = std::find(names.begin(), names.end(), std::string(parent));
            ASSERT_NE(found, names.end()) << names[i] << " has no parent row";
            EXPECT_LT(found - names.begin(), static_cast<std::ptrdiff_t>(i));
        }
    }
}
