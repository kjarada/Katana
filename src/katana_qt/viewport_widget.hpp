#pragma once

// 2D model-space viewport (PLAN.MD Phase 08).
//
// A view of the Document, never a second source of truth (Rule 3): it paints
// whatever the model contains and turns mouse input into Commands. All geometry
// decisions (picking, snapping, view maths) live in katana_cad, where they are
// unit tested; this class only adapts them to Qt events and QPainter.
//
// Rendering uses QPainter. The Vulkan renderer of Phase 15 replaces the paint
// code, not the interaction logic.

#include <functional>
#include <optional>
#include <vector>

#include <QImage>
#include <QPointF>
#include <QString>
#include <QWidget>

#include "katana/cad/document.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/interop/reference_data.hpp"

class QPainter;
class QPen;

namespace katana::qt {

enum class Tool { Select, Point, Line, Polyline, Rectangle, Circle, Arc, Move, Copy };

[[nodiscard]] const char* toString(Tool tool);

class ViewportWidget final : public QWidget {
  public:
    explicit ViewportWidget(katana::cad::Document& document, QWidget* parent = nullptr);

    void setTool(Tool tool);
    [[nodiscard]] Tool tool() const { return tool_; }

    void zoomExtents();
    // Frames one specific box, e.g. a single reference layer.
    void zoomTo(const katana::geometry::Box2& bounds);

    // Reference data (imported imagery and point clouds) belongs to the window;
    // the viewport only paints it, and never mutates it (Rule 3). Null until the
    // window supplies one.
    void setReferenceData(katana::interop::ReferenceData* reference);
    // Drops the cached QImages and per-point colours. Call after a layer is
    // added, removed, or has its display settings changed.
    void invalidateReferenceCache();
    void setGridVisible(bool visible);
    [[nodiscard]] bool gridVisible() const { return gridVisible_; }
    void setSnapEnabled(bool enabled);
    [[nodiscard]] bool snapEnabled() const { return snapEnabled_; }
    void setSnapModes(katana::cad::SnapModes modes);
    [[nodiscard]] katana::cad::SnapModes snapModes() const { return snapModes_; }

    // Abandons the operation in progress (Esc).
    void cancel();

    // Abandons the operation in progress WITHOUT changing the active tool.
    // Required whenever the document under the view is replaced: the collected
    // clicks belong to the drawing that is going away, and committing them into
    // the new one silently produces an entity at coordinates the user never
    // picked there.
    void resetInteraction();

    // Notifications to the main window. All optional.
    std::function<void(const QString& prompt)> onPrompt;
    std::function<void(const QString& error)> onError;
    std::function<void(const katana::geometry::Point2& world,
                       const std::optional<katana::cad::SnapResult>& snap)>
        onCursorMoved;
    std::function<void(Tool tool)> onToolChanged;

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
    using Point2 = katana::geometry::Point2;

    [[nodiscard]] QPointF toScreen(const Point2& world) const;
    [[nodiscard]] Point2 toWorld(const QPointF& screen) const;

    void updateCursor(const QPointF& screen);
    void acceptPoint(const Point2& point);
    void finishOperation(bool close);
    void selectAt(const QPointF& screen, Qt::KeyboardModifiers modifiers);
    void selectInBox(const QPointF& from, const QPointF& to, Qt::KeyboardModifiers modifiers);
    void run(katana::commands::CommandPtr command);
    void updatePrompt();

    void drawGrid(QPainter& painter) const;
    void drawRasters(QPainter& painter) const;
    void drawPointClouds(QPainter& painter) const;
    void drawEntities(QPainter& painter) const;
    void drawGeometry(QPainter& painter, const katana::entity::Geometry& geometry) const;
    // `height` in model units; `rotation` in radians, counter-clockwise.
    void drawText(QPainter& painter, const Point2& position, const std::string& text,
                  double height, double rotation) const;
    void drawPreview(QPainter& painter) const;
    void drawSnapMarker(QPainter& painter) const;

    katana::cad::Document& document_;
    katana::cad::ViewTransform view_;
    bool viewInitialised_ = false;

    Tool tool_ = Tool::Select;
    std::vector<Point2> points_; // clicks collected for the operation in progress
    Point2 cursorWorld_;         // after snapping
    std::optional<katana::cad::SnapResult> activeSnap_;

    bool gridVisible_ = true;
    bool snapEnabled_ = true;
    katana::cad::SnapModes snapModes_ = katana::cad::kDefaultSnapModes;

    katana::interop::ReferenceData* reference_ = nullptr;

    // Converting RGBA bytes to a QImage, and a classification to a colour, are
    // both far too expensive to redo for every frame of a pan. Both are cached
    // against the layer id (and, for clouds, the colour mode) and rebuilt only
    // when that changes. Mutable because painting is logically const.
    struct RasterCache {
        katana::interop::ReferenceId id = 0;
        QImage image;
    };
    struct CloudCache {
        katana::interop::ReferenceId id = 0;
        katana::interop::PointColorMode mode = katana::interop::PointColorMode::Elevation;
        std::vector<QRgb> colors;
    };
    mutable std::vector<RasterCache> rasterCache_;
    mutable std::vector<CloudCache> cloudCache_;

    bool panning_ = false;
    QPointF lastMouse_;
    std::optional<QPointF> boxStart_; // selection rectangle anchor (screen)
    QPointF boxEnd_;
};

} // namespace katana::qt
