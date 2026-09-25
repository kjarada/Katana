// The 3D and elevation view (src/katana_qt/render_view_widget).
//
// The workspace builds a new widget over the same cad::ViewState whenever a
// view changes kind, so what a view keeps across a change is what its state
// keeps. Expected values are worked out by hand from the scene: one line
// from (0, 0) to (100, 50) drawn at the entity elevation 0 has the box
// (0, 0, 0)-(100, 50, 0), and Camera::frame puts the target at its centre,
// (50, 25, 0).

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <memory>
#include <string>
#include <vector>

#include <QApplication>
#include <QMainWindow>
#include <QWheelEvent>

#include "katana/cad/scene.hpp"
#include "katana/cad/section.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::geometry::Point2;
using katana::qt::RenderViewWidget;
using katana::qt::ViewContext;
using katana::qt::ViewWorkspace;
using katana::qt::test::paint;
using katana::qt::test::processEvents;

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

// ---- responsiveness and the look (docs/render.md) -----------------------------------

namespace {

// Pixels of the last paint that are not the view's background.
std::size_t drawnPixels(const RenderViewWidget& view)
{
    const auto& fb = view.framebuffer();
    const katana::render::Rgba background = fb.colorAt(0, 0);
    return static_cast<std::size_t>(std::count_if(
        fb.color().begin(), fb.color().end(), [background](auto c) { return c != background; }));
}

} // namespace

TEST(RenderView, ASelectionClickRebuildsTheOverlayAndNothingUnderIt)
{
    // Every document notification rebuilt the whole scene: 60-115 ms on a
    // 229k-triangle surface to recolour one line. A selection leaves the
    // model's revision where it was, so only the overlay is rebuilt.
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    const auto view = scene.show(state);
    ASSERT_EQ(view->terrainBuilds(), 1);
    ASSERT_EQ(view->entityBuilds(), 1);
    ASSERT_EQ(view->selectionBuilds(), 1);

    scene.document.selection().add(scene.document.model().entities.ids().back());
    scene.document.notifySelectionChanged();
    paint(*view);
    EXPECT_EQ(view->terrainBuilds(), 1);
    EXPECT_EQ(view->entityBuilds(), 1);
    EXPECT_EQ(view->selectionBuilds(), 2);
    EXPECT_EQ(view->sceneLayers().selection.lines.size(), 1u);

    // An edit is a model change: the drawing is rebuilt, the terrain is not.
    ASSERT_TRUE(scene.document
                    .execute(katana::commands::createLine(Point2(0.0, 50.0), Point2(100.0, 0.0)))
                    .ok());
    paint(*view);
    EXPECT_EQ(view->terrainBuilds(), 1);
    EXPECT_EQ(view->entityBuilds(), 2);
}

TEST(RenderView, EightWheelNotchesOutStillShowTheModel)
{
    // The depth range was fixed at the frame, so zooming out pushed the
    // model past the far plane and the view went blank. Without the grid,
    // what is drawn is the line alone.
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    ViewContext context = scene.context();
    context.options.drawGrid = false;
    RenderViewWidget view(context, state);
    view.resize(400, 300);
    paint(view);
    ASSERT_GT(drawnPixels(view), 0u);
    const QPointF centre(200.0, 150.0);
    for (int notch = 0; notch < 8; ++notch) {
        QWheelEvent wheel(centre, centre, QPoint(), QPoint(0, -120), Qt::NoButton,
                          Qt::NoModifier, Qt::NoScrollPhase, false);
        QApplication::sendEvent(&view, &wheel);
    }
    paint(view);
    EXPECT_GT(drawnPixels(view), 0u);
}

TEST(RenderView, AViewFramedBeforeItHasItsSizeFramesAgainAtItsFirstRealSize)
{
    // The window zooms a new view to extents before the dock has laid it
    // out; the frame fitted that size, and a tall thin view then cut the
    // sides off. Until the user moves the camera a resize frames again, so
    // every corner of the drawing's box is inside the 345 x 545 view.
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(900, 120);
    paint(view);
    view.zoomExtents();
    view.resize(345, 545);
    paint(view);
    const auto& box = view.sceneLayers().bounds;
    ASSERT_FALSE(box.empty());
    // The camera counts in device pixels: 345 x 545 at ratio 1.
    const double width = view.framebuffer().width();
    const double height = view.framebuffer().height();
    ASSERT_EQ(state.camera.viewportWidth(), view.framebuffer().width());
    for (int corner = 0; corner < 8; ++corner) {
        const katana::math::Vec3 p((corner & 1) ? box.max.x : box.min.x,
                                   (corner & 2) ? box.max.y : box.min.y,
                                   (corner & 4) ? box.max.z : box.min.z);
        const auto screen = state.camera.project(p);
        ASSERT_TRUE(screen.has_value());
        EXPECT_GE(screen->x, 0.0);
        EXPECT_LE(screen->x, width);
        EXPECT_GE(screen->y, 0.0);
        EXPECT_LE(screen->y, height);
    }
}

TEST(RenderView, TheFrameLabelCountsTheSceneBuildWhenAPaintDidOne)
{
    // The label showed the rasteriser alone, which hid the build a click
    // cost. It is the whole paint, and says so when a build was part of it.
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    QString label;
    view.onFrameStats = [&label](const QString& text) { label = text; };
    view.resize(400, 300);
    paint(view);
    EXPECT_TRUE(label.contains("scene")) << label.toStdString();
    EXPECT_GE(view.lastFrameMilliseconds(), view.lastBuildMilliseconds());
    paint(view);
    EXPECT_FALSE(label.contains("scene")) << "nothing was rebuilt: " << label.toStdString();
}

TEST(RenderView, TheFramebufferIsTheViewsSizeInDevicePixels)
{
    // At the display's own resolution: a 125% display drew at 80% and
    // scaled the image up, beading every 1 px line into 1-2 px steps.
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    const auto view = scene.show(state);
    const double ratio = view->devicePixelRatioF();
    // 1 in the offscreen suite. ctest runs it again at QT_SCALE_FACTOR=1.25
    // (qt_render_view_at_125_percent), where 400 x 300 must be 500 x 375 -
    // and where a ratio Qt did not apply would make the test prove nothing.
    if (const QString factor = qEnvironmentVariable("QT_SCALE_FACTOR"); !factor.isEmpty()) {
        EXPECT_DOUBLE_EQ(ratio, factor.toDouble());
    }
    RecordProperty("devicePixelRatio", std::to_string(ratio));
    RecordProperty("framebuffer", std::to_string(view->framebuffer().width()) + "x" +
                                      std::to_string(view->framebuffer().height()));
    EXPECT_EQ(view->framebuffer().width(), static_cast<int>(std::lround(400 * ratio)));
    EXPECT_EQ(view->framebuffer().height(), static_cast<int>(std::lround(300 * ratio)));
    EXPECT_EQ(state.camera.viewportWidth(), view->framebuffer().width());
}

TEST(RenderView, ANewDrawingLeavesNoSurfaceOfThePreviousOneInTheThreeDView)
{
    // After File > New the previous drawing's TIN was still in the 3D view.
    // Surfaces are lent to the views by the window, not kept in the document,
    // and the document's notification for the new drawing marks only the
    // entities dirty - the terrain layer was never built again. The window
    // now empties its surfaces and tells the workspace, as done here, and a
    // section cut from those surfaces goes with them.
    Document document;
    QMainWindow window;
    auto* views = new ViewWorkspace(document, &window);
    window.setCentralWidget(views);
    window.resize(800, 600);
    window.show();
    processEvents();

    katana::terrain::TinInput input;
    input.points = {{0.0, 0.0, 10.0}, {100.0, 0.0, 12.0}, {100.0, 100.0, 14.0},
                    {0.0, 100.0, 11.0}, {50.0, 50.0, 13.0}};
    auto built = katana::terrain::buildTin(input);
    ASSERT_TRUE(built.ok()) << built.error().describe();
    std::vector<katana::cad::SceneSurface> surfaces(1);
    surfaces.front().name = "Ground";
    surfaces.front().surface = &built->surface;
    views->setSurfaces(&surfaces);

    const ViewState& model = views->openView(ViewKind::Model3D);
    processEvents();
    RenderViewWidget* render = views->renderView(model.id);
    ASSERT_NE(render, nullptr);
    paint(*render);
    ASSERT_FALSE(render->sceneLayers().terrain.empty()) << "the surface was never drawn";

    katana::cad::Section section;
    section.length = 100.0;
    ASSERT_TRUE(views->showSection(section));
    const ViewState* cut = views->viewSet().mostRecent(ViewKind::Section);
    ASSERT_NE(cut, nullptr);
    ASSERT_TRUE(cut->section.has_value());

    // What MainWindow::newDocument does: the new drawing, then the window's
    // own surfaces emptied and the workspace told.
    document.newDocument();
    surfaces.clear();
    views->drawingReplaced();
    paint(*render);

    EXPECT_TRUE(render->sceneLayers().terrain.empty())
        << "the previous drawing's surface is still in the 3D view";
    EXPECT_FALSE(cut->section.has_value()) << "the previous drawing's section is still shown";
}
