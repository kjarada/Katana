#pragma once

// The layer manager (PLAN.MD 20.2, slice 6).
//
// The dock beside the drawing is the quick view: what is on, what is locked,
// what colour. This is the whole table - linetype, line weight, dimension
// style, hatch and how many entities are on each - with the operations that
// need more room than a dock has: move a layer (and its subtree, and the
// entities on it) under another parent, and put the selection on a layer.
//
// A layer name is a PATH ("design/surface/tin1"), so "move" and "rename" are
// the same command: renameLayer takes the subtree and the entities with it.

#include <functional>
#include <memory>
#include <string>

#include <QDialog>
#include <QString>

namespace katana::cad {
class Document;
}

class QTableWidget;

namespace katana::qt {

// No Q_OBJECT: no signals or slots of its own (src/katana_qt/CMakeLists.txt).
class LayerManagerDialog : public QDialog {
  public:
    using Log = std::function<void(const QString& message, bool isError)>;

    LayerManagerDialog(katana::cad::Document& document, Log log, QWidget* parent = nullptr);
    ~LayerManagerDialog() override;

    // For the headless screenshot: selects the first row so the form below
    // is filled in.
    void showFirstRow();

  private:
    void reloadTable();
    void loadSelectedLayer();
    [[nodiscard]] std::string selectedLayer() const;

    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
