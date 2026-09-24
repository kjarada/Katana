#pragma once

// The 3D view drawn on the GPU: a QRhiWidget showing a DrawList through a
// Camera with GpuRenderer (docs/gpu.md, "The widget").
//
// Written to be HOSTED. RenderViewWidget owns the scene building, the empty
// message, the status line and the document; this widget only draws and
// turns the mouse into camera moves - the SAME moves RenderViewWidget makes
// (left drag orbits, or pans in an elevation view; middle or Shift+left drag
// pans; the wheel zooms about the cursor; double-click and E frame the scene;
// 1-5 and 0 pick standard views; P toggles the projection), so a host can put
// either widget in the same place and the user feels no difference.
//
// The camera is the HOST's (a ViewState's, like RenderViewWidget's), held by
// reference: switching between this view and the software one keeps the view.
//
// No Q_OBJECT (Katana has no moc): QRhiWidget's renderFailed signal is
// connected to a lambda, and what the host needs to hear comes out through
// std::function hooks.
//
// Under Qt's offscreen platform QRhiWidget never renders - it reports
// renderFailed - which is why chooseRenderer() sends headless runs to the
// software path, and why the tests exercise GpuRenderer through OffscreenGpu.

#include <functional>
#include <memory>

#include <QPoint>
#include <QRhiWidget>
#include <QString>

#include "gpu_renderer.hpp"
#include "katana/render/camera.hpp"
#include "katana/render/draw_list.hpp"
#include "renderer_choice.hpp"

class QRhiRenderPassDescriptor;
class QRhiTexture;
class QRhiTextureRenderTarget;

namespace katana::qt::gpu {

class GpuSceneView final : public QRhiWidget {
  public:
    // `camera` must outlive the widget.
    explicit GpuSceneView(katana::render::Camera& camera, QWidget* parent = nullptr);
    ~GpuSceneView() override;

    [[nodiscard]] katana::render::Camera& camera() { return camera_; }

    // Packs `list` for the GPU and repaints; the upload happens in the next
    // frame, and only then. Call it when the scene changes, not per frame.
    void setDrawList(const katana::render::DrawList& list);

    void setFrameSettings(const FrameSettings& settings);
    [[nodiscard]] const FrameSettings& frameSettings() const { return settings_; }

    // False for an orthographic elevation view, which is a measured drawing:
    // a left drag then pans instead of orbiting, as RenderViewWidget does.
    void setOrbitAllowed(bool allowed) { orbitAllowed_ = allowed; }

    // Frames the scene: onZoomExtents when the host set it (the host knows
    // what "the scene" is), otherwise the packed draw list's bounds.
    void zoomExtents();

    // True once the widget has failed to render; the host should swap in the
    // software view (chooseRenderer's rule 4).
    [[nodiscard]] bool failed() const { return failed_; }
    [[nodiscard]] const GpuFrameStats& lastStats() const { return stats_; }

    // Raised once, the first time rendering fails (QRhiWidget::renderFailed,
    // a pipeline that does not build, a backend other than Direct3D 11).
    std::function<void(const QString& reason)> onRenderFailed;
    // Raised when the view is clicked, so a workspace can make it active.
    std::function<void()> onActivated;
    // Frames the scene; see zoomExtents().
    std::function<void()> onZoomExtents;
    // "GPU  12345 tri  0.8 ms" after every frame (CPU time to record it).
    std::function<void(const QString&)> onFrameStats;

  protected:
    void initialize(QRhiCommandBuffer* commands) override;
    void render(QRhiCommandBuffer* commands) override;
    void releaseResources() override;

    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;

  private:
    void fail(const QString& reason);
    // zoomExtents() without asking for a repaint, for the first frame.
    void frameScene();
    // Device pixels per logical pixel of the colour buffer actually drawn.
    [[nodiscard]] double pixelRatio() const;

    katana::render::Camera& camera_;
    GpuRenderer renderer_;
    FrameSettings settings_;
    GpuFrameStats stats_;
    bool failed_ = false;
    bool framed_ = false;
    bool orbitAllowed_ = true;

    // Our own render target (setAutoRenderTarget(false)): the widget's colour
    // buffer with a 32-bit FLOAT depth texture, which the automatic target
    // does not offer, and which reversed Z needs.
    std::unique_ptr<QRhiTexture> depth_;
    std::unique_ptr<QRhiTextureRenderTarget> target_;
    // The pass the pipelines were built against. Kept across resizes, which
    // make a new target of the same formats: a compatible pass, so the
    // pipelines need not be rebuilt.
    std::unique_ptr<QRhiRenderPassDescriptor> pass_;

    enum class Drag { None, Orbit, Pan };
    Drag drag_ = Drag::None;
    QPoint lastMouse_;
};

// Builds a GPU view when chooseRenderer(environment) says the GPU, and
// returns null with the decision's reason otherwise: the one call a host
// makes to decide between this widget and the software one.
[[nodiscard]] std::unique_ptr<GpuSceneView>
makeGpuSceneViewIfChosen(katana::render::Camera& camera, const RendererEnvironment& environment,
                         RendererDecision* decision = nullptr, QWidget* parent = nullptr);

} // namespace katana::qt::gpu
