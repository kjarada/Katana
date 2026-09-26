#include "gis_export_dialog.hpp"

#include <QCheckBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QLineEdit>
#include <QVBoxLayout>

#include "format.hpp"
#include "gis_dialog_support.hpp"

namespace katana::qt {

namespace interop = katana::interop;

// ---- vector export --------------------------------------------------------------------------

VectorExportDialog::VectorExportDialog(const QString& format, std::size_t selected, QWidget* parent)
    : QDialog(parent)
{
    setWindowTitle("Export Vector Data - " + format);
    auto* layout = new QVBoxLayout(this);
    auto* form = new QFormLayout();

    selectedOnly_ = new QCheckBox(
        selected == 0 ? QString("Selected entities only (nothing is selected)")
                      : QString("Only the %1 selected entities").arg(grouped(selected)),
        this);
    selectedOnly_->setEnabled(selected != 0);
    selectedOnly_->setChecked(selected != 0);
    form->addRow(QString(), selectedOnly_);

    const interop::VectorExportOptions defaults;
    layerName_ = new QLineEdit(QString::fromStdString(defaults.layerName), this);
    layerName_->setToolTip("The layer's name inside the file, for formats that hold several "
                           "(GeoPackage, KML). Katana's own layers go out as each feature's "
                           "'layer' attribute.");
    form->addRow("Layer name in the file:", layerName_);

    // Arcs and circles have no exact form in these formats. The tolerance is
    // the sagitta - the furthest a chord may stray from the true curve - and
    // is the user's to choose, because it is a loss of accuracy.
    curveTolerance_ = new QDoubleSpinBox(this);
    curveTolerance_->setDecimals(4);
    curveTolerance_->setRange(0.0001, 100.0);
    curveTolerance_->setValue(defaults.curveTolerance);
    curveTolerance_->setSuffix(" units");
    form->addRow("Arcs as chords within:", curveTolerance_);

    properties_ = new QCheckBox("Write entity properties as attributes", this);
    properties_->setChecked(defaults.propertiesAsAttributes);
    form->addRow(QString(), properties_);

    layout->addLayout(form);
    layout->addWidget(okCancel(this, "Export"));
}

bool VectorExportDialog::selectedOnly() const
{
    return selectedOnly_->isEnabled() && selectedOnly_->isChecked();
}

void VectorExportDialog::apply(interop::VectorExportOptions& options) const
{
    if (const QString name = layerName_->text().trimmed(); !name.isEmpty()) {
        options.layerName = name.toStdString();
    }
    options.curveTolerance = curveTolerance_->value();
    options.propertiesAsAttributes = properties_->isChecked();
}

} // namespace katana::qt
