#pragma once

// The 3D view drawn on the GPU: a QRhiWidget showing a DrawList through a
// Camera with GpuRenderer (docs/gpu.md, "The widget").
//
// Written to be HOSTED. RenderViewWidget owns the scene building, the empty
// message, the status line and the document; this widget only draws and
// turns the mouse into camera moves - the SAME moves RenderViewWidget makes
// (left drag orbits, or pans in an elevation view; middle or Shift+left drag
// pans; the wheel zooms towards what is under the cursor, through the host,
// which has the scene to find it in; double-click and E frame the scene;
// 1-5 and 0 pick standard views; P toggles the projection), so a host can put
// either widget in the same place and the user feels no difference.
//
// The camera is the HOST's (a ViewState's, like RenderViewWidget's), held by
// reference: switching between this view and the software one keeps the view.
// Two things make that true. The camera's view - eye, target, zoom - does not
// depend on the viewport's pixel count, only on its aspect: framing, panning
// by pixels and zooming about a pixel all count against the viewport's own
// size. So this widget sizes the camera to its LOGICAL pixels and draws each
// frame through a copy sized to the device pixels it draws, while
// RenderViewWidget sizes it to device pixels before every paint
// (resizeTarget) - and the software view taking over after a failure finds
// the same view at its own resolution. And the widget frames the scene on its
// first frame only when the host has not said the camera is framed already
// (setCameraFramed, fed from ViewState::cameraFramed) - so a view rebuilt
// over a ViewState keeps the user's orbit and zoom.
//
// The API is the build's (renderer_choice.hpp, GpuBackend): Direct3D 11 on
// Windows, Vulkan on Linux.
//
// No Q_OBJECT (Katana has no moc): QRhiWidget's renderFailed signal is
// connected to a lambda, and what the host needs to hear comes out through
// std::function hooks.
//
// Under Qt's offscreen platform QRhiWidget never renders - it reports
// renderFailed - which is why chooseRenderer() sends headless runs to the
// software path, and why the tests exercise GpuRenderer through OffscreenGpu.

#include <cstddef>
#include <functional>
#include <memory>
#include <span>

#include <QPoint>
#include <QPointF>
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
    // The same for a scene of layers drawn with their own depth rules
    // (GpuRenderer::setLayers), and for one layer that changed alone
    // (GpuRenderer::updateLayer) - a selection, faded edges.
    void setLayers(std::span<const LayerSource> layers);
    void updateLayer(std::size_t index, const katana::render::DrawList& list);

    void setFrameSettings(const FrameSettings& settings);
    [[nodiscard]] const FrameSettings& frameSettings() const { return settings_; }

    // False for an orthographic elevation view, which is a measured drawing:
    // a left drag then pans instead of orbiting, as RenderViewWidget does.
    void setOrbitAllowed(bool allowed) { orbitAllowed_ = allowed; }

    // Whether a software device (WARP, lavapipe) may draw this view; by
    // default it is refused as a failure, so the host falls back to the
    // software rasteriser (RendererDecision::softwareDeviceAllowed).
    void setSoftwareDeviceAllowed(bool allowed) { softwareDeviceAllowed_ = allowed; }

    // Frames the scene: onZoomExtents when the host set it (the host knows
    // what "the scene" is), otherwise the packed draw list's bounds - and
    // nothing, for now, when that list is empty.
    void zoomExtents();

    // Whether the camera already frames the scene, so the first frame must
    // leave it alone: the host passes ViewState::cameraFramed (for a state
    // whose cameraKind matches its kind, as RenderViewWidget checks) before
    // the widget first draws. False, the default, frames on the first frame.
    void setCameraFramed(bool framed);
    // True once the camera frames something drawn - framed by this widget,
    // or said to be by the host - for the host to write back into
    // ViewState::cameraFramed. A frame of an empty scene does not count: the
    // first draw list with something in it is framed again.
    [[nodiscard]] bool cameraFramed() const { return framed_ && !framedEmpty_; }

    // True once the widget has failed to render; the host should swap in the
    // software view (chooseRenderer's rule 4).
    [[nodiscard]] bool failed() const { return failed_; }
    [[nodiscard]] const GpuFrameStats& lastStats() const { return stats_; }

    // Raised once, the first time rendering fails (QRhiWidget::renderFailed,
    // a pipeline that does not build, a backend other than the build's, a
    // software device that was not allowed). The
    // camera is in the widget's logical pixels then, as the software view
    // wants it; a software view of a different size must still set its own.
    std::function<void(const QString& reason)> onRenderFailed;
    // Raised when the view is clicked, so a workspace can make it active.
    std::function<void()> onActivated;
    // Frames the scene; see zoomExtents().
    std::function<void()> onZoomExtents;
    // The wheel, in notches (positive in, a fine wheel's fractions too) at
    // the cursor in logical pixels. A host that knows the scene zooms towards
    // what is drawn under the cursor (RenderViewWidget::zoomAtPixel); this
    // widget holds only the packed lists, so without a host it zooms about
    // the target's plane itself (Camera::dollyAtPixel), which stalls over
    // ground beyond that plane (docs/render.md, "Zooming towards the cursor").
    std::function<void(double notches, const QPointF& position)> onWheelZoom;
    // "GPU  12345 tri  0.8 ms" after every frame (CPU time to record it).
    std::function<void(const QString&)> onFrameStats;
    // Called at the start of every frame with the camera it will be drawn
    // through - the host's, sized to the device pixels drawn - before
    // anything is uploaded: where a host fits the depth range to its scene
    // and fades its edges for the frame's scale, as cad::renderLayers does
    // for the software path, and calls updateLayer for what that changed.
    std::function<void(katana::render::Camera& frameCamera)> onPrepareFrame;

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
    void resizeEvent(QResizeEvent* event) override;

  private:
    void fail(const QString& reason);
    // zoomExtents() without asking for a repaint, for the first frame.
    void frameScene();
    // The host's camera in this widget's logical pixels, as RenderViewWidget
    // keeps it: what the mouse moves it by, and what the software view needs.
    void keepCameraLogical();
    // Device pixels per logical pixel of the colour buffer actually drawn.
    [[nodiscard]] double pixelRatio() const;

    katana::render::Camera& camera_;
    GpuRenderer renderer_;
    FrameSettings settings_;
    GpuFrameStats stats_;
    bool failed_ = false;
    // The first frame frames the scene unless this is set: by a frame of
    // something, by the host (setCameraFramed), or by the host's framing hook.
    bool framed_ = false;
    // The last framing was of an empty draw list (the host's hook framed
    // whatever it frames for nothing): the first list with something in it
    // clears framed_, so that it is framed in its turn.
    bool framedEmpty_ = false;
    bool orbitAllowed_ = true;
    bool softwareDeviceAllowed_ = false;
    // True while onPrepareFrame runs, inside render().
    bool inFrame_ = false;

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
// makes to decide between this widget and the software one. The view gets
// the decision's permission for a software device.
[[nodiscard]] std::unique_ptr<GpuSceneView>
makeGpuSceneViewIfChosen(katana::render::Camera& camera, const RendererEnvironment& environment,
                         RendererDecision* decision = nullptr, QWidget* parent = nullptr);

} // namespace katana::qt::gpu
