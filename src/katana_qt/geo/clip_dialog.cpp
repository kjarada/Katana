// GIS > Analysis - GDAL > Clip to Boundary... (clip_dialog.hpp).

#include "geo/clip_dialog.hpp"

#include <QButtonGroup>
#include <QCheckBox>
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

} // namespace

Result<QString> gisClipLine(const GisClipForm& form)
{
    const auto scope = gisScope(form.scope);
    if (!scope) {
        return scope.error();
    }
    QString line = "GIS CLIP " + *scope + " BY ";
    if (form.byFile) {
        const QString path = form.file.trimmed();
        if (path.isEmpty()) {
            return makeError(ErrorCode::InvalidArgument, "choose the boundary's file first");
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
        if (const QString where = form.fileWhere.trimmed(); !where.isEmpty()) {
            auto clause = gisWord("where=" + where, "The file's WHERE clause");
            if (!clause) {
                return clause.error();
            }
            line += " " + *clause;
        }
    } else {
        const auto by = gisScope(form.by);
        if (!by) {
            return makeError(ErrorCode::InvalidArgument, "the boundary: " + by.error().message);
        }
        line += *by;
    }
    if (form.replace) {
        return line + " REPLACE";
    }
    const auto target = gisLayerTarget(form.layer);
    if (!target) {
        return target.error();
    }
    return line + *target;
}

GisClipDialog::GisClipDialog(GisDialogContext context, QWidget* parent)
    : GisToolDialog("gisClip", "Clip to Boundary", std::move(context), parent)
{
    resize(1180, 900);
    auto* sourceGroup = new QButtonGroup(this);
    fromDrawing_ = new QRadioButton("Areas in the drawing (below)", this);
    fromDrawing_->setObjectName("gisClipBySourceDrawing");
    fromFile_ = new QRadioButton("A file", this);
    fromFile_->setObjectName("gisClipBySourceFile");
    sourceGroup->addButton(fromDrawing_);
    sourceGroup->addButton(fromFile_);
    fromDrawing_->setChecked(true);
    auto* sourceRow = new QHBoxLayout;
    sourceRow->addWidget(fromDrawing_);
    sourceRow->addWidget(fromFile_);
    sourceRow->addStretch(1);
    fields().addRow("Clip to:", sourceRow);
    by_ = new ScopeFilterWidget("gisClipBy", this);
    by_->views = this->context().views;
    by_->setChoice(ScopeChoice::Layers);
    by_->onChanged = [this] { refreshCommand(); };
    fields().addRow(by_);
    file_ = addField("gisClipByFile", "File:", "a GeoPackage, shapefile, GeoJSON ...",
                     "FILE: clip to this file's areas, read by GDAL");
    browse_ = new QPushButton("Browse...", this);
    browse_->setObjectName("gisClipByFileBrowse");
    browse_->setAutoDefault(false);
    fields().addRow(QString(), browse_);
    fileLayer_ = addField("gisClipByFileLayer", "File's layer:", "its first",
                          "LAYER: which of the file's layers");
    fileWhere_ = addField("gisClipByFileWhere", "Only where:", "e.g. kind='site'",
                          "where=: an SQL WHERE clause on the file's fields");
    replace_ = new QCheckBox("Cut the entities in place", this);
    replace_->setObjectName("gisClipReplace");
    replace_->setToolTip("REPLACE: each entity keeps its id on its largest piece; the rest are "
                         "made beside it, and what lies wholly outside is deleted - one undo "
                         "step");
    fields().addRow(QString(), replace_);
    layer_ = addField("gisClipLayer", "Result layer:", "default gis/clip",
                      "TO LAYER: the layer the pieces are drawn on, the entities left whole");

    connect(fromDrawing_, &QRadioButton::toggled, this, [this] { sourceChanged(); });
    connect(replace_, &QCheckBox::toggled, this, [this] {
        layer_->setEnabled(!replace_->isChecked());
        refreshCommand();
    });
    connect(browse_, &QPushButton::clicked, this, [this] {
        if (this->context().headless && this->context().headless()) {
            setStatus("A headless session opens no file dialog: fill gisClipByFile with the path "
                      "instead.",
                      true);
            return;
        }
        const QString path = QFileDialog::getOpenFileName(this, "Clip to Boundary", file_->text());
        if (!path.isEmpty()) {
            file_->setText(QDir::toNativeSeparators(path));
            fromFile_->setChecked(true);
        }
    });
    sourceChanged();
}

void GisClipDialog::sourceChanged()
{
    const bool file = fromFile_->isChecked();
    by_->setEnabled(!file);
    file_->setEnabled(file);
    browse_->setEnabled(file);
    fileLayer_->setEnabled(file);
    fileWhere_->setEnabled(file);
    refreshCommand();
}

void GisClipDialog::reload()
{
    if (by_ != nullptr) {
        if (documentAlive()) {
            by_->reload(context().document->model());
        } else {
            by_->reloadViews();
        }
    }
    GisToolDialog::reload();
}

GisClipForm GisClipDialog::form() const
{
    GisClipForm form;
    form.scope = scopeWords();
    form.byFile = fromFile_ != nullptr && fromFile_->isChecked();
    if (by_ != nullptr) {
        if (const auto said = by_->verbWords()) {
            form.by.words = *said;
        } else {
            form.by.error = QString::fromStdString(said.error().message);
        }
    }
    form.file = textOf(file_);
    form.fileLayer = textOf(fileLayer_);
    form.fileWhere = textOf(fileWhere_);
    form.replace = replace_ != nullptr && replace_->isChecked();
    form.layer = textOf(layer_);
    return form;
}

Result<QString> GisClipDialog::command() const
{
    return gisClipLine(form());
}

} // namespace katana::qt
