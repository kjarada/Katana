#pragma once

// The sheet editor's list of sheets (docs/plotting.md, "Editing on the
// canvas"): a thumbnail and the number and name of each sheet, dragged up and
// down to reorder the set.
//
// A drop does not move the item itself. It asks for the move
// (onMoveRequested), the editor makes it as one undoable step
// (plotting::moveSheet), and the list is built again from the document - so
// the list never shows an order the set does not have, and an undo puts both
// back.

#include <functional>

#include <QListWidget>

class QDropEvent;

namespace katana::qt {

class SheetListWidget final : public QListWidget {
  public:
    explicit SheetListWidget(QWidget* parent = nullptr);

    // The row a drop at `position` (in the viewport) goes before: the row
    // under it, or the one after when it is on the lower half of that row;
    // count() below the last.
    [[nodiscard]] int insertionRowAt(const QPoint& position) const;

    // What a drop at `position` does, without the drag: asks for the current
    // sheet to go where the drop indicator is (onMoveRequested, after the
    // caller has returned). False when it would stay where it is.
    bool dropAt(const QPoint& position);

    // The sheet at `from` dropped to end up at `to`; called after the drop
    // has finished, never from inside it.
    std::function<void(int from, int to)> onMoveRequested;

  protected:
    void dropEvent(QDropEvent* event) override;
};

} // namespace katana::qt
