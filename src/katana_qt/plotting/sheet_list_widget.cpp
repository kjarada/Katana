#include "plotting/sheet_list_widget.hpp"

#include <QDropEvent>
#include <QMetaObject>
#include <QPointer>

namespace katana::qt {

SheetListWidget::SheetListWidget(QWidget* parent) : QListWidget(parent)
{
    setSelectionMode(QAbstractItemView::SingleSelection);
    setDragEnabled(true);
    setAcceptDrops(true);
    setDropIndicatorShown(true);
    setDragDropMode(QAbstractItemView::InternalMove);
    setDefaultDropAction(Qt::MoveAction);
}

int SheetListWidget::insertionRowAt(const QPoint& position) const
{
    const QModelIndex index = indexAt(position);
    if (!index.isValid()) {
        // Above the first row or below the last.
        return count() > 0 && position.y() < visualRect(model()->index(0, 0)).top() ? 0 : count();
    }
    const QRect rect = visualRect(index);
    return position.y() > rect.center().y() ? index.row() + 1 : index.row();
}

bool SheetListWidget::dropAt(const QPoint& position)
{
    const int from = currentRow();
    const int before = insertionRowAt(position);
    if (from < 0 || !onMoveRequested) {
        return false;
    }
    // Before a row below it is after one fewer once it has left its place.
    const int to = before > from ? before - 1 : before;
    if (to == from) {
        return false;
    }
    // After the caller has unwound: the move rebuilds this list, and a drop
    // must not have its list cleared under it.
    QPointer<SheetListWidget> self(this);
    QMetaObject::invokeMethod(
        this,
        [self, from, to] {
            if (self && self->onMoveRequested) {
                self->onMoveRequested(from, to);
            }
        },
        Qt::QueuedConnection);
    return true;
}

void SheetListWidget::dropEvent(QDropEvent* event)
{
    // Taken, but as a copy: the drag that started here must not remove the
    // item it carried, since the list is built again from the document.
    event->setDropAction(Qt::CopyAction);
    event->accept();
    stopAutoScroll();
    setState(QAbstractItemView::NoState);
    viewport()->update();
    (void)dropAt(event->position().toPoint());
}

} // namespace katana::qt
