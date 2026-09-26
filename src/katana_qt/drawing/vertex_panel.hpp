#pragma once

// The Vertices panel (docs/drawing.md, "The Vertices panel"): the selected
// polyline's vertices as a live table - index, easting, northing, height,
// bulge, and each segment's bearing (D°MM'SS") and distance - edited in
// place, every edit one undoable step, following the selection.
//
// A thin front end over cad/drawing/vertex_table.hpp, which says what the
// rows are and what typing into a cell does, and cad/drawing/
// vertex_editing.hpp, which makes the edit one command. It shows the first
// polyline in the selection; with none it says so.

#include <optional>

#include <QWidget>

#include "katana/cad/document.hpp"

class QLabel;
class QPushButton;
class QTableWidget;

namespace katana::qt::drawing {

class VertexPanel final : public QWidget {
  public:
    explicit VertexPanel(katana::cad::Document& document, QWidget* parent = nullptr);

    // The polyline shown, if any.
    [[nodiscard]] std::optional<katana::entity::EntityId> shown() const { return shown_; }
    // Reads the document again: the selection and the polyline's vertices.
    void refresh();
    // Types `text` into a cell, as the table's editor does - for the tests
    // and the headless driver.
    [[nodiscard]] katana::core::Status editCell(int row, int column, const QString& text);

  private:
    void onCellChanged(int row, int column);
    void insertAfterCurrent();
    void deleteCurrent();

    katana::cad::Document& document_;
    katana::cad::Document::ListenerHandle listener_;
    std::optional<katana::entity::EntityId> shown_;
    QLabel* title_ = nullptr;
    QTableWidget* table_ = nullptr;
    QPushButton* insert_ = nullptr;
    QPushButton* delete_ = nullptr;
    bool filling_ = false; // the table is being filled, not edited
};

} // namespace katana::qt::drawing
