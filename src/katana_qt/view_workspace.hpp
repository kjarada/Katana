#pragma once

// The drawing views as docks (the user's request of 2026-09-23: views that
// dock, move, minimise, close and go out onto another screen).
//
// This replaces ViewportContainer, which tiled the views into fixed cells of
// one central widget. That design made a view's identity its POSITION: every
// layout change destroyed and rebuilt every widget, so the plan's zoom, a
// second section and any half-picked clicks were lost whenever the user - or
// an import that opened a 3D view - changed the split.
//
// Here each view is a QDockWidget inside a nested QMainWindow, and its state
// (katana::cad::ViewState: camera, plan zoom, hidden layers, section) lives in
// the tested katana::cad::ViewSet for as long as the view is open. Qt's own
// dock machinery gives splitting, tabbing and floating onto any screen; the
// layout presets survive as arrangements (cad::dockSplits). A nested window,
// rather than putting views among the main window's panels, keeps the two
// apart: an arrangement moves views and never the Layers panel, and a panel
// cannot be dropped between two views.
//
// Each dock holds one of three widgets:
//   Plan       ViewportWidget      - the 2D editor
//   Model3D    RenderViewWidget    - orbiting software-rendered 3D
//   Elevation  RenderViewWidget    - the same, locked orthographic
//   Section    SectionViewWidget   - station against elevation
//
// Only Plan views edit; the others are views. The window's grid and snap
// settings apply to EVERY plan view - a setting is not a property of whichever
// view was clicked last - and are remembered so a view opened later matches.
// A TOOL is different: it holds picks made in one view, so it runs in the
// active plan view alone (startTool), and starting one stops any other.

#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include <QDockWidget>
#include <QMainWindow>
#include <QPointer>

#include "dock_chrome.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/cad/viewport_layout.hpp"
#include "render_view_widget.hpp"
#include "section_view_widget.hpp"
#include "viewport_widget.hpp"

class QToolButton;

namespace katana::qt {

class ViewLayersPopup;

// A view's dock. Closing it closes the VIEW (the panels only hide), and it is
// asked for rather than done inside the close event: the close may come from
// a button inside this very dock, and deleting a widget from inside its own
// event handler is a use-after-free.
class ViewDock final : public QDockWidget {
  public:
    ViewDock(katana::cad::ViewId id, QWidget* parent);
    [[nodiscard]] katana::cad::ViewId viewId() const { return id_; }
    std::function<void()> onCloseRequested;

  protected:
    void closeEvent(QCloseEvent* event) override;

  private:
    katana::cad::ViewId id_;
};

class ViewWorkspace final : public QMainWindow {
  public:
    // Opens one plan view, docked: a headless --plot needs a plan view in a
    // window that is never shown.
    explicit ViewWorkspace(katana::cad::Document& document, QWidget* parent = nullptr);
    // Deletes the view docks while the ViewSet their widgets point into is
    // still alive; Qt would otherwise delete them after it.
    ~ViewWorkspace() override;

    ViewWorkspace(const ViewWorkspace&) = delete;
    ViewWorkspace& operator=(const ViewWorkspace&) = delete;

    // ---- chrome: title bars, the tray, maximise, per-view layers --------------
    // Gives every open and future view the main window's title bar: its kind
    // switcher, its Layers button, Zoom Extents and the window buttons, with
    // the active view's bar marked. The chrome belongs to the main window,
    // which shares it with its panels so that one tray holds everything
    // minimised. Without one (a workspace built on its own) the views keep
    // Qt's plain title bar.
    void setChrome(DockChrome* chrome);
    [[nodiscard]] DockChrome* chrome() const { return chrome_; }

    // What the per-view Layers popup reads: the document's layer tree and the
    // window's reference layers (null until the window supplies them).
    [[nodiscard]] katana::cad::Document& document() const { return document_; }
    [[nodiscard]] katana::interop::ReferenceData* referenceData() const { return reference_; }

    // Opens the Layers popup of a view under its Layers button - or under its
    // title bar when it has no chrome. Null for an id that is not open. The
    // popup deletes itself when it closes.
    ViewLayersPopup* showLayersPopup(katana::cad::ViewId id);
    // How many layers and reference layers a view hides that still exist:
    // what its Layers button reports. 0 for an id that is not open.
    [[nodiscard]] std::size_t hiddenCount(katana::cad::ViewId id) const;

    // ---- views ---------------------------------------------------------------
    [[nodiscard]] katana::cad::ViewSet& viewSet() { return views_; }
    [[nodiscard]] const katana::cad::ViewSet& viewSet() const { return views_; }

    // A new view of `kind`, splitting the active view into equal halves -
    // side by side when the active view is wider than it is tall, stacked
    // when it is taller - or on its own when no view is docked. A maximised
    // view is restored first. Activates it when `activate` is true, and also
    // when the active view is hidden (minimised), so that the view the menus
    // act on is never one the user cannot see while another shows.
    katana::cad::ViewState& openView(katana::cad::ViewKind kind, bool activate = true);
    // Closes a view: its dock and widget are deleted and its state dropped.
    // When it was the active view, the one activated in its place is one the
    // user can see, never a minimised view or a tab page left behind another.
    // NotFound for an id that is not open.
    [[nodiscard]] katana::core::Status closeView(katana::cad::ViewId id);
    // Changes what a view shows, keeping its dock where it is and the rest of
    // its state. NotFound for an id that is not open.
    [[nodiscard]] katana::core::Status setViewKind(katana::cad::ViewId id,
                                                   katana::cad::ViewKind kind);
    // NotFound for an id that is not open.
    [[nodiscard]] katana::core::Status activateView(katana::cad::ViewId id);
    // The view of `kind` the user used most recently, shown and raised; or a
    // new one beside the active view when none is open. What "show the 3D
    // view" means now: never turn a view the user is working in into another.
    katana::cad::ViewState& ensureView(katana::cad::ViewKind kind);
    // Re-docks the open views into a preset arrangement (cad::dockSplits),
    // opening views of the default kinds when fewer are docked than the preset
    // has places. Extra docked views are tabbed onto the last place; floating
    // views stay where the user put them. A docked view that was minimised
    // takes its place in the arrangement and leaves the tray - the user asked
    // for the arrangement, and a second 3D view opened while the first sat
    // minimised would be a duplicate. A maximised view is restored first.
    void arrange(katana::cad::LayoutKind kind);

    [[nodiscard]] ViewDock* dockFor(katana::cad::ViewId id) const;
    [[nodiscard]] QWidget* widgetFor(katana::cad::ViewId id) const;
    [[nodiscard]] ViewportWidget* planView(katana::cad::ViewId id) const;
    [[nodiscard]] RenderViewWidget* renderView(katana::cad::ViewId id) const;
    [[nodiscard]] SectionViewWidget* sectionView(katana::cad::ViewId id) const;

    // The active view's kind; Plan when none is open.
    [[nodiscard]] katana::cad::ViewKind activeViewKind();
    // The active view when it is of the kind asked for, else the one of that
    // kind used most recently, else null. Plot, F9, the Standard Views and the
    // vertical exaggeration act through these.
    [[nodiscard]] ViewportWidget* activePlanView();
    [[nodiscard]] RenderViewWidget* activeRenderView();
    [[nodiscard]] SectionViewWidget* activeSectionView();
    [[nodiscard]] std::vector<ViewportWidget*> planViews() const;

    // ---- what every view is given --------------------------------------------
    // Reference data and surfaces are owned by the window; the views borrow
    // them. Passing them here re-points every open and future view.
    void setReferenceData(katana::interop::ReferenceData* reference);
    void setSurfaces(const std::vector<katana::cad::SceneSurface>* surfaces);
    void setMeshes(const std::vector<katana::cad::SceneMesh>* meshes);
    void setSceneOptions(const katana::cad::SceneOptions& options);
    [[nodiscard]] const katana::cad::SceneOptions& sceneOptions() const { return options_; }

    // Shows `section` in the section view used most recently, opening one
    // beside the active view when none is open. Never replaces another view.
    bool showSection(katana::cad::Section section);

    // ---- the tools (include/katana/cad/interactive_tool.hpp) ------------------
    // Starts catalogue tool `id` in the active plan view, first stopping, as
    // Esc stops it, a tool running in any other plan view. NotFound for an id
    // the catalogue does not have; InvalidState when no plan view is open.
    [[nodiscard]] katana::core::Status startTool(std::string_view id);
    // The running tool's id in whichever plan view runs one, "" when none.
    [[nodiscard]] std::string activeToolId() const;
    // True when the running tool is waiting for typed text - a Text's
    // string, a height, a count (cad::ToolInput::Value) - rather than a point
    // or a pick. The command line then hands it a typed line before reading
    // the line as any verb: a text may well begin "Utility" or "Online".
    [[nodiscard]] bool toolTakesText() const;
    // Hands a whole typed line to the running tool - the command line's
    // Enter while a tool runs. False, and nothing done, when no tool runs:
    // the line is then a command.
    bool typeIntoTool(const QString& text);
    // Select: stops the tool running in any plan view, as Esc stops it (a
    // Line chain keeps its finished segments), so the view selects again.
    void stopTool();
    // Enter pressed on an empty command line is Enter pressed in the
    // drawing, as in AutoCAD: it goes to the plan view running a tool (ends
    // a polyline, takes a prompt's default), or with none running to the
    // active plan view, which starts the last tool again. False when no plan
    // view is open.
    bool pressEnter();

    // ---- settings every plan view shares -------------------------------------
    void setGridVisible(bool visible);
    [[nodiscard]] bool gridVisible() const { return gridVisible_; }
    void setSnapEnabled(bool enabled);
    [[nodiscard]] bool snapEnabled() const { return snapEnabled_; }
    void setSnapModes(katana::cad::SnapModes modes);
    [[nodiscard]] katana::cad::SnapModes snapModes() const { return snapModes_; }
    // Esc: the plan views running a tool (or holding typed input for one)
    // cancel it; when none is, every plan view cancels (clears its box and
    // the selection).
    void cancel();
    // Every plan view, floating and minimised ones included: the clicks
    // collected anywhere belong to the drawing that is going away.
    void resetInteraction();
    // The drawing was replaced (File > New, Open) and the window has emptied
    // the surfaces and meshes it lends the views. Every 3D view rebuilds its
    // WHOLE scene - the document's own notification marks only the entities
    // dirty, so the terrain layer kept the previous drawing's surface - and
    // every section, docked or kept by a view of another kind, is dropped:
    // it was cut from surfaces that are gone.
    void drawingReplaced();

    // Frames the ACTIVE view only, as the View menu and the Z command mean it;
    // one view's zoom is not another's business.
    void zoomExtents();
    // Frames one view, whether active or not: its title bar's Zoom Extents
    // button. NotFound for an id that is not open.
    [[nodiscard]] katana::core::Status zoomExtents(katana::cad::ViewId id);
    // Frames every view: after New, Open and an import, when all of them are
    // looking at a drawing that has just changed under them.
    void zoomExtentsAll();
    // Frames `bounds` in every plan view.
    void zoomTo(const katana::geometry::Box2& bounds);
    void invalidateReferenceCache();
    // Repaints every view and rebuilds every 3D scene.
    void refreshAll();
    // Repaints every view without rebuilding anything. A floating dock is a
    // window of its own, so update() on this widget no longer reaches it.
    void repaintViews();
    // Repaints one view after its own hidden layers or references changed:
    // not a document change, so no listener hears it. Also what keeps the
    // view's Layers button saying whether the view hides anything.
    void viewLayersChanged(katana::cad::ViewId id);
    // Drops per-view hidden layers that name no layer any more (after a delete
    // or a rename), in every view. See LayerOverrides::pruneMissing.
    void pruneViewLayers();

    // Forwarded from whichever view raised it, so the window wires them once.
    std::function<void(const QString&)> onStatus;
    // A view's own running readout (RenderViewWidget and SectionViewWidget
    // onFrameStats): a 3D view's frame time, a section's station and
    // elevation under the cursor. Kept apart from onStatus, which is for
    // messages, so the window can show it where it never hides one.
    std::function<void(const QString&)> onFrameStats;
    std::function<void()> onActiveChanged;
    // Raised when a view opens, closes or changes kind: the Window menu lists
    // them.
    std::function<void()> onViewsChanged;
    std::function<void(const QString&)> onPrompt;
    std::function<void(const QString&)> onError;
    std::function<void(const katana::geometry::Point2&,
                       const std::optional<katana::cad::SnapResult>&)>
        onCursorMoved;
    // A tool started (its catalogue id) or ended ("") in any plan view.
    std::function<void(const std::string& toolId)> onActiveToolChanged;
    // What a running tool reports that is not an error ("3 lines", a
    // measurement). Unset, it goes to onStatus, so it is never lost.
    std::function<void(const QString&)> onToolMessage;
    // Printable text typed into a plan view with no tool running ("type
    // anywhere": LINE typed over the drawing starts it), for the window's
    // command line. What is typed while a tool runs is the tool's and never
    // comes here (ViewportWidget::onTextTyped).
    std::function<void(const QString&)> onTextTyped;
    // A plan view's shortcut menu, asked for with no tool running
    // (ViewportWidget::onContextMenu, which has selected the entity under the
    // cursor when nothing was selected), at this screen position.
    std::function<void(const QPoint& globalPos)> onContextMenu;
    // A double click on an entity in a plan view with no tool running
    // (ViewportWidget::onEntityDoubleClicked).
    std::function<void(katana::entity::EntityId id)> onEntityDoubleClicked;
    // The tool Enter at no prompt would run again, in any plan view; "" until
    // one has run. What the shortcut menu's Repeat names.
    [[nodiscard]] const std::string& lastToolId() const { return lastToolId_; }

  private:
    struct View {
        katana::cad::ViewId id = katana::cad::kNoView;
        ViewDock* dock = nullptr;
        ViewportWidget* plan = nullptr;
        RenderViewWidget* render = nullptr;
        SectionViewWidget* section = nullptr;
        // The chrome's bar and the view's own tools on it; null without chrome.
        DockTitleBar* titleBar = nullptr;
        QToolButton* kindButton = nullptr;
        QToolButton* layersButton = nullptr;
        [[nodiscard]] QWidget* widget() const;
    };

    [[nodiscard]] View* find(katana::cad::ViewId id);
    [[nodiscard]] const View* find(katana::cad::ViewId id) const;
    // Builds the widget for the state's kind and puts it in the dock, deleting
    // what the dock held before.
    void buildContent(View& view, katana::cad::ViewState& state);
    // Stops the tool running in `view`'s plan view, as Esc stops it, when it
    // runs one. Called before that widget is deleted - the view changing kind
    // (buildContent) or closing (closeView) - because a ViewportWidget
    // deleted mid-tool never reports that its tool ended, and the window
    // would go on showing the tool's action checked with nothing running.
    void stopToolIn(const View& view);
    void activate(katana::cad::ViewId id);
    void updateTitle(const View& view);
    [[nodiscard]] ViewContext contextFor() const;
    // Puts the chrome's title bar on a view's dock with the view's own tools.
    void installChrome(View& view);
    // Marks the active view's title bar and unmarks the rest.
    void updateActiveMarks();
    // Where the user can see it: not hidden, and floating or within this
    // window (a tab page behind another is parked outside it).
    [[nodiscard]] bool onScreen(const View& view) const;
    // After `hidden` was hidden (minimised) while it was the active view:
    // activates a view on screen, the first in the order opened; when none
    // is by geometry (a window not yet laid out), the first unhidden view,
    // raised so that it is the current page of its tab group. Nothing when no
    // other view is open and unhidden.
    void activateShowingInsteadOf(katana::cad::ViewId hidden);
    // The Layers button's icon and tooltip say whether the view hides anything.
    void updateLayersButton(const View& view);
    // Forwards a plan view's tool hooks to this workspace's, and sends the
    // view's Enter-repeat (onRepeatTool) through startTool as view `id`.
    // Its typed text too (onTextTyped), its shortcut menu and its double
    // click on an entity.
    void wireTools(ViewportWidget& plan, katana::cad::ViewId id);

    katana::cad::Document& document_;
    // Owned by the main window, which deletes its children in the order it
    // made them; a QPointer so that this workspace, whose destructor still
    // deletes docks the chrome watches, does not depend on that order.
    QPointer<DockChrome> chrome_;
    katana::cad::ViewSet views_;
    std::vector<View> docks_;

    katana::interop::ReferenceData* reference_ = nullptr;
    const std::vector<katana::cad::SceneSurface>* surfaces_ = nullptr;
    const std::vector<katana::cad::SceneMesh>* meshes_ = nullptr;
    katana::cad::SceneOptions options_{};

    // The last tool started in any plan view, which Enter at no prompt in any
    // of them repeats; "" until one has run.
    std::string lastToolId_;
    bool gridVisible_ = true;
    bool snapEnabled_ = true;
    katana::cad::SnapModes snapModes_ = katana::cad::kDefaultSnapModes;
};

} // namespace katana::qt
