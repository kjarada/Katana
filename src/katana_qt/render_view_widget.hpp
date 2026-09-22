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
#include "katana/cad/viewport_layout.hpp"
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
    // `cell` is owned by the ViewportLayout and outlives this widget; the
    // camera lives there so that a layout change does not reset the view.
    RenderViewWidget(ViewContext context, katana::cad::ViewportCell* cell,
                     QWidget* parent = nullptr);

  private:
    // Never null: a view with no meshes draws none rather than testing for
    // a pointer at each of the three places that ask.
    [[nodiscard]] const std::vector<katana::cad::SceneMesh>& meshes() const
    {
        static const std::vector<katana::cad::SceneMesh> kNone;
        return context_.meshes != nullptr ? *context_.meshes : kNone;
    }

  public:

    void setContext(const ViewContext& context);
    void setCell(katana::cad::ViewportCell* cell);
    [[nodiscard]] katana::cad::ViewportCell* cell() const { return cell_; }

    // Rebuilds the draw list on the next paint. Call when the document changes.
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

    // Raised when this view is clicked, so the container can make it active.
    std::function<void()> onActivated;
    std::function<void(const QString&)> onStatus;

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
    [[nodiscard]] katana::render::Camera& camera();

    ViewContext context_;
    katana::cad::ViewportCell* cell_ = nullptr;

    katana::cad::SceneBuilder builder_;
    katana::render::DrawList list_;
    katana::render::Rasterizer rasterizer_;
    katana::render::Framebuffer framebuffer_;
    katana::render::RenderStats stats_;
    double lastFrameMs_ = 0.0;
    bool sceneDirty_ = true;
    bool framed_ = false; // the first paint frames the scene

    enum class Drag { None, Orbit, Pan };
    Drag drag_ = Drag::None;
    QPoint lastMouse_;
};

} // namespace katana::qt
