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
#include <map>
#include <string>
#include <utility>
#include <QList>
#include <vector>

#include <QImage>
#include <QPointF>
#include <QString>
#include <QWidget>

#include "katana/cad/plot.hpp"
#include "katana/cad/scene.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/cad/style_drawing.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/interop/reference_data.hpp"

class QPainter;
class QPen;

namespace katana::qt {

enum class Tool { Select, Point, Line, Polyline, Rectangle, Circle, Arc, Move, Copy };

[[nodiscard]] const char* toString(Tool tool);

class ViewportWidget final : public QWidget {
  public:
    // The current view, for a caller that wants the same centre on paper.
    [[nodiscard]] const katana::cad::ViewTransform& viewTransform() const { return view_; }

    // Plots the drawing to a PDF: the same drawing code as the screen, painted
    // through a sheet transform, with every line as wide as its layer's line
    // weight says in millimetres (PLAN.MD Phase 22). Entities, hatches and
    // alignments; not rasters or point clouds, which at plot resolution would
    // be enormous and are a later slice. Fails with FileExportFailure when the
    // file cannot be written, and with whatever cad::sheetFor refuses.
    [[nodiscard]] katana::core::Status plotToPdf(const QString& path,
                                                 const katana::cad::PlotSettings& settings);

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
    // The session's meshes (PLAN.MD 20.2 slice 4), drawn in plan as their
    // footprints. A pointer to the MainWindow's vector, so one added later
    // appears without this being called again.
    void setMeshes(const std::vector<katana::cad::SceneMesh>* meshes);
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
    // Dash patterns already built THIS FRAME, by linetype name and pen width.
    // A pattern also depends on the view scale, which is constant within a
    // frame, so the cache is cleared at the top of every drawEntities. It is
    // mutable because drawing does not change the document.
    mutable std::map<std::pair<std::string, double>, QList<qreal>> dashCache_;
    void drawGeometry(QPainter& painter, const katana::entity::Geometry& geometry) const;
    // A 12d definition's strokes and texts, honouring the pen changes in it.
    void drawStyleDrawing(QPainter& painter, const katana::cad::StyleDrawing& drawing) const;
    void drawStyleText(QPainter& painter, const katana::cad::StyleTextMark& text) const;
    // Draws the definition along the entity's plan shape. FALSE when nothing
    // came of it, so the caller knows to draw the plain line instead - a
    // 12d linestyle replaces the line rather than decorating it.
    [[nodiscard]] bool drawLineStyle(QPainter& painter,
                                     const katana::entity::LineStyle& definition,
                                     const katana::entity::Geometry& geometry) const;
    [[nodiscard]] static QPen penFor(const QPen& entityPen, const std::string& pen);
    // Model units to one plot millimetre, which is what a `paperstyle` uses.
    [[nodiscard]] double paperScale() const;
    void drawSymbol(QPainter& painter, const std::string& symbol, const Point2& centre,
                    double size) const;
    void drawMeshFootprints(QPainter& painter) const;
    // `height` in model units; `rotation` in radians, counter-clockwise.
    // Fills a closed polyline with the hatch pattern resolved for the entity
    // being drawn, if any. `screen` is that boundary already transformed, so a
    // solid fill costs nothing beyond the polygon the caller built anyway.
    void drawHatch(QPainter& painter, const katana::geometry::Polyline2& boundary,
                   const QPolygonF& screen) const;

    // Every alignment in the document, as an amber overlay above the drawing:
    // the centreline, a tick and a chainage label at each key station (the
    // ends and every TS, SC, CS, ST), and the name at the start. An overlay
    // rather than an entity because an alignment is a definition other things
    // are cut along, not a line in the drawing - the same reason a surface is
    // not an entity.
    void drawAlignments(QPainter& painter) const;

    void drawText(QPainter& painter, const Point2& position, const std::string& text,
                  double height, double rotation) const;
    void drawPreview(QPainter& painter) const;
    void drawSnapMarker(QPainter& painter) const;

    katana::cad::Document& document_;
    // Declared after document_ and destroyed before anything else here: the
    // listener it owns captures this widget.
    katana::cad::Document::ListenerHandle documentListener_;
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
    const std::vector<katana::cad::SceneMesh>* meshes_ = nullptr;

    // The dimension style in force for the entity currently being drawn.
    // Resolved once per entity in drawEntities rather than per draw call, and
    // held here because drawGeometry's visitor is handed only the geometry.
    // Mutable because painting is logically const, like the caches below.
    mutable katana::entity::DimensionStyle dimensionStyle_{};

    // The hatch pattern in force for the entity currently being drawn, or null
    // when it is not hatched. Resolved once per entity for the same reason
    // dimensionStyle_ is, and owned by the document, so this only ever points
    // at a pattern the model is holding for the duration of the paint.
    mutable const katana::entity::HatchPattern* hatch_ = nullptr;

    // Device pixels per millimetre of paper while plotting; 0 on screen,
    // where a line weight has no paper to be millimetres of and every line
    // is a hairline. Set by plotToPdf for the duration of the plot only.
    double paperPixelsPerMillimetre_ = 0.0;

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
