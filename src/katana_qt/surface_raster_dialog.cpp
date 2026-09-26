#include "surface_raster_dialog.hpp"

#include <QComboBox>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QLabel>
#include <QPushButton>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <utility>

#include "format.hpp"
#include "gis_dialog_support.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace interop = katana::interop;

// ---- surface -> DEM -------------------------------------------------------------------------

SurfaceRasterDialog::SurfaceRasterDialog(std::vector<SurfaceChoice> surfaces, QWidget* parent)
    : QDialog(parent), surfaces_(std::move(surfaces))
{
    setWindowTitle("Export Surface as DEM");
    auto* layout = new QVBoxLayout(this);
    auto* form = new QFormLayout();

    surface_ = new QComboBox(this);
    for (const SurfaceChoice& choice : surfaces_) {
        surface_->addItem(choice.name);
    }
    form->addRow("Surface:", surface_);

    cellSize_ = new QDoubleSpinBox(this);
    cellSize_->setDecimals(3);
    cellSize_->setRange(0.001, 100'000.0);
    cellSize_->setSuffix(" units");
    form->addRow("Cell size:", cellSize_);

    grid_ = new QLabel(this);
    form->addRow("Grid:", grid_);
    auto* note = new QLabel("Each cell holds the surface's elevation at its centre; a cell "
                            "whose centre is off the surface is no-data.",
                            this);
    note->setWordWrap(true);
    note->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    form->addRow(QString(), note);
    layout->addLayout(form);
    buttons_ = okCancel(this, "Export");
    layout->addWidget(buttons_);

    const auto suggest = [this] {
        const int index = surfaceIndex();
        if (index >= 0) {
            cellSize_->setValue(interop::suggestedCellSize(surfaces_[static_cast<std::size_t>(index)].bounds));
        }
    };
    connect(surface_, &QComboBox::currentIndexChanged, this, [this, suggest] {
        suggest();
        refreshGrid();
    });
    connect(cellSize_, &QDoubleSpinBox::valueChanged, this, [this] { refreshGrid(); });
    suggest();
    refreshGrid();
}

int SurfaceRasterDialog::surfaceIndex() const { return surface_->currentIndex(); }

interop::SurfaceRasterOptions SurfaceRasterDialog::options() const
{
    interop::SurfaceRasterOptions options;
    options.cellSize = cellSize_->value();
    return options;
}

// The grid the export will write, with the same arithmetic and the same
// ceiling as exportSurfaceRaster, so OK is refused here rather than failing
// after the file dialog.
void SurfaceRasterDialog::refreshGrid()
{
    const int index = surfaceIndex();
    QPushButton* ok = buttons_->button(QDialogButtonBox::Ok);
    if (index < 0) {
        grid_->setText("no surface");
        ok->setEnabled(false);
        return;
    }
    const katana::geometry::Box2& bounds = surfaces_[static_cast<std::size_t>(index)].bounds;
    const double cell = cellSize_->value();
    const double columns = std::max(1.0, std::ceil(bounds.width() / cell));
    const double rows = std::max(1.0, std::ceil(bounds.height() / cell));
    const double cells = columns * rows;
    const auto limit = static_cast<double>(interop::SurfaceRasterOptions{}.maxCells);
    grid_->setText(QString("%1 x %2 cells%3")
                       .arg(grouped(static_cast<std::uint64_t>(columns)))
                       .arg(grouped(static_cast<std::uint64_t>(rows)))
                       .arg(cells > limit ? QString(" - over the %1-cell limit; use a larger cell")
                                                .arg(grouped(static_cast<std::uint64_t>(limit)))
                                          : QString()));
    ok->setEnabled(cells <= limit);
}

} // namespace katana::qt
