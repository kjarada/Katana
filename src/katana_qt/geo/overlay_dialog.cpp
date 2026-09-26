// GIS > Analysis - GDAL > Overlay... (overlay_dialog.hpp).

#include "geo/overlay_dialog.hpp"

#include <QButtonGroup>
#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QFileDialog>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QPushButton>
#include <QRadioButton>

#include <utility>

#include "katana/cad/document.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

QString textOf(const QLineEdit* edit)
{
    return edit != nullptr ? edit->text() : QString();
}

// " key=value" for a value given, quoted when it holds a blank.
Result<QString> option(const QString& key, const QString& text, const QString& field)
{
    const QString value = text.trimmed();
    if (value.isEmpty()) {
        return QString();
    }
    auto word = gisWord(key + "=" + value, field);
    if (!word) {
        return word.error();
    }
    return " " + *word;
}

} // namespace

Result<QString> gisOverlayLine(const GisOverlayForm& form)
{
    const auto subject = gisScope(form.subject);
    if (!subject) {
        return subject.error();
    }
    QString line = "GIS OVERLAY " + form.operation + " " + *subject + " WITH ";
    if (form.withFile) {
        const QString path = form.file.trimmed();
        if (path.isEmpty()) {
            return makeError(ErrorCode::InvalidArgument, "choose the file to overlay with first");
        }
        auto word = gisWord(path, "The file's path");
        if (!word) {
            return word.error();
        }
        line += "FILE " + *word;
        if (const QString layer = form.fileLayer.trimmed(); !layer.isEmpty()) {
            auto name = gisWord(layer, "The file's layer");
            if (!name) {
                return name.error();
            }
            line += " LAYER " + *name;
        }
        auto where = option("where", form.fileWhere, "The file's WHERE clause");
        if (!where) {
            return where.error();
        }
        line += *where;
    } else {
        const auto with = gisScope(form.with);
        if (!with) {
            return makeError(ErrorCode::InvalidArgument,
                             "what to overlay with: " + with.error().message);
        }
        line += *with;
    }
    for (const auto& [key, text, field] :
         {std::tuple{QString("keep"), form.keep, QString("The subject's properties")},
          std::tuple{QString("keepwith"), form.keepWith, QString("The overlay's properties")},
          std::tuple{QString("csv"), form.csv, QString("The rows' file")}}) {
        auto words = option(key, text, field);
        if (!words) {
            return words.error();
        }
        line += *words;
    }
    auto target = gisLayerTarget(form.layer);
    if (!target) {
        return target.error();
    }
    line += *target;
    if (form.overwrite && !form.csv.trimmed().isEmpty()) {
        line += " OVERWRITE";
    }
    return line;
}

GisOverlayDialog::GisOverlayDialog(GisDialogContext context, QWidget* parent)
    : GisToolDialog("gisOverlay", "Overlay", std::move(context), parent)
{
    resize(1180, 900);
    operation_ = new QComboBox(this);
    operation_->setObjectName("gisOverlayOperation");
    operation_->addItems(
        {"intersection", "difference", "union", "symdifference", "identity", "update", "clip"});
    operation_->setToolTip("intersection: where both are (easement area per lot); difference: the "
                           "subject less the overlay (net developable area); union: all of both, "
                           "in pieces; identity: the subject split by the overlay");
    connect(operation_, &QComboBox::currentTextChanged, this, [this] { refreshCommand(); });
    fields().addRow("Operation:", operation_);

    auto* sourceGroup = new QButtonGroup(this);
    fromDrawing_ = new QRadioButton("What the drawing holds (below)", this);
    fromDrawing_->setObjectName("gisOverlayWithSourceDrawing");
    fromFile_ = new QRadioButton("A file", this);
    fromFile_->setObjectName("gisOverlayWithSourceFile");
    sourceGroup->addButton(fromDrawing_);
    sourceGroup->addButton(fromFile_);
    fromDrawing_->setChecked(true);
    auto* sourceRow = new QHBoxLayout;
    sourceRow->addWidget(fromDrawing_);
    sourceRow->addWidget(fromFile_);
    sourceRow->addStretch(1);
    fields().addRow("Overlay with:", sourceRow);
    with_ = new ScopeFilterWidget("gisOverlayWith", this);
    with_->views = this->context().views;
    with_->setChoice(ScopeChoice::Layers);
    with_->onChanged = [this] { refreshCommand(); };
    fields().addRow(with_);
    file_ = addField("gisOverlayWithFile", "File:", "a GeoPackage, shapefile, GeoJSON ...",
                     "FILE: overlay with this file's features, read through GDAL");
    browse_ = new QPushButton("Browse...", this);
    browse_->setObjectName("gisOverlayWithFileBrowse");
    browse_->setAutoDefault(false);
    fields().addRow(QString(), browse_);
    fileLayer_ = addField("gisOverlayWithFileLayer", "File's layer:", "its first",
                          "LAYER: which of the file's layers");
    fileWhere_ = addField("gisOverlayWithFileWhere", "Only where:", "e.g. kind='corridor'",
                          "where=: an SQL WHERE clause on the file's fields");
    keep_ = addField("gisOverlayKeep", "Keep subject's:", "all its properties",
                     "keep=a,b | all | none: the subject's properties each piece carries");
    keepWith_ = addField("gisOverlayKeepWith", "Keep overlay's:", "all its properties",
                         "keepwith=a,b | all | none: the overlay's properties each piece carries");
    layer_ = addField("gisOverlayLayer", "Result layer:", "default gis/overlay",
                      "TO LAYER: the layer the pieces are drawn on");
    csv_ = addField("gisOverlayCsv", "Rows to CSV:", "none",
                    "csv=: the rows - each piece's area or length and properties - written to "
                    "this file too");
    overwrite_ = new QCheckBox("Replace the CSV file if it exists", this);
    overwrite_->setObjectName("gisOverlayOverwrite");
    connect(overwrite_, &QCheckBox::toggled, this, [this] { refreshCommand(); });
    fields().addRow(QString(), overwrite_);

    connect(fromDrawing_, &QRadioButton::toggled, this, [this] { sourceChanged(); });
    connect(browse_, &QPushButton::clicked, this, [this] {
        if (this->context().headless && this->context().headless()) {
            setStatus("A headless session opens no file dialog: fill gisOverlayWithFile with the "
                      "path instead.",
                      true);
            return;
        }
        const QString path = QFileDialog::getOpenFileName(this, "Overlay With", file_->text());
        if (!path.isEmpty()) {
            file_->setText(QDir::toNativeSeparators(path));
            fromFile_->setChecked(true);
        }
    });
    sourceChanged();
}

void GisOverlayDialog::sourceChanged()
{
    const bool file = fromFile_->isChecked();
    with_->setEnabled(!file);
    file_->setEnabled(file);
    browse_->setEnabled(file);
    fileLayer_->setEnabled(file);
    fileWhere_->setEnabled(file);
    refreshCommand();
}

void GisOverlayDialog::reload()
{
    if (with_ != nullptr) {
        if (documentAlive()) {
            with_->reload(context().document->model());
        } else {
            with_->reloadViews();
        }
    }
    GisToolDialog::reload();
}

GisOverlayForm GisOverlayDialog::form() const
{
    GisOverlayForm form;
    form.operation = operation_ != nullptr ? operation_->currentText() : QString("intersection");
    form.subject = scopeWords();
    form.withFile = fromFile_ != nullptr && fromFile_->isChecked();
    if (with_ != nullptr) {
        if (const auto said = with_->verbWords()) {
            form.with.words = *said;
        } else {
            form.with.error = QString::fromStdString(said.error().message);
        }
    }
    form.file = textOf(file_);
    form.fileLayer = textOf(fileLayer_);
    form.fileWhere = textOf(fileWhere_);
    form.keep = textOf(keep_);
    form.keepWith = textOf(keepWith_);
    form.layer = textOf(layer_);
    form.csv = textOf(csv_);
    form.overwrite = overwrite_ != nullptr && overwrite_->isChecked();
    return form;
}

Result<QString> GisOverlayDialog::command() const
{
    return gisOverlayLine(form());
}

} // namespace katana::qt
