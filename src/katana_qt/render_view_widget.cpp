#include "render_view_widget.hpp"

#include "theme.hpp"
#include "view_focus.hpp"

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <vector>

#include <QKeyEvent>
#include <QLinearGradient>
#include <QMouseEvent>
#include <QPainter>
#include <QTimer>
#include <QWheelEvent>

#if defined(KATANA_HAS_GPU)
#include "gpu/gpu_scene_view.hpp"
#endif

namespace katana::qt {

namespace {

// Radians per pixel of drag. A full turn in roughly 800 pixels, which is about
// a screen width and matches what every other CAD package feels like.
constexpr double kOrbitPerPixel = 0.008;
// One wheel notch is 120 eighths of a degree; 1.15 per notch gives a
// comfortable 2x in five notches.
constexpr double kZoomPerNotch = 1.15;

// The ground behind the model, and behind the empty view's message.
const QColor kBackground(28, 30, 36);

#if defined(KATANA_HAS_GPU)
// Rule 4 of gpu::chooseRenderer: a GPU view that failed once is not tried
// again behind the user's back, in this view or any other of the session.
// The GUI thread's alone, as every widget is.
bool gpuFailedThisSession = false;

// The layers in cad::renderLayers' order, with its depth rules (scene.hpp:
// the grid, the edges and the selection's casing test depth and write none).
constexpr std::size_t kGridLayer = 0;
constexpr std::size_t kEdgesLayer = 2;
constexpr std::size_t kEntitiesLayer = 3;
constexpr std::size_t kSelectionCasingLayer = 4;
constexpr std::size_t kSelectionLayer = 5;
#endif

} // namespace

// Paints the legend and the empty message over the GPU view, which covers
// the widget and so hides anything the widget paints itself. QRhiWidget
// composes the widgets above it, so a transparent one on top shows. It takes
// no input: the GPU view under it has the mouse.
class RenderViewWidget::Overlay final : public QWidget {
  public:
    explicit Overlay(RenderViewWidget& owner) : QWidget(&owner), owner_(owner)
    {
        setAttribute(Qt::WA_TransparentForMouseEvents);
        setAttribute(Qt::WA_NoSystemBackground);
        setFocusPolicy(Qt::NoFocus);
    }

  protected:
    void paintEvent(QPaintEvent* /*event*/) override
    {
        QPainter painter(this);
        owner_.paintOverlays(painter);
    }

  private:
    RenderViewWidget& owner_;
};

RenderViewWidget::RenderViewWidget(ViewContext context, katana::cad::ViewState& state,
                                   QWidget* parent)
    : QWidget(parent), context_(context), state_(state)
{
    context_.options.layers = &state_.layers;
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    // The whole widget is painted from the framebuffer every time, so Qt need
    // not clear it first: that is a full-window fill saved per frame.
    setAttribute(Qt::WA_OpaquePaintEvent, true);
    setMinimumSize(40, 40);
    // A widget rebuilt over a camera that is already framed - the view went
    // to a section and came back - must not frame it again at its first
    // paint: that reset the target and the distance the user had zoomed to.
    // cameraKind is checked as well so that a state whose flag outlived a
    // reconfiguration still frames.
    framed_ = state_.cameraFramed && state_.cameraKind == state_.kind;
    listenTo(context_.document);
    activateOnFocus(*this, [this] {
        if (onActivated) {
            onActivated();
        }
    });
    makeGpuView();
}

double RenderViewWidget::sceneScale() const { return gpuView_ != nullptr ? 1.0 : pixelRatio(); }

void RenderViewWidget::requestFrame()
{
#if defined(KATANA_HAS_GPU)
    if (gpuView_ != nullptr) {
        gpuView_->update();
        if (overlay_ != nullptr) {
            overlay_->update();
        }
        return;
    }
#endif
    update();
}

void RenderViewWidget::makeGpuView()
{
#if defined(KATANA_HAS_GPU)
    gpu::RendererDecision decision;
    auto view = gpu::makeGpuSceneViewIfChosen(
        camera(), gpu::currentRendererEnvironment(false, gpuFailedThisSession), &decision, this);
    rendererReason_ = QString::fromStdString(decision.reason);
    if (!view) {
        return;
    }
    gpuView_ = view.release(); // a child of this widget: Qt owns it from here
    gpuView_->setGeometry(rect());
    // The software view's own test (the constructor): a camera already framed
    // for this kind of view is kept through the first frame.
    gpuView_->setCameraFramed(framed_);
    gpuView_->setOrbitAllowed(state_.kind != katana::cad::ViewKind::Elevation);
    gpuView_->installEventFilter(this);
    // Focus given to this view reaches the GPU view, which has the keys; and
    // focus arriving there activates the view as it does this widget
    // (view_focus.hpp, which matches the exact widget focused).
    setFocusProxy(gpuView_);
    activateOnFocus(*gpuView_, [this] {
        if (onActivated) {
            onActivated();
        }
    });
    gpuView_->onActivated = [this] {
        if (onActivated) {
            onActivated();
        }
    };
    // The host frames: it knows what "the scene" is, and frames what it built.
    gpuView_->onZoomExtents = [this] { zoomExtents(); };
    gpuView_->onPrepareFrame = [this](katana::render::Camera& frameCamera) {
        prepareGpuFrame(frameCamera);
    };
    gpuView_->onFrameStats = [this](const QString& text) {
        if (!onFrameStats) {
            return;
        }
        QString line = QString("%1  %2")
                           .arg(QString::fromLatin1(katana::cad::toString(state_.kind)), text);
        if (rebuiltSinceStats_) {
            line += QString(" (scene %1 ms)").arg(lastBuildMs_, 0, 'f', 1);
            rebuiltSinceStats_ = false;
        }
        onFrameStats(line);
    };
    gpuView_->onRenderFailed = [this](const QString& why) {
        gpuFailedThisSession = true;
        // Not from inside the failing widget's own call: after it returns.
        QTimer::singleShot(0, this, [this, why] { dropGpuView(why); });
    };
    overlay_ = new Overlay(*this);
    overlay_->setGeometry(rect());
    gpuView_->show();
    overlay_->raise();
    overlay_->show();
    // Widths in logical pixels from now on (sceneScale), so the scene is
    // rebuilt for the GPU on its first frame.
    terrainDirty_ = entitiesDirty_ = selectionDirty_ = true;
#endif
}

void RenderViewWidget::dropGpuView(const QString& reason)
{
#if defined(KATANA_HAS_GPU)
    if (gpuView_ == nullptr) {
        return;
    }
    gpuView_->removeEventFilter(this);
    setFocusProxy(nullptr);
    gpuView_->hide();
    gpuView_->deleteLater();
    gpuView_ = nullptr;
    delete overlay_;
    overlay_ = nullptr;
    rendererReason_ = QStringLiteral("the GPU renderer failed (%1); the software renderer draws")
                          .arg(reason);
    if (onStatus) {
        onStatus(QStringLiteral("3D view: %1").arg(rendererReason_));
    }
    // The software view counts the camera in device pixels, and builds widths
    // for them: rebuildIfNeeded sees the scale change.
    gpuLayersStale_ = true;
    update();
#else
    (void)reason;
#endif
}

bool RenderViewWidget::eventFilter(QObject* watched, QEvent* event)
{
#if defined(KATANA_HAS_GPU)
    if (watched == gpuView_) {
        switch (event->type()) {
        case QEvent::MouseButtonPress:
        case QEvent::Wheel:
        case QEvent::KeyPress:
            // The user is moving the camera now, as in mousePressEvent and
            // wheelEvent: a resize no longer frames the scene again.
            refitOnResize_ = false;
            break;
        default:
            break;
        }
    }
#endif
    return QWidget::eventFilter(watched, event);
}

void RenderViewWidget::prepareGpuFrame(katana::render::Camera& frameCamera)
{
#if defined(KATANA_HAS_GPU)
    if (gpuView_ == nullptr) {
        return;
    }
    const bool rebuilt = terrainDirty_ || entitiesDirty_ || selectionDirty_;
    rebuildIfNeeded();
    if (rebuilt) {
        rebuiltSinceStats_ = true;
        if (overlay_ != nullptr) {
            overlay_->update(); // the legend and the empty message follow the scene
        }
    }
    // The rebuild may have framed the host's camera (a first scene with
    // something in it): the frame is drawn through the camera as it is now.
    const int width = frameCamera.viewportWidth();
    const int height = frameCamera.viewportHeight();
    frameCamera = camera();
    frameCamera.setViewportSize(width, height);
    gpuView_->setOrbitAllowed(state_.kind != katana::cad::ViewKind::Elevation);

    // As cad::renderLayers: the depth range fitted to what is drawn, every
    // frame, and the edges faded for how large the triangles are now.
    katana::math::AABB depthBox = layers_.bounds;
    depthBox.expand(layers_.grid.bounds());
    depthBox.expand(layers_.selection.bounds()); // its ghosts (SceneLayers::bounds)
    frameCamera.fitDepthRange(depthBox);
    std::vector<float> applied;
    applied.reserve(layers_.edgeRuns.size());
    for (const auto& run : layers_.edgeRuns) {
        applied.push_back(run.applied);
    }
    const bool drawEdges = katana::cad::SceneBuilder::fadeEdges(layers_, frameCamera);
    bool edgesChanged = drawEdges != gpuEdgesShown_;
    for (std::size_t i = 0; i < applied.size() && !edgesChanged; ++i) {
        edgesChanged = applied[i] != layers_.edgeRuns[i].applied;
    }
    sendLayersToGpu(drawEdges, edgesChanged);
#else
    (void)frameCamera;
#endif
}

void RenderViewWidget::sendLayersToGpu(bool drawEdges, bool edgesChanged)
{
#if defined(KATANA_HAS_GPU)
    if (gpuView_ == nullptr) {
        return;
    }
    static const katana::render::DrawList kNothing;
    const katana::render::DrawList& edges = drawEdges ? layers_.edges : kNothing;
    if (gpuLayersStale_) {
        const std::array<gpu::LayerSource, 6> layers{{
            {&layers_.grid, false},
            {&layers_.terrain, true},
            {&edges, false},
            {&layers_.entities, true},
            {&layers_.selectionCasing, false},
            {&layers_.selection, true},
        }};
        gpuView_->setLayers(layers);
    } else {
        // An edit of the drawing over a large surface re-sends the drawing
        // alone, against the origin the terrain set, as the software view
        // rebuilds only what changed.
        if (gpuDrawingStale_) {
            gpuView_->updateLayer(kGridLayer, layers_.grid);
            gpuView_->updateLayer(kEntitiesLayer, layers_.entities);
        }
        if (gpuDrawingStale_ || gpuSelectionStale_) {
            gpuView_->updateLayer(kSelectionCasingLayer, layers_.selectionCasing);
            gpuView_->updateLayer(kSelectionLayer, layers_.selection);
        }
        if (edgesChanged) {
            gpuView_->updateLayer(kEdgesLayer, edges);
        }
    }
    gpuLayersStale_ = false;
    gpuDrawingStale_ = false;
    gpuSelectionStale_ = false;
    gpuEdgesShown_ = drawEdges;
#else
    (void)drawEdges;
    (void)edgesChanged;
#endif
}

void RenderViewWidget::setContext(const ViewContext& context)
{
    context_ = context;
    context_.options.layers = &state_.layers;
    listenTo(context_.document);
    invalidateScene();
}

void RenderViewWidget::listenTo(katana::cad::Document* document)
{
    if (document == listenedDocument_) {
        return;
    }
    documentListener_.reset();
    listenedDocument_ = document;
    if (document != nullptr) {
        // Every edit, undo, layer change and selection change: the 3D view
        // was rebuilt only when something else happened to invalidate it, so
        // a line drawn in plan never appeared here and an erased one stayed
        // (audit QT-05). The rebuild is lazy - this marks the scene dirty and
        // asks for one paint, and Qt folds a burst of changes into that paint.
        documentListener_ = document->addListener([this] { documentChanged(); });
        builtRevision_ = document->modelRevision();
    }
}

void RenderViewWidget::documentChanged()
{
    // The revision moves for every command, undo, redo and opened drawing and
    // for nothing else (Document::modelRevision), so a notification that
    // leaves it where it was is a selection or a current-layer change: only
    // the overlay depends on that. A click used to rebuild the whole scene -
    // 60 to 115 ms on a 229k-triangle surface - to recolour one line.
    if (context_.document != nullptr && context_.document->modelRevision() != builtRevision_) {
        entitiesDirty_ = true;
    }
    selectionDirty_ = true;
    requestFrame();
}

void RenderViewWidget::invalidateScene()
{
    terrainDirty_ = true;
    entitiesDirty_ = true;
    selectionDirty_ = true;
    requestFrame();
}

void RenderViewWidget::setStandardView(katana::render::StandardView view)
{
    camera().setStandardView(view);
    requestFrame();
}

void RenderViewWidget::setProjection(katana::render::Projection projection)
{
    camera().setProjection(projection);
    requestFrame();
}

void RenderViewWidget::setVerticalExaggeration(double factor)
{
    if (!std::isfinite(factor) || factor <= 0.0) {
        return;
    }
    context_.options.verticalExaggeration = factor;
    // The datum is the middle of what is drawn so exaggerating does not also
    // launch the model off the top of the screen. Not only when there are
    // surfaces: a scene of meshes alone needs its datum as much.
    if (context_.document != nullptr) {
        katana::cad::SceneOptions flat = context_.options;
        flat.verticalExaggeration = 1.0;
        const auto box = katana::cad::sceneBounds(*context_.document, surfaces(), flat, meshes());
        context_.options.exaggerationDatum = box.empty() ? 0.0 : box.center().z;
    }
    invalidateScene();
    zoomExtents();
}

void RenderViewWidget::rebuildIfNeeded()
{
    if (context_.document == nullptr) {
        return;
    }
    // Line widths and point sizes follow the display the view is on - for
    // the software framebuffer; the GPU view scales logical ones itself.
    const auto ratio = static_cast<float>(sceneScale());
    if (ratio != context_.options.pixelScale) {
        context_.options.pixelScale = ratio;
        terrainDirty_ = entitiesDirty_ = selectionDirty_ = true;
    }
    // The view's own ghost switch (ViewState::selectionGhosts), which only
    // the selection overlay reads.
    if (state_.selectionGhosts != context_.options.selectionGhosts) {
        context_.options.selectionGhosts = state_.selectionGhosts;
        selectionDirty_ = true;
    }
    if (!terrainDirty_ && !entitiesDirty_ && !selectionDirty_) {
        return;
    }
    const auto started = std::chrono::steady_clock::now();
    // Each layer is built from the ones before it: the drawing drapes on the
    // terrain and takes its datum from it, the grid stands on that datum and
    // spans the bounds of both.
    const bool gridDirty = terrainDirty_ || entitiesDirty_;
    const bool terrainRebuilt = terrainDirty_;
    if (terrainDirty_) {
        builder_.buildTerrain(surfaces(), meshes(), context_.options, layers_);
        ++terrainBuilds_;
        entitiesDirty_ = true;
    }
    if (entitiesDirty_) {
        builder_.buildEntities(*context_.document, surfaces(), context_.options, layers_);
        builtRevision_ = context_.document->modelRevision();
        ++entityBuilds_;
        selectionDirty_ = true;
    }
    if (selectionDirty_) {
        builder_.buildSelection(*context_.document, surfaces(), context_.options, layers_);
        ++selectionBuilds_;
    }
    if (gridDirty) {
        builder_.buildGrid(context_.options, layers_);
    }
    // What the GPU view must be given at its next frame (prepareGpuFrame).
    if (terrainRebuilt) {
        gpuLayersStale_ = true;
    } else if (gridDirty) {
        gpuDrawingStale_ = true;
    } else {
        gpuSelectionStale_ = true;
    }
    terrainDirty_ = entitiesDirty_ = selectionDirty_ = false;
    lastBuildMs_ = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() -
                                                             started)
                       .count();
    // Empty when nothing but the grid was built.
    sceneEmpty_ = layers_.terrain.empty() && layers_.edges.empty() && layers_.entities.empty();
    if (framedEmpty_ && !sceneEmpty_) {
        zoomExtents();
    }
}

void RenderViewWidget::zoomExtents()
{
    if (context_.document == nullptr) {
        return;
    }
    // The box of what was BUILT, so the framing sees exactly the z the
    // linework was put at (draped, own heights, datum) - and a rebuild is due
    // anyway before the next paint.
    framedEmpty_ = false; // so the rebuild below does not frame again
    rebuildIfNeeded();
    // The GPU view frames through this: it is given the scene now, so that
    // what it frames is what it will draw (GpuSceneView::frameScene). Its
    // first frame fades the edges; until then they are drawn as built.
    if (gpuLayersStale_ || gpuDrawingStale_ || gpuSelectionStale_) {
        sendLayersToGpu(gpuLayersStale_ || gpuEdgesShown_, false);
    }
    auto box = layers_.bounds;
    framedEmpty_ = box.empty() || sceneEmpty_;
    if (framedEmpty_) {
        // Nothing to frame: show a patch of ground round the origin rather
        // than leaving the camera wherever it was, which looks like a broken
        // view - and the paint says there is nothing to show. 100 m across
        // and a tenth of that in height, so the frame is the ground plane
        // seen at a slant rather than a cube.
        constexpr double kHalf = 50.0;
        box = katana::math::AABB(katana::math::Vec3(-kHalf, -kHalf, -0.1 * kHalf),
                                 katana::math::Vec3(kHalf, kHalf, 0.1 * kHalf));
    }
    camera().frame(box);
    framed_ = true;
    refitOnResize_ = true;
    // Only a frame of something drawn is worth keeping for the next widget:
    // one of the empty ground is replaced by the first real frame anyway.
    state_.cameraFramed = !framedEmpty_;
    requestFrame();
}

double RenderViewWidget::pixelRatio() const
{
    const double ratio = devicePixelRatioF();
    return std::isfinite(ratio) && ratio > 0.0 ? ratio : 1.0;
}

bool RenderViewWidget::resizeTarget()
{
    // In DEVICE pixels: a 3D view on a 125% display rendered at 80% of its
    // resolution and was scaled up nearest-neighbour, so a 1 px line beaded
    // into 1 and 2 px steps while the plan view beside it stayed crisp.
    const double ratio = pixelRatio();
    const int w = std::max(static_cast<int>(std::lround(width() * ratio)), 1);
    const int h = std::max(static_cast<int>(std::lround(height() * ratio)), 1);
    if (framebuffer_.width() == w && framebuffer_.height() == h &&
        camera().viewportWidth() == w && camera().viewportHeight() == h) {
        return true;
    }
    if (auto status = framebuffer_.resize(w, h); !status) {
        if (onStatus) {
            onStatus(QString::fromStdString(status.error().describe()));
        }
        return false;
    }
    camera().setViewportSize(w, h);
    return true;
}

void RenderViewWidget::resizeEvent(QResizeEvent* event)
{
    QWidget::resizeEvent(event);
#if defined(KATANA_HAS_GPU)
    if (gpuView_ != nullptr) {
        gpuView_->setGeometry(rect());
        if (overlay_ != nullptr) {
            overlay_->setGeometry(rect());
        }
        // Framed again at the GPU view's next frame, once it has the new size
        // - framing now would fit the aspect it had.
        if (framed_ && refitOnResize_ && context_.document != nullptr) {
            gpuView_->setCameraFramed(false);
        }
        return;
    }
#endif
    if (!resizeTarget()) {
        return;
    }
    // A frame made before the view had its real size fitted the wrong aspect
    // - or none, before the first resize - and cut the sides off. Until the
    // user moves the camera, every resize frames again.
    if (framed_ && refitOnResize_ && context_.document != nullptr) {
        zoomExtents();
    }
}

void RenderViewWidget::paintEvent(QPaintEvent* /*event*/)
{
    if (gpuView_ != nullptr) {
        return; // the GPU view covers the widget and draws the frame
    }
    const auto started = std::chrono::steady_clock::now();
    QPainter painter(this);
    emptyMessageShown_ = false;
    // The display the view is on may have changed since the last resize.
    if (!resizeTarget() || framebuffer_.empty()) {
        painter.fillRect(rect(), kBackground);
        return;
    }
    const bool rebuilt = terrainDirty_ || entitiesDirty_ || selectionDirty_;
    if (!framed_) {
        zoomExtents();
    }
    rebuildIfNeeded();

    // The depth range fitted, the edges faded and the layers drawn in their
    // order and with their depth rules: cad::renderLayers, so the tests draw
    // exactly what the view does.
    katana::render::RenderOptions options;
    options.background =
        katana::render::rgba(kBackground.red(), kBackground.green(), kBackground.blue());
    std::string failure;
    if (auto result = katana::cad::renderLayers(layers_, camera(), rasterizer_, framebuffer_,
                                                options)) {
        stats_ = *result;
    } else {
        stats_ = katana::render::RenderStats{};
        failure = result.error().describe();
    }

    if (!failure.empty()) {
        painter.fillRect(rect(), QColor(60, 20, 20));
        painter.setPen(Qt::white);
        painter.drawText(rect(), Qt::AlignCenter, QString::fromStdString(failure));
        return;
    }

    // No copy: the framebuffer's bytes ARE the image's bytes for this call.
    // Its pixel ratio makes Qt draw the device-sized image over the widget's
    // logical size one to one, instead of scaling it up.
    QImage image(reinterpret_cast<const uchar*>(framebuffer_.color().data()),
                 framebuffer_.width(), framebuffer_.height(),
                 framebuffer_.width() * static_cast<int>(sizeof(katana::render::Rgba)),
                 QImage::Format_ARGB32);
    image.setDevicePixelRatio(pixelRatio());
    painter.drawImage(0, 0, image);
    paintOverlays(painter);

    lastFrameMs_ = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() -
                                                             started)
                       .count();
    if (onFrameStats) {
        // The whole paint - build, render and blit - and the build on its own
        // when this paint did one.
        QString text = QString("%1  %2 tri  %3 ms")
                           .arg(QString::fromLatin1(katana::cad::toString(state_.kind)))
                           .arg(stats_.trianglesRasterised)
                           .arg(lastFrameMs_, 0, 'f', 1);
        if (rebuilt) {
            text += QString(" (scene %1 ms)").arg(lastBuildMs_, 0, 'f', 1);
        }
        onFrameStats(text);
    }
}

void RenderViewWidget::paintOverlays(QPainter& painter)
{
    emptyMessageShown_ = false;
    drawLegend(painter);
    // Only for a drawing with nothing in it. A scene of the grid alone is
    // also what a drawing whose every layer is hidden - in the document or in
    // this view - builds, and telling that user to draw or import something
    // would be wrong; the plan view says nothing then either
    // (ViewportWidget::drawEmptyHint).
    if (sceneEmpty_ && drawingIsEmpty()) {
        emptyMessageShown_ = drawEmptyMessage(painter);
    }
}

// A small bar of the one elevation ramp with its two ends, so a colour can be
// read as a height. Only when a surface is coloured by elevation and the view
// has room for it.
void RenderViewWidget::drawLegend(QPainter& painter) const
{
    if (!layers_.hasRamp() || height() < 160 || width() < 160) {
        return;
    }
    constexpr int kBarWidth = 10;
    constexpr int kBarHeight = 110;
    const QFontMetrics metrics = painter.fontMetrics();
    const QString high = QString::number(layers_.rampHigh, 'f', 2);
    const QString low = QString::number(layers_.rampLow, 'f', 2);
    const int textWidth = std::max(metrics.horizontalAdvance(high), metrics.horizontalAdvance(low));
    const QRect bar(width() - 16 - kBarWidth - textWidth - 6, 16, kBarWidth, kBarHeight);

    painter.save();
    QLinearGradient gradient(bar.topLeft(), bar.bottomLeft());
    constexpr int kStops = 8;
    for (int i = 0; i <= kStops; ++i) {
        const double t = static_cast<double>(i) / kStops;
        const katana::render::Rgba c = katana::cad::elevationRampColor(1.0 - t);
        gradient.setColorAt(t, QColor(katana::render::redOf(c), katana::render::greenOf(c),
                                      katana::render::blueOf(c)));
    }
    QColor backing = kBackground;
    backing.setAlpha(200);
    painter.setPen(Qt::NoPen);
    painter.setBrush(backing);
    painter.drawRoundedRect(bar.adjusted(-6, -6 - metrics.height() / 2, textWidth + 12,
                                         6 + metrics.height() / 2),
                            4, 4);
    painter.setBrush(gradient);
    painter.drawRect(bar);
    painter.setPen(theme::textMuted());
    painter.drawText(QPoint(bar.right() + 6, bar.top() + metrics.ascent() / 2), high);
    painter.drawText(QPoint(bar.right() + 6, bar.bottom() + metrics.ascent() / 2), low);
    painter.restore();
}

bool RenderViewWidget::drawingIsEmpty() const
{
    if (context_.document == nullptr) {
        return true;
    }
    const auto& model = context_.document->model();
    return model.entities.empty() && model.alignments.empty() && surfaces().empty() &&
           meshes().empty();
}

// An empty 3D view looked broken: a grid and nothing on it, with no word of
// why. It says what would put something there.
bool RenderViewWidget::drawEmptyMessage(QPainter& painter) const
{
    const QString message = QStringLiteral("Nothing to show in 3D yet.\n"
                                           "Draw or import something, or build a surface "
                                           "from the Terrain menu.");
    const QRect area = rect().adjusted(12, 12, -12, -12);
    const int flags = Qt::AlignCenter | Qt::TextWordWrap;
    const QRect text = painter.fontMetrics().boundingRect(area, flags, message);
    if (text.height() > area.height()) {
        return false; // too small a view to say it in; the grid alone says less wrongly
    }
    painter.save();
    // A backing of the view's own ground so the grid does not strike through
    // the words, and the chrome's muted text so it reads as the application
    // talking rather than as something in the model.
    painter.setPen(Qt::NoPen);
    QColor backing = kBackground;
    backing.setAlpha(225);
    painter.setBrush(backing);
    painter.drawRoundedRect(text.adjusted(-12, -8, 12, 8), 6, 6);
    painter.setPen(theme::textMuted());
    painter.drawText(area, flags, message);
    painter.restore();
    return true;
}

void RenderViewWidget::mousePressEvent(QMouseEvent* event)
{
    if (onActivated) {
        onActivated();
    }
    setFocus(Qt::MouseFocusReason);
    lastMouse_ = event->pos();
    refitOnResize_ = false; // the user is moving the camera now

    const bool panModifier = (event->modifiers() & Qt::ShiftModifier) != 0;
    if (event->button() == Qt::MiddleButton || (event->button() == Qt::LeftButton && panModifier)) {
        drag_ = Drag::Pan;
    } else if (event->button() == Qt::LeftButton) {
        // An orthographic elevation view is a measured drawing; orbiting it
        // would quietly turn it into something you cannot scale off. Pan only.
        drag_ = state_.kind == katana::cad::ViewKind::Elevation ? Drag::Pan : Drag::Orbit;
    } else {
        drag_ = Drag::None;
    }
}

void RenderViewWidget::mouseMoveEvent(QMouseEvent* event)
{
    if (drag_ == Drag::None) {
        return;
    }
    const QPoint delta = event->pos() - lastMouse_;
    lastMouse_ = event->pos();
    if (drag_ == Drag::Orbit) {
        // Dragging right turns the model to the right, so the camera goes the
        // other way; dragging down tips the top towards you.
        camera().orbit(-delta.x() * kOrbitPerPixel, delta.y() * kOrbitPerPixel);
    } else {
        // The camera counts in framebuffer (device) pixels.
        camera().panPixels(delta.x() * pixelRatio(), delta.y() * pixelRatio());
    }
    update();
}

void RenderViewWidget::mouseReleaseEvent(QMouseEvent* /*event*/) { drag_ = Drag::None; }

void RenderViewWidget::mouseDoubleClickEvent(QMouseEvent* /*event*/) { zoomExtents(); }

void RenderViewWidget::wheelEvent(QWheelEvent* event)
{
    const double notches = event->angleDelta().y() / 120.0;
    if (notches == 0.0) {
        return;
    }
    const double factor = std::pow(1.0 / kZoomPerNotch, notches);
    const QPointF position = event->position() * pixelRatio();
    refitOnResize_ = false;
    camera().dollyAtPixel(factor, position.x(), position.y());
    update();
    event->accept();
}

void RenderViewWidget::keyPressEvent(QKeyEvent* event)
{
    using katana::render::StandardView;
    switch (event->key()) {
    case Qt::Key_1:
        setStandardView(StandardView::Top);
        return;
    case Qt::Key_2:
        setStandardView(StandardView::Front);
        return;
    case Qt::Key_3:
        setStandardView(StandardView::Right);
        return;
    case Qt::Key_4:
        setStandardView(StandardView::Back);
        return;
    case Qt::Key_5:
        setStandardView(StandardView::Left);
        return;
    case Qt::Key_0:
        setStandardView(StandardView::IsoSouthWest);
        return;
    case Qt::Key_P:
        setProjection(camera().projection() == katana::render::Projection::Perspective
                          ? katana::render::Projection::Orthographic
                          : katana::render::Projection::Perspective);
        return;
    case Qt::Key_E:
        zoomExtents();
        return;
    default:
        break;
    }
    QWidget::keyPressEvent(event);
}

} // namespace katana::qt
