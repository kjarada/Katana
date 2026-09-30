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
#include <optional>
#include <sstream>
#include <string>
#include <vector>

#include <QApplication>
#include <QDir>
#include <QImage>
#include <QMainWindow>
#include <QMouseEvent>
#include <QWheelEvent>

#include "katana/cad/scene.hpp"
#include "katana/cad/scene_zoom.hpp"
#include "katana/cad/section.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
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
    RecordProperty("devicePixelRatio", katana::core::formatExactReal(ratio));
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

// ---- zooming with the wheel (docs/render.md, "Zooming towards the cursor") ------------
//
// The wheel anchored every notch on the plane through the orbit target, facing
// the eye - not on what is drawn under the cursor. Over ground beyond that
// plane the eye closed in on the anchor and the zoom stalled: each notch took
// 13% of what was left of the way, so after about 30 notches nothing on screen
// moved, and the pan, scaled by that collapsed distance, died with it. Over
// ground nearer than the plane the eye went through the ground and the view
// went blank. The cases below find the ground under the cursor by hand - the
// ray through the cursor's pixel met with the plane z = 10 - and never ask the
// view what it zoomed towards.

namespace {

constexpr double kGroundHeight = 10.0;
// One wheel notch in eighths of a degree, and the magnification it stands for
// (2x in five notches).
constexpr int kNotch = 120;
constexpr double kPerNotch = 1.15;
// The view's background (render_view_widget.cpp, kBackground).
constexpr katana::render::Rgba kViewBackground = katana::render::rgba(28, 30, 36);

// Flat ground 200 m square at z = 10, from an 11 x 11 grid of points 20 m
// apart, and nothing else: the datum is its floor, so the navigation grid
// stands in the same plane. Framed from the south-west, as a 3D view opens,
// the target is the middle of the ground, (100, 100, 10), ON the ground: the
// ground above the middle of the view lies beyond the target's plane and the
// ground below the middle nearer than it.
struct FlatGround {
    Document document;
    ViewSet views;
    std::optional<katana::terrain::TinBuildResult> built;
    std::vector<katana::cad::SceneSurface> surfaces;

    FlatGround()
    {
        katana::terrain::TinInput input;
        for (int j = 0; j <= 10; ++j) {
            for (int i = 0; i <= 10; ++i) {
                input.points.emplace_back(20.0 * i, 20.0 * j, kGroundHeight);
            }
        }
        auto tin = katana::terrain::buildTin(input);
        EXPECT_TRUE(tin.ok()) << tin.error().describe();
        if (tin.ok()) {
            built = std::move(*tin);
            surfaces.resize(1);
            surfaces.front().name = "Ground";
            surfaces.front().surface = &built->surface;
        }
    }
    FlatGround(const FlatGround&) = delete;
    FlatGround& operator=(const FlatGround&) = delete;

    [[nodiscard]] ViewContext context()
    {
        ViewContext context;
        context.document = &document;
        context.surfaces = &surfaces;
        return context;
    }
};

// Where the ray through the cursor meets the ground; nothing when it does
// not. The cursor is in the widget's logical pixels, the camera counts in its
// device pixels.
std::optional<katana::math::Vec3> groundUnder(const katana::render::Camera& camera,
                                              QPointF cursor, double ratio)
{
    const katana::math::Ray ray = camera.rayThroughPixel(cursor.x() * ratio, cursor.y() * ratio);
    if (ray.direction.z == 0.0) {
        return std::nullopt;
    }
    const double t = (kGroundHeight - ray.origin.z) / ray.direction.z;
    if (!(t > 0.0)) {
        return std::nullopt;
    }
    return ray.at(t);
}

// How far in front of the eye `point` is along the view direction. A pixel's
// footprint there is proportional to it, so the ratio of two depths of one
// point is how much a zoom magnified it.
double depthOf(const katana::render::Camera& camera, const katana::math::Vec3& point)
{
    return (point - camera.eye()).dot(camera.forward());
}

void turnWheel(QWidget& view, QPointF at, int eighths)
{
    QWheelEvent wheel(at, view.mapToGlobal(at), QPoint(), QPoint(0, eighths), Qt::NoButton,
                      Qt::NoModifier, Qt::NoScrollPhase, false);
    QApplication::sendEvent(&view, &wheel);
}

void middleDrag(QWidget& view, QPointF from, QPointF to)
{
    QMouseEvent press(QEvent::MouseButtonPress, from, view.mapToGlobal(from), Qt::MiddleButton,
                      Qt::MiddleButton, Qt::NoModifier);
    QApplication::sendEvent(&view, &press);
    QMouseEvent move(QEvent::MouseMove, to, view.mapToGlobal(to), Qt::NoButton, Qt::MiddleButton,
                     Qt::NoModifier);
    QApplication::sendEvent(&view, &move);
    QMouseEvent release(QEvent::MouseButtonRelease, to, view.mapToGlobal(to), Qt::MiddleButton,
                        Qt::NoButton, Qt::NoModifier);
    QApplication::sendEvent(&view, &release);
}

// Pixels of the last paint that are anything but the view's background.
std::size_t paintedPixels(const RenderViewWidget& view)
{
    const auto& fb = view.framebuffer();
    return static_cast<std::size_t>(std::count_if(
        fb.color().begin(), fb.color().end(), [](auto c) { return c != kViewBackground; }));
}

// With KATANA_RENDER_VIEW_IMAGES set to a directory, saves what the view last
// painted there as <name>.png: a zoom looked at rather than read in numbers.
void saveForLooking(const RenderViewWidget& view, const std::string& name)
{
    const QString directory = qEnvironmentVariable("KATANA_RENDER_VIEW_IMAGES");
    const auto& fb = view.framebuffer();
    if (directory.isEmpty() || fb.empty()) {
        return;
    }
    QDir().mkpath(directory);
    const QImage image(reinterpret_cast<const uchar*>(fb.color().data()), fb.width(), fb.height(),
                       fb.width() * static_cast<int>(sizeof(katana::render::Rgba)),
                       QImage::Format_ARGB32);
    (void)image.copy().save(QDir(directory).filePath(QString::fromStdString(name) + ".png"));
}

std::string twoDigits(int n) { return (n < 10 ? "0" : "") + std::to_string(n); }

} // namespace

TEST(RenderView, EveryWheelNotchMagnifiesTheGroundUnderTheCursorByTheSameFactor)
{
    // Above the middle of the view the ground lies beyond the target's plane.
    // Anchored on that plane, the magnification of the ground under the
    // cursor tended to depth / (depth - distance) and stopped there. Every
    // notch must magnify it by 1.15, and the same point of the ground must
    // stay under the cursor.
    FlatGround scene;
    ASSERT_TRUE(scene.built.has_value());
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(400, 300);
    paint(view);
    const double ratio = view.devicePixelRatioF();
    // ctest runs this again at QT_SCALE_FACTOR=1.25
    // (qt_render_view_zoom_at_125_percent), where the zoom must pick at the
    // cursor times 1.25; a ratio Qt did not apply would prove nothing there.
    if (const QString factor = qEnvironmentVariable("QT_SCALE_FACTOR"); !factor.isEmpty()) {
        ASSERT_DOUBLE_EQ(ratio, factor.toDouble());
    }
    const QPointF cursor(200.0, 90.0);
    const auto start = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(start.has_value());
    ASSERT_GT(depthOf(state.camera, *start), 1.1 * state.camera.distance())
        << "the ground under the cursor is not beyond the target's plane, so this proves nothing";
    saveForLooking(view, "zoom_far_00");

    const double first = depthOf(state.camera, *start);
    double depth = first;
    double worstGain = kPerNotch;
    double drift = 0.0;
    std::ostringstream gains;
    for (int notch = 1; notch <= 40; ++notch) {
        turnWheel(view, cursor, kNotch);
        const auto under = groundUnder(state.camera, cursor, ratio);
        ASSERT_TRUE(under.has_value()) << "no ground under the cursor after notch " << notch;
        const double now = depthOf(state.camera, *under);
        const double gain = depth / now;
        if (std::abs(gain - kPerNotch) > std::abs(worstGain - kPerNotch)) {
            worstGain = gain;
        }
        drift = std::max(drift, (*under - *start).length());
        if (notch == 1 || notch % 10 == 0) {
            gains << " notch " << notch << ": x" << gain << " (x" << first / now << " in all);";
            paint(view);
            saveForLooking(view, "zoom_far_" + twoDigits(notch));
        }
        depth = now;
    }
    EXPECT_NEAR(worstGain, kPerNotch, 1e-9) << gains.str();
    const double expected = std::pow(kPerNotch, 40.0);
    EXPECT_NEAR(first / depth, expected, 1e-6 * expected) << gains.str();
    // A micrometre, on ground some 300 m from where the eye started.
    EXPECT_LT(drift, 1e-6) << "the ground under the cursor slid by " << drift << " m;"
                           << gains.str();
}

TEST(RenderView, WheelNotchesOverTheNearGroundNeverTakeTheEyeThroughIt)
{
    // Below the middle of the view the ground is nearer than the target's
    // plane: the old anchor lay under the ground, so the eye went through it
    // and the view was blank from then on - the ground behind the eye and
    // nothing in front of it. The eye must stay above the ground, the ground
    // under the cursor in front of it and magnified by 1.15 a notch, and the
    // view must go on drawing it.
    FlatGround scene;
    ASSERT_TRUE(scene.built.has_value());
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(400, 300);
    paint(view);
    const double ratio = view.devicePixelRatioF();
    const QPointF cursor(200.0, 250.0);
    const auto start = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(start.has_value());
    ASSERT_LT(depthOf(state.camera, *start), 0.9 * state.camera.distance())
        << "the ground under the cursor is not nearer than the target's plane, so this proves "
           "nothing";
    saveForLooking(view, "zoom_near_00");

    int eyeUnder = 0;
    int blank = 0;
    int lost = 0;
    double depth = depthOf(state.camera, *start);
    double worstGain = kPerNotch;
    for (int notch = 1; notch <= 40; ++notch) {
        turnWheel(view, cursor, kNotch);
        paint(view);
        if (eyeUnder == 0 && !(state.camera.eye().z > kGroundHeight)) {
            eyeUnder = notch;
        }
        if (blank == 0 && paintedPixels(view) == 0) {
            blank = notch;
        }
        const auto under = groundUnder(state.camera, cursor, ratio);
        if (!under) {
            lost = lost == 0 ? notch : lost;
        } else if (lost == 0) {
            const double now = depthOf(state.camera, *under);
            if (std::abs(depth / now - kPerNotch) > std::abs(worstGain - kPerNotch)) {
                worstGain = depth / now;
            }
            depth = now;
        }
        if (notch % 5 == 0) {
            saveForLooking(view, "zoom_near_" + twoDigits(notch));
        }
    }
    EXPECT_EQ(eyeUnder, 0) << "the eye went under the ground at notch " << eyeUnder;
    EXPECT_EQ(blank, 0) << "the view drew nothing but its background from notch " << blank;
    EXPECT_EQ(lost, 0) << "no ground under the cursor from notch " << lost;
    EXPECT_NEAR(worstGain, kPerNotch, 1e-9);
}

TEST(RenderView, AfterZoomingInADragStillCarriesTheGroundUnderTheCursorWithIt)
{
    // A pan moves the target's plane by the drag, scaled by the camera's
    // distance. Stalled, the distance had shrunk to centimetres while the
    // ground stayed far off, and a 100 px drag moved the ground under the
    // cursor a tenth of a pixel. Zoomed towards the ground, the target's
    // plane is the ground's, and the ground under the cursor follows the drag
    // exactly: 100 logical pixels, which the camera counts in device pixels.
    FlatGround scene;
    ASSERT_TRUE(scene.built.has_value());
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(400, 300);
    paint(view);
    const double ratio = view.devicePixelRatioF();
    const QPointF cursor(200.0, 90.0);
    for (int notch = 0; notch < 30; ++notch) {
        turnWheel(view, cursor, kNotch);
    }
    const auto under = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(under.has_value());
    const auto before = state.camera.project(*under);
    ASSERT_TRUE(before.has_value());

    middleDrag(view, cursor, cursor + QPointF(100.0, 0.0));
    const auto after = state.camera.project(*under);
    ASSERT_TRUE(after.has_value());
    EXPECT_NEAR(after->x - before->x, 100.0 * ratio, 1e-6 * 100.0 * ratio);
    EXPECT_NEAR(after->y - before->y, 0.0, 1e-6 * 100.0 * ratio);
}

TEST(RenderView, ANotchOutAfterZoomingInShrinksTheGroundUnderTheCursorByTheSameFactor)
{
    // Stalled, the eye sat almost on the old anchor and a notch out moved it
    // back 15% of almost nothing: the ground under the cursor barely shrank.
    // A notch out must undo a notch in, and as many notches out as went in
    // must bring the eye back to where it started.
    FlatGround scene;
    ASSERT_TRUE(scene.built.has_value());
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(400, 300);
    paint(view);
    const double ratio = view.devicePixelRatioF();
    const QPointF cursor(200.0, 90.0);
    const katana::math::Vec3 eye = state.camera.eye();
    const auto start = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(start.has_value());
    const double startDepth = depthOf(state.camera, *start);

    for (int notch = 0; notch < 30; ++notch) {
        turnWheel(view, cursor, kNotch);
    }
    const auto in = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(in.has_value());
    const double inDepth = depthOf(state.camera, *in);
    turnWheel(view, cursor, -kNotch);
    const auto out = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(out.has_value());
    EXPECT_NEAR(depthOf(state.camera, *out) / inDepth, kPerNotch, 1e-9);

    for (int notch = 1; notch < 30; ++notch) {
        turnWheel(view, cursor, -kNotch);
    }
    EXPECT_NEAR((state.camera.eye() - eye).length(), 0.0, 1e-9 * startDepth);
    const auto back = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(back.has_value());
    EXPECT_NEAR(depthOf(state.camera, *back), startDepth, 1e-9 * startDepth);
}

// ---- off the model: the background round it in a docked view ----------------------------
//
// The window's 3D dock is tall and narrow, about 330 x 520, and a scene framed
// to its width leaves two thirds of it background: the datum beyond the grid,
// where nothing is drawn. There the wheel still anchored on the target's
// plane, so below the model the eye dived under the ground and above it the
// eye converged on a point in the air, and either way the whole view was
// blank from about the twentieth notch on (docs/render.md, "Zooming towards
// the cursor").

namespace {

// Nothing of the view's own layers - terrain, drawing, the grid's plane - is
// under the cursor.
bool nothingDrawnUnder(const RenderViewWidget& view, const katana::render::Camera& camera,
                       QPointF cursor, double ratio)
{
    return !katana::cad::pickDrawnPoint(view.sceneLayers(), camera, cursor.x() * ratio,
                                        cursor.y() * ratio)
                .has_value();
}

// How deep the scene's box reaches along the view: its furthest corner.
double reachOf(const RenderViewWidget& view, const katana::render::Camera& camera)
{
    const katana::math::AABB box = katana::cad::sceneDepthBox(view.sceneLayers());
    double reach = -1.0e300;
    for (int corner = 0; corner < 8; ++corner) {
        const katana::math::Vec3 p((corner & 1) != 0 ? box.max.x : box.min.x,
                                   (corner & 2) != 0 ? box.max.y : box.min.y,
                                   (corner & 4) != 0 ? box.max.z : box.min.z);
        reach = std::max(reach, depthOf(camera, p));
    }
    return reach;
}

void leftDrag(QWidget& view, QPointF from, QPointF to)
{
    QMouseEvent press(QEvent::MouseButtonPress, from, view.mapToGlobal(from), Qt::LeftButton,
                      Qt::LeftButton, Qt::NoModifier);
    QApplication::sendEvent(&view, &press);
    QMouseEvent move(QEvent::MouseMove, to, view.mapToGlobal(to), Qt::NoButton, Qt::LeftButton,
                     Qt::NoModifier);
    QApplication::sendEvent(&view, &move);
    QMouseEvent release(QEvent::MouseButtonRelease, to, view.mapToGlobal(to), Qt::LeftButton,
                        Qt::NoButton, Qt::NoModifier);
    QApplication::sendEvent(&view, &release);
}

} // namespace

TEST(RenderView, InADockSizedViewTheWheelBelowTheModelZoomsTowardsTheDatumAndBackOutAgain)
{
    // Below the model the ground is the datum, the plane the grid stands on,
    // nearer than the target's plane: the old anchor lay under it, and the
    // eye went under the ground and stayed there. The eye must stay above
    // it, the datum's point under the cursor must divide its depth by 1.15 a
    // notch, and as many notches out must bring the view back as it was.
    // Not that the view never goes blank: the model lies in the plane the
    // zoom closes on, which the eye comes to see edge on - from h above it
    // ground L away is atan(h / L) below the horizon, and the top of this
    // view is 35.26 - 22.5 = 12.76 degrees below it - so the model leaves
    // the top of the view as the eye nears the empty ground it went towards.
    FlatGround scene;
    ASSERT_TRUE(scene.built.has_value());
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(330, 520);
    paint(view);
    const std::size_t painted = paintedPixels(view);
    ASSERT_GT(painted, 1000u);
    const double ratio = view.devicePixelRatioF();
    const QPointF cursor(165.0, 510.0);
    ASSERT_TRUE(nothingDrawnUnder(view, state.camera, cursor, ratio))
        << "the cursor is over the model, so this proves nothing";
    const katana::math::Vec3 eye = state.camera.eye();
    const auto start = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(start.has_value());
    double depth = depthOf(state.camera, *start);
    ASSERT_LT(depth, 0.9 * state.camera.distance())
        << "the datum is not nearer than the target's plane, so the old anchor did not dive";
    saveForLooking(view, "dock_below_00");
    for (int notch = 1; notch <= 40; ++notch) {
        turnWheel(view, cursor, kNotch);
        ASSERT_GT(state.camera.eye().z, kGroundHeight)
            << "the eye is under the datum at notch " << notch;
        const auto under = groundUnder(state.camera, cursor, ratio);
        ASSERT_TRUE(under.has_value()) << notch;
        const double now = depthOf(state.camera, *under);
        ASSERT_NEAR(depth / now, kPerNotch, 1e-9) << "notch " << notch;
        depth = now;
        if (notch % 10 == 0) {
            paint(view);
            saveForLooking(view, "dock_below_" + twoDigits(notch));
        }
    }
    for (int notch = 1; notch <= 40; ++notch) {
        turnWheel(view, cursor, -kNotch);
    }
    EXPECT_NEAR((state.camera.eye() - eye).length(), 0.0, 1e-9 * state.camera.distance());
    paint(view);
    saveForLooking(view, "dock_below_back");
    EXPECT_NEAR(static_cast<double>(paintedPixels(view)), static_cast<double>(painted),
                0.01 * static_cast<double>(painted));
}

TEST(RenderView, InADockSizedViewTheWheelAboveTheModelKeepsMagnifyingTheSceneAsFarAsItReaches)
{
    // Above the model the ray meets the datum beyond everything the scene
    // reaches, so the zoom goes as deep as the scene reaches: the furthest
    // corner of its box divides its depth by 1.15 a notch, and everything
    // drawn, all of it nearer, grows at least as fast. Anchored on the
    // target's plane, that corner lay beyond the anchor and stalled.
    FlatGround scene;
    ASSERT_TRUE(scene.built.has_value());
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(330, 520);
    paint(view);
    const double ratio = view.devicePixelRatioF();
    const QPointF cursor(165.0, 15.0);
    ASSERT_TRUE(nothingDrawnUnder(view, state.camera, cursor, ratio))
        << "the cursor is over the model, so this proves nothing";
    double reach = reachOf(view, state.camera);
    const auto datum = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(!datum || depthOf(state.camera, *datum) > reach)
        << "the datum is within the scene's reach, so this is the case below the model";
    ASSERT_GT(reach, 1.1 * state.camera.distance())
        << "the scene reaches no further than the target's plane, so nothing stalled";
    saveForLooking(view, "dock_above_00");
    for (int notch = 1; notch <= 40; ++notch) {
        turnWheel(view, cursor, kNotch);
        const double now = reachOf(view, state.camera);
        ASSERT_NEAR(reach / now, kPerNotch, 1e-9) << "notch " << notch;
        reach = now;
        ASSERT_GT(state.camera.eye().z, kGroundHeight) << notch;
        if (notch % 10 == 0) {
            paint(view);
            saveForLooking(view, "dock_above_" + twoDigits(notch));
        }
    }
}

// ---- the pan and the orbit after a zoom ----------------------------------------------------

TEST(RenderView, AfterAZoomTheOrbitTurnsAboutThePointOfTheViewAxisAsDeepAsWhatTheZoomWentTowards)
{
    // The zoom moves the pivot to the depth of what the cursor points at
    // (Camera::setPivotDepth): on the view axis, as deep as the ground under
    // the cursor. An orbit then turns about that point and keeps it.
    FlatGround scene;
    ASSERT_TRUE(scene.built.has_value());
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(400, 300);
    paint(view);
    const double ratio = view.devicePixelRatioF();
    const QPointF cursor(200.0, 90.0);
    for (int notch = 0; notch < 10; ++notch) {
        turnWheel(view, cursor, kNotch);
    }
    const auto under = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(under.has_value());
    const double depth = depthOf(state.camera, *under);
    EXPECT_NEAR(state.camera.distance(), depth, 1e-9 * depth);
    const katana::math::Vec3 pivot = state.camera.eye() + state.camera.forward() * depth;
    EXPECT_NEAR((state.camera.target() - pivot).length(), 0.0, 1e-9 * depth);

    leftDrag(view, QPointF(200.0, 150.0), QPointF(300.0, 170.0));
    EXPECT_NEAR((state.camera.target() - pivot).length(), 0.0, 1e-9 * depth);
    EXPECT_NEAR(state.camera.distance(), depth, 1e-9 * depth);
}

TEST(RenderView, AfterAZoomAPanStartedElsewhereMovesTheGroundThereInProportionToThePivotsDepth)
{
    // A pan slides the camera across the view by the drag, counted at the
    // pivot's depth: the ground at the depth the zoom went to follows the
    // drag one to one, and ground at depth d moves pivot / d of it - as a
    // perspective pan always did, the pivot being where the zoom left it.
    FlatGround scene;
    ASSERT_TRUE(scene.built.has_value());
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(400, 300);
    paint(view);
    const double ratio = view.devicePixelRatioF();
    for (int notch = 0; notch < 10; ++notch) {
        turnWheel(view, QPointF(200.0, 90.0), kNotch);
    }
    const QPointF from(200.0, 250.0);
    const auto ground = groundUnder(state.camera, from, ratio);
    ASSERT_TRUE(ground.has_value());
    const double d = depthOf(state.camera, *ground);
    const double pivot = state.camera.distance();
    ASSERT_LT(d, 0.8 * pivot) << "the ground there is as deep as the pivot, so this proves little";
    const auto before = state.camera.project(*ground);
    ASSERT_TRUE(before.has_value());
    middleDrag(view, from, from + QPointF(100.0, 0.0));
    const auto after = state.camera.project(*ground);
    ASSERT_TRUE(after.has_value());
    const double expected = 100.0 * ratio * pivot / d;
    EXPECT_NEAR(after->x - before->x, expected, 1e-6 * expected);
    EXPECT_NEAR(after->y - before->y, 0.0, 1e-6 * expected);
}

// ---- where the zoom stops ------------------------------------------------------------------

TEST(RenderView, AZoomHeldAtItsLimitSaysSoOnTheStatusLineAndNothingBefore)
{
    // The zoom stops where the view could no longer be drawn exactly
    // (cad::minimumApproach): here the near plane's limit, 1e-7 / 1e-3 =
    // 0.1 mm, the double arithmetic's bound on ground a few hundred metres
    // from the origin being nanometres. From a depth D the eye reaches it at
    // n = ln(D / 1e-4) / ln 1.15 notches, so ground between 1e-4 x 1.15^100 =
    // 117 m and 1e-4 x 1.15^130 = 8.1 km deep is not reached in 100 notches
    // and is in 130. A wheel that stopped without a word read as the fault
    // the zoom once had, so the view says why it stopped - and only then.
    FlatGround scene;
    ASSERT_TRUE(scene.built.has_value());
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(400, 300);
    std::vector<QString> said;
    view.onStatus = [&said](const QString& text) { said.push_back(text); };
    paint(view);
    const double ratio = view.devicePixelRatioF();
    const QPointF cursor(200.0, 90.0);
    const auto ground = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(ground.has_value());
    const double depth = depthOf(state.camera, *ground);
    ASSERT_GT(depth, 1e-4 * std::pow(kPerNotch, 100.0)) << "the limit comes within 100 notches";
    ASSERT_LT(depth, 1e-4 * std::pow(kPerNotch, 130.0)) << "the limit is not reached in 130";
    const auto limitSaid = [&said] {
        return std::ranges::count_if(said, [](const QString& text) {
            return text == QStringLiteral("3D view: zoomed in as far as it can be drawn exactly");
        });
    };
    for (int notch = 0; notch < 100; ++notch) {
        turnWheel(view, cursor, kNotch);
    }
    EXPECT_EQ(limitSaid(), 0) << "the view said it was held before it was";
    for (int notch = 100; notch < 130; ++notch) {
        turnWheel(view, cursor, kNotch);
    }
    EXPECT_GT(limitSaid(), 0) << "the zoom stopped without a word";
    const auto under = groundUnder(state.camera, cursor, ratio);
    ASSERT_TRUE(under.has_value());
    EXPECT_NEAR(depthOf(state.camera, *under), 1e-4, 1e-6 * 1e-4);
    // A notch out moves away from the limit and says nothing more.
    const auto before = limitSaid();
    turnWheel(view, cursor, -kNotch);
    EXPECT_EQ(limitSaid(), before);
}
