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
#include <string_view>
#include <utility>
#include <QList>
#include <vector>

#include <QImage>
#include <QPoint>
#include <QPointF>
#include <QString>
#include <QWidget>

#include "katana/cad/plot.hpp"
#include "katana/cad/scene.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/cad/style_drawing.hpp"
#include "katana/cad/style_resolver.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/interop/reference_data.hpp"

#include "customisation/style_painter.hpp"
#include "tools/tool_host.hpp"

class QPainter;
class QPen;

namespace katana::qt {

// The window's first eight tools, from before the tool catalogue. Each one
// but Select is now a catalogue tool (toolId), run by the view's ToolHost like
// every other; the enum stays only while MainWindow's toolbar and command
// words still name tools by it, and goes when they name catalogue ids.
enum class Tool { Select, Point, Line, Polyline, Rectangle, Circle, Arc, Move, Copy };

[[nodiscard]] const char* toString(Tool tool);
// The catalogue id a legacy tool runs as ("draw.line"); "" for Select.
[[nodiscard]] const char* toolId(Tool tool);
// The legacy tool a catalogue id is, if it is one of the eight; Select for "".
[[nodiscard]] std::optional<Tool> legacyTool(std::string_view id);

class ViewportWidget final : public QWidget {
  public:
    // The current view, for a caller that wants the same centre on paper. It
    // is the view's STATE (ViewState::plan), not the widget's, so it survives
    // the widget being rebuilt when the view changes kind and back.
    [[nodiscard]] const katana::cad::ViewTransform& viewTransform() const { return state_.plan; }

    // Plots the drawing to a PDF: the same drawing code as the screen, painted
    // through a sheet transform, with every line as wide as its layer's line
    // weight says in millimetres (PLAN.MD Phase 22). Entities, hatches and
    // alignments; not rasters or point clouds, which at plot resolution would
    // be enormous and are a later slice. Fails with FileExportFailure when the
    // file cannot be written, and with whatever cad::sheetFor refuses.
    [[nodiscard]] katana::core::Status plotToPdf(const QString& path,
                                                 const katana::cad::PlotSettings& settings);

    // `state` belongs to the workspace's ViewSet and outlives this widget: it
    // is what survives when the view changes kind or its dock floats, so the
    // layers hidden in this view, and its pan and zoom, live there.
    ViewportWidget(katana::cad::Document& document, katana::cad::ViewState& state,
                   QWidget* parent = nullptr);

    [[nodiscard]] katana::cad::ViewState& state() const { return state_; }

    // How many entities the last paint (or plot) drew: those that passed the
    // visibility rule for this view and lay in the visible area. For the
    // headless checks, which cannot look at pixels, to prove that a layer
    // hidden in one view is gone from that view and from no other.
    [[nodiscard]] std::size_t lastDrawnEntityCount() const { return lastDrawnEntities_; }

    // Legacy: Select stops the running tool; the others start their
    // catalogue tool (toolId).
    void setTool(Tool tool);
    // The legacy tool running, Select when none is or when the running tool
    // is not one of the eight.
    [[nodiscard]] Tool tool() const;

    // ---- the catalogue's tools (include/katana/cad/interactive_tool.hpp) -----
    // Starts catalogue tool `id` in this view. NotFound for an unknown id.
    [[nodiscard]] katana::core::Status startTool(std::string_view id);
    // The running tool's id, "" when the view is selecting.
    [[nodiscard]] std::string activeToolId() const { return tools_.activeId(); }
    [[nodiscard]] bool toolActive() const { return tools_.active(); }
    // The host itself, for a caller that wants its hooks or to drive it.
    [[nodiscard]] tools::ToolHost& toolHost() { return tools_; }
    // A whole line of typed input handed to the running tool, as the command
    // line hands it over when Enter is pressed there. False, and nothing
    // done, when no tool is running: the line is then the command line's.
    bool typeIntoTool(const QString& text);
    // What has been typed into the view for the running tool and not yet
    // entered: shown after the prompt, sent by Enter, cleared by Esc.
    [[nodiscard]] const QString& typedInput() const { return typed_; }
    // How many rubber-band shapes and markers the last paint drew, for the
    // headless tests that cannot look at pixels.
    [[nodiscard]] std::size_t lastPreviewCount() const { return lastPreviewCount_; }

    // Frames what THIS view draws: the entities its layers let through
    // (cad::drawnExtent with this view's hidden layers), the reference layers
    // visible and not hidden here, the meshes and the alignments.
    void zoomExtents();
    // Frames one specific box, e.g. a single reference layer.
    void zoomTo(const katana::geometry::Box2& bounds);
    // The box zoomExtents frames. Empty when this view has nothing to show.
    [[nodiscard]] katana::geometry::Box2 drawnBounds() const;

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

    // Esc: clears typed input first; then ends the running tool (keeping the
    // work tools::escapeKeepsWork says Esc keeps); with no tool running,
    // abandons a selection box, and then clears the selection.
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
    // Legacy: raised with the legacy tool when one of the eight starts, and
    // with Select when a tool ends.
    std::function<void(Tool tool)> onToolChanged;
    // Raised with the catalogue id when a tool starts in this view, and with
    // "" when it ends.
    std::function<void(const std::string& toolId)> onActiveToolChanged;
    // What a running tool reports that is not an error: "3 lines", "2
    // selected", a measurement. Refusals go to onError.
    std::function<void(const QString& message)> onToolMessage;
    // Raised when the user clicks into this view or moves the keyboard focus
    // into it, so the workspace can make it the active one; not when Qt moves
    // the focus itself (view_focus.hpp, focusChoosesView). It can be raised
    // twice for one click (the press, then the focus it gives); the workspace
    // ignores re-activating the active view.
    std::function<void()> onActivated;
    // Printable text typed into this view that the view has no use for
    // itself: "type anywhere", as in AutoCAD, where typing LINE over the
    // drawing starts the command. The window forwards it to the command line.
    // Never raised for a key the view handles (Esc, Enter, Delete, and C while
    // a polyline is being drawn) or for a Ctrl or Alt chord - those are
    // shortcuts, not text.
    std::function<void(const QString& text)> onTextTyped;
    // A right-click with the Select tool and nothing half-picked, at the
    // screen position the menu should open at; and the keyboard's menu key
    // under the same conditions. While a drawing tool is working, a
    // right-click still finishes or cancels it, and with no handler set it
    // cancels as it always has.
    std::function<void(const QPoint& globalPos)> onContextMenu;

  protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;
    bool event(QEvent* event) override;
    void contextMenuEvent(QContextMenuEvent* event) override;

  private:
    using Point2 = katana::geometry::Point2;

    // Fits `bounds` into the view, and keeps fitting it on every resize until
    // the user pans or zooms. A view is routinely framed and THEN resized: an
    // import frames every view and then opens a 3D view beside the plan,
    // which takes part of its space, and the plan kept a scale chosen for a
    // size it no longer had - the drawing ran off its edges. Once the user
    // has panned or zoomed, a resize keeps their centre and scale instead.
    void frame(const katana::geometry::Box2& bounds);
    // What the first paint of a view that has never been framed does, and
    // the "nothing drawn yet" hint for an empty drawing.
    void frameOnFirstPaint();
    void drawEmptyHint(QPainter& painter) const;
    // True when nothing at all is loaded: no entity, alignment, reference
    // layer or mesh. What the empty-drawing hint is shown for.
    [[nodiscard]] bool drawingIsEmpty() const;

    [[nodiscard]] QPointF toScreen(const Point2& world) const;
    [[nodiscard]] Point2 toWorld(const QPointF& screen) const;

    void updateCursor(const QPointF& screen);
    // A left click while a tool runs, by what the tool expects.
    void toolClick(const QPointF& screen);
    // Enter, Space and a right-click while a tool runs: the typed input when
    // there is some, otherwise the tool's Enter.
    void toolEnter();
    // The view's pick aperture in model units at the current zoom.
    [[nodiscard]] double pickTolerance() const;
    // The entity under `screen` that this view lets be picked, if any.
    [[nodiscard]] std::optional<katana::entity::EntityId> entityAt(const QPointF& screen) const;
    void wireToolHost();
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
    // Draws the definition along the entity's plan shape through the shared
    // style painter. FALSE when no pattern was laid - too fine, too long or
    // nothing to lay it along - so the caller draws the plain line instead:
    // a 12d linestyle replaces the line rather than decorating it.
    [[nodiscard]] bool drawLineStyle(QPainter& painter, const StylePaintTarget& target,
                                     const katana::cad::FlatDefinition& definition,
                                     const katana::entity::Geometry& geometry) const;
    // Model units to one plot millimetre, which is what a `paperstyle` uses.
    [[nodiscard]] double paperScale() const;
    // The plain point mark's half-width in model units at the current scale.
    [[nodiscard]] double plainMarkHalfWidth() const;
    void drawSymbol(QPainter& painter, const StylePaintTarget& target, const std::string& symbol,
                    const Point2& centre, double size) const;
    // Flattened 12d definitions, kept between frames and keyed on the
    // document's library generation, so thousands of coded points do not
    // re-flatten their symbol every frame. Mutable because painting is
    // logically const.
    mutable katana::cad::DefinitionCache definitions_;
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
    // The running tool's prompt and what has been typed for it, in a band
    // along the bottom of the view.
    void drawPrompt(QPainter& painter) const;
    void drawSnapMarker(QPainter& painter) const;

    katana::cad::Document& document_;
    katana::cad::ViewState& state_;
    mutable std::size_t lastDrawnEntities_ = 0;
    // Declared after document_ and destroyed before anything else here: the
    // listener it owns captures this widget.
    katana::cad::Document::ListenerHandle documentListener_;
    // The pan and zoom are state_.plan and whether the view has been framed
    // is state_.planFramed: both outlive this widget. This is only the box of
    // the last frame, refitted on a resize until the user pans or zooms; a
    // widget rebuilt over the same state starts without it, which errs on the
    // side of keeping the zoom.
    std::optional<katana::geometry::Box2> framedBox_;

    // The tool running in this view, if any. Declared after document_, which
    // it holds a reference to.
    tools::ToolHost tools_;
    QString typed_;
    mutable std::size_t lastPreviewCount_ = 0;
    Point2 cursorWorld_; // after snapping
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
    // The plot's settings while plotting, for the paper colour rule (D7).
    katana::cad::PlotSettings plotSettings_{};

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
