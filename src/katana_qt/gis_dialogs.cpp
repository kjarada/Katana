#include "gis_dialogs.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QVBoxLayout>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <utility>

#include "format.hpp"
#include "import_placement.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace interop = katana::interop;

namespace {

// What the file is, above the options, in the words Dataset Information uses:
// the same formatDescription, so the dialog and the info window cannot
// describe one file two ways.
QLabel* summaryLabel(const interop::SourceDescription& source, QWidget* parent)
{
    auto* label = new QLabel(QString::fromStdString(interop::formatDescription(source)).trimmed(),
                             parent);
    label->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    label->setTextInteractionFlags(Qt::TextSelectableByMouse);
    label->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    label->setWordWrap(false);
    return label;
}

QDialogButtonBox* okCancel(QDialog* dialog, const QString& okText)
{
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, dialog);
    buttons->button(QDialogButtonBox::Ok)->setText(okText);
    QObject::connect(buttons, &QDialogButtonBox::accepted, dialog, &QDialog::accept);
    QObject::connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    return buttons;
}

// ASPRS LAS 1.4 R15 table 17, the standard point classes. 8 and 12 are
// reserved in 1.4 (Model Key-point and Overlap in 1.2 and 1.3), so they are
// named as reserved rather than given a meaning the file may not have.
constexpr std::array<std::pair<int, const char*>, 19> kAsprsClasses{{
    {0, "Created, never classified"},
    {1, "Unclassified"},
    {2, "Ground"},
    {3, "Low vegetation"},
    {4, "Medium vegetation"},
    {5, "High vegetation"},
    {6, "Building"},
    {7, "Low point (noise)"},
    {8, "Reserved"},
    {9, "Water"},
    {10, "Rail"},
    {11, "Road surface"},
    {12, "Reserved"},
    {13, "Wire - guard (shield)"},
    {14, "Wire - conductor (phase)"},
    {15, "Transmission tower"},
    {16, "Wire-structure connector"},
    {17, "Bridge deck"},
    {18, "High noise"},
}};

} // namespace

// ---- vector import --------------------------------------------------------------------------

VectorImportDialog::VectorImportDialog(const interop::SourceDescription& source,
                                       const katana::geometry::Box2& drawing, QWidget* parent)
    : QDialog(parent)
{
    setObjectName("vectorImportDialog");
    setWindowTitle("Import Vector Data - " +
                   QString::fromStdWString(source.path.filename().wstring()));
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(summaryLabel(source, this));

    auto* form = new QFormLayout();
    sourceLayer_ = new QComboBox(this);
    sourceLayer_->setObjectName("importSourceLayer");
    // -1 is interop's "every layer", and the one worth defaulting to: a
    // GeoPackage's layers are usually meant to arrive together.
    sourceLayer_->addItem("All layers", -1);
    for (std::size_t i = 0; i < source.vectorLayers.size(); ++i) {
        const auto& layer = source.vectorLayers[i];
        sourceLayer_->addItem(QString("%1  (%2 %3)")
                                  .arg(QString::fromStdString(layer.name))
                                  .arg(grouped(layer.featureCount))
                                  .arg(QString::fromStdString(layer.geometryType)),
                              static_cast<int>(i));
    }
    form->addRow("Source layer:", sourceLayer_);

    targetLayer_ = new QLineEdit(this);
    targetLayer_->setObjectName("importTargetLayer");
    targetLayer_->setPlaceholderText("one Katana layer per source layer");
    targetLayer_->setToolTip("Put every imported entity on this layer. Left empty, each source "
                             "layer - or each DXF layer - becomes a Katana layer of its own.");
    form->addRow("Katana layer:", targetLayer_);

    attributes_ = new QCheckBox("Keep feature attributes as entity properties", this);
    attributes_->setObjectName("importAttributes");
    attributes_->setChecked(interop::VectorImportOptions{}.attributesAsProperties);
    form->addRow(QString(), attributes_);
    layout->addLayout(form);
    // The file's path is not shown as a line: this dialog's layer and
    // attribute choices have no IMPORT word, so no line does all it does.
    placement_ = new ImportPlacementBox(drawing, QString(), this);
    layout->addWidget(placement_);
    layout->addWidget(okCancel(this, "Import"));
}

interop::VectorImportOptions VectorImportDialog::options() const
{
    interop::VectorImportOptions options;
    options.sourceLayerIndex = sourceLayer_->currentData().toInt();
    options.targetLayer = targetLayer_->text().trimmed().toStdString();
    options.attributesAsProperties = attributes_->isChecked();
    return options;
}

// ---- raster import --------------------------------------------------------------------------

RasterImportDialog::RasterImportDialog(const interop::SourceDescription& source, QWidget* parent)
    : QDialog(parent)
{
    setWindowTitle("Import Raster - " + QString::fromStdWString(source.path.filename().wstring()));
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(summaryLabel(source, this));

    auto* form = new QFormLayout();
    name_ = new QLineEdit(QString::fromStdWString(source.path.stem().wstring()), this);
    form->addRow("Name:", name_);

    // The DISPLAY copy's longest side. The file itself is never resampled for
    // analysis - Surface From Raster reads the band through GDAL - so this is
    // a trade of sharpness against memory (4 bytes a pixel), nothing more.
    resolution_ = new QComboBox(this);
    for (const int pixels : {1024, 2048, 4096, 8192}) {
        resolution_->addItem(QString("%1 px  (%2 MB at most)")
                                 .arg(pixels)
                                 .arg(static_cast<qulonglong>(pixels) * pixels * 4 / (1024 * 1024)),
                             pixels);
    }
    resolution_->setCurrentIndex(
        resolution_->findData(interop::RasterImportOptions{}.maxPixels));
    form->addRow("Display resolution:", resolution_);
    layout->addLayout(form);
    layout->addWidget(okCancel(this, "Import"));
}

interop::RasterImportOptions RasterImportDialog::options() const
{
    interop::RasterImportOptions options;
    options.name = name_->text().trimmed().toStdString();
    options.maxPixels = resolution_->currentData().toInt();
    return options;
}

// ---- point cloud import ---------------------------------------------------------------------

PointCloudImportDialog::PointCloudImportDialog(const interop::SourceDescription& source,
                                               QWidget* parent)
    : QDialog(parent), pointCount_(source.pointCloud ? source.pointCloud->pointCount : 0)
{
    setWindowTitle("Import Point Cloud - " +
                   QString::fromStdWString(source.path.filename().wstring()));
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(summaryLabel(source, this));

    auto* form = new QFormLayout();
    name_ = new QLineEdit(QString::fromStdWString(source.path.stem().wstring()), this);
    form->addRow("Name:", name_);

    // The budget is what makes a billion-point file open like a small one;
    // the ceiling is QSpinBox's own (an int), far above what fits in memory
    // as PointCloudPoint values (40 bytes each: 2^31 of them is 80 GB).
    budget_ = new QSpinBox(this);
    budget_->setRange(1'000, std::numeric_limits<int>::max());
    budget_->setSingleStep(500'000);
    budget_->setGroupSeparatorShown(true);
    budget_->setValue(static_cast<int>(interop::PointCloudImportOptions{}.budget));
    form->addRow("Points to keep:", budget_);

    classification_ = new QComboBox(this);
    classification_->addItem("Every class", -1);
    for (const auto& [value, name] : kAsprsClasses) {
        classification_->addItem(QString("%1 - %2").arg(value).arg(name), value);
    }
    form->addRow("Classification:", classification_);

    // COPC answers a level-of-detail query from its own octree instead of
    // being decimated; offered only where the file can answer it, because the
    // engine refuses the question of any other file.
    const bool copc = source.pointCloud && source.pointCloud->copc;
    useResolution_ = new QCheckBox("Read the COPC octree at a point spacing of", this);
    useResolution_->setEnabled(copc);
    resolution_ = new QDoubleSpinBox(this);
    resolution_->setDecimals(3);
    resolution_->setRange(0.001, 1'000'000.0);
    resolution_->setValue(1.0);
    resolution_->setSuffix(" units");
    resolution_->setEnabled(false);
    auto* resolutionRow = new QHBoxLayout();
    resolutionRow->addWidget(useResolution_);
    resolutionRow->addWidget(resolution_);
    form->addRow(copc ? QString("Level of detail:") : QString("Level of detail (COPC only):"),
                 resolutionRow);

    estimate_ = new QLabel(this);
    estimate_->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    form->addRow(QString(), estimate_);
    layout->addLayout(form);
    layout->addWidget(okCancel(this, "Import"));

    connect(budget_, &QSpinBox::valueChanged, this, [this] { refreshEstimate(); });
    connect(classification_, &QComboBox::currentIndexChanged, this, [this] { refreshEstimate(); });
    connect(useResolution_, &QCheckBox::toggled, this, [this](bool on) {
        resolution_->setEnabled(on);
        refreshEstimate();
    });
    refreshEstimate();
}

void PointCloudImportDialog::refreshEstimate()
{
    if (useResolution_->isChecked()) {
        estimate_->setText("The octree levels at or above that spacing, up to the budget.");
        return;
    }
    const auto budget = static_cast<std::uint64_t>(budget_->value());
    const std::uint32_t step =
        katana::pointcloud::PointCloudEngine::decimationForBudget(pointCount_, budget);
    QString text = step > 1 ? QString("One point in %1 - about %2 of %3.")
                                  .arg(step)
                                  .arg(grouped(pointCount_ / step))
                                  .arg(grouped(pointCount_))
                            : QString("Every point - %1.").arg(grouped(pointCount_));
    // A class filter thins after the step, and how much depends on the
    // file: say so rather than print a number that is only an upper bound.
    if (classification_->currentData().toInt() >= 0) {
        text += " Fewer with a class filter.";
    }
    estimate_->setText(text);
}

interop::PointCloudImportOptions PointCloudImportDialog::options() const
{
    interop::PointCloudImportOptions options;
    options.name = name_->text().trimmed().toStdString();
    options.budget = static_cast<std::uint64_t>(budget_->value());
    if (const int value = classification_->currentData().toInt(); value >= 0) {
        options.classification = static_cast<std::uint8_t>(value);
    }
    if (useResolution_->isChecked()) {
        options.resolution = resolution_->value();
    }
    return options;
}

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

// ---- information ----------------------------------------------------------------------------

DatasetInfoDialog::DatasetInfoDialog(const QString& title, const QString& text, QWidget* parent)
    : QDialog(parent)
{
    setWindowTitle(title);
    resize(720, 480);
    auto* layout = new QVBoxLayout(this);
    auto* view = new QPlainTextEdit(text, this);
    view->setReadOnly(true);
    view->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    view->setLineWrapMode(QPlainTextEdit::NoWrap);
    layout->addWidget(view);
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Close, this);
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    layout->addWidget(buttons);
}

} // namespace katana::qt
