// Terrain > Analysis > Terrain Shading (terrain_shading_dialog.hpp).

#include "geo/terrain_shading_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QShowEvent>
#include <QVBoxLayout>

#include <initializer_list>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::qt {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

bool lit(const QString& style)
{
    return style == "hillshade" || style == "relief+hillshade";
}

// The ramp a coloured style paints with unless another is chosen, as the
// verb has it; empty for a hillshade, which is grey.
QString defaultRamp(const QString& style)
{
    if (style == "relief" || style == "relief+hillshade") {
        return "terrain";
    }
    if (style == "slope") {
        return "slope";
    }
    if (style == "plain") {
        return "grey";
    }
    return {};
}

QString number(double value)
{
    return QString::fromStdString(katana::core::formatExactReal(value));
}

} // namespace

Result<QString> shadingLine(const ShadingForm& form)
{
    if (form.source.trimmed().isEmpty()) {
        return invalid("Source: there is no surface or raster to shade; build a surface or "
                       "import a DEM first");
    }
    QString line = "RASTER SHADE " + form.source.trimmed() + " style=" + form.style;
    if (lit(form.style)) {
        const bool everySide = form.variant == "multidirectional";
        if (form.azimuth != 315.0 && !everySide) {
            line += " azimuth=" + number(form.azimuth);
        }
        if (form.altitude != 45.0) {
            line += " altitude=" + number(form.altitude);
        }
        if (form.zFactor != 1.0) {
            line += " z=" + number(form.zFactor);
        }
        if (form.variant != "regular") {
            line += " variant=" + form.variant;
        }
    }
    const QString ramp = form.ramp.trimmed();
    if (!defaultRamp(form.style).isEmpty()) {
        if (!ramp.isEmpty() && ramp.compare(defaultRamp(form.style), Qt::CaseInsensitive) != 0) {
            const auto word = lineWord(QDir::fromNativeSeparators(ramp));
            if (!word) {
                return invalid("Ramp: a colour-map path cannot hold a double quote");
            }
            line += " ramp=" + *word;
        }
        const QString low = form.rangeMin.trimmed();
        const QString high = form.rangeMax.trimmed();
        if (low.isEmpty() != high.isEmpty()) {
            return invalid("Range: both ends, or neither for the data's own");
        }
        if (!low.isEmpty()) {
            if (!numberList(low, 1, false) || !numberList(high, 1, false) ||
                !(high.toDouble() > low.toDouble())) {
                return invalid("Range: two numbers, the low end before the high");
            }
            line += " range=" + low + "," + high;
        }
    }
    if (!form.name.trimmed().isEmpty()) {
        const auto name = lineWord(form.name.trimmed());
        if (!name) {
            return invalid("Name: a name cannot hold a double quote");
        }
        line += " NAME " + *name;
    }
    if (!form.save.trimmed().isEmpty()) {
        const auto file = lineWord(QDir::fromNativeSeparators(form.save.trimmed()));
        if (!file) {
            return invalid("Save to: a path cannot hold a double quote");
        }
        line += " save=" + *file;
        if (form.overwrite) {
            line += " OVERWRITE";
        }
    }
    return line;
}

TerrainShadingDialog::TerrainShadingDialog(TerrainDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("terrainShadingDialog");
    setWindowTitle("Terrain Shading");
    auto* layout = new QVBoxLayout(this);
    auto* form = new QFormLayout();

    source_ = new QComboBox(this);
    source_->setObjectName("shadingSource");
    form->addRow("Source:", source_);
    style_ = new QComboBox(this);
    style_->setObjectName("shadingStyle");
    style_->addItem("Hillshade", "hillshade");
    style_->addItem("Colour relief", "relief");
    style_->addItem("Relief over hillshade", "relief+hillshade");
    style_->addItem("Slope", "slope");
    style_->addItem("Plain (grey)", "plain");
    form->addRow("Style:", style_);

    azimuth_ = new QDoubleSpinBox(this);
    azimuth_->setObjectName("shadingAzimuth");
    azimuth_->setRange(0.0, 360.0);
    azimuth_->setDecimals(1);
    azimuth_->setValue(315.0);
    azimuth_->setSuffix(" deg");
    azimuth_->setToolTip("Where the light comes from, clockwise from north.");
    form->addRow("Light from:", azimuth_);
    altitude_ = new QDoubleSpinBox(this);
    altitude_->setObjectName("shadingAltitude");
    altitude_->setRange(0.0, 90.0);
    altitude_->setDecimals(1);
    altitude_->setValue(45.0);
    altitude_->setSuffix(" deg");
    form->addRow("Light height:", altitude_);
    zFactor_ = new QDoubleSpinBox(this);
    zFactor_->setObjectName("shadingZFactor");
    zFactor_->setRange(0.01, 1000.0);
    zFactor_->setDecimals(2);
    zFactor_->setValue(1.0);
    zFactor_->setToolTip("Vertical exaggeration of the heights before they are lit.");
    form->addRow("Exaggeration:", zFactor_);
    variant_ = new QComboBox(this);
    variant_->setObjectName("shadingVariant");
    for (const char* variant : {"regular", "combined", "multidirectional", "igor"}) {
        variant_->addItem(variant, variant);
    }
    form->addRow("Hillshade:", variant_);

    ramp_ = new QComboBox(this);
    ramp_->setObjectName("shadingRamp");
    ramp_->setEditable(true);
    ramp_->addItems({"terrain", "diverging", "slope", "grey"});
    ramp_->setToolTip("A built-in ramp, or the path of a GDAL colour-map file typed in.");
    form->addRow("Ramp:", ramp_);
    auto* rangeRow = new QHBoxLayout();
    rangeMin_ = new QLineEdit(this);
    rangeMin_->setObjectName("shadingRangeMin");
    rangeMin_->setPlaceholderText("the data's least");
    rangeMax_ = new QLineEdit(this);
    rangeMax_->setObjectName("shadingRangeMax");
    rangeMax_->setPlaceholderText("the data's greatest");
    rangeRow->addWidget(rangeMin_);
    rangeRow->addWidget(rangeMax_);
    form->addRow("Range:", rangeRow);

    name_ = new QLineEdit(this);
    name_->setObjectName("shadingName");
    name_->setPlaceholderText("the source's and the style's");
    form->addRow("Name:", name_);
    auto* saveRow = new QHBoxLayout();
    save_ = new QLineEdit(this);
    save_->setObjectName("shadingSave");
    save_->setPlaceholderText("also write the picture to a GeoTIFF (.tif)");
    saveBrowse_ = new QPushButton("Browse...", this);
    saveBrowse_->setObjectName("shadingSaveBrowse");
    saveRow->addWidget(save_);
    saveRow->addWidget(saveBrowse_);
    form->addRow("Save to:", saveRow);
    overwrite_ = new QCheckBox("Replace a file already there", this);
    overwrite_->setObjectName("shadingOverwrite");
    form->addRow(QString(), overwrite_);
    command_ = terrainCommandField(this, "shadingCommand");
    form->addRow("Command:", command_);
    layout->addLayout(form);

    auto* buttons = new QHBoxLayout();
    buttons->addStretch();
    preview_ = new QPushButton("Preview", this);
    preview_->setObjectName("shadingPreview");
    run_ = new QPushButton("Shade", this);
    run_->setObjectName("shadingRun");
    run_->setDefault(true);
    buttons->addWidget(preview_);
    buttons->addWidget(run_);
    layout->addLayout(buttons);
    reply_ = terrainReplyField(this, "shadingReply");
    layout->addWidget(reply_);
    runner_ = std::make_unique<TerrainRun>(context_, *reply_);

    connect(source_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    connect(style_, &QComboBox::currentIndexChanged, this, [this] {
        // The style's own ramp, as the verb would take it.
        const QString ramp = defaultRamp(style_->currentData().toString());
        if (!ramp.isEmpty()) {
            ramp_->setCurrentText(ramp);
        }
        refresh();
    });
    for (QDoubleSpinBox* spin : {azimuth_, altitude_, zFactor_}) {
        connect(spin, &QDoubleSpinBox::valueChanged, this, [this] { refresh(); });
    }
    connect(variant_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    connect(ramp_, &QComboBox::currentTextChanged, this, [this] { refresh(); });
    for (QLineEdit* field : {rangeMin_, rangeMax_, name_, save_}) {
        connect(field, &QLineEdit::textChanged, this, [this] { refresh(); });
    }
    connect(overwrite_, &QCheckBox::toggled, this, [this] { refresh(); });
    connect(saveBrowse_, &QPushButton::clicked, this, [this] { browse(); });
    connect(preview_, &QPushButton::clicked, this, [this] { run(true); });
    connect(run_, &QPushButton::clicked, this, [this] { run(false); });
    reload();
    refresh();
}

TerrainShadingDialog::~TerrainShadingDialog() = default;

void TerrainShadingDialog::reload()
{
    fillSources(*source_,
                terrainSources(context_, TerrainSourceKinds::Surfaces | TerrainSourceKinds::Rasters));
}

ShadingForm TerrainShadingDialog::form() const
{
    ShadingForm form;
    form.source = source_->currentData().toString();
    form.style = style_->currentData().toString();
    form.azimuth = azimuth_->value();
    form.altitude = altitude_->value();
    form.zFactor = zFactor_->value();
    form.variant = variant_->currentData().toString();
    form.ramp = ramp_->currentText();
    form.rangeMin = rangeMin_->text();
    form.rangeMax = rangeMax_->text();
    form.name = name_->text();
    form.save = save_->text();
    form.overwrite = overwrite_->isChecked();
    return form;
}

Result<QString> TerrainShadingDialog::command() const
{
    return shadingLine(form());
}

void TerrainShadingDialog::refresh()
{
    const QString style = style_->currentData().toString();
    const bool lights = lit(style);
    for (QWidget* field : std::initializer_list<QWidget*>{altitude_, zFactor_, variant_}) {
        field->setEnabled(lights);
    }
    azimuth_->setEnabled(lights && variant_->currentData().toString() != "multidirectional");
    const bool coloured = !defaultRamp(style).isEmpty();
    for (QWidget* field : std::initializer_list<QWidget*>{ramp_, rangeMin_, rangeMax_}) {
        field->setEnabled(coloured);
    }
    overwrite_->setEnabled(!save_->text().trimmed().isEmpty());
    const auto line = command();
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(line ? QString() : QString::fromStdString(line.error().message));
    preview_->setEnabled(line.ok());
    run_->setEnabled(line.ok());
}

void TerrainShadingDialog::browse()
{
    // A headless run has nobody to answer a file dialog: the field is filled
    // by name instead (--fill shadingSave=...).
    if (context_.headless && context_.headless()) {
        reply_->setPlainText("A headless session opens no file dialog: fill shadingSave with the "
                             "path instead.");
        return;
    }
    const QString chosen = QFileDialog::getSaveFileName(this, "Save the Shading", "shading.tif",
                                                        "GeoTIFF (*.tif *.tiff)");
    if (chosen.isEmpty()) {
        return;
    }
    save_->setText(QDir::fromNativeSeparators(chosen));
    // The save dialog asked before it gave back a file that is there.
    overwrite_->setChecked(QFileInfo::exists(chosen));
}

void TerrainShadingDialog::run(bool preview)
{
    const auto line = command();
    if (!line) {
        reply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    runner_->run(preview ? *line + " PREVIEW" : *line);
}

void TerrainShadingDialog::showEvent(QShowEvent* event)
{
    reload();
    refresh();
    QDialog::showEvent(event);
}

} // namespace katana::qt
