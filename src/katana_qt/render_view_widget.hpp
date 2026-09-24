#pragma once

// 3D / elevation viewport (PLAN.MD Phase 15).
//
// Rule 3 again: this widget owns a camera and a framebuffer and NOTHING else.
// It asks katana_cad to turn the document into a DrawList, hands that to the
// software rasteriser, and blits the result. Every geometric decision - what to
// draw, how an arc is chorded, where the scene's bounds are, what a drag does
// to a camera - lives in a tested library below it. Replacing the rasteriser
// with a Vulkan one changes the two lines that call it and nothing else.
//
// The framebuffer is wrapped in a QImage WITHOUT copying: Framebuffer stores
// 0xAARRGGBB row-major, which is exactly QImage::Format_ARGB32.

#include <functional>

#include <QImage>
#include <QPoint>
#include <QWidget>

#include "katana/cad/scene.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/render/rasterizer.hpp"

namespace katana::qt {

// Everything a view needs from the application, by reference. The window owns
// all of it; a view never copies the model.
struct ViewContext {
    katana::cad::Document* document = nullptr;
    // Pointers to the MainWindow's vectors, not copies: a surface or a mesh
    // added later must reach every view without rebuilding them.
    const std::vector<katana::cad::SceneSurface>* surfaces = nullptr;
    const std::vector<katana::cad::SceneMesh>* meshes = nullptr;
    katana::cad::SceneOptions options{};
};

class RenderViewWidget final : public QWidget {
  public:
    // `state` belongs to the workspace's ViewSet and outlives this widget; the
    // camera and the layers hidden in this view live there, so changing the
    // view's kind and back, or floating its dock, does not reset either.
    RenderViewWidget(ViewContext context, katana::cad::ViewState& state,
                     QWidget* parent = nullptr);

  private:
    // Never null: a view with no meshes or surfaces draws none rather than
    // testing for a pointer at each of the places that ask.
    [[nodiscard]] const std::vector<katana::cad::SceneMesh>& meshes() const
    {
        static const std::vector<katana::cad::SceneMesh> kNone;
        return context_.meshes != nullptr ? *context_.meshes : kNone;
    }
    [[nodiscard]] const std::vector<katana::cad::SceneSurface>& surfaces() const
    {
        static const std::vector<katana::cad::SceneSurface> kNone;
        return context_.surfaces != nullptr ? *context_.surfaces : kNone;
    }

  public:

    // Takes everything but the layers: those are this view's own, and the
    // workspace hands every render view the same context.
    void setContext(const ViewContext& context);
    [[nodiscard]] katana::cad::ViewState& state() const { return state_; }
    [[nodiscard]] katana::render::Camera& camera() { return state_.camera; }
    // Lines in the last scene built, every layer counted: the headless
    // checks' proof that a layer hidden in this view left it, and that an
    // edit reached this view.
    [[nodiscard]] std::size_t lastSceneLineCount() const
    {
        return layers_.grid.lines.size() + layers_.terrain.lines.size() +
               layers_.edges.lines.size() + layers_.entities.lines.size() +
               layers_.selection.lines.size();
    }
    // The scene as last built, by layer.
    [[nodiscard]] const katana::cad::SceneLayers& sceneLayers() const { return layers_; }
    // How many times each layer has been built: the proof that a selection
    // click rebuilds the overlay and nothing under it.
    [[nodiscard]] int terrainBuilds() const { return terrainBuilds_; }
    [[nodiscard]] int entityBuilds() const { return entityBuilds_; }
    [[nodiscard]] int selectionBuilds() const { return selectionBuilds_; }
    // The framebuffer the last paint rendered, in device pixels.
    [[nodiscard]] const katana::render::Framebuffer& framebuffer() const { return framebuffer_; }
    // True when the last scene built had nothing to show - no drawn entity,
    // surface or mesh, only the grid. What the framing goes by.
    [[nodiscard]] bool sceneEmpty() const { return sceneEmpty_; }
    // True when the last paint told the user there is nothing to show and
    // what would put something there: only for an empty scene of a drawing
    // with nothing in it, and only where the view had room to say it.
    [[nodiscard]] bool emptyMessageShown() const { return emptyMessageShown_; }

    // Rebuilds the draw list on the next paint. The view calls it itself on
    // every document change (it listens to the document it is given); call it
    // for what the document does not announce - surfaces, meshes, the scene
    // options, this view's own hidden layers.
    void invalidateScene();
    void zoomExtents();
    void setStandardView(katana::render::StandardView view);
    void setProjection(katana::render::Projection projection);
    void setVerticalExaggeration(double factor);
    [[nodiscard]] double verticalExaggeration() const
    {
        return context_.options.verticalExaggeration;
    }

    // Milliseconds the last paint took - any scene build it did, the render
    // and the blit - and what it contained. Shown in the status bar: a number
    // you can see is the only kind anyone acts on, and a number that left out
    // the build hid the 60-115 ms a selection click cost.
    [[nodiscard]] double lastFrameMilliseconds() const { return lastFrameMs_; }
    [[nodiscard]] double lastBuildMilliseconds() const { return lastBuildMs_; }
    [[nodiscard]] const katana::render::RenderStats& lastStats() const { return stats_; }

    // Raised when this view is clicked or the user moves the keyboard focus
    // into it (view_focus.hpp), so the workspace can make it active; the
    // workspace ignores re-activating the active view.
    std::function<void()> onActivated;
    // Messages the user must see: a failure, never a statistic.
    std::function<void(const QString&)> onStatus;
    // What each frame drew and how long it took ("3D  1916 tri  1.6 ms"), on
    // every paint. Separate from onStatus because a statistic raised per
    // frame wrote over the prompt and the last error in the status bar on
    // every orbit; it belongs in a label of its own.
    std::function<void(const QString&)> onFrameStats;

  protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;

  private:
    void rebuildIfNeeded();
    // A document notification: the drawing changed (its revision moved), or
    // only something else did - the selection, the current layer.
    void documentChanged();
    // Sizes the framebuffer and the camera for the widget's size in DEVICE
    // pixels. False when it could not.
    bool resizeTarget();
    [[nodiscard]] double pixelRatio() const;
    // The elevation ramp's legend, when a surface is coloured by it.
    void drawLegend(QPainter& painter) const;
    // Registers with the context's document, or ends the registration when
    // there is none; a no-op when it is the document already listened to.
    void listenTo(katana::cad::Document* document);
    // True when the document has no entity or alignment and the view has no
    // surface or mesh: nothing that any layer could be hiding.
    [[nodiscard]] bool drawingIsEmpty() const;
    // False when the view is too small to hold the message, which is then
    // left out.
    bool drawEmptyMessage(QPainter& painter) const;

    ViewContext context_;
    katana::cad::ViewState& state_;

    katana::cad::SceneBuilder builder_;
    katana::cad::SceneLayers layers_;
    katana::render::Rasterizer rasterizer_;
    katana::render::Framebuffer framebuffer_;
    katana::render::RenderStats stats_;
    double lastFrameMs_ = 0.0;
    double lastBuildMs_ = 0.0;
    // What needs building before the next paint, layer by layer; each one
    // dirties the layers built from it (rebuildIfNeeded).
    bool terrainDirty_ = true;
    bool entitiesDirty_ = true;
    bool selectionDirty_ = true;
    int terrainBuilds_ = 0;
    int entityBuilds_ = 0;
    int selectionBuilds_ = 0;
    // The drawing's revision the entities were built from.
    std::uint64_t builtRevision_ = 0;
    // Until the user moves the camera, a resize frames the scene again: the
    // first frame is often made before the dock has its real size.
    bool refitOnResize_ = false;
    bool sceneEmpty_ = true;
    bool emptyMessageShown_ = false;
    // The first paint frames the scene unless the camera was framed already
    // (ViewState::cameraFramed): the constructor sets this from the state.
    bool framed_ = false;
    // The last frame was of an empty scene, so it framed a patch of ground
    // round the origin. The first rebuild that finds something to show frames
    // that instead: a line drawn at survey coordinates would otherwise be
    // kilometres outside a view that kept looking at the origin.
    bool framedEmpty_ = false;

    enum class Drag { None, Orbit, Pan };
    Drag drag_ = Drag::None;
    QPoint lastMouse_;

    katana::cad::Document* listenedDocument_ = nullptr;
    // Declared LAST so it is destroyed FIRST: the listener it owns captures
    // this widget and touches the members above, so the registration must end
    // before any of them goes. See ViewportWidget and docs/cad.md, "A listener
    // lives exactly as long as the thing it notifies" - a raw registration
    // outliving its widget was a crash five runs in six.
    katana::cad::Document::ListenerHandle documentListener_;
};

} // namespace katana::qt
