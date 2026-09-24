#pragma once

// The styles and linetypes manager.
//
// One window for what decides how a line or a point is drawn: the drawing's
// styles, its own dash linetypes and the session's library linestyles, with a
// Diagnostics tab for the names nothing defines and the names two sources
// define (decision D2's collisions). Everything it changes goes through the
// ordinary commands, so every edit made here is on the same undo stack as one
// made on the command line - and its Undo and Redo buttons are that stack's.
//
// NON-MODAL: Format > Styles and Linetypes shows it beside the drawing (the
// workbench's show()); it may also be exec()'d. It hears the Document through a
// DocumentWatcher, so an undo, an import or a library load made anywhere
// reloads it once, from the event loop, keeping the selected rows; and every
// button that acts on the drawing's selection reads the selection at the
// moment it is pressed.
//
// THE QT-02 RULE. A form writes back only the fields a person EDITED
// (cad::StyleFields); an unedited Save is no command at all
// (cad::editStylesCommand, updateStyleIfChanged); and every name field keeps
// a name it cannot list (NamePicker, kept_name_combo.hpp). Selecting several
// styles shows <varies> for the fields they differ in, and Save leaves those
// fields of each style alone unless they were edited.
//
// No modal box is ever opened by a button: names are asked for in a prompt
// row inside the dialog and a purge is checked in a panel, so a headless
// session and a test drive every action by objectName.
//
// objectNames: tabs "managerTabs" with pages "stylesPage", "linetypesPage",
// "diagnosticsPage"; see style_manager.cpp for each field and button.

#include <memory>
#include <string>
#include <vector>

#include <QDialog>

#include "customisation/customisation_context.hpp"
#include "katana/cad/style_manager_rows.hpp"

namespace katana::qt {

// No Q_OBJECT: no signals or slots of its own; it connects to lambdas, which
// is what keeps moc out of this target (src/katana_qt/CMakeLists.txt).
class StyleManagerDialog : public QDialog {
  public:
    // `context.document` and `context.log` must be set. `thumbnails` may be
    // null (the dialog then keeps its own cache); `selectAndShow` may be
    // empty (Select Users then only selects). The Document may be destroyed
    // before the dialog: the dialog then does nothing more.
    explicit StyleManagerDialog(const CustomisationContext& context, QWidget* parent = nullptr);
    ~StyleManagerDialog() override;

    // For the headless screenshot: selects the first row of the styles and
    // the linetypes tables, so a grab shows the forms and previews filled in.
    void showFirstRows();

    // The styles selected in the Styles tab, by name, in table order; and
    // replacing that selection (names the table lacks are ignored). For the
    // workbench ("open on this style") and for tests.
    [[nodiscard]] std::vector<std::string> selectedStyles() const;
    void selectStyles(const std::vector<std::string>& names);
    // The same for the Linetypes tab, where a name may be both a drawing
    // linetype and a library linestyle (a D2 collision): `origin` says which.
    [[nodiscard]] std::vector<std::pair<std::string, katana::cad::LinetypeOrigin>>
    selectedLinetypes() const;
    void selectLinetype(const std::string& name, katana::cad::LinetypeOrigin origin);

  private:
    struct Impl;
    // A pointer rather than members, so this header stays free of the widget
    // types the dialog is built from and its rebuild stays cheap.
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
