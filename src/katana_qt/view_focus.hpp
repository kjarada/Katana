#pragma once

// When the keyboard focus arriving in a view makes it the active view, the
// one the menus, Zoom Extents and F9 act on. A click always does (each view's
// mousePressEvent); the focus does only when the USER moved it. One rule for
// the plan, 3D and section views, because the workspace treats them alike.

#include <functional>
#include <utility>

#include <QApplication>
#include <QEvent>
#include <QFocusEvent>
#include <QObject>
#include <QWidget>

namespace katana::qt {

// Whether focus arriving in `view` for `reason`, from `previous` (the widget
// that held it, or null), is the user choosing that view.
[[nodiscard]] inline bool focusChoosesView(Qt::FocusReason reason, const QWidget* previous,
                                           const QWidget& view)
{
    switch (reason) {
    case Qt::TabFocusReason:
    case Qt::BacktabFocusReason:
        // Qt moves the focus along the tab chain ITSELF when the widget
        // holding it is hidden or disabled, and the workspace hides a view
        // whenever it changes the view's kind (QDockWidget::setWidget),
        // closes it or rearranges the docks (removeDockWidget). Whichever
        // view the chain reached next became active: the menus then acted on
        // an unrelated view, and a close overruled ViewSet's "most recently
        // active of the rest". Tab pressed in a view that is still there is
        // a choice.
        return previous == nullptr || (previous->isVisible() && previous->isEnabled());
    case Qt::ActiveWindowFocusReason:
        // A window coming forward gives the focus back to the widget that
        // last held it. A floating view's own window coming forward is the
        // user choosing that view. The main window comes forward whenever
        // the user returns to it - after a dialog, or to reach its menu bar
        // from a floating view - and the view holding its focus was not
        // chosen: with it, a floating view could never be what the menu bar
        // acted on. A floating dock, alone or grouped, is a window with a
        // parent (the workspace); the main window has none.
        return view.window()->parentWidget() != nullptr;
    case Qt::PopupFocusReason:   // a closing menu gives it back to the view under it
    case Qt::MenuBarFocusReason: // so does leaving the menu bar by the keyboard
        return false;
    default: // a click, a shortcut, or the application giving the view focus
        return true;
    }
}

// Calls `activate` each time the focus arrives in `view` and focusChoosesView
// says the user chose it, for as long as `view` lives. The rule needs the
// widget the focus came from, which only QApplication::focusChanged says,
// and the reason, which only the FocusIn event carries. Qt sends the event
// and then emits the signal, so a filter on the view keeps the reason for
// the signal that follows.
inline void activateOnFocus(QWidget& view, std::function<void()> activate)
{
    class ReasonFilter final : public QObject {
      public:
        using QObject::QObject;
        Qt::FocusReason reason = Qt::OtherFocusReason;

        bool eventFilter(QObject* /*watched*/, QEvent* event) override
        {
            if (event->type() == QEvent::FocusIn) {
                reason = static_cast<QFocusEvent*>(event)->reason();
            }
            return false;
        }
    };
    // A child of the view, so the filter and the connection end with it.
    auto* filter = new ReasonFilter(&view);
    view.installEventFilter(filter);
    QObject::connect(qApp, &QApplication::focusChanged, filter,
                     [filter, &view, activate = std::move(activate)](QWidget* previous,
                                                                     QWidget* now) {
                         if (now == &view && focusChoosesView(filter->reason, previous, view)) {
                             activate();
                         }
                     });
}

} // namespace katana::qt
