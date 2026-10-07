// File > Export > Export Drawing's dialog (gis_export_dialog.hpp).

#include "gis_export_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFileDialog>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QPushButton>
#include <QRegularExpression>

#include <utility>

#include "customisation/scope_filter_widget.hpp"
#include "geo/replies.hpp"
#include "katana/core/text.hpp"
#include "katana/interop/export.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

// " key=value" for each blank-separated KEY=VALUE of `text`.
Result<QString> optionWords(const QString& key, const QString& text, const QString& field)
{
    QString words;
    for (const QString& option : text.split(QRegularExpression("\\s+"), Qt::SkipEmptyParts)) {
        if (option.indexOf('=') <= 0) {
            return invalid(field + ": an option is KEY=VALUE, not '" + option + "'");
        }
        if (option.contains('"')) {
            return invalid(field + " holds a double quote, which a command line cannot carry");
        }
        words += " " + key + "=" + option;
    }
    return words;
}

} // namespace

bool exportIsNative(const QString& file)
{
    const QString lower = file.trimmed().toLower();
    return lower.endsWith(".dxf") || lower.endsWith(".12da") || lower.endsWith(".12daz");
}

Result<QString> vectorExportLine(const VectorExportForm& form)
{
    const QString file = form.file.trimmed();
    if (file.isEmpty()) {
        return invalid("choose the file to write first");
    }
    if (file.contains('"')) {
        return invalid("the file's path holds a double quote, which a command line cannot carry");
    }
    const auto scope = gisScope(form.scope);
    if (!scope) {
        return scope.error();
    }
    QString line = "EXPORT \"" + file + "\" " + *scope;
    if (exportIsNative(file)) {
        return line; // a .dxf and a .12da take the scope alone
    }
    if (const QString name = form.layerName.trimmed(); !name.isEmpty()) {
        if (form.split) {
            return invalid("one file layer per drawing layer is named after it: clear the layer "
                           "name");
        }
        auto word = gisWord(name, "The layer name");
        if (!word) {
            return word.error();
        }
        line += " layername=" + *word;
    }
    if (form.split) {
        line += " split=layer";
    }
    if (form.append) {
        line += " append";
    }
    if (const QString crs = form.crs.trimmed(); !crs.isEmpty() && crs != "project") {
        auto word = gisWord(crs, "The coordinate system");
        if (!word) {
            return word.error();
        }
        line += " crs=" + *word;
    }
    auto creation = optionWords("co", form.creationOptions, "Creation options");
    if (!creation) {
        return creation.error();
    }
    auto layer = optionWords("lco", form.layerOptions, "Layer options");
    if (!layer) {
        return layer.error();
    }
    line += *creation + *layer;
    if (form.textAsPoints) {
        line += " text=points";
    }
    const auto curve = gisNumberOption("curve", form.curve, "Arcs as chords within");
    if (!curve) {
        return curve.error();
    }
    if (!curve->isEmpty() &&
        !(katana::core::parseFiniteDouble(form.curve.trimmed().toStdString()).value_or(0.0) > 0.0)) {
        return invalid("Arcs as chords within must be a distance above 0");
    }
    line += *curve;
    if (!form.properties) {
        line += " properties=no";
    }
    return line;
}

VectorExportDialog::VectorExportDialog(GisDialogContext context, QWidget* parent)
    : GisToolDialog("vectorExport", "Export Drawing", std::move(context), parent)
{
    file_ = addField("vectorExportFile", "File:", "the file to write, e.g. site.gpkg",
                     "Its extension picks the format: .gpkg, .geojson, .shp, .kml, .dxf, .12da ...");
    browse_ = new QPushButton("Browse...", this);
    browse_->setObjectName("vectorExportBrowse");
    browse_->setAutoDefault(false);
    fields().addRow(QString(), browse_);
    connect(browse_, &QPushButton::clicked, this, [this] {
        QStringList filters;
        for (const katana::interop::FormatChoice& format : katana::interop::vectorExportFormats()) {
            filters << (QString::fromStdString(format.description) + " (*." +
                        QString::fromStdString(format.extension) + ")");
        }
        filters << "12d Archive (*.12da)" << "12d Archive, zipped (*.12daz)";
        const QString chosen =
            QFileDialog::getSaveFileName(this, "Export", file_->text(), filters.join(";;"));
        if (!chosen.isEmpty()) {
            setFile(chosen);
        }
    });

    layerName_ = addField("vectorExportLayerName", "Layer name in the file:", "katana",
                          "layername=: the layer's name inside the file, for formats that hold "
                          "several (GeoPackage, KML). Katana's own layers go out as each "
                          "feature's 'layer' attribute.");
    split_ = new QCheckBox("One file layer per drawing layer", this);
    split_->setObjectName("vectorExportSplit");
    split_->setToolTip("split=layer: each drawing layer becomes a layer of the file, named after it");
    fields().addRow(QString(), split_);
    append_ = new QCheckBox("Add to the file rather than replace it", this);
    append_->setObjectName("vectorExportAppend");
    append_->setToolTip("append: a GeoPackage keeps its layers and gains this one; a layer name "
                        "it has already is refused");
    fields().addRow(QString(), append_);

    crs_ = new QComboBox(this);
    crs_->setObjectName("vectorExportCrs");
    crs_->setEditable(true);
    crs_->addItems({"project", "native", "EPSG:4326"});
    crs_->setToolTip("project: the project's coordinate system (a GeoJSON in longitude and "
                     "latitude, as RFC 7946 says); native: the project's where the format allows "
                     "it; a code: the coordinates moved into it (crs=)");
    fields().addRow("Coordinates:", crs_);

    creationOptions_ = addField("vectorExportCreationOptions", "Creation options:",
                                "none, e.g. VERSION=1.4",
                                "co=: the driver's creation options, KEY=VALUE separated by "
                                "blanks, each checked against its list (FORMATS OPTIONS <driver>)");
    layerOptions_ = addField("vectorExportLayerOptions", "Layer options:",
                             "none, e.g. SPATIAL_INDEX=NO",
                             "lco=: the driver's layer creation options, checked likewise");

    text_ = new QComboBox(this);
    text_->setObjectName("vectorExportText");
    text_->addItem("Leave text out (counted)", QString("skip"));
    text_->addItem("Text as points with its text, height and angle", QString("points"));
    fields().addRow("Text:", text_);

    // Arcs and circles have no exact form in these formats. The tolerance is
    // the sagitta - the furthest a chord may stray from the true curve - and
    // is the user's to choose, because it is a loss of accuracy.
    curve_ = addField("vectorExportCurve", "Arcs as chords within:", "0.001",
                      "curve=: the largest distance a chord may stray from its arc, in model "
                      "units");
    properties_ = new QCheckBox("Write entity properties as attributes", this);
    properties_->setObjectName("vectorExportProperties");
    properties_->setChecked(true);
    fields().addRow(QString(), properties_);

    // A file handed to someone is usually the whole drawing.
    if (ScopeFilterWidget* scope = scopeControls()) {
        scope->setChoice(ScopeChoice::Drawing);
    }

    connect(file_, &QLineEdit::textChanged, this, [this] { updateEnabled(); });
    for (QCheckBox* box : {split_, append_, properties_}) {
        connect(box, &QCheckBox::toggled, this, [this] { refreshCommand(); });
    }
    connect(split_, &QCheckBox::toggled, this, [this] { updateEnabled(); });
    connect(crs_, &QComboBox::currentTextChanged, this, [this] { refreshCommand(); });
    connect(text_, &QComboBox::currentIndexChanged, this, [this] { refreshCommand(); });
    updateEnabled();
}

void VectorExportDialog::setFile(const QString& file)
{
    file_->setText(file);
}

void VectorExportDialog::updateEnabled()
{
    const bool gdal = !exportIsNative(file_->text());
    for (QWidget* field : std::initializer_list<QWidget*>{
             layerName_, split_, append_, crs_, creationOptions_, layerOptions_, text_, curve_,
             properties_}) {
        field->setEnabled(gdal);
    }
    layerName_->setEnabled(gdal && !split_->isChecked());
    refreshCommand();
}

VectorExportForm VectorExportDialog::form() const
{
    VectorExportForm form;
    form.file = file_ != nullptr ? file_->text() : QString();
    form.scope = scopeWords();
    if (layerName_ == nullptr) {
        return form; // the frame asks before the fields are made
    }
    form.layerName = layerName_->isEnabled() ? layerName_->text() : QString();
    form.split = split_->isChecked();
    form.append = append_->isChecked();
    form.crs = crs_->currentText();
    form.creationOptions = creationOptions_->text();
    form.layerOptions = layerOptions_->text();
    form.textAsPoints = text_->currentData().toString() == "points";
    form.curve = curve_->text();
    form.properties = properties_->isChecked();
    return form;
}

Result<QString> VectorExportDialog::command() const
{
    return vectorExportLine(form());
}

QString VectorExportDialog::summary(const QString& reply) const
{
    for (const auto& record : katana::app::geo::parseRecords(reply.toStdString())) {
        if (record.kind == "export" && record.get("preview") == std::optional<std::string>("yes")) {
            return QString("Preview: %1 entities would be written; nothing was.")
                .arg(QString::fromStdString(record.get("entities").value_or("?")));
        }
        if (record.kind == "export" && record.get("ran") == std::optional<std::string>("no")) {
            return "The scope took nothing: nothing was written.";
        }
        if (record.kind == "exported") {
            const std::string count = record.get("features").value_or(
                record.get("entities").value_or("?"));
            return QString("Wrote %1 to %2.")
                .arg(QString::fromStdString(count) + " features",
                     QString::fromStdString(record.get("file").value_or("the file")));
        }
    }
    return GisToolDialog::summary(reply);
}

} // namespace katana::qt
