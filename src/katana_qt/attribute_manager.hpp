#pragma once

// The attribute manager (PLAN.MD 20.2, slice 5).
//
// A 12d string carries a TREE of attributes - `group { name "Asset" ... }` -
// which the importer flattens to properties keyed "Asset/Dimensions/Size",
// and per-vertex attributes keyed "vertex/3/Name". The properties panel shows
// those flat keys, which is honest but unreadable on a real survey string
// with thirty of them. This dialog puts the tree back: branches for the
// groups, leaves for the values, with their types, and add, edit, rename and
// remove as ordinary commands so every change is on the one undo stack.
//
// It acts on the SELECTION. With several entities selected an edit applies to
// all of them, and a value they do not share is shown as such rather than
// pretending they agree.

#include <functional>
#include <memory>

#include <QDialog>
#include <QString>

namespace katana::cad {
class Document;
}

// Qt's own, at global scope - see style_manager.hpp for what happens when
// one of these is declared inside katana::qt instead.
class QTreeWidget;
class QTreeWidgetItem;

namespace katana::qt {

// No Q_OBJECT: it declares no signals or slots of its own (src/katana_qt/CMakeLists.txt).
class AttributeManagerDialog : public QDialog {
  public:
    using Log = std::function<void(const QString& message, bool isError)>;

    AttributeManagerDialog(katana::cad::Document& document, Log log, QWidget* parent = nullptr);
    ~AttributeManagerDialog() override;

    // For the headless screenshot: opens the tree so a grab shows the
    // attributes rather than a row of collapsed branches.
    void expandAll();

  private:
    void reloadTree();
    // The full key of the selected leaf ("Asset/Dimensions/Size"), empty when
    // a branch or nothing is selected: a branch is not a property, it is the
    // part of a name before a slash.
    [[nodiscard]] std::string selectedKey() const;

    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
