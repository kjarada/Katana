#pragma once

// Host for the tiled viewports (PLAN.MD Phase 08).
//
// The container owns no geometry of its own: katana::cad::ViewportLayout
// decides where every cell is and which is active, and this class arranges real
// widgets to match. Splitting the window is therefore a tested calculation
// rather than a pile of QSplitter nesting, and the rule that a click lands in
// exactly one viewport is asserted in the cad tests rather than discovered.
//
// Each cell holds one of three widgets:
//   Plan       ViewportWidget      - the existing 2D editor, unchanged
//   Model3D    RenderViewWidget    - orbiting software-rendered 3D
//   Elevation  RenderViewWidget    - the same, locked orthographic
//   Section    SectionViewWidget   - station against elevation
//
// Only the Plan cells edit; the others are views. That is deliberate and is
// what keeps the command interpreter, snapping and selection untouched by this
// whole feature.

#include <functional>
#include <memory>
#include <vector>

#include <QWidget>

#include "katana/cad/viewport_layout.hpp"
#include "render_view_widget.hpp"
#include "section_view_widget.hpp"
#include "viewport_widget.hpp"

namespace katana::qt {

class ViewportContainer final : public QWidget {
  public:
    ViewportContainer(katana::cad::Document& document, QWidget* parent = nullptr);

    void setLayoutKind(katana::cad::LayoutKind kind);
    [[nodiscard]] katana::cad::LayoutKind layoutKind() const { return layout_.layout(); }
    [[nodiscard]] katana::cad::ViewportLayout& layout() { return layout_; }

    // Changes what the ACTIVE cell shows.
    void setActiveViewKind(katana::cad::ViewKind kind);
    [[nodiscard]] katana::cad::ViewKind activeViewKind() const;

    // The plan viewport of the active cell, or the first plan cell, or null
    // when the layout has none. The window routes tool changes and snapping
    // settings through this, so those keep working whatever the split.
    [[nodiscard]] ViewportWidget* activePlanView();
    [[nodiscard]] RenderViewWidget* activeRenderView();
    [[nodiscard]] SectionViewWidget* activeSectionView();
    // Every plan viewport, for settings that apply to all of them.
    [[nodiscard]] std::vector<ViewportWidget*> planViews();

    // Reference data and surfaces are owned by the window; the views borrow
    // them. Passing them here re-points every existing and future cell.
    void setReferenceData(katana::interop::ReferenceData* reference);
    void setSurfaces(const std::vector<katana::cad::SceneSurface>* surfaces);
    void setMeshes(const std::vector<katana::cad::SceneMesh>* meshes);
    void setSceneOptions(const katana::cad::SceneOptions& options);
    [[nodiscard]] const katana::cad::SceneOptions& sceneOptions() const { return options_; }

    // Shows `section` in the active Section cell, switching a cell to Section
    // if none is showing one. Returns false when the layout has no cell to use.
    bool showSection(katana::cad::Section section);

    // ---- the single-viewport surface -------------------------------------
    //
    // The window drove one ViewportWidget before there were tiles. These keep
    // that interface, applying to EVERY plan cell rather than to one, and
    // remembering the setting so a cell created later comes up matching. They
    // are no-ops when the layout currently holds no plan cell, which is a
    // legitimate state (four 3D views) and must not be a crash.
    void setTool(Tool tool);
    [[nodiscard]] Tool tool() const { return tool_; }
    void setGridVisible(bool visible);
    [[nodiscard]] bool gridVisible() const { return gridVisible_; }
    void setSnapEnabled(bool enabled);
    [[nodiscard]] bool snapEnabled() const { return snapEnabled_; }
    void setSnapModes(katana::cad::SnapModes modes);
    [[nodiscard]] katana::cad::SnapModes snapModes() const { return snapModes_; }
    void cancel();
    void resetInteraction();
    // Frames the active view; every plan view when the active one is not one.
    void zoomExtents();
    void zoomTo(const katana::geometry::Box2& bounds);
    void invalidateReferenceCache();

    void refreshAll();
    void zoomExtentsActive();

    // Forwarded from whichever view raised it.
    std::function<void(const QString&)> onStatus;
    std::function<void()> onActiveChanged;
    // Plan viewport callbacks, re-emitted so the window wires them once.
    std::function<void(const QString&)> onPrompt;
    std::function<void(const QString&)> onError;
    std::function<void(const katana::geometry::Point2&,
                       const std::optional<katana::cad::SnapResult>&)>
        onCursorMoved;
    std::function<void(Tool)> onToolChanged;

  protected:
    void resizeEvent(QResizeEvent* event) override;
    void paintEvent(QPaintEvent* event) override;
    // Plan viewports do not report activation themselves - they predate tiling
    // and know nothing about it - so the container watches their mouse presses.
    bool eventFilter(QObject* watched, QEvent* event) override;

  private:
    struct Cell {
        std::unique_ptr<ViewportWidget> plan;
        std::unique_ptr<RenderViewWidget> render;
        std::unique_ptr<SectionViewWidget> section;
        [[nodiscard]] QWidget* widget() const;
    };

    void rebuildWidgets();
    void buildCellWidget(std::size_t index);
    void applyGeometry();
    void activate(std::size_t index);
    [[nodiscard]] ViewContext contextFor() const;

    katana::cad::Document& document_;
    katana::cad::ViewportLayout layout_;
    std::vector<Cell> cells_;

    katana::interop::ReferenceData* reference_ = nullptr;
    const std::vector<katana::cad::SceneSurface>* surfaces_ = nullptr;
    const std::vector<katana::cad::SceneMesh>* meshes_ = nullptr;
    katana::cad::SceneOptions options_{};

    // Settings mirrored onto every plan viewport as it is created, so a new
    // cell does not come up with the grid on when the user turned it off.
    Tool tool_ = Tool::Select;
    bool gridVisible_ = true;
    bool snapEnabled_ = true;
    katana::cad::SnapModes snapModes_ = katana::cad::kDefaultSnapModes;
};

} // namespace katana::qt
