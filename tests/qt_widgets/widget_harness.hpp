#pragma once

// What the view widgets' tests share: painting a widget now, making a window
// the active one, and building any of the three views with a counter on its
// onActivated - the hook the workspace turns into "this is the view the menus
// act on".

#include <functional>

#include <QApplication>
#include <QCoreApplication>
#include <QFileDialog>
#include <QWidget>

#include "katana/cad/document.hpp"
#include "katana/cad/view_set.hpp"
#include "render_view_widget.hpp"
#include "section_view_widget.hpp"
#include "viewport_widget.hpp"

namespace katana::qt::test {

// A paint at the widget's current size, as the screen would do it. The
// widgets frame at their first paint, so this is also "the view has been
// seen".
inline void paint(QWidget& widget) { (void)widget.grab(); }

inline void processEvents()
{
    // Twice: an activation queued by the first pass is delivered by the
    // second.
    QCoreApplication::processEvents();
    QCoreApplication::processEvents();
}

// Shows `window` and makes it the active window. Focus is only ever given to
// a widget in the active window, so every focus test starts here. False when
// the platform would not activate it, which the caller asserts on: a focus
// test that never had focus would pass for nothing.
inline bool showActive(QWidget& window)
{
    window.show();
    window.activateWindow();
    for (int pass = 0; pass < 50 && QApplication::activeWindow() != &window; ++pass) {
        QCoreApplication::processEvents();
    }
    return QApplication::activeWindow() == &window;
}

// Chooses `file` in a QFileDialog drawn with Qt's own widgets and accepts it,
// as typing the name and pressing Open does. QFileDialog::selectFile writes
// the name into the dialog's file-name box only while that box does NOT have
// the keyboard focus (qfiledialog.cpp: `if (!isVisible() ||
// !d->lineEdit()->hasFocus())`), and the box takes the focus when the dialog
// is activated - on the offscreen platform, in the first pass of the
// dialog's own event loop. A poll that came before the activation chose the
// file; one that came after left the box empty, and Open with an empty box
// keeps the dialog up: the image test waited for ever, the Sheet Set tests
// wrote to the default name or waited, depending on the load of the machine.
// Taking the focus off the box first makes the choice the same either way.
inline void chooseFile(QFileDialog& dialog, const QString& file)
{
    if (QWidget* focused = dialog.focusWidget()) {
        focused->clearFocus();
    }
    dialog.selectFile(file);
    // QFileDialog's own accept is protected; through QDialog it is the same
    // virtual call a click on Open makes.
    static_cast<QDialog&>(dialog).accept();
}

// A view of `kind` in `parent`, as ViewWorkspace::buildContent makes one,
// calling `activated` whenever it raises onActivated.
inline QWidget* makeView(katana::cad::ViewKind kind, katana::cad::Document& document,
                         katana::cad::ViewState& state, QWidget* parent,
                         std::function<void()> activated)
{
    switch (kind) {
    case katana::cad::ViewKind::Plan: {
        auto* view = new ViewportWidget(document, state, parent);
        view->onActivated = std::move(activated);
        return view;
    }
    case katana::cad::ViewKind::Model3D:
    case katana::cad::ViewKind::Elevation: {
        ViewContext context;
        context.document = &document;
        auto* view = new RenderViewWidget(context, state, parent);
        view->onActivated = std::move(activated);
        return view;
    }
    case katana::cad::ViewKind::Section: {
        auto* view = new SectionViewWidget(state, parent);
        view->onActivated = std::move(activated);
        return view;
    }
    }
    return nullptr;
}

} // namespace katana::qt::test
