#pragma once

// The Properties panel's tree: what the selection is and holds, read one
// level at a time.
//
// The panel was a two-column table built whole on every refresh. A string
// brought in from a survey archive carries its attributes flattened into
// '/' names - thirty per vertex over a thousand vertices is thirty thousand
// rows - and every one of them was made before the panel could show the
// first (the owner's request of 2026-09-26: "i want the attributes in the
// properties to show as a tree structure, because some lines are very long
// and contain lots of attributes in the vertices, so need a smart way to load
// all these information").
//
// Now:
//   * the rows are GROUPS - General, Geometry, Vertices, Attributes, Source -
//     and the attributes are the tree their names make, read by
//     cad::propertyOutline (the reading PROP TREE replies with, so the panel
//     and an agent cannot disagree about it);
//   * a level is read only when it is opened, and made a page at a time
//     (kPropertyPage rows): a "Show more" row at its end makes the next page;
//   * a vertex is one row, its position and height, with its own attributes
//     beneath it;
//   * the filter box above lists every property of the selection whose name
//     or value holds the words typed, however deep, a page at a time;
//   * which rows were open survives a refresh, so an undo elsewhere does not
//     fold up the vertex being read.
//
// Several entities are read as one, as the attribute manager reads them: a
// value shows only where all of them agree, else "<varies>".
//
// It edits nothing. The Style row above it (MainWindow) and Edit >
// Attributes do, through commands; a read-only tree cannot run a command from
// inside its own signal, the crash recorded in docs/desktop.md.

#include <QString>
#include <QWidget>

#include <cstddef>
#include <functional>
#include <memory>
#include <vector>

#include "katana/entity/entity.hpp"

class QLineEdit;
class QTreeView;

namespace katana::cad {
class Document;
}

namespace katana::qt {

struct PropertyRow;

// A level of rows, made on demand: `count` of them, the i-th by `row(i)`.
struct PropertyLevel {
    std::size_t count = 0;
    std::function<PropertyRow(std::size_t index)> row;
};

// One row of the tree.
struct PropertyRow {
    QString label;
    QString value;
    // The whole name and value when the columns cut them short.
    QString tip;
    // Its identity among its siblings, which is what an opened row is
    // remembered by across a refresh.
    QString key;
    bool heading = false; // a group: drawn bold
    bool muted = false;   // a count or <varies>, not a value: drawn muted
    bool open = false;    // opened when first shown
    // Its level, read when it is first opened; empty for a leaf.
    std::function<PropertyLevel()> children;
};

// Rows made at a time, in a level and in the filter's list: enough to fill
// a tall panel twice, few enough to make in well under a frame.
inline constexpr std::size_t kPropertyPage = 200;

// A level of rows already made.
[[nodiscard]] PropertyLevel fixedLevel(std::vector<PropertyRow> rows);

// The top of the tree for what `document` has selected: two rows when
// nothing is, the groups for one entity, and General (with how many of each
// type) and the shared Attributes for several.
[[nodiscard]] PropertyLevel selectionLevel(const katana::cad::Document& document);

// Every property and source value of the selection whose name or value holds
// `text` (any case), by whole name, in name order.
[[nodiscard]] PropertyLevel matchingLevel(const katana::cad::Document& document,
                                          const QString& text);

// No Q_OBJECT: it declares no signals or slots of its own (src/katana_qt/CMakeLists.txt).
class PropertyTreePanel : public QWidget {
  public:
    explicit PropertyTreePanel(QWidget* parent = nullptr);
    ~PropertyTreePanel() override;

    // Shows the selection of `document`, which must outlive the panel's next
    // refresh: the levels not yet opened read it when they are. The rows
    // open before keep open where they are still there.
    void showSelection(const katana::cad::Document& document);

    // The tree ("propertyTable", what --report reads) and the filter
    // ("propertyFilter").
    [[nodiscard]] QTreeView* tree() const;
    [[nodiscard]] QLineEdit* filter() const;

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
