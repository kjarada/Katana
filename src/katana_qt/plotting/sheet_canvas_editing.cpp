// The sheet canvas's selection of several viewports, its view of the paper and
// the cursor (docs/plotting.md, "Editing on the canvas"): SheetCanvas members
// kept out of sheet_editor.cpp, which holds the canvas's painting and its
// mouse, so that file changes only where a drag, a band or a key is handled.
//
//   the selection   any number of the sheet's viewports, one of them the
//                   primary (selected()); pruned after every change, so an
//                   undo cannot leave a viewport selected that is gone
//   the view        zoom to the selection, the paper grid and the rulers
//   the cursor      the paper and world position under it (sheet_readout.hpp),
//                   a plan's "auto" decided once per change
//   a drag          where each viewport is drawn while it moves, its own paint
//                   going with it, and Escape abandoning it

#include <algorithm>
#include <map>
#include <string>
#include <utility>
#include <vector>

#include <QFocusEvent>
#include <QKeyEvent>
#include <QPainter>

#include "katana/cad/plotting/viewport_edits.hpp"
#include "plotting/sheet_readout.hpp"
#include "plotting/sheet_rulers.hpp"
#include "sheet_editor.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using katana::geometry::Box2;
using katana::geometry::Point2;
using plotting::Sheet;
using plotting::Viewport;

// ---- the selection -------------------------------------------------------------------

void SheetCanvas::selectionChanged()
{
    update();
    if (onSelectionChanged) {
        onSelectionChanged(selected_);
    }
}

std::vector<std::string> SheetCanvas::selectedIds() const
{
    std::vector<std::string> ids;
    if (const Sheet* sheet = currentSheet()) {
        for (const Viewport& v : sheet->viewports) {
            if (isSelected(v.id)) {
                ids.push_back(v.id);
            }
        }
    }
    return ids;
}

bool SheetCanvas::isSelected(std::string_view viewportId) const
{
    return !viewportId.empty() && std::ranges::find(selection_, viewportId) != selection_.end();
}

void SheetCanvas::setSelection(std::vector<std::string> ids, std::string primary)
{
    std::vector<std::string> kept;
    for (std::string& id : ids) {
        if (viewport(id) != nullptr && std::ranges::find(kept, id) == kept.end()) {
            kept.push_back(std::move(id));
        }
    }
    if (std::ranges::find(kept, primary) == kept.end()) {
        primary = kept.empty() ? std::string{} : kept.back();
    }
    if (kept == selection_ && primary == selected_) {
        return;
    }
    selection_ = std::move(kept);
    selected_ = std::move(primary);
    selectionChanged();
}

void SheetCanvas::toggleSelected(const std::string& viewportId)
{
    if (viewport(viewportId) == nullptr) {
        return;
    }
    std::vector<std::string> ids = selection_;
    if (isSelected(viewportId)) {
        std::erase(ids, viewportId);
        setSelection(std::move(ids), selected_ == viewportId ? std::string{} : selected_);
    } else {
        ids.push_back(viewportId);
        setSelection(std::move(ids), viewportId);
    }
}

void SheetCanvas::selectAll()
{
    std::vector<std::string> ids;
    if (const Sheet* sheet = currentSheet()) {
        for (const Viewport& v : sheet->viewports) {
            if (!v.rect.empty()) {
                ids.push_back(v.id);
            }
        }
    }
    setSelection(std::move(ids), selected_);
}

void SheetCanvas::cycleSelection(bool forward)
{
    if (const Sheet* sheet = currentSheet()) {
        select(plotting::cycleViewport(*sheet, selected_, forward));
    }
}

// Drops from the selection what is no longer on the sheet: after an undo, a
// delete, a paste that was undone.
void SheetCanvas::pruneSelection()
{
    const auto gone = [this](const std::string& id) { return viewport(id) == nullptr; };
    if (std::ranges::none_of(selection_, gone) && (selected_.empty() || !gone(selected_))) {
        return;
    }
    std::erase_if(selection_, gone);
    if (selected_.empty() || gone(selected_)) {
        selected_ = selection_.empty() ? std::string{} : selection_.back();
    }
    selectionChanged();
}

// ---- the view -----------------------------------------------------------------------

double SheetCanvas::rulerPixels() const { return rulers_ ? kSheetRulerPixels : 0.0; }

void SheetCanvas::zoomToSelection()
{
    const Sheet* sheet = currentSheet();
    if (sheet == nullptr) {
        return;
    }
    const std::vector<std::string> ids = selectedIds();
    zoomTo(plotting::viewportBounds(*sheet, ids));
}

void SheetCanvas::setSnapToGrid(bool on)
{
    snapGrid_ = on;
    update();
}

void SheetCanvas::setRulersShown(bool on)
{
    // The paper stays where it is; the rulers cover or uncover the desk.
    rulers_ = on;
    update();
}

// ---- the cursor ---------------------------------------------------------------------

ResolvedViewport SheetCanvas::resolvedPlan(const Viewport& v) const
{
    if (!v.autoScale && !v.autoCentre) {
        return ResolvedViewport{v.scale, v.centre};
    }
    if (resolvedVersion_ != version_) {
        resolved_.clear();
        resolvedVersion_ = version_;
    }
    if (const auto it = resolved_.find(v.id); it != resolved_.end()) {
        return it->second;
    }
    // With the set and the sheet, so an automatic key plan frames the
    // sheets it outlines as the painter frames them.
    const ResolvedViewport at = resolvePlanViewport(v, editor_.source(), document_.sheetSet(), sheet_);
    resolved_.emplace(v.id, at);
    return at;
}

SheetCursorReadout SheetCanvas::readoutAt(const QPointF& widget) const
{
    const Sheet* sheet = currentSheet();
    if (sheet == nullptr) {
        return {};
    }
    return sheetCursorReadout(*sheet, widgetToPaper(widget),
                              [this](const Viewport& v) { return resolvedPlan(v); });
}

void SheetCanvas::updateReadout(const QPointF& widget)
{
    readout_ = readoutAt(widget);
    cursorPaper_ = readout_.onSheet ? std::optional<Point2>(readout_.paper) : std::nullopt;
    if (onCursorMoved) {
        onCursorMoved(readout_);
    }
    // The rulers mark the cursor: only they need painting for a move.
    if (rulers_ && drag_ == Drag::None) {
        const int r = kSheetRulerPixels;
        update(QRect(0, 0, width(), r));
        update(QRect(0, 0, r, height()));
    }
}

// ---- a drag -------------------------------------------------------------------------

Box2 SheetCanvas::liveRect(const std::string& id) const
{
    const Viewport* v = viewport(id);
    if (v == nullptr) {
        return {};
    }
    if (pressMoved_ && drag_ == Drag::Move) {
        if (const auto it = startRects_.find(id); it != startRects_.end()) {
            return startRects_.size() == 1
                       ? liveRect_
                       : Box2(it->second.min + liveDelta_, it->second.max + liveDelta_);
        }
    }
    if (pressMoved_ && drag_ == Drag::Resize && id == selected_) {
        return liveRect_;
    }
    return v->rect;
}

void SheetCanvas::paintDraggedContent(QPainter& painter)
{
    const bool moving = drag_ == Drag::Move && pressMoved_ && !startRects_.empty();
    const bool sizing = drag_ == Drag::Resize && pressMoved_;
    if ((!moving && !sizing) || image_.isNull()) {
        return;
    }
    // The last paint, in the image's own pixels: where a paper rectangle was
    // painted in it.
    const double ratio = image_.devicePixelRatio();
    const QPointF shift = origin_ - imageOrigin_;
    const auto inImage = [&](const Box2& paper) {
        const QRectF r = widgetRect(paper).translated(-shift);
        return QRectF(r.topLeft() * ratio, r.size() * ratio);
    };
    std::map<std::string, Box2> from;
    if (moving) {
        from = startRects_;
    } else {
        from.emplace(selected_, startRect_);
    }
    painter.save();
    // Where they were, faded: the paper beneath them is not painted yet.
    for (const auto& [id, start] : from) {
        const QRectF r = widgetRect(start);
        painter.fillRect(r, QColor(255, 255, 255, 190));
        painter.setPen(QPen(QColor(120, 122, 128), 1.0, Qt::DotLine));
        painter.drawRect(r);
    }
    for (const auto& [id, start] : from) {
        const QRectF to = widgetRect(liveRect(id));
        if (moving) {
            painter.drawImage(to, image_, inImage(start));
            continue;
        }
        // Sized: what it showed stays centred - a plan keeps its centre and
        // scale - and the rest is empty paper until it is painted again.
        const QRectF was = widgetRect(start);
        painter.save();
        painter.setClipRect(to);
        painter.fillRect(to, Qt::white);
        painter.drawImage(was.translated(to.center() - was.center()), image_, inImage(start));
        painter.restore();
    }
    painter.restore();
}

void SheetCanvas::cancelDrag()
{
    drag_ = Drag::None;
    pressMoved_ = false;
    narrowOnClick_ = false;
    startRects_.clear();
    panViewId_.clear();
    viewShift_ = Point2();
    guideX_.reset();
    guideY_.reset();
    unsetCursor();
    update();
}

// ---- the keys, and the cursor leaving -----------------------------------------------

void SheetCanvas::keyReleaseEvent(QKeyEvent* event)
{
    if (event->key() == Qt::Key_Space && !event->isAutoRepeat()) {
        spaceHeld_ = false;
        if (drag_ != Drag::Pan) {
            unsetCursor();
        }
        return;
    }
    QWidget::keyReleaseEvent(event);
}

bool SheetCanvas::event(QEvent* event)
{
    // Tab steps through the viewports rather than out of the canvas; on a
    // sheet with none it moves the focus on as usual.
    if (event->type() == QEvent::KeyPress) {
        const auto* key = static_cast<QKeyEvent*>(event);
        const Sheet* sheet = currentSheet();
        if ((key->key() == Qt::Key_Tab || key->key() == Qt::Key_Backtab) && sheet != nullptr &&
            !plotting::cycleViewport(*sheet, {}, true).empty()) {
            const bool back =
                key->key() == Qt::Key_Backtab || (key->modifiers() & Qt::ShiftModifier) != 0;
            cycleSelection(!back);
            return true;
        }
    }
    return QWidget::event(event);
}

void SheetCanvas::leaveEvent(QEvent* event)
{
    // Off the canvas: the rulers' mark and the status line go blank.
    cursorPaper_.reset();
    readout_ = {};
    if (onCursorMoved) {
        onCursorMoved(readout_);
    }
    if (rulers_) {
        const int r = kSheetRulerPixels;
        update(QRect(0, 0, width(), r));
        update(QRect(0, 0, r, height()));
    }
    QWidget::leaveEvent(event);
}

void SheetCanvas::focusOutEvent(QFocusEvent* event)
{
    // Space let go where the canvas could not hear it: no hand left behind.
    spaceHeld_ = false;
    if (drag_ == Drag::None) {
        unsetCursor();
    }
    QWidget::focusOutEvent(event);
}

} // namespace katana::qt
