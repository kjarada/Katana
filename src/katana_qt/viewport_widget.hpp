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

#include <cstdint>
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
#include "drawing/grip_controller.hpp"
#include "plan_painter.hpp"
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

    // Plots the drawing as this view shows it - its hidden layers and
    // reference layers - through plotPlanToPdf (plan_painter.hpp): the same
    // painter as the screen, on paper. Fails as that does.
    [[nodiscard]] katana::core::Status plotToPdf(const QString& path,
                                                 const katana::cad::PlotSettings& settings);
    // `settings` fitted to what this view draws (drawnBounds, as Zoom Extents
    // frames it): the first standard scale the drawing fits the printable
    // area at, centred on it; the sheet otherwise as given. What Plot's Fit
    // plots. Fails with InvalidArgument when the view draws nothing, and
    // with whatever cad::fitScale refuses.
    [[nodiscard]] katana::core::Result<katana::cad::PlotSettings>
    fittedPlot(katana::cad::PlotSettings settings) const;
    // The drawing as this view shows it, painted by the same plan painter
    // into an image of `size` pixels on `background` (transparent for none):
    // the view's centre, at the scale that fits what the view shows into the
    // image, paper-sized marks scaled alike - so twice the view's size is the
    // view at twice the resolution. The view's furniture (the grid, a tool's
    // preview, the snap marker, the prompt) is left out. `asPlotted` draws
    // it as a plot does, for a white ground: white pens black (D7) and line
    // weights in millimetres of the image. Read-only: the view and its kept
    // drawing are untouched. For SNAPSHOT (plotting/view_image_export.hpp).
    [[nodiscard]] QImage renderToImage(const QSize& size, const QColor& background,
                                       bool asPlotted = false) const;

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
    // Of the selection, how many the last paint drew as ghosts: selected, on
    // a layer the document shows and this view hides
    // (PlanPaintStats::ghostsDrawn; ViewState::selectionGhosts).
    [[nodiscard]] std::size_t lastGhostCount() const { return lastGhosts_; }
    // The grips this view offers on the selection as its last paint built
    // them: none on an entity on a layer this view hides, however selected.
    [[nodiscard]] std::size_t gripCount() const { return grips_.grips().size(); }

    // ---- what a frame costs -------------------------------------------------
    // The drawing is painted into an image kept between paints, and a paint
    // whose view, drawing, layers, library, selection, reference layers and
    // meshes are all as they were only lays that image down and draws the
    // view's own furniture over it (tool previews, the snap marker, the
    // selection box, the prompt). A mouse move then costs the furniture, not
    // the drawing: measured on a 27.6k-entity survey drawing it was a whole
    // repaint every move. How many times the drawing itself has been painted,
    // for the tests that prove a move does not repaint it.
    [[nodiscard]] std::size_t drawingPaintCount() const { return drawingPaints_; }
    // The last paint, whole, and the last time the drawing was painted, in
    // milliseconds: what onFrameStats reports.
    [[nodiscard]] double lastFrameMilliseconds() const { return lastFrameMs_; }
    [[nodiscard]] double lastDrawingMilliseconds() const { return lastDrawingMs_; }
    // Raised after every paint with the view's statistics - "Plan  N drawn
    // X ms", the drawing's own time and whether this frame reused it - as the
    // 3D view reports its triangles and frame time.
    std::function<void(const QString&)> onFrameStats;

    // Every plan view's lines on screen as a cosmetic one-pixel pen instead
    // of the 1.5 px hairline: 5-8x cheaper to stroke (docs/plan_view.md). A
    // process-wide choice, the View menu's "Thin screen lines (faster)", on
    // by default; plots keep their paper-millimetre weights either way.
    // Views read it at their next paint; the caller repaints them.
    static void setThinScreenLines(bool thin);
    [[nodiscard]] static bool thinScreenLines();

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
    // What the running tool wants next; nullopt when none runs.
    [[nodiscard]] std::optional<katana::cad::ToolInput> toolExpects() const
    {
        return tools_.expects();
    }
    // The host itself, for a caller that wants its hooks or to drive it.
    [[nodiscard]] tools::ToolHost& toolHost() { return tools_; }
    // The grips shown on the selection while no tool runs (docs/drawing.md).
    [[nodiscard]] drawing::GripController& gripController() { return grips_; }
    // The grip under `screen` (a pixel of this view) within the pick
    // aperture, when the grips are showing: what the shortcut menu is
    // opened on, when it is opened on one.
    [[nodiscard]] std::optional<katana::cad::Grip> gripAt(const QPointF& screen);
    // The points object snap tracking has acquired, most recent first.
    [[nodiscard]] const std::vector<katana::geometry::Point2>& trackingPoints() const
    {
        return tracking_.points();
    }
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
    // What the last paint drew of the running tool's preview, by role
    // (drawing/feedback_painter.hpp), with its caption: what the headless
    // pointer record prints and the widget tests assert.
    struct PreviewCounts {
        std::size_t shapes = 0;
        std::size_t markers = 0;
        std::size_t target = 0;
        std::size_t added = 0;
        std::size_t removed = 0;
        std::size_t enter = 0;
        std::size_t focus = 0;
        bool refused = false;
        std::string caption;
    };
    [[nodiscard]] const PreviewCounts& lastPreviewCounts() const { return lastPreviewCounts_; }
    // The running tool's prompt as the band along the bottom of the view last
    // drew it, cut to the view's width - in the middle with nothing typed,
    // so the tool's name and what Enter does both stay; empty with no tool.
    [[nodiscard]] const QString& promptBandText() const { return promptBand_; }
    // The pointer at model point `at` as a person's mouse is: the view is
    // painted, `at` mapped through its transform, a real move sent there and,
    // with `click`, a left press and release with `modifiers`; then painted
    // again. For the headless --hover and --click steps (docs/headless.md),
    // which have no mouse. Answers the record they print - "pointer:
    // action=hover x= y= tool= expects= shapes= markers= target= added=
    // removed= enter= focus= refused= caption= prompt=", the caption and the
    // prompt quoted as every reply's text is (core::replyQuoted), so
    // core::readReplyRecord reads it back. InvalidArgument for a point
    // outside the view.
    [[nodiscard]] katana::core::Result<std::string>
    pointerAt(const katana::geometry::Point2& at, bool click,
              Qt::KeyboardModifiers modifiers = Qt::NoModifier);

    // Frames what THIS view draws: the entities its layers let through
    // (cad::drawnExtent with this view's hidden layers), the reference layers
    // visible and not hidden here, the meshes and the alignments.
    void zoomExtents();
    // Frames one specific box, e.g. a single reference layer.
    void zoomTo(const katana::geometry::Box2& bounds);
    // The box zoomExtents frames. Empty when this view has nothing to show.
    [[nodiscard]] katana::geometry::Box2 drawnBounds() const;

    // Raised when this view's pan or zoom has just changed: a wheel notch, a
    // middle-drag pan and a frame (Zoom Extents, zoomTo) with `byUser` true,
    // the first paint's frame with false - on nothing too, when the view
    // draws nothing and keeps the place it started at. What linked views
    // follow (ViewWorkspace::viewMoved). Never raised by holdView or by a
    // change made to the state from outside, so a view following another
    // never reports the move it was given.
    std::function<void(bool byUser)> onViewMoved;
    // The Zoom Extents a middle double-click asks for, done by the host when
    // it sets this: it knows the link, and a linked view that draws nothing
    // frames what the link draws rather than the origin
    // (ViewWorkspace::zoomExtents), as GpuSceneView::onZoomExtents hands a 3D
    // view's to its host. Unset, zoomExtents().
    std::function<void()> onZoomExtents;
    // Keeps the view where its state now says, as a linked view must: forgets
    // the box it was framed on, so a resize keeps the centre and scale rather
    // than refitting that box, and repaints. Raises nothing.
    void holdView();
    // Frames the view as its first paint would, unless it has been framed:
    // for a ZOOM asked of a view not yet painted - a script's VIEWS OPEN plan
    // then ZOOM IN, which run with no paint between - so the zoom starts
    // from what the view draws and not from the place a new view holds
    // until then (ViewWorkspace::zoomView).
    void frameIfUnframed();

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
    // The snaps are the document's drafting settings (Document::drafting),
    // shared by every view and the SNAP verb.
    [[nodiscard]] bool snapEnabled() const { return document_.drafting().snapEnabled; }
    void setSnapModes(katana::cad::SnapModes modes);
    [[nodiscard]] katana::cad::SnapModes snapModes() const { return document_.drafting().snapModes; }

    // Esc: clears typed input first; then ends the running tool (keeping the
    // work tools::escapeKeepsWork says Esc keeps); with no tool running,
    // abandons a selection box, and then clears the selection.
    void cancel();

    // Abandons the operation in progress: ends the running tool WITHOUT
    // committing anything (tools::ToolHost::abandon) and drops typed input
    // and a selection box. Required whenever the document under the view is
    // replaced: the collected clicks belong to the drawing that is going away,
    // and committing them into the new one silently produces an entity at
    // coordinates the user never picked there. Safe before or after the
    // replacement, since nothing is rebuilt from the document here.
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
    // Enter or Space with no tool running in this view: repeat the last tool.
    // Set by whoever keeps one tool running per workspace (ViewWorkspace),
    // which knows the last tool the user ran in ANY view and stops one still
    // running in another. Unset, the view starts the last tool it ran itself.
    std::function<void()> onRepeatTool;
    // Raised when the user clicks into this view or moves the keyboard focus
    // into it, so the workspace can make it the active one; not when Qt moves
    // the focus itself (view_focus.hpp, focusChoosesView). It can be raised
    // twice for one click (the press, then the focus it gives); the workspace
    // ignores re-activating the active view.
    std::function<void()> onActivated;
    // Printable text typed into this view that the view has no use for
    // itself: "type anywhere", as in AutoCAD, where typing LINE over the
    // drawing starts the command. The window forwards it to the command line.
    // Never raised for a key the view handles (Esc, Enter, Space, Delete), for
    // anything typed while a tool runs (that is the tool's input, kept in
    // typedInput()), or for a Ctrl or Alt chord - those are shortcuts, not
    // text.
    std::function<void(const QString& text)> onTextTyped;
    // A right-click with no tool running and no selection box being dragged,
    // at the screen position the menu should open at; and the keyboard's
    // menu key under the same conditions. While a tool runs, a right-click
    // is Enter instead, and with no handler set a right-click cancels (Esc).
    // With nothing selected, the entity under the cursor is selected first,
    // so the menu acts on what was clicked; a selection there already is
    // kept, as it is the menu's subject.
    std::function<void(const QPoint& globalPos)> onContextMenu;
    // A double click on an entity with no tool running, after its first
    // click has selected it: the window opens what edits it (a text's
    // editor, or the Properties panel). Not raised on empty space or while a
    // tool runs, where the second click is the tool's.
    std::function<void(katana::entity::EntityId id)> onEntityDoubleClicked;

  protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;
    void keyReleaseEvent(QKeyEvent* event) override;
    // The pointer has left the view: no preview until it is back (pointerSeen_).
    void leaveEvent(QEvent* event) override;
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
    // Its snap aperture, the reach of an Endpoint snap and of a vertex pick.
    [[nodiscard]] double snapAperture() const;
    // The running tool's prompt as the band and the command line show it:
    // "Insert Vertex: Click on polyline 2 where the new vertex goes".
    [[nodiscard]] QString promptLine() const;
    // The entity under `screen` that this view lets be picked, if any.
    [[nodiscard]] std::optional<katana::entity::EntityId> entityAt(const QPointF& screen) const;
    void wireToolHost();
    void selectAt(const QPointF& screen, Qt::KeyboardModifiers modifiers);
    // Before the shortcut menu opens at `screen`: the entity there becomes
    // the selection when nothing is selected.
    void selectForMenu(const QPointF& screen);
    void selectInBox(const QPointF& from, const QPointF& to, Qt::KeyboardModifiers modifiers);
    // What a box from `from` to `to` picks through this view's layers: what
    // it encloses dragged left to right, what it touches right to left.
    [[nodiscard]] std::vector<katana::entity::EntityId> pickedInBox(const QPointF& from,
                                                                    const QPointF& to) const;
    // A click (or, `dragged`, a box) at a running tool's selection step. It
    // GATHERS, as AutoCAD's "Select objects" does: a plain click or box adds
    // what it picks, and one with Shift or Ctrl held (`takeOut`) removes it.
    // Replacing the selection at each plain click, as the Select tool does,
    // would leave only the last of several cutting edges picked; and the
    // Select tool's Ctrl toggles and its Shift adds, so neither could take a
    // whole window of picks back out.
    void gatherForTool(const QPointF& from, const QPointF& to, bool dragged, bool takeOut);
    // Typed text for the running tool; U (or Undo) at a selection step is
    // stepBack, so it takes back a click as Ctrl+Z does.
    void sendTyped(const std::string& text);
    // Ctrl+Z while a tool runs: at a selection step, the last change there
    // (selectionSteps_); otherwise, or with none left, the tool's own undo.
    void stepBack();
    // selectionSteps_, emptied first if the tool it was kept for has been
    // replaced, restarted or ended since (ToolHost::generation).
    std::vector<std::optional<std::vector<katana::entity::EntityId>>>& selectionSteps();
    void run(katana::commands::CommandPtr command);
    void updatePrompt();

    // What the plan painter draws from and through (plan_painter.hpp): the
    // document with the window's reference data and meshes, this view's
    // transform and hidden layers, and the screen.
    [[nodiscard]] PlanSource paintSource() const;
    [[nodiscard]] PlanFrame paintFrame() const;
    [[nodiscard]] PlanPaintOptions screenOptions() const;

    // Everything the kept drawing image depends on: when any of it differs
    // from the last paint's, the drawing is painted again. The document's
    // notifications count every command, undo, selection change, library
    // and current-layer change; the fingerprints catch what changes without
    // one - a reference layer shown or hidden in its panel, a mesh's style.
    struct DrawingKey {
        double centreX = 0.0;
        double centreY = 0.0;
        double scale = 0.0;
        double width = 0.0;
        double height = 0.0;
        double deviceRatio = 0.0;
        std::uint64_t notifications = 0;
        std::uint64_t modelRevision = 0;
        std::uint64_t libraryGeneration = 0;
        std::uint64_t selection = 0;
        std::uint64_t references = 0;
        std::uint64_t meshes = 0;
        katana::cad::LayerOverrides layers;
        std::set<std::uint64_t> hiddenReferences;
        bool grid = false;
        bool thinLines = false;
        bool ghosts = false; // ViewState::selectionGhosts
        friend bool operator==(const DrawingKey&, const DrawingKey&) = default;
    };
    [[nodiscard]] DrawingKey drawingKey(double deviceRatio) const;
    // The running tool's preview; answers whether it drew the snap marker
    // too, beneath its glyphs (FeedbackFrame::beneathGlyphs), or left it to
    // be drawn after.
    [[nodiscard]] bool drawPreview(QPainter& painter) const;
    // The running tool's prompt and what has been typed for it, in a band
    // along the bottom of the view.
    void drawPrompt(QPainter& painter) const;
    void drawSnapMarker(QPainter& painter) const;
    // The grips of the selection, and a grip being dragged, over the drawing.
    void drawGrips(QPainter& painter) const;
    // Whether grips take the input now: no tool running.
    [[nodiscard]] bool gripsLive() const { return !tools_.active(); }

    katana::cad::Document& document_;
    katana::cad::ViewState& state_;
    mutable std::size_t lastDrawnEntities_ = 0;
    mutable std::size_t lastGhosts_ = 0;
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
    // The selection's grips; declared after document_, which it refers to.
    drawing::GripController grips_;
    QPointF gripPress_; // where a grip press began, to tell a drag from a click
    QString typed_;
    // What each change at the running tool's selection step replaced, for
    // Ctrl+Z there. A click or box there changes the DOCUMENT's selection,
    // which the tool reads at Enter (selectionNow), so the tool never sees it
    // and cannot step it back. An entry is the selection before a click or
    // box changed it, or nullopt for an input the tool took itself (All),
    // which the tool steps back: one list, so both go back in the order made.
    std::vector<std::optional<std::vector<katana::entity::EntityId>>> selectionSteps_;
    std::uint64_t selectionStepsOf_ = 0; // the ToolHost::generation it is for
    // The tool Enter at no prompt runs again when onRepeatTool is unset; ""
    // until one has run here.
    std::string lastToolId_;
    mutable std::size_t lastPreviewCount_ = 0;
    mutable PreviewCounts lastPreviewCounts_;
    Point2 cursorWorld_; // after snapping
    // Whether the pointer is over this view: until it first is, cursorWorld_
    // is a place nobody pointed at - the model's origin - and a preview there
    // (a started tool's, a headless run's) would mark whatever vertex lies at
    // 0,0; once it has left, cursorWorld_ is where it left, and a tool
    // started from a menu or a toolbar previewed there - a vertex dragged to
    // the view's edge, a refusal at a vertex nobody was pointing at. Set by
    // every move (updateCursor), cleared by leaveEvent.
    bool pointerSeen_ = false;
    // The prompt band's line as last drawn, cut to fit (promptBandText).
    mutable QString promptBand_;
    std::optional<katana::cad::SnapResult> activeSnap_;
    // What constrained the cursor (Ortho, Polar 45°, a lock): its tooltip.
    QString trackingLabel_;
    // Object snap tracking (docs/drawing.md): the points acquired, and the
    // one the cursor's tracking path runs from, drawn dotted.
    katana::cad::TrackingPoints tracking_;
    std::optional<Point2> trackingFrom_;

    bool gridVisible_ = true;

    katana::interop::ReferenceData* reference_ = nullptr;
    const std::vector<katana::cad::SceneMesh>* meshes_ = nullptr;

    // What the painter keeps between frames - flattened definitions, dash
    // patterns, fonts, imagery and point-cloud colours - owned here, one per
    // view, and a second for plots so a plot does not throw away the
    // screen's. Mutable because painting is logically const.
    mutable PlanPaintCache paintCache_;
    PlanPaintCache plotCache_;

    // The drawing as last painted, and what it was painted for.
    QImage drawing_;
    std::optional<DrawingKey> drawingKey_;
    std::size_t drawingPaints_ = 0;
    // Counted by the document listener: one per notification.
    std::uint64_t notifications_ = 0;
    // Counted when the window says its reference data changed.
    std::uint64_t referenceRevision_ = 0;
    double lastFrameMs_ = 0.0;
    double lastDrawingMs_ = 0.0;

    bool panning_ = false;
    QPointF lastMouse_;
    std::optional<QPointF> boxStart_; // selection rectangle anchor (screen)
    QPointF boxEnd_;
};

} // namespace katana::qt
