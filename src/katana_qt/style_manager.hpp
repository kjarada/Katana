#pragma once

// The styles and linetypes manager (PLAN.MD 20.2, slice 3).
//
// One dialog for both tables, because they are one subject: a style names a
// linetype, and a 12d import fills both at once. Everything it does goes
// through the ordinary commands, so every edit made here is on the same undo
// stack as an edit made on the command line.

#include <functional>
#include <memory>
#include <string>

#include <QDialog>
#include <QString>

#include "katana/core/error.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad {
class Document;
}

// Qt's own, at global scope: declared inside katana::qt it would be a new
// class of the same name, which is what the first build of this file did.
class QTableWidget;

namespace katana::qt {

// No Q_OBJECT: it declares no signals or slots of its own and connects to
// lambdas, which is what keeps moc out of this target (src/katana_qt/CMakeLists.txt).
class StyleManagerDialog : public QDialog {
  public:
    // `log` writes to the main window's command log: the dialog reports what
    // each command did there rather than opening a box over a box, and the
    // second argument marks a failure, as MainWindow::logMessage does.
    using Log = std::function<void(const QString& message, bool isError)>;

    StyleManagerDialog(katana::cad::Document& document, Log log, QWidget* parent = nullptr);
    ~StyleManagerDialog() override;

    // For the headless screenshot: lays the dialog out and selects the first
    // row of each table, so a grab shows the form and the previews filled in
    // rather than an empty dialog.
    void showFirstRows();

  private:
    // Rebuilds both tables from the model. Called after every command this
    // dialog runs - never from a table's own signal, which is how the layer
    // panel once crashed (docs/cad.md).
    void reloadTables();
    void loadSelectedStyle();
    void loadSelectedLinetype();
    void refreshStylePreview();
    void refreshLinetypePreview();
    // The form's contents as a table item. The style keeps the name it is
    // stored under: renaming is its own command, so that the entities
    // wearing it can come too.
    [[nodiscard]] katana::entity::Style styleFromForm(const std::string& name) const;
    // Fails with the reason when the lengths do not parse or do not
    // alternate; the table's own validate() has the real rules.
    [[nodiscard]] katana::core::Result<katana::entity::Linetype>
    linetypeFromForm(const std::string& name) const;
    static void selectRow(QTableWidget* table, const QString& name);

    struct Impl;
    // A pointer rather than members, so that this header stays free of the
    // Qt widget types the dialog is built from (PLAN.MD Rule 4 keeps
    // third-party types out of public headers; this one is internal, but the
    // same discipline keeps its rebuild cheap).
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
