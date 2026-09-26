#include "gis_import_dialogs.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QGroupBox>
#include <QLineEdit>
#include <QListWidget>
#include <QPointer>
#include <QPushButton>
#include <QRegularExpression>
#include <QSpinBox>
#include <QVBoxLayout>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <utility>

#include "format.hpp"
#include "geo/replies.hpp"
#include "geo/vector_support.hpp"
#include "gis_dialog_support.hpp"
#include "import_placement.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/import_placement.hpp"
#include "katana/core/text.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace interop = katana::interop;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

// The file as the first word of its IMPORT line: always quoted, so a path
// holding a blank - or a word IMPORT reads as an option - is one word.
Result<QString> pathWord(const std::filesystem::path& path)
{
    const QString text = QString::fromStdU16String(path.generic_u16string());
    if (text.contains('"')) {
        return invalid("the file's path holds a double quote, which a command line cannot carry");
    }
    return "\"" + text + "\"";
}

// The read-only line a dialog shows, and its Import button enabled only when
// there is one.
QLineEdit* commandField(const QString& name, QWidget* parent)
{
    auto* field = new QLineEdit(parent);
    field->setObjectName(name);
    field->setReadOnly(true);
    field->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    field->setToolTip("The line Import runs - type it on the command line, or give it to "
                      "katana_cli or katana_mcp, and it does the same");
    return field;
}

void showCommand(QLineEdit* field, QPushButton* button, const Result<QString>& line)
{
    field->setText(line ? *line : QString());
    field->setPlaceholderText(
        line ? QString() : "nothing to import yet: " + QString::fromStdString(line.error().message));
    if (button != nullptr) {
        button->setEnabled(line.ok());
    }
}

// OK named Import, whose button the command enables, and Cancel.
QDialogButtonBox* importButtons(QDialog* dialog, QPushButton*& import)
{
    QDialogButtonBox* buttons = okCancel(dialog, "Import");
    import = buttons->button(QDialogButtonBox::Ok);
    return buttons;
}

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

// ---- the words -------------------------------------------------------------------------------

Result<QString> importOptionWord(const QString& key, const QString& value, const QString& field)
{
    if (value.contains('"') || value.contains('\n')) {
        return invalid(field + " holds a double quote or a line break, which a command line "
                               "cannot carry");
    }
    const bool blank = value.isEmpty() ||
                       std::any_of(value.begin(), value.end(), [](QChar c) { return c.isSpace(); });
    return " " + key + "=" + (blank ? "\"" + value + "\"" : value);
}

// ---- vector import --------------------------------------------------------------------------

VectorImportDialog::VectorImportDialog(const interop::SourceDescription& source,
                                       const katana::geometry::Box2& drawing,
                                       GisDialogContext context, QWidget* parent)
    : QDialog(parent), path_(source.path), context_(std::move(context))
{
    setObjectName("vectorImportDialog");
    setWindowTitle("Import Vector Data - " +
                   QString::fromStdWString(source.path.filename().wstring()));
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(summaryLabel(source, this));

    auto* middle = new QHBoxLayout();
    auto* form = new QFormLayout();

    // Every layer ticked is no layers= word: the whole file, as a
    // GeoPackage's layers are usually meant to arrive together.
    layers_ = new QListWidget(this);
    layers_->setObjectName("vectorImportLayers");
    layers_->setToolTip("The file's layers to read (layers=); every one ticked reads them all");
    for (const auto& layer : source.vectorLayers) {
        auto* item = new QListWidgetItem(QString("%1  (%2 %3)")
                                             .arg(QString::fromStdString(layer.name))
                                             .arg(grouped(layer.featureCount))
                                             .arg(QString::fromStdString(layer.geometryType)),
                                         layers_);
        item->setData(Qt::UserRole, QString::fromStdString(layer.name));
        item->setFlags(item->flags() | Qt::ItemIsUserCheckable);
        item->setCheckState(Qt::Checked);
    }
    layers_->setMaximumHeight(110);
    form->addRow("Layers:", layers_);

    where_ = new QLineEdit(this);
    where_->setObjectName("vectorImportWhere");
    where_->setPlaceholderText("every feature, e.g. kind = 'lot'");
    where_->setToolTip("An OGR SQL WHERE clause, applied by the file's driver (where=)");
    form->addRow("Only where:", where_);

    sql_ = new QLineEdit(this);
    sql_->setObjectName("vectorImportSql");
    sql_->setPlaceholderText("none, e.g. SELECT * FROM parcels WHERE area_m2 > 7000");
    sql_->setToolTip("A SELECT run on the file; its rows are imported in place of its layers "
                     "(sql=)");
    form->addRow("SQL:", sql_);
    dialect_ = new QComboBox(this);
    dialect_->setObjectName("vectorImportDialect");
    dialect_->addItem("The driver's own", QString());
    dialect_->addItem("OGR SQL", QString("ogrsql"));
    dialect_->addItem("SQLite", QString("sqlite"));
    form->addRow("SQL dialect:", dialect_);

    fields_ = new QLineEdit(this);
    fields_->setObjectName("vectorImportFields");
    fields_->setPlaceholderText("every attribute");
    fields_->setToolTip("Only these attributes become properties, a,b (fields=)");
    form->addRow("Attributes:", fields_);
    attributes_ = new QCheckBox("Keep feature attributes as entity properties", this);
    attributes_->setObjectName("vectorImportAttributes");
    attributes_->setChecked(interop::VectorImportOptions{}.attributesAsProperties);
    form->addRow(QString(), attributes_);

    target_ = new QLineEdit(this);
    target_->setObjectName("vectorImportTarget");
    target_->setPlaceholderText("one Katana layer per source layer");
    target_->setToolTip("Put every imported entity on this layer (target=). Left empty, each "
                        "source layer - or each DXF layer - becomes a Katana layer of its own.");
    form->addRow("Katana layer:", target_);

    max_ = new QSpinBox(this);
    max_->setObjectName("vectorImportMax");
    max_->setRange(0, std::numeric_limits<int>::max());
    max_->setSpecialValueText("every feature");
    max_->setToolTip("At most this many features (max=)");
    form->addRow("At most:", max_);

    openOptions_ = new QLineEdit(this);
    openOptions_->setObjectName("vectorImportOpenOptions");
    openOptions_->setPlaceholderText("none, e.g. FLATTEN_NESTED_ATTRIBUTES=YES");
    openOptions_->setToolTip("The driver's open options, KEY=VALUE separated by blanks (oo=); "
                             "each is checked against the driver's own list. FORMATS OPTIONS "
                             "<driver> lists them.");
    form->addRow("Open options:", openOptions_);

    crs_ = new QComboBox(this);
    crs_->setObjectName("vectorImportCrs");
    crs_->addItem("As the file has them", QString());
    crs_->addItem("Into the project's coordinate system", QString("project"));
    crs_->addItem("Set the project's from the file, if it has none", QString("adopt"));
    crs_->setToolTip("crs=project reprojects the features; crs=adopt sets the project's "
                     "coordinate system in the same undo step");
    form->addRow("Coordinates:", crs_);
    assumeCrs_ = new QLineEdit(this);
    assumeCrs_->setObjectName("vectorImportAssumeCrs");
    assumeCrs_->setPlaceholderText("the file's own, e.g. EPSG:28356");
    assumeCrs_->setToolTip("The coordinate system of a file that declares none (srs=)");
    form->addRow("File's CRS if none:", assumeCrs_);
    middle->addLayout(form, 3);

    // What of the drawing bounds the read: the shared "Apply to" and "Only
    // those that match" controls (CLAUDE.md section 1.1).
    auto* scopeBox = new QGroupBox("Only what lies within", this);
    auto* scopeLayout = new QVBoxLayout(scopeBox);
    useScope_ = new QCheckBox("Limit the import to this part of the drawing", scopeBox);
    useScope_->setObjectName("vectorImportUseScope");
    scope_ = new ScopeFilterWidget("vectorImport", scopeBox);
    scope_->views = context_.views;
    scope_->onChanged = [this] { refresh(); };
    if (context_.document != nullptr) {
        scope_->reload(context_.document->model());
    } else {
        scope_->reloadViews();
    }
    scope_->setEnabled(false);
    clip_ = new QCheckBox("Cut features at its edge (clip)", scopeBox);
    clip_->setObjectName("vectorImportClip");
    clip_->setEnabled(false);
    scopeLayout->addWidget(useScope_);
    scopeLayout->addWidget(scope_, 1);
    scopeLayout->addWidget(clip_);
    middle->addWidget(scopeBox, 2);
    layout->addLayout(middle);

    placement_ = new ImportPlacementBox(drawing, QString(), this);
    layout->addWidget(placement_);

    command_ = commandField("vectorImportCommand", this);
    auto* commandForm = new QFormLayout();
    commandForm->addRow("Command:", command_);
    layout->addLayout(commandForm);
    auto* previewRow = new QHBoxLayout();
    matchCount_ = new QLabel(this);
    matchCount_->setObjectName("vectorImportMatchCount");
    matchCount_->setWordWrap(true);
    preview_ = new QPushButton("Preview", this);
    preview_->setObjectName("vectorImportPreview");
    preview_->setAutoDefault(false);
    preview_->setToolTip("Run the line with PREVIEW: how many features the filters take, and "
                         "nothing imported");
    previewRow->addWidget(matchCount_, 1);
    previewRow->addWidget(preview_);
    layout->addLayout(previewRow);
    layout->addWidget(importButtons(this, import_));

    connect(layers_, &QListWidget::itemChanged, this, [this] { refresh(); });
    for (QLineEdit* edit : {where_, sql_, fields_, target_, openOptions_, assumeCrs_}) {
        connect(edit, &QLineEdit::textChanged, this, [this] { refresh(); });
    }
    for (QComboBox* combo : {dialect_, crs_}) {
        connect(combo, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    }
    connect(max_, &QSpinBox::valueChanged, this, [this] { refresh(); });
    for (QCheckBox* box : {attributes_, clip_}) {
        connect(box, &QCheckBox::toggled, this, [this] { refresh(); });
    }
    connect(useScope_, &QCheckBox::toggled, this, [this](bool on) {
        scope_->setEnabled(on);
        clip_->setEnabled(on);
        refresh();
    });
    // The placement group says its choice by its buttons and spin boxes.
    for (QAbstractButton* button : placement_->findChildren<QAbstractButton*>()) {
        connect(button, &QAbstractButton::toggled, this, [this] { refresh(); });
    }
    for (QDoubleSpinBox* spin : placement_->findChildren<QDoubleSpinBox*>()) {
        connect(spin, &QDoubleSpinBox::valueChanged, this, [this] { refresh(); });
    }
    connect(preview_, &QPushButton::clicked, this, [this] { preview(); });
    refresh();
}

Result<QString> VectorImportDialog::command() const
{
    auto path = pathWord(path_);
    if (!path) {
        return path.error();
    }
    QString line = "IMPORT " + *path;
    const katana::cad::ImportPlacement placement = placement_->placement();
    if (const std::string word = katana::cad::placementWord(placement); !word.empty()) {
        line += " " + QString::fromStdString(word);
    }

    QStringList chosen;
    for (int i = 0; i < layers_->count(); ++i) {
        const QListWidgetItem* item = layers_->item(i);
        if (item->checkState() == Qt::Checked) {
            chosen << item->data(Qt::UserRole).toString();
        }
    }
    const QString sql = sql_->text().trimmed();
    if (chosen.size() != layers_->count()) {
        if (chosen.isEmpty()) {
            return invalid("tick at least one layer");
        }
        if (!sql.isEmpty()) {
            return invalid("SQL reads the rows its statement gives, not layers: tick every "
                           "layer, or clear the SQL");
        }
        for (const QString& name : chosen) {
            if (name.contains(',')) {
                return invalid("the layer '" + name + "' has a comma in its name, which "
                               "layers= cannot carry");
            }
        }
        auto word = importOptionWord("layers", chosen.join(','), "A layer's name");
        if (!word) {
            return word.error();
        }
        line += *word;
    }
    if (const QString where = where_->text().trimmed(); !where.isEmpty()) {
        auto word = importOptionWord("where", where, "Only where");
        if (!word) {
            return word.error();
        }
        line += *word;
    }
    if (!sql.isEmpty()) {
        const QString dialect = dialect_->currentData().toString();
        // One word of a line: SQLite's "identifiers" become [identifiers],
        // which it reads alike (vector::sqlForLine, GIS SQL's rule).
        auto text = katana::app::geo::vector::sqlForLine(sql.toStdString(), dialect == "sqlite");
        if (!text) {
            return invalid("the SQL: " + QString::fromStdString(text.error().message));
        }
        auto word = importOptionWord("sql", QString::fromStdString(*text), "The SQL");
        if (!word) {
            return word.error();
        }
        line += *word;
        if (!dialect.isEmpty()) {
            line += " dialect=" + dialect;
        }
    }
    if (const QString fields = fields_->text().trimmed(); !fields.isEmpty()) {
        QStringList names;
        for (const QString& name : fields.split(',', Qt::SkipEmptyParts)) {
            names << name.trimmed();
        }
        auto word = importOptionWord("fields", names.join(','), "Attributes");
        if (!word) {
            return word.error();
        }
        line += *word;
    }
    if (!attributes_->isChecked()) {
        line += " attributes=no";
    }
    if (const QString target = target_->text().trimmed(); !target.isEmpty()) {
        auto word = importOptionWord("target", target, "The Katana layer");
        if (!word) {
            return word.error();
        }
        line += *word;
    }
    if (max_->value() > 0) {
        line += " max=" + QString::number(max_->value());
    }
    for (const QString& option :
         openOptions_->text().split(QRegularExpression("\\s+"), Qt::SkipEmptyParts)) {
        if (option.indexOf('=') <= 0) {
            return invalid("an open option is KEY=VALUE, not '" + option + "'");
        }
        auto word = importOptionWord("oo", option, "An open option");
        if (!word) {
            return word.error();
        }
        line += *word;
    }
    if (const QString crs = crs_->currentData().toString(); !crs.isEmpty()) {
        line += " crs=" + crs;
    }
    if (const QString assumed = assumeCrs_->text().trimmed(); !assumed.isEmpty()) {
        auto word = importOptionWord("srs", assumed, "The file's CRS");
        if (!word) {
            return word.error();
        }
        line += *word;
    }
    if (useScope_->isChecked()) {
        if (placement.mode != katana::cad::ImportPlacementMode::Keep) {
            return invalid("a scope is in the drawing's coordinates: keep the data where it is "
                           "to limit it to one");
        }
        auto words = scope_->verbWords();
        if (!words) {
            return invalid("the scope: " + QString::fromStdString(words.error().message));
        }
        line += " " + words->trimmed();
        if (clip_->isChecked()) {
            line += " clip";
        }
    }
    return line;
}

void VectorImportDialog::refresh()
{
    showCommand(command_, import_, command());
    preview_->setEnabled(import_->isEnabled());
}

void VectorImportDialog::preview()
{
    const auto line = command();
    if (!line) {
        matchCount_->setText(QString::fromStdString(line.error().message));
        return;
    }
    if (!context_.run) {
        matchCount_->setText("Nothing here can run a command.");
        return;
    }
    const VerbOutcome outcome = context_.run(*line + " PREVIEW");
    // Interactively the line became a job: its reply comes when it ends.
    if (context_.await &&
        context_.await(outcome, [guard = QPointer<VectorImportDialog>(this)](const VerbOutcome& done) {
            if (guard != nullptr) {
                guard->showPreview(done);
            }
        })) {
        matchCount_->setText("Reading with the filters...");
        return;
    }
    showPreview(outcome);
}

void VectorImportDialog::showPreview(const VerbOutcome& outcome)
{
    if (!outcome.ok) {
        matchCount_->setText(outcome.error.section('\n', 0, 0));
        return;
    }
    for (const auto& record : katana::app::geo::parseRecords(outcome.reply.toStdString())) {
        if (record.kind != "import") {
            continue;
        }
        if (record.get("ran") == std::optional<std::string>("no")) {
            matchCount_->setText("The scope takes nothing of the drawing: nothing would be read.");
            return;
        }
        const auto text = [&record](const char* key) {
            return QString::fromStdString(record.get(key).value_or(std::string("?")));
        };
        matchCount_->setText(QString("%1 of %2 features match; %3 entities would be imported.")
                                 .arg(text("features"), text("of"), text("entities")));
        return;
    }
    matchCount_->setText(outcome.reply.trimmed().section('\n', 0, 0));
}

QString VectorImportDialog::matchText() const
{
    return matchCount_->text();
}

// ---- raster import --------------------------------------------------------------------------

RasterImportDialog::RasterImportDialog(const interop::SourceDescription& source, QWidget* parent)
    : QDialog(parent), path_(source.path)
{
    setObjectName("rasterImportDialog");
    setWindowTitle("Import Raster - " + QString::fromStdWString(source.path.filename().wstring()));
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(summaryLabel(source, this));

    auto* form = new QFormLayout();
    name_ = new QLineEdit(QString::fromStdWString(source.path.stem().wstring()), this);
    name_->setObjectName("rasterImportName");
    form->addRow("Name:", name_);

    // The DISPLAY copy's longest side. The file itself is never resampled for
    // analysis - Surface From Raster reads the band through GDAL - so this is
    // a trade of sharpness against memory (4 bytes a pixel), nothing more.
    resolution_ = new QComboBox(this);
    resolution_->setObjectName("rasterImportMaxPixels");
    for (const int pixels : {1024, 2048, 4096, 8192}) {
        resolution_->addItem(QString("%1 px  (%2 MB at most)")
                                 .arg(pixels)
                                 .arg(static_cast<qulonglong>(pixels) * pixels * 4 / (1024 * 1024)),
                             pixels);
    }
    resolution_->setCurrentIndex(
        resolution_->findData(interop::RasterImportOptions{}.maxPixels));
    form->addRow("Display resolution:", resolution_);

    band_ = new QSpinBox(this);
    band_->setObjectName("rasterImportBand");
    band_->setRange(0, std::max(1, source.raster ? source.raster->bandCount : 1));
    band_->setSpecialValueText("As the file's colours");
    band_->setToolTip("One band alone, as grey (band=): a multispectral image's near infrared, "
                      "or one grid of a stack");
    form->addRow("Band:", band_);

    subdataset_ = new QComboBox(this);
    subdataset_->setObjectName("rasterImportSubdataset");
    subdataset_->addItem("The file itself", QString());
    for (std::size_t i = 0; i < source.subdatasets.size(); ++i) {
        subdataset_->addItem(QString("%1  %2")
                                 .arg(i + 1)
                                 .arg(QString::fromStdString(source.subdatasets[i].description)),
                             QString::number(i + 1));
    }
    subdataset_->setEnabled(!source.subdatasets.empty());
    form->addRow("Dataset inside:", subdataset_);

    crs_ = new QComboBox(this);
    crs_->setObjectName("rasterImportCrs");
    crs_->addItem("Keep the project's coordinate system", QString());
    crs_->addItem("Set the project's from the raster, if it has none", QString("adopt"));
    form->addRow("Coordinates:", crs_);
    layout->addLayout(form);

    command_ = commandField("rasterImportCommand", this);
    auto* commandForm = new QFormLayout();
    commandForm->addRow("Command:", command_);
    layout->addLayout(commandForm);
    layout->addWidget(importButtons(this, import_));

    connect(name_, &QLineEdit::textChanged, this, [this] { refresh(); });
    for (QComboBox* combo : {resolution_, subdataset_, crs_}) {
        connect(combo, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    }
    connect(band_, &QSpinBox::valueChanged, this, [this] { refresh(); });
    refresh();
}

Result<QString> RasterImportDialog::command() const
{
    auto path = pathWord(path_);
    if (!path) {
        return path.error();
    }
    QString line = "IMPORT " + *path;
    const QString name = name_->text().trimmed();
    if (!name.isEmpty() && name != QString::fromStdWString(path_.stem().wstring())) {
        auto word = importOptionWord("name", name, "The name");
        if (!word) {
            return word.error();
        }
        line += *word;
    }
    if (const int pixels = resolution_->currentData().toInt();
        pixels != interop::RasterImportOptions{}.maxPixels) {
        line += " maxpixels=" + QString::number(pixels);
    }
    if (band_->value() > 0) {
        line += " band=" + QString::number(band_->value());
    }
    if (const QString subdataset = subdataset_->currentData().toString(); !subdataset.isEmpty()) {
        line += " subdataset=" + subdataset;
    }
    if (const QString crs = crs_->currentData().toString(); !crs.isEmpty()) {
        line += " crs=" + crs;
    }
    return line;
}

void RasterImportDialog::refresh()
{
    showCommand(command_, import_, command());
}

// ---- point cloud import ---------------------------------------------------------------------

PointCloudImportDialog::PointCloudImportDialog(const interop::SourceDescription& source,
                                               QWidget* parent)
    : QDialog(parent), path_(source.path),
      pointCount_(source.pointCloud ? source.pointCloud->pointCount : 0)
{
    setObjectName("pointCloudImportDialog");
    setWindowTitle("Import Point Cloud - " +
                   QString::fromStdWString(source.path.filename().wstring()));
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(summaryLabel(source, this));

    auto* form = new QFormLayout();
    name_ = new QLineEdit(QString::fromStdWString(source.path.stem().wstring()), this);
    name_->setObjectName("pointCloudImportName");
    form->addRow("Name:", name_);

    // The budget is what makes a billion-point file open like a small one;
    // the ceiling is QSpinBox's own (an int), far above what fits in memory
    // as PointCloudPoint values (40 bytes each: 2^31 of them is 80 GB).
    budget_ = new QSpinBox(this);
    budget_->setObjectName("pointCloudImportBudget");
    budget_->setRange(1'000, std::numeric_limits<int>::max());
    budget_->setSingleStep(500'000);
    budget_->setGroupSeparatorShown(true);
    budget_->setValue(static_cast<int>(interop::PointCloudImportOptions{}.budget));
    form->addRow("Points to keep:", budget_);

    classification_ = new QComboBox(this);
    classification_->setObjectName("pointCloudImportClasses");
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
    useResolution_->setObjectName("pointCloudImportUseResolution");
    useResolution_->setEnabled(copc);
    resolution_ = new QDoubleSpinBox(this);
    resolution_->setObjectName("pointCloudImportResolution");
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

    command_ = commandField("pointCloudImportCommand", this);
    auto* commandForm = new QFormLayout();
    commandForm->addRow("Command:", command_);
    layout->addLayout(commandForm);
    layout->addWidget(importButtons(this, import_));

    connect(name_, &QLineEdit::textChanged, this, [this] { refresh(); });
    connect(budget_, &QSpinBox::valueChanged, this, [this] {
        refreshEstimate();
        refresh();
    });
    connect(classification_, &QComboBox::currentIndexChanged, this, [this] {
        refreshEstimate();
        refresh();
    });
    connect(useResolution_, &QCheckBox::toggled, this, [this](bool on) {
        resolution_->setEnabled(on);
        refreshEstimate();
        refresh();
    });
    connect(resolution_, &QDoubleSpinBox::valueChanged, this, [this] { refresh(); });
    refreshEstimate();
    refresh();
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

Result<QString> PointCloudImportDialog::command() const
{
    auto path = pathWord(path_);
    if (!path) {
        return path.error();
    }
    QString line = "IMPORT " + *path;
    const QString name = name_->text().trimmed();
    if (!name.isEmpty() && name != QString::fromStdWString(path_.stem().wstring())) {
        auto word = importOptionWord("name", name, "The name");
        if (!word) {
            return word.error();
        }
        line += *word;
    }
    if (const auto budget = static_cast<std::uint64_t>(budget_->value());
        budget != interop::PointCloudImportOptions{}.budget) {
        line += " budget=" + QString::number(budget);
    }
    if (const int value = classification_->currentData().toInt(); value >= 0) {
        line += " class=" + QString::number(value);
    }
    if (useResolution_->isChecked()) {
        line += " resolution=" +
                QString::fromStdString(katana::core::formatExactReal(resolution_->value()));
    }
    return line;
}

void PointCloudImportDialog::refresh()
{
    showCommand(command_, import_, command());
}

} // namespace katana::qt
