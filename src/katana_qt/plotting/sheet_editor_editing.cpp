// The sheet editor's editing of several viewports at once, and the furniture
// that goes with it (docs/plotting.md, "Editing on the canvas"): the Edit and
// View menus - cut, copy, paste, duplicate, delete, select all, Tab through the
// views, zoom to the selection, the paper grid, the rulers, the next and
// previous sheet - the sheet list's thumbnails and dragging, and the cursor's
// position in the status bar.
//
// Every menu entry is a QAction with an objectName and a shortcut, and each
// one calls a public SheetEditor function that returns a Status, so an agent
// drives the same edits without the menus. Each edit is ONE undoable step
// through viewport_edits.hpp.

#include <algorithm>
#include <cstddef>
#include <string>
#include <utility>
#include <vector>

#include <QAction>
#include <QClipboard>
#include <QGuiApplication>
#include <QIcon>
#include <QKeySequence>
#include <QLabel>
#include <QListWidget>
#include <QMenu>
#include <QMenuBar>
#include <QMimeData>
#include <QPainter>
#include <QPixmap>
#include <QShowEvent>
#include <QStatusBar>
#include <QTimer>

#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/viewport_edits.hpp"
#include "plotting/sheet_list_widget.hpp"
#include "plotting/sheet_readout.hpp"
#include "plotting/sheet_set_menu.hpp"
#include "plotting/viewport_clipboard.hpp"
#include "sheet_editor.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::geometry::Point2;
using plotting::Sheet;
using plotting::SheetSet;

namespace {

// How long the list waits after a change before it paints thumbnails, so a
// burst of edits costs one round of painting.
constexpr int kThumbnailDelayMs = 30;

QString views(std::size_t count)
{
    return QString("%1 view%2").arg(count).arg(count == 1 ? "" : "s");
}

// A sheet's picture before it is painted: blank paper of its shape.
QImage blankPaper(const Sheet& sheet, QSize box)
{
    const auto paper = katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    const double scale =
        std::min((box.width() - 2.0) / paper.widthMm, (box.height() - 2.0) / paper.heightMm);
    const QSizeF size(paper.widthMm * scale, paper.heightMm * scale);
    QImage image(box, QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::transparent);
    QPainter painter(&image);
    const QRectF rect(QPointF((box.width() - size.width()) / 2.0, (box.height() - size.height()) / 2.0),
                      size);
    painter.fillRect(rect, Qt::white);
    painter.setPen(QColor(120, 122, 128));
    painter.drawRect(rect.adjusted(0.5, 0.5, -0.5, -0.5));
    return image;
}

// A picture as the list shows it: the same in a selected row, which would
// otherwise tint it - a sheet's picture is its paper.
QIcon sheetIcon(const QImage& picture)
{
    const QPixmap pixmap = QPixmap::fromImage(picture);
    QIcon icon;
    icon.addPixmap(pixmap, QIcon::Normal);
    icon.addPixmap(pixmap, QIcon::Selected);
    return icon;
}

} // namespace

void SheetEditor::setUpEditing()
{
    // The list: a thumbnail beside each sheet, and dragging to reorder.
    list_->setIconSize(thumbnails_.box());
    list_->setSpacing(2);
    if (auto* sheets = dynamic_cast<SheetListWidget*>(list_)) {
        sheets->onMoveRequested = [this](int from, int to) {
            if (auto s = moveSheetTo(static_cast<std::size_t>(from), static_cast<std::size_t>(to)); !s) {
                report(QString::fromStdString(s.error().describe()), true);
            }
        };
    }
    thumbnailTimer_ = new QTimer(this);
    thumbnailTimer_->setObjectName(QStringLiteral("sheetThumbnailTimer"));
    thumbnailTimer_->setSingleShot(true);
    connect(thumbnailTimer_, &QTimer::timeout, this, [this] { paintNextThumbnail(); });

    // Where the cursor is, on the status bar's right.
    cursorStatus_ = new QLabel(this);
    cursorStatus_->setObjectName(QStringLiteral("sheetCursorStatus"));
    // Wide enough for a long line - an A0 sheet, a key plan, map coordinates
    // in the millions - so the numbers never run under the size grip.
    cursorStatus_->setMinimumWidth(cursorStatus_->fontMetrics().horizontalAdvance(
                                       QStringLiteral("X 1189.0  Y 841.0 mm   vp100 Key plan 1:50000   "
                                                      "E 9999999.999  N 9999999.999")) +
                                   16);
    statusBar()->addPermanentWidget(cursorStatus_);
    canvas_->onCursorMoved = [this](const SheetCursorReadout& readout) {
        cursorStatus_->setText(readoutText(readout));
    };

    const auto reportFailure = [this](const Status& status) {
        if (!status) {
            report(QString::fromStdString(status.error().describe()), true);
        }
    };
    // An action of the window, so its shortcut works wherever the focus is in
    // it (a text box keeps the keys it edits with).
    const auto make = [this](QMenu* menu, const QString& text, const char* name,
                             const QKeySequence& key, const QString& tip) {
        auto* action = new QAction(text, this);
        action->setObjectName(QString::fromLatin1(name));
        action->setShortcut(key);
        action->setToolTip(tip);
        action->setStatusTip(tip);
        addAction(action);
        menu->addAction(action);
        return action;
    };

    addSheetSetMenu(*this, document_, *menuBar());
    QMenu* edit = menuBar()->addMenu(QStringLiteral("&Edit"));
    edit->setObjectName(QStringLiteral("sheetEditMenu"));
    connect(make(edit, QStringLiteral("Cu&t"), "sheetCut", QKeySequence::Cut,
                 QStringLiteral("Copy the selected views to the clipboard and remove them")),
            &QAction::triggered, this, [this, reportFailure] { reportFailure(cutSelection()); });
    connect(make(edit, QStringLiteral("&Copy"), "sheetCopy", QKeySequence::Copy,
                 QStringLiteral("Copy the selected views to the clipboard, for this sheet, another "
                                "sheet or another project")),
            &QAction::triggered, this, [this, reportFailure] { reportFailure(copySelection()); });
    connect(make(edit, QStringLiteral("&Paste"), "sheetPaste", QKeySequence::Paste,
                 QStringLiteral("Paste the copied views onto this sheet, with new ids")),
            &QAction::triggered, this, [this, reportFailure] { reportFailure(paste()); });
    connect(make(edit, QStringLiteral("&Duplicate"), "sheetDuplicateViews",
                 QKeySequence(Qt::CTRL | Qt::Key_D),
                 QStringLiteral("Copies of the selected views, 5 mm right and down")),
            &QAction::triggered, this, [this, reportFailure] { reportFailure(duplicateSelection()); });
    connect(make(edit, QStringLiteral("De&lete"), "sheetDeleteViews", QKeySequence::Delete,
                 QStringLiteral("Remove the selected views")),
            &QAction::triggered, this, [this, reportFailure] { reportFailure(removeSelectedViewport()); });
    edit->addSeparator();
    connect(make(edit, QStringLiteral("Select &All"), "sheetSelectAll", QKeySequence::SelectAll,
                 QStringLiteral("Select every view on this sheet")),
            &QAction::triggered, canvas_, [this] { canvas_->selectAll(); });
    // Tab itself reaches the canvas, which steps; these name it in the menu.
    connect(make(edit, QStringLiteral("Select &Next View\tTab"), "sheetSelectNextView", QKeySequence(),
                 QStringLiteral("Select the next view on the sheet")),
            &QAction::triggered, canvas_, [this] { canvas_->cycleSelection(true); });
    connect(make(edit, QStringLiteral("Select Pre&vious View\tShift+Tab"), "sheetSelectPreviousView",
                 QKeySequence(), QStringLiteral("Select the previous view on the sheet")),
            &QAction::triggered, canvas_, [this] { canvas_->cycleSelection(false); });

    QMenu* view = menuBar()->addMenu(QStringLiteral("&View"));
    view->setObjectName(QStringLiteral("sheetViewMenu"));
    if (QAction* fit = findChild<QAction*>(QStringLiteral("sheetFitPage"))) {
        view->addAction(fit);
    }
    connect(make(view, QStringLiteral("&Zoom to Selection"), "sheetZoomSelection",
                 QKeySequence(Qt::SHIFT | Qt::Key_Home),
                 QStringLiteral("Fill the window with the selected views")),
            &QAction::triggered, canvas_, [this] { canvas_->zoomToSelection(); });
    view->addSeparator();
    // The canvas holds whether the grid and the rulers are on (an agent sets
    // them there); these flip its state and show it, never a copy of it.
    QAction* grid = make(view, QStringLiteral("Snap to &Grid"), "sheetSnapGrid", QKeySequence(Qt::Key_F9),
                         QStringLiteral("Snap dragged views to a 5 mm paper grid, drawn faintly; the "
                                        "edges of other views still come first"));
    grid->setCheckable(true);
    grid->setChecked(canvas_->snapsToGrid());
    connect(grid, &QAction::triggered, canvas_, [this, grid] {
        canvas_->setSnapToGrid(!canvas_->snapsToGrid());
        grid->setChecked(canvas_->snapsToGrid());
    });
    QAction* rulers = make(view, QStringLiteral("&Rulers"), "sheetShowRulers",
                           QKeySequence(Qt::CTRL | Qt::Key_R),
                           QStringLiteral("Rulers in paper millimetres along the top and left"));
    rulers->setCheckable(true);
    rulers->setChecked(canvas_->rulersShown());
    connect(rulers, &QAction::triggered, canvas_, [this, rulers] {
        canvas_->setRulersShown(!canvas_->rulersShown());
        rulers->setChecked(canvas_->rulersShown());
    });
    connect(view, &QMenu::aboutToShow, this, [this, grid, rulers] {
        grid->setChecked(canvas_->snapsToGrid());
        rulers->setChecked(canvas_->rulersShown());
    });
    view->addSeparator();
    QAction* previous = make(view, QStringLiteral("Pre&vious Sheet"), "sheetPreviousSheet",
                             QKeySequence(Qt::Key_PageUp), QStringLiteral("Show the sheet before this one"));
    connect(previous, &QAction::triggered, this, [this] {
        if (currentSheet() > 0) {
            setCurrentSheet(currentSheet() - 1);
        }
    });
    QAction* next = make(view, QStringLiteral("&Next Sheet"), "sheetNextSheet", QKeySequence(Qt::Key_PageDown),
                         QStringLiteral("Show the sheet after this one"));
    connect(next, &QAction::triggered, this, [this] { setCurrentSheet(currentSheet() + 1); });
    // PgUp and PgDn step through the sheets from the canvas and the list, not
    // the window: a spin box or a text box of the properties keeps them for
    // its own paging (it does not claim them from a window shortcut).
    for (QAction* action : {previous, next}) {
        removeAction(action);
        action->setShortcutContext(Qt::WidgetWithChildrenShortcut);
        canvas_->addAction(action);
        list_->addAction(action);
    }

    // Paste follows the clipboard, which another program may fill.
    if (QClipboard* clipboard = QGuiApplication::clipboard()) {
        connect(clipboard, &QClipboard::dataChanged, this, [this] { updateEditActions(); });
    }
    updateEditActions();
}

void SheetEditor::updateEditActions()
{
    const bool selection = !canvas_->selectedIds().empty();
    for (const char* name :
         {"sheetCut", "sheetCopy", "sheetDuplicateViews", "sheetDeleteViews", "sheetZoomSelection"}) {
        if (QAction* action = findChild<QAction*>(QString::fromLatin1(name))) {
            action->setEnabled(selection);
        }
    }
    if (QAction* action = findChild<QAction*>(QStringLiteral("sheetPaste"))) {
        const QClipboard* clipboard = QGuiApplication::clipboard();
        const QMimeData* mime = clipboard != nullptr ? clipboard->mimeData() : nullptr;
        action->setEnabled(mime != nullptr &&
                           mime->hasFormat(QString::fromLatin1(kViewportMimeType)));
    }
}

void SheetEditor::showEvent(QShowEvent* event)
{
    QMainWindow::showEvent(event);
    refreshThumbnails();
}

// ---- the selection's edits ---------------------------------------------------------

Status SheetEditor::nudgeSelection(Point2 deltaMm)
{
    const std::vector<std::string> ids = canvas_->selectedIds();
    if (ids.empty()) {
        return makeError(ErrorCode::InvalidState, "no views are selected");
    }
    return plotting::moveViewports(document_, currentSheet(), ids, deltaMm,
                                   ids.size() == 1 ? "MOVE_VIEWPORT" : "MOVE_VIEWPORTS");
}

Status SheetEditor::copySelection()
{
    const std::vector<std::string> ids = canvas_->selectedIds();
    if (ids.empty()) {
        return makeError(ErrorCode::InvalidState, "select the views to copy first");
    }
    auto copies = plotting::copyViewports(document_.sheetSet(), currentSheet(), ids);
    if (!copies) {
        return copies.error();
    }
    if (auto s = copyViewportsToClipboard(*copies); !s) {
        return s;
    }
    // Paste is ready now, whenever the clipboard gets round to saying so.
    updateEditActions();
    report(QString("Copied %1.").arg(views(copies->size())));
    return {};
}

Status SheetEditor::cutSelection()
{
    if (auto s = copySelection(); !s) {
        return s;
    }
    const std::vector<std::string> ids = canvas_->selectedIds();
    return plotting::removeViewports(document_, currentSheet(), ids, "CUT_VIEWPORTS");
}

Status SheetEditor::paste()
{
    auto viewports = viewportsOnClipboard();
    if (!viewports) {
        return viewports.error();
    }
    if (document_.sheetSet().sheets.empty()) {
        // A sheet to paste onto, added in the paste's own step: one Undo
        // takes both away, rather than leaving a blank sheet behind.
        plotting::SheetSet set = document_.sheetSet();
        plotting::Sheet sheet = plotting::blankSheet({}, "SHEET 1");
        sheet.id = plotting::nextSheetId(set);
        set.sheets.push_back(std::move(sheet));
        auto pasted = plotting::pasteViewports(set, 0, std::move(*viewports));
        if (!pasted) {
            return pasted.error();
        }
        if (Status s = document_.setSheetSet(set, "PASTE_VIEWPORTS"); !s) {
            return s;
        }
        setCurrentSheet(0);
        canvas_->setSelection(*pasted);
        report(QString("Pasted %1 onto a new sheet.").arg(views(pasted->size())));
        return {};
    }
    auto ids = plotting::pasteViewports(document_, currentSheet(), std::move(*viewports));
    if (!ids) {
        return ids.error();
    }
    canvas_->setSelection(*ids);
    report(QString("Pasted %1.").arg(views(ids->size())));
    return {};
}

Status SheetEditor::duplicateSelection()
{
    const std::vector<std::string> selected = canvas_->selectedIds();
    if (selected.empty()) {
        return makeError(ErrorCode::InvalidState, "select the views to duplicate first");
    }
    auto ids = plotting::duplicateViewports(document_, currentSheet(), selected);
    if (!ids) {
        return ids.error();
    }
    canvas_->setSelection(*ids);
    return {};
}

Status SheetEditor::moveSheetTo(std::size_t from, std::size_t to)
{
    if (from == to && from < document_.sheetSet().sheets.size()) {
        return {};
    }
    if (auto s = plotting::moveSheet(document_, from, to); !s) {
        return s;
    }
    setCurrentSheet(to);
    return {};
}

// ---- thumbnails ---------------------------------------------------------------------

void SheetEditor::refreshThumbnails()
{
    const SheetSet& set = document_.sheetSet();
    thumbnails_.prune(set);
    // The last picture of each sheet stands in until it is painted again.
    for (int row = 0; row < list_->count() && static_cast<std::size_t>(row) < set.sheets.size(); ++row) {
        const Sheet& sheet = set.sheets[static_cast<std::size_t>(row)];
        QImage picture = thumbnails_.cached(sheet.id);
        if (picture.isNull()) {
            picture = blankPaper(sheet, thumbnails_.box());
        }
        list_->item(row)->setIcon(sheetIcon(picture));
    }
    if (thumbnailTimer_ != nullptr && !set.sheets.empty()) {
        thumbnailTimer_->start(kThumbnailDelayMs);
    }
}

void SheetEditor::paintNextThumbnail()
{
    // A hidden editor paints nothing; showing it starts again.
    if (!isVisible()) {
        return;
    }
    const SheetSet& set = document_.sheetSet();
    const SheetSource from = source();
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        if (!thumbnails_.isStale(set, i, from)) {
            continue;
        }
        const QImage picture = thumbnails_.thumbnail(set, i, from, list_->devicePixelRatioF());
        if (QListWidgetItem* item = list_->item(static_cast<int>(i))) {
            item->setIcon(sheetIcon(picture));
        }
        // One a turn of the event loop, so the editor stays responsive.
        thumbnailTimer_->start(0);
        return;
    }
}

std::size_t SheetEditor::pendingThumbnails() const
{
    return thumbnails_.staleCount(document_.sheetSet(), source());
}

void SheetEditor::renderThumbnailsNow()
{
    const SheetSet& set = document_.sheetSet();
    const SheetSource from = source();
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        if (thumbnails_.isStale(set, i, from)) {
            const QImage picture = thumbnails_.thumbnail(set, i, from, list_->devicePixelRatioF());
            if (QListWidgetItem* item = list_->item(static_cast<int>(i))) {
                item->setIcon(sheetIcon(picture));
            }
        }
    }
}

} // namespace katana::qt
