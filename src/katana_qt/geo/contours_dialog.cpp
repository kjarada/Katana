// Terrain > Analysis > Contours (contours_dialog.hpp).

#include "geo/contours_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QShowEvent>
#include <QSpinBox>
#include <QVBoxLayout>

#include <utility>

#include "katana/cad/document.hpp"

namespace katana::qt {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

bool isSurface(const QString& source)
{
    return source.startsWith("SURFACE ");
}

} // namespace

Result<QString> contourLine(const ContourForm& form)
{
    if (form.source.trimmed().isEmpty()) {
        return invalid("Source: there is no surface or raster to contour; build a surface or "
                       "import a DEM first");
    }
    const QString interval = form.interval.trimmed();
    if (!numberList(interval, 1, false) || !(interval.toDouble() > 0.0)) {
        return invalid("Interval: a positive height, as 0.5");
    }
    QString line = "CONTOUR " + form.source.trimmed() + " interval=" + interval +
                   " major=" + QString::number(form.majorEvery);
    const QString base = form.base.trimmed();
    if (!base.isEmpty()) {
        if (!numberList(base, 1, false)) {
            return invalid("Base: a height, as 100");
        }
        line += " base=" + base;
    }
    const QString layer = form.layer.trimmed();
    if (!layer.isEmpty()) {
        const auto word = lineWord(layer);
        if (!word) {
            return invalid("Layer: a layer name cannot hold a double quote");
        }
        line += " layer=" + *word;
    }
    if (form.smooth != 0 && !isSurface(form.source)) {
        line += " smooth=" + QString::number(form.smooth);
    }
    if (form.clip) {
        if (!form.scopeError.isEmpty()) {
            return invalid("Keep inside: " + form.scopeError);
        }
        line += " " + form.scope.trimmed();
    }
    return line;
}

ContoursDialog::ContoursDialog(TerrainDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("contoursDialog");
    setWindowTitle("Contours");
    auto* layout = new QVBoxLayout(this);
    auto* form = new QFormLayout();

    source_ = new QComboBox(this);
    source_->setObjectName("contourSource");
    source_->setToolTip("A surface is contoured exactly on its triangles; a raster by GDAL at "
                        "its full resolution.");
    form->addRow("Source:", source_);
    interval_ = new QLineEdit("1", this);
    interval_->setObjectName("contourInterval");
    form->addRow("Interval:", interval_);
    majorEvery_ = new QSpinBox(this);
    majorEvery_->setObjectName("contourMajorEvery");
    majorEvery_->setRange(0, 1000);
    majorEvery_->setValue(5);
    majorEvery_->setSpecialValueText("none");
    form->addRow("Major every:", majorEvery_);
    base_ = new QLineEdit(this);
    base_->setObjectName("contourBase");
    base_->setPlaceholderText("0");
    form->addRow("Counted from:", base_);
    layer_ = new QLineEdit("terrain/contours", this);
    layer_->setObjectName("contourLayer");
    layer_->setToolTip("Major contours go to <layer>/major, the others to <layer>/minor.");
    form->addRow("Layer:", layer_);
    smooth_ = new QComboBox(this);
    smooth_->setObjectName("contourSmooth");
    smooth_->addItem("None", 0);
    smooth_->addItem("3 cells", 3);
    smooth_->addItem("5 cells", 5);
    smooth_->setToolTip("A raster's gaussian smoothing before it is contoured, for "
                        "presentation; the reply says smoothed=yes.");
    form->addRow("Smoothing:", smooth_);
    layout->addLayout(form);

    clip_ = new QCheckBox("Keep inside closed shapes of the drawing", this);
    clip_->setObjectName("contourClip");
    layout->addWidget(clip_);
    scope_ = new ScopeFilterWidget("contour", this);
    scope_->views = context_.views;
    // The closed shapes a person has just picked, at first.
    scope_->setChoice(ScopeChoice::Selection);
    layout->addWidget(scope_);

    auto* commandRow = new QFormLayout();
    command_ = terrainCommandField(this, "contourCommand");
    commandRow->addRow("Command:", command_);
    layout->addLayout(commandRow);

    auto* buttons = new QHBoxLayout();
    buttons->addStretch();
    preview_ = new QPushButton("Preview", this);
    preview_->setObjectName("contourPreview");
    run_ = new QPushButton("Draw Contours", this);
    run_->setObjectName("contourRun");
    run_->setDefault(true);
    buttons->addWidget(preview_);
    buttons->addWidget(run_);
    layout->addLayout(buttons);
    reply_ = terrainReplyField(this, "contourReply");
    layout->addWidget(reply_);
    runner_ = std::make_unique<TerrainRun>(context_, *reply_);

    connect(source_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    connect(smooth_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    connect(majorEvery_, &QSpinBox::valueChanged, this, [this] { refresh(); });
    connect(clip_, &QCheckBox::toggled, this, [this] { refresh(); });
    for (QLineEdit* field : {interval_, base_, layer_}) {
        connect(field, &QLineEdit::textChanged, this, [this] { refresh(); });
    }
    scope_->onChanged = [this] { refresh(); };
    connect(preview_, &QPushButton::clicked, this, [this] { run(true); });
    connect(run_, &QPushButton::clicked, this, [this] { run(false); });
    reload();
    refresh();
}

ContoursDialog::~ContoursDialog() = default;

void ContoursDialog::reload()
{
    fillSources(*source_,
                terrainSources(context_, TerrainSourceKinds::Surfaces | TerrainSourceKinds::Rasters));
    if (context_.document != nullptr) {
        scope_->reload(context_.document->model());
    } else {
        scope_->reloadViews();
    }
}

ContourForm ContoursDialog::form() const
{
    ContourForm form;
    form.source = source_->currentData().toString();
    form.interval = interval_->text();
    form.majorEvery = majorEvery_->value();
    form.base = base_->text();
    form.layer = layer_->text();
    form.smooth = smooth_->currentData().toInt();
    form.clip = clip_->isChecked();
    if (auto words = scope_->verbWords()) {
        form.scope = *words;
    } else {
        form.scopeError = QString::fromStdString(words.error().message);
    }
    return form;
}

Result<QString> ContoursDialog::command() const
{
    return contourLine(form());
}

void ContoursDialog::refresh()
{
    smooth_->setEnabled(!isSurface(source_->currentData().toString()));
    scope_->setEnabled(clip_->isChecked());
    const auto line = command();
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(line ? QString() : QString::fromStdString(line.error().message));
    preview_->setEnabled(line.ok());
    run_->setEnabled(line.ok());
}

void ContoursDialog::run(bool preview)
{
    const auto line = command();
    if (!line) {
        reply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    runner_->run(preview ? *line + " PREVIEW" : *line);
}

void ContoursDialog::showEvent(QShowEvent* event)
{
    reload();
    refresh();
    QDialog::showEvent(event);
}

} // namespace katana::qt
