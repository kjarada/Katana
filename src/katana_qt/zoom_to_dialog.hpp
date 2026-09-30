#pragma once

// View > Zoom To: frame, in a view of the workspace, what a scope takes - the
// shared "Apply to" and "Only those that match" (scope_filter_widget.hpp) -
// through the ZOOM line it builds (docs/desktop.md, "Zoom To"; the verb is
// cad/view_verbs.hpp). The dialog frames nothing itself: it hands
// ZOOM <scope words> view=<id> to the window's one executor, so the log shows
// what ran, and a scope that takes nothing is ZOOM's own answer (matched=0,
// nothing moved), shown in the status line. Non-modal and kept, so several
// scopes can be tried in turn; linked views follow each ZOOM as they follow
// a wheel.
//
// Object names: zoomToDialog; the scope and filter controls with the prefix
// zoomTo (zoomToScopeSelection, zoomToScopeDrawing, zoomToFilterLayer ...);
// zoomToView (the active view, then every open plan view by its title and
// id: the kinds that frame a scope, cad::zoomTakes);
// zoomToLine (the line the controls say, updated as they change);
// zoomToRun; zoomToStatus (what ZOOM answered); zoomToClose.

#include <QDialog>

#include "command_runner.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"

class QComboBox;
class QLabel;

namespace katana::qt {

class ScopeFilterWidget;
class ViewWorkspace;

struct ZoomToContext {
    const katana::cad::Document* document = nullptr;
    // The views to choose among and to answer the View scope; null in a
    // test that needs none (the view list then offers the active view alone).
    ViewWorkspace* views = nullptr;
    CommandRunner run;
};

class ZoomToDialog final : public QDialog {
  public:
    explicit ZoomToDialog(ZoomToContext context, QWidget* parent = nullptr);

    // Reads the layers and the open views again, keeping what is chosen:
    // when the dialog is shown, and after the drawing or the views changed.
    void refresh();
    // The ZOOM line the controls say; fails naming what they cannot say.
    [[nodiscard]] katana::core::Result<QString> line() const;
    // What Zoom does: build the line, run it, show the answer. False when the
    // controls did not read or ZOOM refused, with the reason in the status.
    bool zoom();

  private:
    void showLine();

    ZoomToContext context_;
    ScopeFilterWidget* scope_ = nullptr;
    QComboBox* view_ = nullptr;
    QLabel* line_ = nullptr;
    QLabel* status_ = nullptr;
};

} // namespace katana::qt
