#include "surface_raster_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QShowEvent>
#include <QSignalBlocker>
#include <QStringList>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <utility>

#include "format.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/interop/terrain_io.hpp"
#include "katana/terrain/surface_store.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

namespace interop = katana::interop;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

QPlainTextEdit* replyField(QWidget* parent, const QString& name)
{
    auto* reply = new QPlainTextEdit(parent);
    reply->setObjectName(name);
    reply->setReadOnly(true);
    reply->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    reply->setMinimumHeight(110);
    return reply;
}

QLineEdit* commandField(QWidget* parent, const QString& name)
{
    auto* command = new QLineEdit(parent);
    command->setObjectName(name);
    command->setReadOnly(true);
    command->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    return command;
}

QString mutedStyle()
{
    return QString("color: %1").arg(theme::textMuted().name());
}

// A list of numbers separated by commas, as a line writes it ("2,8",
// "0,0,40,30"); each must read as a number, locale-independently.
bool numbers(const QString& text, int count, bool whole)
{
    const QStringList items = text.split(',');
    if (count > 0 && items.size() != count) {
        return false;
    }
    return std::ranges::all_of(items, [whole](const QString& item) {
        const std::string word = item.trimmed().toStdString();
        return whole ? katana::core::parseInteger(word).has_value()
                     : katana::core::parseFiniteDouble(word).has_value();
    });
}

} // namespace

// ---- Surface From ---------------------------------------------------------------------------

Result<QString> surfaceFromLine(const SurfaceFromForm& form)
{
    QString line = "SURFACE FROM ";
    if (form.kind == SurfaceFromKind::Drawing) {
        if (!form.scopeError.isEmpty()) {
            return invalid("Apply to: " + form.scopeError);
        }
        line += form.scope.trimmed().isEmpty() ? QString("DRAWING") : form.scope.trimmed();
    } else {
        if (form.source.trimmed().isEmpty()) {
            return invalid(form.kind == SurfaceFromKind::Cloud
                               ? "Source: there is no point cloud to build a surface from"
                               : "Source: there is no raster to build a surface from");
        }
        line += form.source.trimmed();
        if (form.kind == SurfaceFromKind::Cloud && !form.classes.trimmed().isEmpty()) {
            const QString classes = QString(form.classes).remove(' ');
            if (!numbers(classes, 0, true)) {
                return invalid("Classes: whole numbers separated by commas, as 2,8");
            }
            line += " classes=" + classes;
        }
        if (!form.maxPoints.trimmed().isEmpty()) {
            if (!numbers(form.maxPoints.trimmed(), 1, true)) {
                return invalid("Most points: a whole number");
            }
            line += " max=" + form.maxPoints.trimmed();
        }
        if (form.kind == SurfaceFromKind::Raster && !form.area.trimmed().isEmpty()) {
            const QString area = QString(form.area).remove(' ');
            if (!numbers(area, 4, false)) {
                return invalid("Window: four numbers x0,y0,x1,y1");
            }
            line += " AREA " + area;
        }
    }
    if (!form.name.trimmed().isEmpty()) {
        const auto name = lineWord(form.name.trimmed());
        if (!name) {
            return invalid("Name: a name cannot hold a double quote");
        }
        line += " NAME " + *name;
    }
    return line;
}

SurfaceFromDialog::SurfaceFromDialog(TerrainDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("surfaceFromDialog");
    setWindowTitle("Surface From");
    auto* layout = new QVBoxLayout(this);
    auto* form = new QFormLayout();

    kind_ = new QComboBox(this);
    kind_->setObjectName("surfaceFromKind");
    kind_->addItem("Point cloud", static_cast<int>(SurfaceFromKind::Cloud));
    kind_->addItem("Raster", static_cast<int>(SurfaceFromKind::Raster));
    kind_->addItem("Drawing", static_cast<int>(SurfaceFromKind::Drawing));
    form->addRow("From:", kind_);

    source_ = new QComboBox(this);
    source_->setObjectName("surfaceFromSource");
    form->addRow("Source:", source_);
    classes_ = new QLineEdit(this);
    classes_->setObjectName("surfaceFromClasses");
    classes_->setPlaceholderText("its ground (class 2), or every return when it has none");
    form->addRow("Classes:", classes_);
    maxPoints_ = new QLineEdit(this);
    maxPoints_->setObjectName("surfaceFromMax");
    maxPoints_->setPlaceholderText("400000");
    form->addRow("Most points:", maxPoints_);
    area_ = new QLineEdit(this);
    area_->setObjectName("surfaceFromArea");
    area_->setPlaceholderText("x0,y0,x1,y1 - blank: the whole raster");
    form->addRow("Window:", area_);
    layout->addLayout(form);

    scope_ = new ScopeFilterWidget("surfaceFrom", this);
    scope_->views = context_.views;
    // What the window's Surface From Drawing always took: everything drawn.
    scope_->setChoice(ScopeChoice::Drawing);
    if (auto* drawn = scope_->findChild<QCheckBox*>("surfaceFromDrawnOnly")) {
        drawn->setChecked(true);
    }
    layout->addWidget(scope_);

    auto* nameRow = new QFormLayout();
    name_ = new QLineEdit(this);
    name_->setObjectName("surfaceFromName");
    name_->setPlaceholderText("the source's name");
    nameRow->addRow("Name:", name_);
    command_ = commandField(this, "surfaceFromCommand");
    nameRow->addRow("Command:", command_);
    layout->addLayout(nameRow);

    auto* buttons = new QHBoxLayout();
    buttons->addStretch();
    preview_ = new QPushButton("Preview", this);
    preview_->setObjectName("surfaceFromPreview");
    run_ = new QPushButton("Build Surface", this);
    run_->setObjectName("surfaceFromRun");
    run_->setDefault(true);
    buttons->addWidget(preview_);
    buttons->addWidget(run_);
    layout->addLayout(buttons);
    reply_ = replyField(this, "surfaceFromReply");
    layout->addWidget(reply_);
    runner_ = std::make_unique<TerrainRun>(context_, *reply_);

    connect(kind_, &QComboBox::currentIndexChanged, this, [this] {
        reload();
        refresh();
    });
    connect(source_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    for (QLineEdit* field : {classes_, maxPoints_, area_, name_}) {
        connect(field, &QLineEdit::textChanged, this, [this] { refresh(); });
    }
    scope_->onChanged = [this] { refresh(); };
    connect(preview_, &QPushButton::clicked, this, [this] { run(true); });
    connect(run_, &QPushButton::clicked, this, [this] { run(false); });
    reload();
    refresh();
}

SurfaceFromDialog::~SurfaceFromDialog() = default;

void SurfaceFromDialog::showKind(SurfaceFromKind kind, const QString& words)
{
    kind_->setCurrentIndex(kind_->findData(static_cast<int>(kind)));
    reload();
    if (!words.isEmpty()) {
        const int found = source_->findData(words);
        if (found >= 0) {
            source_->setCurrentIndex(found);
        }
    }
    refresh();
}

void SurfaceFromDialog::reload()
{
    const auto kind = static_cast<SurfaceFromKind>(kind_->currentData().toInt());
    fillSources(*source_, terrainSources(context_, kind == SurfaceFromKind::Cloud
                                                       ? TerrainSourceKinds::Clouds
                                                       : TerrainSourceKinds::Rasters));
    if (context_.document != nullptr) {
        scope_->reload(context_.document->model());
    } else {
        scope_->reloadViews();
    }
}

SurfaceFromForm SurfaceFromDialog::form() const
{
    SurfaceFromForm form;
    form.kind = static_cast<SurfaceFromKind>(kind_->currentData().toInt());
    form.source = source_->currentData().toString();
    form.classes = classes_->text();
    form.maxPoints = maxPoints_->text();
    form.area = area_->text();
    if (auto words = scope_->verbWords()) {
        form.scope = *words;
    } else {
        form.scopeError = QString::fromStdString(words.error().message);
    }
    form.name = name_->text();
    return form;
}

Result<QString> SurfaceFromDialog::command() const
{
    return surfaceFromLine(form());
}

void SurfaceFromDialog::refresh()
{
    const auto kind = static_cast<SurfaceFromKind>(kind_->currentData().toInt());
    source_->setEnabled(kind != SurfaceFromKind::Drawing);
    classes_->setEnabled(kind == SurfaceFromKind::Cloud);
    maxPoints_->setEnabled(kind != SurfaceFromKind::Drawing);
    area_->setEnabled(kind == SurfaceFromKind::Raster);
    scope_->setEnabled(kind == SurfaceFromKind::Drawing);
    const auto line = command();
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(line ? QString() : QString::fromStdString(line.error().message));
    preview_->setEnabled(line.ok());
    run_->setEnabled(line.ok());
}

void SurfaceFromDialog::run(bool preview)
{
    const auto line = command();
    if (!line) {
        reply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    runner_->run(preview ? *line + " PREVIEW" : *line);
}

void SurfaceFromDialog::showEvent(QShowEvent* event)
{
    reload();
    refresh();
    QDialog::showEvent(event);
}

// ---- Export Surface as DEM -----------------------------------------------------------------

Result<QString> surfaceExportLine(const SurfaceExportForm& form)
{
    if (form.surface.trimmed().isEmpty()) {
        return invalid("Surface: there is no surface to export; build one from the Terrain menu");
    }
    if (form.file.trimmed().isEmpty()) {
        return invalid("File: choose where the DEM goes (.tif, .asc or .img)");
    }
    if (!(form.cell > 0.0) || !std::isfinite(form.cell)) {
        return invalid("Cell: a positive length");
    }
    const auto surface = lineWord(form.surface.trimmed());
    const auto file = lineWord(QDir::fromNativeSeparators(form.file.trimmed()));
    if (!surface || !file) {
        return invalid(!surface ? "Surface: a name cannot hold a double quote"
                                : "File: a path cannot hold a double quote");
    }
    QString line = "SURFACE EXPORT " + *surface + " " + *file +
                   " cell=" + QString::fromStdString(katana::core::formatExactReal(form.cell)) +
                   " type=" + form.type;
    if (form.cog) {
        line += " COG";
    }
    if (form.overwrite) {
        line += " OVERWRITE";
    }
    return line;
}

SurfaceRasterDialog::SurfaceRasterDialog(TerrainDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("surfaceRasterDialog");
    setWindowTitle("Export Surface as DEM");
    auto* layout = new QVBoxLayout(this);
    auto* form = new QFormLayout();

    surface_ = new QComboBox(this);
    surface_->setObjectName("surfaceRasterSource");
    form->addRow("Surface:", surface_);

    cellSize_ = new QDoubleSpinBox(this);
    cellSize_->setObjectName("surfaceRasterCell");
    cellSize_->setDecimals(3);
    cellSize_->setRange(0.001, 100'000.0);
    cellSize_->setSuffix(" units");
    form->addRow("Cell size:", cellSize_);

    grid_ = new QLabel(this);
    grid_->setObjectName("surfaceRasterGrid");
    form->addRow("Grid:", grid_);
    auto* note = new QLabel("Each cell holds the surface's elevation at its centre; a cell "
                            "whose centre is off the surface is no-data.",
                            this);
    note->setWordWrap(true);
    note->setStyleSheet(mutedStyle());
    form->addRow(QString(), note);

    type_ = new QComboBox(this);
    type_->setObjectName("surfaceRasterType");
    type_->addItems({"Float32", "Float64"});
    form->addRow("Stored as:", type_);
    cog_ = new QCheckBox("Cloud Optimised GeoTIFF", this);
    cog_->setObjectName("surfaceRasterCog");
    form->addRow(QString(), cog_);

    auto* fileRow = new QHBoxLayout();
    file_ = new QLineEdit(this);
    file_->setObjectName("surfaceRasterFile");
    file_->setPlaceholderText("dem.tif, dem.asc or dem.img");
    browse_ = new QPushButton("Browse...", this);
    browse_->setObjectName("surfaceRasterBrowse");
    fileRow->addWidget(file_);
    fileRow->addWidget(browse_);
    form->addRow("File:", fileRow);
    overwrite_ = new QCheckBox("Replace a file already there", this);
    overwrite_->setObjectName("surfaceRasterOverwrite");
    form->addRow(QString(), overwrite_);
    command_ = commandField(this, "surfaceRasterCommand");
    form->addRow("Command:", command_);
    layout->addLayout(form);

    auto* buttons = new QHBoxLayout();
    buttons->addStretch();
    preview_ = new QPushButton("Preview", this);
    preview_->setObjectName("surfaceRasterPreview");
    run_ = new QPushButton("Export", this);
    run_->setObjectName("surfaceRasterRun");
    run_->setDefault(true);
    buttons->addWidget(preview_);
    buttons->addWidget(run_);
    layout->addLayout(buttons);
    reply_ = replyField(this, "surfaceRasterReply");
    layout->addWidget(reply_);
    runner_ = std::make_unique<TerrainRun>(context_, *reply_);

    connect(surface_, &QComboBox::currentIndexChanged, this, [this] {
        suggestCell();
        refresh();
    });
    connect(cellSize_, &QDoubleSpinBox::valueChanged, this, [this] { refresh(); });
    connect(type_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    connect(cog_, &QCheckBox::toggled, this, [this] { refresh(); });
    connect(overwrite_, &QCheckBox::toggled, this, [this] { refresh(); });
    connect(file_, &QLineEdit::textChanged, this, [this] { refresh(); });
    connect(browse_, &QPushButton::clicked, this, [this] { browse(); });
    connect(preview_, &QPushButton::clicked, this, [this] { run(true); });
    connect(run_, &QPushButton::clicked, this, [this] { run(false); });
    reload();
    refresh();
}

SurfaceRasterDialog::~SurfaceRasterDialog() = default;

void SurfaceRasterDialog::reload()
{
    const QString kept = surface_->currentText();
    {
        const QSignalBlocker quiet(surface_);
        surface_->clear();
        if (context_.surfaces != nullptr) {
            for (const katana::terrain::NamedSurface& named : context_.surfaces->all()) {
                surface_->addItem(QString::fromStdString(named.name));
            }
        }
        const int again = surface_->findText(kept);
        surface_->setCurrentIndex(again >= 0 ? again : (surface_->count() > 0 ? 0 : -1));
    }
    if (surface_->currentText() != kept) {
        suggestCell();
    }
}

void SurfaceRasterDialog::suggestCell()
{
    if (context_.surfaces == nullptr) {
        return;
    }
    if (const katana::terrain::NamedSurface* named =
            context_.surfaces->find(surface_->currentText().toStdString())) {
        cellSize_->setValue(interop::suggestedCellSize(named->surface->bounds()));
    }
}

SurfaceExportForm SurfaceRasterDialog::form() const
{
    SurfaceExportForm form;
    form.surface = surface_->currentText();
    form.file = file_->text();
    form.cell = cellSize_->value();
    form.type = type_->currentText();
    form.cog = cog_->isChecked();
    form.overwrite = overwrite_->isChecked();
    return form;
}

Result<QString> SurfaceRasterDialog::command() const
{
    return surfaceExportLine(form());
}

// The grid the export will write, with the same arithmetic and the same
// ceiling as surfaceGrid, so Export is refused here rather than failing
// after the job starts.
void SurfaceRasterDialog::refresh()
{
    overLimit_ = false;
    const katana::terrain::NamedSurface* named =
        context_.surfaces == nullptr ? nullptr
                                     : context_.surfaces->find(surface_->currentText().toStdString());
    if (named == nullptr) {
        grid_->setText("no surface");
    } else {
        const katana::geometry::Box2& bounds = named->surface->bounds();
        const double cell = cellSize_->value();
        const double columns = std::max(1.0, std::ceil(bounds.width() / cell));
        const double rows = std::max(1.0, std::ceil(bounds.height() / cell));
        const auto limit = static_cast<double>(interop::SurfaceRasterOptions{}.maxCells);
        overLimit_ = columns * rows > limit;
        grid_->setText(QString("%1 x %2 cells%3")
                           .arg(grouped(static_cast<std::uint64_t>(columns)))
                           .arg(grouped(static_cast<std::uint64_t>(rows)))
                           .arg(overLimit_ ? QString(" - over the %1-cell limit; use a larger cell")
                                                 .arg(grouped(static_cast<std::uint64_t>(limit)))
                                           : QString()));
    }
    const auto line = command();
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(line ? QString() : QString::fromStdString(line.error().message));
    preview_->setEnabled(line.ok());
    run_->setEnabled(line.ok() && !overLimit_);
}

void SurfaceRasterDialog::browse()
{
    // A headless run has nobody to answer a file dialog: the field is filled
    // by name instead (--fill surfaceRasterFile=...).
    if (context_.headless && context_.headless()) {
        reply_->setPlainText("A headless session opens no file dialog: fill surfaceRasterFile "
                             "with the path instead.");
        return;
    }
    QStringList filters;
    for (const interop::FormatChoice& format : interop::rasterExportFormats()) {
        filters << (QString::fromStdString(format.description) + " (*." +
                    QString::fromStdString(format.extension) + ")");
    }
    const QString chosen = QFileDialog::getSaveFileName(
        this, "Export Surface as DEM", surface_->currentText() + ".tif", filters.join(";;"));
    if (chosen.isEmpty()) {
        return;
    }
    file_->setText(QDir::fromNativeSeparators(chosen));
    // The save dialog asked before it gave back a file that is there.
    overwrite_->setChecked(QFileInfo::exists(chosen));
}

void SurfaceRasterDialog::run(bool preview)
{
    const auto line = command();
    if (!line) {
        reply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    runner_->run(preview ? *line + " PREVIEW" : *line);
}

void SurfaceRasterDialog::showEvent(QShowEvent* event)
{
    reload();
    refresh();
    QDialog::showEvent(event);
}

} // namespace katana::qt
