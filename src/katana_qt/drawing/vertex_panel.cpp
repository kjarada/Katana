#include "drawing/vertex_panel.hpp"

#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QPushButton>
#include <QTableWidget>
#include <QVBoxLayout>

#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/cad/drawing/vertex_table.hpp"

namespace katana::qt::drawing {

namespace cad = katana::cad;
using katana::entity::Entity;
using katana::entity::EntityId;

VertexPanel::VertexPanel(cad::Document& document, QWidget* parent)
    : QWidget(parent), document_(document)
{
    setObjectName("vertexPanel");
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(4, 4, 4, 4);
    layout->setSpacing(4);
    title_ = new QLabel(this);
    title_->setObjectName("vertexPanelTitle");
    layout->addWidget(title_);

    table_ = new QTableWidget(0, cad::kVertexColumnCount, this);
    table_->setObjectName("vertexTable");
    QStringList headers;
    for (int c = 0; c < cad::kVertexColumnCount; ++c) {
        headers << QString::fromUtf8(cad::toString(static_cast<cad::VertexColumn>(c)));
    }
    table_->setHorizontalHeaderLabels(headers);
    table_->verticalHeader()->setVisible(false);
    table_->horizontalHeader()->setSectionResizeMode(QHeaderView::ResizeToContents);
    table_->setSelectionBehavior(QAbstractItemView::SelectRows);
    table_->setSelectionMode(QAbstractItemView::SingleSelection);
    table_->setToolTip("<b>Vertices</b><br>Type into a cell to edit it: easting, northing, "
                       "height (empty for none), bulge (0 straight), or the bearing and "
                       "distance of the segment from this vertex, which move the next one. "
                       "Each edit is one undo step.");
    layout->addWidget(table_, 1);

    auto* buttons = new QHBoxLayout();
    insert_ = new QPushButton("Insert After", this);
    insert_->setObjectName("vertexInsertButton");
    insert_->setToolTip("Adds a vertex halfway along the segment after the current row.");
    delete_ = new QPushButton("Delete", this);
    delete_->setObjectName("vertexDeleteButton");
    delete_->setToolTip("Removes the current row's vertex.");
    buttons->addWidget(insert_);
    buttons->addWidget(delete_);
    buttons->addStretch(1);
    layout->addLayout(buttons);

    connect(table_, &QTableWidget::cellChanged, this,
            [this](int row, int column) { onCellChanged(row, column); });
    connect(insert_, &QPushButton::clicked, this, [this] { insertAfterCurrent(); });
    connect(delete_, &QPushButton::clicked, this, [this] { deleteCurrent(); });
    listener_ = document_.addListener([this] { refresh(); });
    refresh();
}

void VertexPanel::refresh()
{
    shown_.reset();
    const Entity* polyline = nullptr;
    for (const EntityId id : document_.selection().ids()) {
        const Entity* entity = document_.model().entities.find(id);
        if (entity != nullptr && cad::isPolylineEntity(*entity)) {
            polyline = entity;
            break;
        }
    }
    filling_ = true;
    if (polyline == nullptr) {
        title_->setText("Select a polyline to see its vertices.");
        table_->setRowCount(0);
        insert_->setEnabled(false);
        delete_->setEnabled(false);
        filling_ = false;
        return;
    }
    shown_ = polyline->id;
    const auto shape = *cad::readPolyline(*polyline);
    title_->setText(QString("Polyline %1: %2 vertices, %3%4")
                        .arg(polyline->id)
                        .arg(shape.vertices.size())
                        .arg(shape.closed ? "closed" : "open")
                        .arg(shape.hasArcs() ? ", with arcs" : ""));
    const auto rows = cad::vertexRows(shape);
    table_->setRowCount(static_cast<int>(rows.size()));
    for (int r = 0; r < static_cast<int>(rows.size()); ++r) {
        for (int c = 0; c < cad::kVertexColumnCount; ++c) {
            auto* item = new QTableWidgetItem(QString::fromStdString(
                cad::cellText(rows[static_cast<std::size_t>(r)], static_cast<cad::VertexColumn>(c))));
            if (c == static_cast<int>(cad::VertexColumn::Index)) {
                item->setFlags(item->flags() & ~Qt::ItemIsEditable);
            } else {
                item->setTextAlignment(Qt::AlignRight | Qt::AlignVCenter);
            }
            table_->setItem(r, c, item);
        }
    }
    insert_->setEnabled(shape.segmentCount() > 0);
    delete_->setEnabled(shape.vertices.size() > katana::geometry::minimumVertices(shape));
    filling_ = false;
}

katana::core::Status VertexPanel::editCell(int row, int column, const QString& text)
{
    if (!shown_) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidState,
                                       "no polyline is shown");
    }
    const auto index = static_cast<std::size_t>(row);
    const auto which = static_cast<cad::VertexColumn>(column);
    const std::string typed = text.toStdString();
    const cad::AngleConvention angles = document_.drafting().angles;
    // The panel shows bearings, so a typed bearing is one unless it is a
    // quadrant form (which parseDirection reads either way).
    const cad::AngleConvention convention =
        which == cad::VertexColumn::Bearing ? cad::AngleConvention::Bearing : angles;
    return document_.execute(cad::editPolyline(
        *shown_, "VERTEX_SET",
        [index, which, typed, convention](const katana::geometry::CurvePolyline2& polyline) {
            return cad::editVertexCell(polyline, index, which, typed, convention);
        }));
}

void VertexPanel::onCellChanged(int row, int column)
{
    if (filling_) {
        return;
    }
    const QString text = table_->item(row, column) != nullptr ? table_->item(row, column)->text()
                                                              : QString();
    const auto status = editCell(row, column, text);
    if (!status) {
        title_->setText(QString::fromStdString(status.error().message));
        // Put the cell back as the drawing has it.
        const QString said = title_->text();
        refresh();
        title_->setText(said);
    }
}

void VertexPanel::insertAfterCurrent()
{
    if (!shown_) {
        return;
    }
    const std::size_t row = static_cast<std::size_t>(std::max(0, table_->currentRow()));
    (void)document_.execute(cad::editPolyline(
        *shown_, "VERTEX_INSERT", [row](const katana::geometry::CurvePolyline2& polyline) {
            const std::size_t segment = std::min(row, polyline.segmentCount() - 1);
            const auto middle = std::visit([](const auto& piece) { return piece.pointAt(0.5); },
                                           polyline.segment(segment));
            return katana::geometry::insertVertex(polyline, segment, middle);
        }));
}

void VertexPanel::deleteCurrent()
{
    if (!shown_ || table_->currentRow() < 0) {
        return;
    }
    const std::size_t row = static_cast<std::size_t>(table_->currentRow());
    const auto status = document_.execute(cad::editPolyline(
        *shown_, "VERTEX_DELETE", [row](const katana::geometry::CurvePolyline2& polyline) {
            return katana::geometry::deleteVertex(polyline, row);
        }));
    if (!status) {
        title_->setText(QString::fromStdString(status.error().message));
    }
}

} // namespace katana::qt::drawing
