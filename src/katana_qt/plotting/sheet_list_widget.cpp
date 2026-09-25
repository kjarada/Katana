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
    // Before the first row whose middle is below the point. Not indexAt: in
    // the spacing between two rows, or beside a short row, it finds nothing,
    // and a drop there must still go between those rows, not to the end.
    for (int row = 0; row < count(); ++row) {
        if (position.y() <= visualItemRect(item(row)).center().y()) {
            return row;
        }
    }
    return count();
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
