// Where a geoprocessing dataset comes from, as a dialog picks it
// (binding_picker.hpp).

#include "geo/binding_picker.hpp"

#include <QComboBox>
#include <QDir>
#include <QFileDialog>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSignalBlocker>
#include <QStackedWidget>
#include <QVBoxLayout>

#include <utility>

#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace katana::qt {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// The kinds in the order the Kind choice offers them, with its text.
struct KindChoice {
    BindingKind kind;
    const char* text;
};
constexpr KindChoice kKinds[] = {
    {BindDrawing, "Drawing"},
    {BindRaster, "Reference raster"},
    {BindSurface, "Surface"},
    {BindFile, "File"},
};

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

} // namespace

unsigned bindingKindsFor(unsigned datasetKinds)
{
    unsigned kinds = BindFile;
    if ((datasetKinds & gp::DatasetKind::Vector) != 0) {
        kinds |= BindDrawing;
    }
    if ((datasetKinds & gp::DatasetKind::Raster) != 0) {
        kinds |= BindRaster | BindSurface;
    }
    return kinds;
}

BindingPicker::BindingPicker(const QString& prefix, unsigned kinds, GeoDialogContext context,
                             QWidget* parent)
    : QWidget(parent), prefix_(prefix), context_(std::move(context))
{
    setObjectName(prefix + "Source");
    kind_ = new QComboBox(this);
    kind_->setObjectName(prefix + "Kind");
    kind_->setToolTip("Where the data comes from: what is drawn, a reference raster, a surface "
                      "or a file");
    pages_ = new QStackedWidget(this);
    for (const KindChoice& choice : kKinds) {
        if ((kinds & choice.kind) == 0) {
            continue;
        }
        kind_->addItem(choice.text, static_cast<unsigned>(choice.kind));
        auto* page = new QWidget(pages_);
        auto* form = new QFormLayout(page);
        form->setContentsMargins(0, 0, 0, 0);
        switch (choice.kind) {
        case BindDrawing:
            scope_ = new ScopeFilterWidget(prefix, page);
            scope_->views = context_.views;
            scope_->setChoice(ScopeChoice::Drawing);
            scope_->onChanged = [this] { changed(); };
            form->addRow(scope_);
            break;
        case BindRaster:
            raster_ = new QComboBox(page);
            raster_->setObjectName(prefix + "Raster");
            raster_->setPlaceholderText("no reference raster yet - GIS > Import Raster, or REFS");
            raster_->setToolTip("A reference raster, read at full precision from its own file "
                                "(RASTER <id>)");
            connect(raster_, &QComboBox::currentIndexChanged, this, [this] { changed(); });
            form->addRow("Raster:", raster_);
            break;
        case BindSurface: {
            surface_ = new QComboBox(page);
            surface_->setObjectName(prefix + "Surface");
            surface_->setPlaceholderText("no surface yet - Terrain > Surfaces");
            surface_->setToolTip("A surface, sampled at the centre of each cell (SURFACE <name>)");
            cell_ = new QLineEdit(page);
            cell_->setObjectName(prefix + "Cell");
            cell_->setPlaceholderText("suggested for its extent");
            cell_->setToolTip("CELL: the side of each cell the surface is sampled on, metres");
            connect(surface_, &QComboBox::currentIndexChanged, this, [this] { changed(); });
            connect(cell_, &QLineEdit::textChanged, this, [this] { changed(); });
            form->addRow("Surface:", surface_);
            form->addRow("Cell (m):", cell_);
            break;
        }
        case BindFile: {
            file_ = new QLineEdit(page);
            file_->setObjectName(prefix + "File");
            file_->setPlaceholderText("a path, a /vsi path or a URL");
            file_->setToolTip("FILE: GDAL opens it as it is - a /vsi path or a URL passes through");
            browse_ = new QPushButton("Browse...", page);
            browse_->setObjectName(prefix + "Browse");
            browse_->setAutoDefault(false);
            connect(file_, &QLineEdit::textChanged, this, [this] { changed(); });
            connect(browse_, &QPushButton::clicked, this, [this] { browse(); });
            auto* row = new QHBoxLayout;
            row->addWidget(file_, 1);
            row->addWidget(browse_);
            form->addRow("File:", row);
            break;
        }
        }
        pages_->addWidget(page);
    }
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    auto* kindRow = new QFormLayout;
    kindRow->addRow("From:", kind_);
    layout->addLayout(kindRow);
    layout->addWidget(pages_, 1);
    connect(kind_, &QComboBox::currentIndexChanged, this, [this](int index) {
        pages_->setCurrentIndex(index);
        changed();
    });
    reload();
}

BindingKind BindingPicker::kind() const
{
    return static_cast<BindingKind>(kind_->currentData().toUInt());
}

bool BindingPicker::setKind(BindingKind kind)
{
    const int index = kind_->findData(static_cast<unsigned>(kind));
    if (index < 0) {
        return false;
    }
    kind_->setCurrentIndex(index);
    return true;
}

Result<QString> BindingPicker::words() const
{
    switch (kind()) {
    case BindDrawing: {
        auto scope = scope_->verbWords();
        if (!scope) {
            return invalid("the drawing's scope: " + QString::fromStdString(scope.error().message));
        }
        return *scope;
    }
    case BindRaster:
        if (raster_->currentIndex() < 0) {
            return invalid("choose a reference raster");
        }
        return "RASTER " + raster_->currentData().toString();
    case BindSurface: {
        if (surface_->currentIndex() < 0) {
            return invalid("choose a surface");
        }
        auto name = lineWord(surface_->currentText(), "the surface's name");
        if (!name) {
            return name.error();
        }
        QString words = "SURFACE " + *name;
        const QString cell = cell_->text().trimmed();
        if (!cell.isEmpty()) {
            const auto length = katana::core::parseFiniteDouble(cell.toStdString());
            if (!length || !(*length > 0.0)) {
                return invalid("the cell must be a positive number of metres, not '" + cell + "'");
            }
            words += " CELL " + cell;
        }
        return words;
    }
    case BindFile: {
        const QString path = file_->text().trimmed();
        if (path.isEmpty()) {
            return invalid("choose a file");
        }
        auto word = lineWord(QDir::fromNativeSeparators(path), "the file's path");
        if (!word) {
            return word.error();
        }
        return "FILE " + *word;
    }
    }
    return invalid("choose where the data comes from");
}

void BindingPicker::reload()
{
    const QSignalBlocker quietKind(kind_);
    if (raster_ != nullptr) {
        const QSignalBlocker quiet(raster_);
        const QVariant kept = raster_->currentData();
        raster_->clear();
        if (context_.reference != nullptr) {
            for (const katana::interop::RasterOverlay& raster : context_.reference->rasters()) {
                raster_->addItem(QString("%1  %2").arg(raster.id).arg(QString::fromStdString(raster.name)),
                                 QString::number(raster.id));
            }
        }
        const int index = raster_->findData(kept);
        raster_->setCurrentIndex(index >= 0 ? index : (raster_->count() > 0 ? 0 : -1));
    }
    if (surface_ != nullptr) {
        const QSignalBlocker quiet(surface_);
        const QString kept = surface_->currentText();
        surface_->clear();
        if (context_.surfaces != nullptr) {
            for (const katana::terrain::NamedSurface& surface : context_.surfaces->all()) {
                surface_->addItem(QString::fromStdString(surface.name));
            }
        }
        const int index = surface_->findText(kept);
        surface_->setCurrentIndex(index >= 0 ? index : (surface_->count() > 0 ? 0 : -1));
    }
    if (scope_ != nullptr) {
        if (context_.document != nullptr) {
            scope_->reload(context_.document->model());
        } else {
            scope_->reloadViews();
        }
    }
}

void BindingPicker::changed()
{
    if (onChanged) {
        onChanged();
    }
}

void BindingPicker::browse()
{
    if (context_.headless && context_.headless()) {
        if (say) {
            say("A headless session opens no file dialog: fill " + file_->objectName() +
                " with the path instead.");
        }
        return;
    }
    const QString path = QFileDialog::getOpenFileName(this, "Choose a Dataset", file_->text().trimmed(),
                                                      "All files (*)");
    if (!path.isEmpty()) {
        file_->setText(QDir::toNativeSeparators(path));
    }
}

} // namespace katana::qt
