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
    // Lines in the last scene built: the headless checks' proof that a layer
    // hidden in this view left it, and that an edit reached this view.
    [[nodiscard]] std::size_t lastSceneLineCount() const { return list_.lines.size(); }
    // True when the last scene built had nothing to show - no drawn entity,
    // surface or mesh, only the grid. The view then says so on screen.
    [[nodiscard]] bool sceneEmpty() const { return sceneEmpty_; }

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

    // Milliseconds the last frame took, and what it contained. Shown in the
    // status bar: PLAN.MD section 32 sets a 16 ms budget, and a number you can
    // see is the only kind anyone acts on.
    [[nodiscard]] double lastFrameMilliseconds() const { return lastFrameMs_; }
    [[nodiscard]] const katana::render::RenderStats& lastStats() const { return stats_; }

    // Raised when this view is clicked or the keyboard focus moves into it, so
    // the workspace can make it active; the workspace ignores re-activating
    // the active view.
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
    void focusInEvent(QFocusEvent* event) override;

  private:
    void rebuildIfNeeded();
    // Registers with the context's document, or ends the registration when
    // there is none; a no-op when it is the document already listened to.
    void listenTo(katana::cad::Document* document);
    void drawEmptyMessage(QPainter& painter) const;

    ViewContext context_;
    katana::cad::ViewState& state_;

    katana::cad::SceneBuilder builder_;
    katana::render::DrawList list_;
    // The grid on its own, to tell an empty scene from one with something in
    // it (rebuildIfNeeded). Kept so its buffers are reused.
    katana::render::DrawList gridOnly_;
    katana::render::Rasterizer rasterizer_;
    katana::render::Framebuffer framebuffer_;
    katana::render::RenderStats stats_;
    double lastFrameMs_ = 0.0;
    bool sceneDirty_ = true;
    bool sceneEmpty_ = true;
    bool framed_ = false; // the first paint frames the scene
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
