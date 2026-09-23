// Visibility and lock inherit DOWN the layer tree (PLAN.MD section 10, 5.1): a
// layer switched off or locked by an ancestor must be undrawn, unpickable and
// uneditable exactly as though it had been switched off or locked itself.
//
// Until 2026-09-23 only the layer panel honoured that: it greyed the child,
// while the viewport, the 3D scene, sections, snapping, picking and the
// commands all looked at the entity's own layer and nothing above it (audit
// MOD-01, MOD-02, MOD-03, REN-03, REN-04). Each test below hides or locks the
// PARENT and asserts on an entity on the CHILD.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/scene.hpp"
#include "katana/cad/section.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/commands/entity_commands.hpp"

using katana::cad::Document;
using katana::entity::EntityId;
using katana::entity::Layer;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Vec2;

namespace {

constexpr const char* kParent = "design";
constexpr const char* kChild = "design/surface/tin1";

// A document with one line, (0,0)-(10,0), on the grandchild layer. A Document
// is neither copied nor moved, so each test owns one and this fills it.
struct Drawing {
    Document document;
    EntityId line = katana::entity::kInvalidEntityId;

    Drawing()
    {
        Layer child;
        child.name = kChild;
        EXPECT_TRUE(document.execute(katana::commands::createLayer(child)).ok());
        katana::commands::EntityAttributes on;
        on.layer = kChild;
        EXPECT_TRUE(
            document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(10.0, 0.0), on))
                .ok());
        line = document.model().entities.ids().back();
    }
};

void setParent(Document& document, bool visible, bool locked)
{
    Layer parent = *document.model().layers.find(kParent);
    parent.visible = visible;
    parent.locked = locked;
    ASSERT_TRUE(document.execute(katana::commands::updateLayer(parent)).ok());
}

const katana::entity::Entity& lineOf(const Drawing& drawing)
{
    return *drawing.document.model().entities.find(drawing.line);
}

} // namespace

TEST(CadLayerInheritance, TheChildsOwnFlagsAreClearSoOnlyTheParentCanBeResponsible)
{
    // The premise of every test below: the child layer itself is visible and
    // unlocked, and adding it created the parent and middle layer.
    const Drawing drawing;
    const auto& layers = drawing.document.model().layers;
    ASSERT_NE(layers.find(kParent), nullptr);
    ASSERT_NE(layers.find("design/surface"), nullptr);
    EXPECT_TRUE(layers.find(kChild)->visible);
    EXPECT_FALSE(layers.find(kChild)->locked);
    EXPECT_TRUE(katana::cad::isDrawn(drawing.document.model(), lineOf(drawing), katana::cad::kNoLayerOverrides));
    EXPECT_TRUE(katana::cad::isSelectable(drawing.document.model(), lineOf(drawing), katana::cad::kNoLayerOverrides));
}

TEST(CadLayerInheritance, HidingTheParentHidesTheGrandchildFromDrawingPickingAndSnapping)
{
    Drawing drawing;
    setParent(drawing.document, false, false);
    const auto& model = drawing.document.model();

    EXPECT_FALSE(katana::cad::isDrawn(model, lineOf(drawing), katana::cad::kNoLayerOverrides));
    EXPECT_FALSE(katana::cad::isSelectable(model, lineOf(drawing), katana::cad::kNoLayerOverrides));
    EXPECT_FALSE(katana::cad::pickEntity(model, Point2(5.0, 0.0), 1.0).has_value());
    EXPECT_TRUE(katana::cad::pickInBox(model, Box2(Point2(-1, -1), Point2(11, 1)),
                                       katana::cad::BoxSelectionMode::Window)
                    .empty());
    katana::cad::SnapRequest request;
    request.cursor = Point2(10.0, 0.0);
    request.aperture = 1.0;
    EXPECT_FALSE(katana::cad::snap(model, request).has_value())
        << "a hidden line's endpoint is not something to snap to";

    // And switching the parent back on restores all of it: the inherited
    // state is recomputed when the parent changes, not frozen at creation.
    setParent(drawing.document, true, false);
    EXPECT_TRUE(katana::cad::isDrawn(drawing.document.model(), lineOf(drawing), katana::cad::kNoLayerOverrides));
    EXPECT_TRUE(
        katana::cad::pickEntity(drawing.document.model(), Point2(5.0, 0.0), 1.0).has_value());
}

TEST(CadLayerInheritance, LockingTheParentProtectsTheGrandchildFromEveryEdit)
{
    // The audit's case: lock "design", SELECT ALL, ERASE - the line on
    // design/surface/tin1 was erased.
    Drawing drawing;
    setParent(drawing.document, true, true);

    // Still drawn - a locked layer reads as background, it does not vanish -
    // but not selectable.
    EXPECT_TRUE(katana::cad::isDrawn(drawing.document.model(), lineOf(drawing), katana::cad::kNoLayerOverrides));
    EXPECT_FALSE(katana::cad::isSelectable(drawing.document.model(), lineOf(drawing), katana::cad::kNoLayerOverrides));

    // Every edit is refused by the model, whatever path asked for it, and the
    // refusal says which layer holds the lock.
    const auto erase = drawing.document.execute(katana::commands::deleteEntities({drawing.line}));
    ASSERT_FALSE(erase.ok());
    EXPECT_EQ(erase.error().code, katana::core::ErrorCode::CommandRejected);
    EXPECT_NE(erase.error().message.find("design"), std::string::npos) << erase.error().message;
    EXPECT_FALSE(
        drawing.document.execute(katana::commands::moveEntities({drawing.line}, Vec2(1.0, 0.0)))
            .ok());
    EXPECT_NE(drawing.document.model().entities.find(drawing.line), nullptr);

    // Nothing new may be put on it either.
    katana::commands::EntityAttributes on;
    on.layer = kChild;
    EXPECT_FALSE(drawing.document
                     .execute(katana::commands::createLine(Point2(0, 1), Point2(1, 1), on))
                     .ok());

    setParent(drawing.document, true, false);
    EXPECT_TRUE(drawing.document.execute(katana::commands::deleteEntities({drawing.line})).ok());
}

TEST(CadLayerInheritance, TheThreeDSceneLeavesOutWhatTheParentHides)
{
    Drawing drawing;
    katana::cad::SceneBuilder builder;
    katana::cad::SceneOptions options;
    options.drawGrid = false;
    katana::render::DrawList shown;
    builder.build(drawing.document, {}, options, shown);
    ASSERT_EQ(shown.lines.size(), 1u);

    setParent(drawing.document, false, false);
    katana::render::DrawList hidden;
    builder.build(drawing.document, {}, options, hidden);
    EXPECT_TRUE(hidden.lines.empty());

    // Nor does it frame around it: a hidden stray used to pull zoom-extents.
    EXPECT_TRUE(katana::cad::sceneBounds(drawing.document, {}, options).empty());
}

TEST(CadLayerInheritance, ASectionDoesNotCrossWhatTheParentHides)
{
    Drawing drawing;
    katana::geometry::Polyline2 across;
    across.vertices = {Point2(5.0, -5.0), Point2(5.0, 5.0)}; // crosses the line at (5, 0)

    const auto before = katana::cad::extractSection(across, {}, &drawing.document.model());
    ASSERT_TRUE(before.ok()) << before.error().describe();
    EXPECT_EQ(before->crossings.size(), 1u);

    setParent(drawing.document, false, false);
    const auto after = katana::cad::extractSection(across, {}, &drawing.document.model());
    ASSERT_TRUE(after.ok()) << after.error().describe();
    EXPECT_TRUE(after->crossings.empty());
}

TEST(CadLayerInheritance, MovingABranchUnderAHiddenParentHidesIt)
{
    // A rename changes a subtree's ancestors, so its inherited state must be
    // recomputed from its NEW parents.
    Drawing drawing;
    Layer archive;
    archive.name = "archive";
    archive.visible = false;
    ASSERT_TRUE(drawing.document.execute(katana::commands::createLayer(archive)).ok());

    auto& model = drawing.document.model();
    auto layers = model.layers; // a copy to rename, so the document is untouched
    const auto moved = layers.renameSubtree("design/surface", "archive/surface");
    ASSERT_TRUE(moved.ok()) << moved.error().describe();
    EXPECT_FALSE(layers.effectivelyVisible("archive/surface/tin1"));
    EXPECT_TRUE(layers.effectivelyVisible("design"));
}
