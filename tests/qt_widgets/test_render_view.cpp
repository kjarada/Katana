// The 3D and elevation view (src/katana_qt/render_view_widget).
//
// The workspace builds a new widget over the same cad::ViewState whenever a
// view changes kind, so what a view keeps across a change is what its state
// keeps. Expected values are worked out by hand from the scene: one line
// from (0, 0) to (100, 50) drawn at the entity elevation 0 has the box
// (0, 0, 0)-(100, 50, 0), and Camera::frame puts the target at its centre,
// (50, 25, 0).

#include <gtest/gtest.h>

#include <memory>

#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::geometry::Point2;
using katana::qt::RenderViewWidget;
using katana::qt::ViewContext;
using katana::qt::test::paint;

namespace {

struct OneLine {
    Document document;
    ViewSet views;

    OneLine()
    {
        EXPECT_TRUE(
            document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(100.0, 50.0)))
                .ok());
    }

    [[nodiscard]] ViewContext context()
    {
        ViewContext context;
        context.document = &document;
        return context;
    }

    // A widget over `state` as the workspace builds one, seen once.
    std::unique_ptr<RenderViewWidget> show(ViewState& state)
    {
        auto view = std::make_unique<RenderViewWidget>(context(), state);
        view->resize(400, 300);
        paint(*view);
        return view;
    }
};

void expectTarget(const ViewState& state, double x, double y, double z)
{
    EXPECT_DOUBLE_EQ(state.camera.target().x, x);
    EXPECT_DOUBLE_EQ(state.camera.target().y, y);
    EXPECT_DOUBLE_EQ(state.camera.target().z, z);
}

} // namespace

TEST(RenderView, AThreeDViewTurnedIntoASectionAndBackKeepsItsTargetAndDistance)
{
    // ViewState::cameraKind keeps the orbit's ANGLE through a section (cad's
    // ViewSet test), but the rebuilt widget framed the scene at its first
    // paint, which moved the target back to the middle of the drawing and
    // the distance back to "all of it" - a zoom onto one culvert was lost.
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    {
        const auto first = scene.show(state);
        expectTarget(state, 50.0, 25.0, 0.0); // the first paint framed the line
        // The user zooms in towards a point off the middle and pans.
        first->camera().dollyAtPixel(0.25, 100.0, 80.0);
        first->camera().panPixels(40.0, -25.0);
    }
    const katana::render::Camera zoomed = state.camera;
    ASSERT_NE(zoomed.target().x, 50.0) << "the zoom moved the target, or this proves nothing";

    ASSERT_TRUE(scene.views.setKind(state.id, ViewKind::Section).ok());
    ASSERT_TRUE(scene.views.setKind(state.id, ViewKind::Model3D).ok());
    const auto second = scene.show(state);

    EXPECT_EQ(state.camera.target().x, zoomed.target().x);
    EXPECT_EQ(state.camera.target().y, zoomed.target().y);
    EXPECT_EQ(state.camera.target().z, zoomed.target().z);
    EXPECT_EQ(state.camera.distance(), zoomed.distance());
    EXPECT_EQ(state.camera.orthographicHeight(), zoomed.orthographicHeight());
    EXPECT_EQ(state.camera.azimuth(), zoomed.azimuth());
    EXPECT_EQ(state.camera.elevation(), zoomed.elevation());
}

TEST(RenderView, AViewWhoseCameraWasPointedAfreshFramesTheDrawingAgain)
{
    // 3D -> Elevation points the camera at the front (ViewSet::setKind), so
    // the elevation's first paint must frame the drawing: a target kept from
    // the 3D zoom would leave the elevation looking at a corner of it.
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    {
        const auto first = scene.show(state);
        first->camera().dollyAtPixel(0.25, 100.0, 80.0);
        first->camera().panPixels(40.0, -25.0);
    }
    ASSERT_TRUE(scene.views.setKind(state.id, ViewKind::Elevation).ok());
    const auto elevation = scene.show(state);
    expectTarget(state, 50.0, 25.0, 0.0);
}

TEST(RenderView, AnEditReachesAThreeDViewAlreadyOpen)
{
    // Audit QT-05: the 3D view was rebuilt only when something else happened
    // to invalidate it, so a line drawn in plan never appeared in it and an
    // undone one stayed. A second line inside the first one's box leaves the
    // grid as it was, so the open view must now build what a view opened
    // afresh on the same drawing builds, and after an undo what it built
    // before.
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    const auto open = scene.show(state);
    const std::size_t before = open->lastSceneLineCount();

    ASSERT_TRUE(scene.document
                    .execute(katana::commands::createLine(Point2(0.0, 50.0), Point2(100.0, 0.0)))
                    .ok());
    paint(*open);
    ViewState& other = scene.views.add(ViewKind::Model3D);
    const auto fresh = scene.show(other);
    EXPECT_GT(fresh->lastSceneLineCount(), before) << "the second line added nothing to draw";
    EXPECT_EQ(open->lastSceneLineCount(), fresh->lastSceneLineCount());

    ASSERT_TRUE(scene.document.undo().ok());
    paint(*open);
    EXPECT_EQ(open->lastSceneLineCount(), before);
}

TEST(RenderView, AnEmptyDrawingSaysThereIsNothingToShow)
{
    Document document;
    ViewSet views;
    ViewState& state = views.add(ViewKind::Model3D);
    ViewContext context;
    context.document = &document;
    RenderViewWidget view(context, state);
    view.resize(400, 300);
    paint(view);
    EXPECT_TRUE(view.sceneEmpty());
    EXPECT_TRUE(view.emptyMessageShown());
}

TEST(RenderView, ADrawingWhoseLayersAreAllHiddenIsNotCalledEmpty)
{
    // The one line is on layer "0". Hidden in this view, or in the document,
    // the scene is the grid alone - but the drawing is not empty, and telling
    // the user to draw or import something would be wrong.
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    const auto view = scene.show(state);
    ASSERT_FALSE(view->sceneEmpty());
    EXPECT_FALSE(view->emptyMessageShown());

    ASSERT_TRUE(state.layers.hide("0"));
    view->invalidateScene(); // the view's own layers are not the document's to announce
    paint(*view);
    ASSERT_TRUE(view->sceneEmpty());
    EXPECT_FALSE(view->emptyMessageShown());

    ASSERT_TRUE(state.layers.show("0"));
    katana::entity::Layer hidden;
    hidden.name = "0";
    hidden.visible = false;
    ASSERT_TRUE(scene.document.execute(katana::commands::updateLayer(hidden)).ok());
    paint(*view);
    ASSERT_TRUE(view->sceneEmpty());
    EXPECT_FALSE(view->emptyMessageShown());
}
