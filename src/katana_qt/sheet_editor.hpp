#pragma once

// The sheet editor: a project's sheets on paper, laid out by hand or
// generated, and plotted (docs/plotting.md, "The sheet editor").
//
// A window of its own beside the drawing, as the style manager is, rather
// than a view kind: sheets are paper, not a way of looking at the model, and a
// sheet set is edited while the drawing views go on showing the drawing.
//
//   the sheets    listed on the left with thumbnails: add, duplicate, remove,
//                 reorder (drag), rename
//   the canvas    the current sheet painted by the sheet painter - exactly
//                 what plots - with its viewports picked (one, several, a
//                 rubber band), dragged and resized on handles, snapped to the
//                 drawing area, each other and the paper grid, copied and
//                 pasted; rulers in paper millimetres, and the paper and world
//                 position under the cursor; Shift-drag pans the drawing inside
//                 a plan viewport
//   properties    on the right: the selected viewport's, else the sheet's,
//                 with the title-block values this sheet overrides
//   the toolbar   generate sheets, add a view, tile with a preset, the
//                 title block and logo, plot the sheet or the whole set
//   the checks    below: what a plot would get wrong, checked again a moment
//                 after each change (plotting/sheet_checks.hpp)
//
// Every change is ONE undoable step through sheet_commands.hpp, a drag
// committed on release, so the command stack never fills with the positions a
// drag passed through. The editor listens to the document and redraws on
// every change, undo and redo included.

#include <cstddef>
#include <functional>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <QImage>
#include <QMainWindow>
#include <QPointF>
#include <QWidget>

#include "command_runner.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/preflight.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"
#include "katana/cad/plotting/viewport_edits.hpp"
#include "katana/core/error.hpp"
#include "plotting/sheet_readout.hpp"
#include "plotting/sheet_thumbnails.hpp"
#include "sheet_painter.hpp"

class QDialog;
class QListWidget;
class QScrollArea;
class QLabel;
class QTimer;

namespace katana::qt {

class SheetEditor;
class SheetChecksDock;

// What the sheet verbs are told that the document does not hold, from what
// the sheets are drawn from (`source`): the extent GENERATE covers and the
// surfaces its sections cut, SHEETS CHECK with the painter's knowledge, what
// a view shows (with its imagery), an automatic section's fit and the set as
// drawn (plotting::SheetVerbContext). The window's command line and the
// editor's own lines are given this one, so a line means the same typed or
// built by a dialog.
[[nodiscard]] katana::cad::plotting::SheetVerbContext
sheetVerbContextFor(const katana::cad::Document& document, const std::function<SheetSource()>& source);

// The paper: one sheet, painted and edited.
class SheetCanvas final : public QWidget {
  public:
    SheetCanvas(katana::cad::Document& document, SheetEditor& editor, QWidget* parent = nullptr);

    void setSheet(std::size_t index);
    [[nodiscard]] std::size_t sheet() const { return sheet_; }
    // The selected viewport's id; empty for none (the sheet itself).
    [[nodiscard]] const std::string& selected() const { return selected_; }
    void select(std::string viewportId);
    // What the canvas has cut and drawn, per revision: the properties read an
    // automatic section's scale through it, so nothing is cut twice.
    [[nodiscard]] SheetPaintCache& paintCache() { return cache_; }

    // The whole paper in the window.
    void fitPage();
    // Paint again from the document: after any change to it.
    void invalidate();

    // Logical pixels per paper millimetre, and where the paper's top-left is.
    [[nodiscard]] double zoom() const { return zoom_; }
    [[nodiscard]] QPointF paperToWidget(const katana::geometry::Point2& paper) const;
    [[nodiscard]] katana::geometry::Point2 widgetToPaper(const QPointF& widget) const;

    // What the last paint reported: viewports it could not draw.
    [[nodiscard]] const SheetPaintStats& lastStats() const { return stats_; }
    // How many times the sheet has been painted, for tests of the cache.
    [[nodiscard]] int renders() const { return renders_; }

    std::function<void(const std::string&)> onSelectionChanged;
    std::function<void(const QPointF& global, const std::string& viewportId)> onContextMenu;

    // ---- several viewports at once ------------------------------------------------
    // The selection is any number of the sheet's viewports; one of them, the
    // PRIMARY (selected()), carries the handles and the properties. A drag,
    // the arrows, Delete, Copy and Duplicate act on them all.
    //
    // Every selected viewport's id, in the sheet's order (back to front).
    [[nodiscard]] std::vector<std::string> selectedIds() const;
    [[nodiscard]] bool isSelected(std::string_view viewportId) const;
    // Selects `ids` (those not on the sheet are dropped) with `primary` as the
    // primary, or the last of `ids` when `primary` is empty or not among them.
    void setSelection(std::vector<std::string> ids, std::string primary = {});
    // Ctrl-click: `viewportId` into the selection as its primary, or out of it.
    void toggleSelected(const std::string& viewportId);
    void selectAll();
    // Tab: the next (previous) viewport in the sheet's order, alone.
    void cycleSelection(bool forward);

    // ---- the view -----------------------------------------------------------------
    // Paper `box` filling the window with a margin; the page when empty.
    void zoomTo(const katana::geometry::Box2& box);
    void zoomToSelection();
    // The paper grid (plotting::kPaperGridMm) a drag snaps to, drawn faintly
    // while on.
    void setSnapToGrid(bool on);
    [[nodiscard]] bool snapsToGrid() const { return snapGrid_; }
    [[nodiscard]] double gridSpacing() const { return gridMm_; }
    // The rulers along the top and left, in paper millimetres.
    void setRulersShown(bool on);
    [[nodiscard]] bool rulersShown() const { return rulers_; }

    // What is under widget point `widget`: the paper position, the viewport
    // and, over a plan, the world point there.
    [[nodiscard]] SheetCursorReadout readoutAt(const QPointF& widget) const;
    // The readout at the cursor's last position over the canvas.
    [[nodiscard]] const SheetCursorReadout& cursorReadout() const { return readout_; }
    std::function<void(const SheetCursorReadout&)> onCursorMoved;

  protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;
    void contextMenuEvent(QContextMenuEvent* event) override;
    bool event(QEvent* event) override;
    void keyReleaseEvent(QKeyEvent* event) override;
    void leaveEvent(QEvent* event) override;
    void focusOutEvent(QFocusEvent* event) override;

  private:
    enum class Drag { None, Move, Resize, Pan, PanView, Band };

    [[nodiscard]] const katana::cad::plotting::Sheet* currentSheet() const;
    [[nodiscard]] double paperHeight() const;
    [[nodiscard]] const katana::cad::plotting::Viewport* viewport(const std::string& id) const;
    [[nodiscard]] std::string viewportAt(const QPointF& widget) const;
    // 0..7 clockwise from the top-left corner; nothing when not on a handle
    // of the selected viewport.
    [[nodiscard]] std::optional<int> handleAt(const QPointF& widget) const;
    [[nodiscard]] QRectF widgetRect(const katana::geometry::Box2& paper) const;
    void render();
    void updateDrag(const QPointF& widget, Qt::KeyboardModifiers modifiers);
    void commitDrag();

    katana::cad::Document& document_;
    SheetEditor& editor_;
    SheetPaintCache cache_;
    SheetPaintStats stats_;
    std::size_t sheet_ = 0;
    std::string selected_;

    double zoom_ = 2.0;
    QPointF origin_{20.0, 20.0};
    bool fitted_ = false;

    // The painted sheet, and what it was painted at.
    QImage image_;
    QPointF imageOrigin_;
    double imageZoom_ = 0.0;
    std::uint64_t version_ = 1;
    std::uint64_t imageVersion_ = 0;
    int renders_ = 0;

    Drag drag_ = Drag::None;
    int handle_ = 0;
    QPointF pressWidget_;
    katana::geometry::Point2 pressPaper_{};
    katana::geometry::Box2 startRect_;
    katana::geometry::Box2 liveRect_;
    katana::geometry::Point2 startCentre_{};
    katana::geometry::Point2 viewShift_{}; // paper mm the drawing was dragged by
    std::optional<double> guideX_;
    std::optional<double> guideY_;

    // Where the widget's paper view starts: below and right of the rulers.
    [[nodiscard]] double rulerPixels() const;
    // The rectangle `id` is drawn at now: where a drag has it, else its own.
    [[nodiscard]] katana::geometry::Box2 liveRect(const std::string& id) const;
    // The drawn scale and centre of a plan, "auto" decided; cached until the
    // next change, so the cursor readout costs nothing per move.
    [[nodiscard]] ResolvedViewport resolvedPlan(const katana::cad::plotting::Viewport& viewport) const;
    void pruneSelection();
    void selectionChanged();
    void cancelDrag();
    void updateReadout(const QPointF& widget);
    void paintDraggedContent(QPainter& painter);

    // Every selected id, the primary (selected_) among them; in the order
    // they were selected.
    std::vector<std::string> selection_;
    // A group move: each moving viewport's rectangle at the press, their
    // bounds, and how far they have been dragged (snapped).
    std::map<std::string, katana::geometry::Box2> startRects_;
    katana::geometry::Box2 startBounds_;
    katana::geometry::Point2 liveDelta_{};
    // A press has become a drag once the cursor has gone a few pixels.
    bool pressMoved_ = false;
    // A click on one of several selected viewports keeps the group for a
    // drag, and selects it alone when it is let go without one.
    std::string pressHit_;
    bool narrowOnClick_ = false;
    // The plan Shift-drag pans; a Shift-click without a drag toggles it.
    std::string panViewId_;
    // The rubber band's far corner (pressPaper_ is the other), and whether it
    // adds to the selection rather than replacing it.
    katana::geometry::Point2 bandCorner_{};
    bool bandAdds_ = false;
    bool spaceHeld_ = false;
    bool snapGrid_ = false;
    double gridMm_ = katana::cad::plotting::kPaperGridMm;
    bool rulers_ = true;
    SheetCursorReadout readout_;
    std::optional<katana::geometry::Point2> cursorPaper_;
    mutable std::map<std::string, ResolvedViewport> resolved_;
    mutable std::uint64_t resolvedVersion_ = 0;
};

class SheetEditor final : public QMainWindow {
  public:
    // What the sheets are drawn from, asked for at every paint: the drawing,
    // the window's surfaces and reference data, the logo, the fields.
    using SourceProvider = std::function<SheetSource()>;

    SheetEditor(katana::cad::Document& document, SourceProvider source, QWidget* parent = nullptr);
    ~SheetEditor() override;

    SheetEditor(const SheetEditor&) = delete;
    SheetEditor& operator=(const SheetEditor&) = delete;

    [[nodiscard]] SheetSource source() const { return source_(); }
    [[nodiscard]] SheetCanvas* canvas() const { return canvas_; }
    [[nodiscard]] std::size_t currentSheet() const;
    void setCurrentSheet(std::size_t index);

    // Everything read again from the document: the list, the canvas, the
    // properties.
    void refresh();

    // ---- the edits the toolbar makes; each one undoable step ----------------------
    // A viewport of `kind` on the current sheet, placed in the free drawing
    // area, with sensible defaults (a plan fitted to the drawing, a section
    // along the first alignment); selected. A new sheet first when there is none.
    katana::core::Status addViewport(katana::cad::plotting::ViewportKind kind);
    katana::core::Status tile(katana::cad::plotting::TilingPreset preset);
    katana::core::Status addBlankSheet();
    // Every selected viewport (plotting::removeViewports).
    katana::core::Status removeSelectedViewport();
    // Plots the current sheet, or every sheet, to one PDF at `path` in the
    // set's page setup (its plot style and resolution). Problems are
    // reported through onMessage, and so are the errors the checks find on
    // the sheets plotted (the Checks dock is shown), which never stop it.
    katana::core::Status plotToPdf(const QString& path, bool allSheets);

    // The dialogs; each applies what it is given as one step.
    //
    // Generate Sheets: every layout and every option GENERATE takes. OK
    // builds the GENERATE line and runs it (runLine), so the dialog and the
    // verb cannot come to lay out different sheets. generateSheets runs the
    // dialog and waits; openGenerateDialog, the toolbar's, opens it and
    // returns it (object name sheetGenerateDialog), which is how a headless
    // session and a test fill it. Its fields are listed at buildGenerateDialog.
    void generateSheets();
    QDialog* openGenerateDialog();
    void editTitleBlock();

    // ---- the lines the editor runs ------------------------------------------------
    // The window's one executor (MainWindow::runVerbLine, command_runner.hpp).
    // The editor's dialogs and new panel fields build the verb line a person
    // would type - GENERATE, VIEW SET, SHEET SUGGESTPAPER, SHEETS SAVE - and
    // hand it here, so it is echoed in the command log, kept in the history
    // and undone as a typed line is.
    void setCommandRunner(CommandRunner run) { run_ = std::move(run); }
    // `line` through the runner; with none (an editor made without a window,
    // as the tests make one), through the interpreter's own sheet verbs
    // (plotting::runSheetVerb) with sheetVerbContextFor this editor's source,
    // the line and what it replied posted through onMessage as the window's
    // log would show them. The reply's first line, or the error, goes on the
    // status bar.
    VerbOutcome runLine(const QString& line);
    // Where the window's plan view looks, for Generate Sheets' "The current
    // plan view"; unset or nothing, and the choice is not offered.
    std::function<std::optional<katana::geometry::Box2>()> planViewArea;
    // A headless session (MainWindow::setHeadless): no file dialog and no
    // question is ever put; the item says instead which verb asks nothing.
    void setHeadless(bool headless) { headless_ = headless; }
    [[nodiscard]] bool headless() const { return headless_; }
    // `text` on the status bar and through onMessage into the window's log,
    // as an error when `error`: what the editor's menus say.
    void report(const QString& text, bool error = false);

    // ---- the preflight checks (plotting/sheet_checks.hpp) -----------------------------
    // The Checks dock, run again a moment after every change; its findings
    // are the last run's.
    [[nodiscard]] SheetChecksDock* checks() const { return checks_; }
    // Checks every sheet now, lists the findings in the dock and shows it.
    const std::vector<katana::cad::plotting::Finding>& checkSheets();
    // Goes to where a finding is: its sheet made current and its viewport
    // selected, what to do about it on the status bar.
    void showFinding(const katana::cad::plotting::Finding& finding);

    // Messages for the main window's log: the text, and whether it is an error.
    std::function<void(const QString&, bool)> onMessage;

    // ---- the selection's edits (plotting/sheet_editor_editing.cpp); each one
    // undoable step through viewport_edits.hpp ---------------------------------------
    // Every selected viewport moved by `deltaMm` (the arrows: 1 mm, Shift 10).
    katana::core::Status nudgeSelection(katana::geometry::Point2 deltaMm);
    // Copies of the selected viewports on the system clipboard (no step).
    katana::core::Status copySelection();
    katana::core::Status cutSelection();
    // The clipboard's viewports onto the current sheet, with new ids, one step
    // from where they were copied while that place is taken
    // (plotting::pasteOffset); the pasted ones are selected.
    katana::core::Status paste();
    katana::core::Status duplicateSelection();
    // Sheet `from` moved to `to` and shown: what a drop in the list does.
    katana::core::Status moveSheetTo(std::size_t from, std::size_t to);

    // The sheet list's thumbnails: how many are still to be painted, and
    // painting them now rather than while idle.
    [[nodiscard]] std::size_t pendingThumbnails() const;
    void renderThumbnailsNow();
    [[nodiscard]] const SheetThumbnails& thumbnails() const { return thumbnails_; }

  protected:
    // Thumbnails are painted only while the editor is shown.
    void showEvent(QShowEvent* event) override;

  private:
    void buildActions();
    void rebuildList();
    void rebuildProperties();
    // Generate Sheets built, not yet shown; it deletes itself when closed.
    QDialog* buildGenerateDialog();
    // Hidden layers... for the view `id`: the layers it hides, ticked off,
    // run as VIEW SET id hide= show= for what changed.
    void chooseHiddenLayers(const std::string& id, const katana::cad::LayerOverrides& hidden);
    void showContextMenu(const QPointF& global, const std::string& viewportId);
    void plotInteractive(bool allSheets);
    // Runs the checks on the sheets a plot takes (the current one, or all),
    // shows the Checks dock when they find an error, and says how many.
    std::size_t checkBeforePlot(bool allSheets);

    katana::cad::Document& document_;
    SourceProvider source_;
    SheetCanvas* canvas_ = nullptr;
    QListWidget* list_ = nullptr;
    QScrollArea* properties_ = nullptr;
    QLabel* status_ = nullptr;
    bool rebuilding_ = false;
    katana::cad::Document::ListenerHandle listener_;
    SheetChecksDock* checks_ = nullptr;
    CommandRunner run_;
    bool headless_ = false;

    // The Edit and View menus, the sheet list's dragging and thumbnails, the
    // cursor readout (plotting/sheet_editor_editing.cpp).
    void setUpEditing();
    // Cut, Copy, Duplicate, Delete and Zoom to Selection only with a
    // selection, Paste only with views on the clipboard: a disabled action
    // leaves its key to the text box that has the focus.
    void updateEditActions();
    void refreshThumbnails();
    void paintNextThumbnail();
    SheetThumbnails thumbnails_;
    QTimer* thumbnailTimer_ = nullptr;
    QLabel* cursorStatus_ = nullptr;
};

} // namespace katana::qt
