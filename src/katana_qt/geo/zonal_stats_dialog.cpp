// Terrain > Analysis > Statistics by Area (zonal_stats_dialog.hpp).

#include "geo/zonal_stats_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QListWidget>
#include <QListWidgetItem>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRegularExpression>
#include <QShowEvent>
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

// What the list offers, in the verb's words, the first five ticked: the
// verb's own default set.
struct StatChoice {
    const char* word;
    const char* label;
    bool ticked;
};
constexpr StatChoice kStatChoices[] = {
    {"mean", "Mean", true},       {"min", "Least", true},          {"max", "Greatest", true},
    {"count", "Cells (count)", true}, {"sum", "Sum", true},        {"median", "Median", false},
    {"stdev", "Standard deviation", false}, {"variance", "Variance", false},
    {"mode", "Commonest value", false},     {"variety", "Distinct values", false},
};

} // namespace

Result<QString> zonalLine(const ZonalForm& form)
{
    if (form.source.trimmed().isEmpty()) {
        return invalid("Source: there is no surface or raster to measure; build a surface or "
                       "import a DEM first");
    }
    if (!form.scopeError.isEmpty()) {
        return invalid("Zones: " + form.scopeError);
    }
    if (form.stats.isEmpty()) {
        return invalid("Statistics: tick at least one");
    }
    static const QRegularExpression key(QStringLiteral("^[A-Za-z0-9_.-]+$"));
    const QString prefix = form.prefix.trimmed();
    if (!key.match(prefix).hasMatch()) {
        return invalid("Property prefix: letters, digits, '_', '.' and '-', as zone");
    }
    QString line = "RASTER ZONAL " + form.source.trimmed();
    if (!form.scope.trimmed().isEmpty()) {
        line += " " + form.scope.trimmed();
    }
    line += " stats=" + form.stats.join(',') + " prefix=" + prefix + " pixels=" + form.pixels;
    const QString csv = form.csv.trimmed();
    if (!csv.isEmpty()) {
        const auto word = lineWord("csv=" + csv);
        if (!word) {
            return invalid("CSV file: a file name cannot hold a double quote");
        }
        line += " " + *word;
        if (form.overwrite) {
            line += " OVERWRITE";
        }
    }
    return line;
}

ZonalStatsDialog::ZonalStatsDialog(TerrainDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("zonalStatsDialog");
    setWindowTitle("Statistics by Area");
    auto* layout = new QVBoxLayout(this);
    auto* form = new QFormLayout();

    source_ = new QComboBox(this);
    source_->setObjectName("zonalSource");
    form->addRow("Source:", source_);
    layout->addLayout(form);

    scope_ = new ScopeFilterWidget("zonal", this);
    scope_->views = context_.views;
    scope_->setChoice(ScopeChoice::Selection);
    scope_->setToolTip("The zones: the closed shapes these take. Open lines and points bound no "
                       "area and are left out.");
    layout->addWidget(scope_);

    auto* options = new QFormLayout();
    stats_ = new QListWidget(this);
    stats_->setObjectName("zonalStats");
    for (const StatChoice& choice : kStatChoices) {
        auto* item = new QListWidgetItem(choice.label, stats_);
        item->setData(Qt::UserRole, QString(choice.word));
        item->setFlags(item->flags() | Qt::ItemIsUserCheckable);
        item->setCheckState(choice.ticked ? Qt::Checked : Qt::Unchecked);
    }
    stats_->setMaximumHeight(120);
    options->addRow("Statistics:", stats_);
    prefix_ = new QLineEdit("zone", this);
    prefix_->setObjectName("zonalPrefix");
    prefix_->setToolTip("Each statistic is written on its zone as <prefix>_<statistic>: "
                        "zone_mean, zone_count ...");
    options->addRow("Property prefix:", prefix_);
    pixels_ = new QComboBox(this);
    pixels_->setObjectName("zonalPixels");
    pixels_->addItem("Each cell by the part inside (fractional)", "fractional");
    pixels_->addItem("Cells whose centre is inside", "centre");
    pixels_->addItem("Every cell the shape touches", "all-touched");
    options->addRow("Cells:", pixels_);
    csv_ = new QLineEdit(this);
    csv_->setObjectName("zonalCsv");
    csv_->setPlaceholderText("blank: no file");
    options->addRow("Also to CSV:", csv_);
    overwrite_ = new QCheckBox("Replace the file if it is there", this);
    overwrite_->setObjectName("zonalOverwrite");
    options->addRow("", overwrite_);
    layout->addLayout(options);

    auto* commandRow = new QFormLayout();
    command_ = terrainCommandField(this, "zonalCommand");
    commandRow->addRow("Command:", command_);
    layout->addLayout(commandRow);

    auto* buttons = new QHBoxLayout();
    buttons->addStretch();
    preview_ = new QPushButton("Preview", this);
    preview_->setObjectName("zonalPreview");
    run_ = new QPushButton("Measure", this);
    run_->setObjectName("zonalRun");
    run_->setDefault(true);
    buttons->addWidget(preview_);
    buttons->addWidget(run_);
    layout->addLayout(buttons);
    reply_ = terrainReplyField(this, "zonalReply");
    layout->addWidget(reply_);
    runner_ = std::make_unique<TerrainRun>(context_, *reply_);

    connect(source_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    connect(pixels_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    connect(stats_, &QListWidget::itemChanged, this, [this] { refresh(); });
    for (QLineEdit* field : {prefix_, csv_}) {
        connect(field, &QLineEdit::textChanged, this, [this] { refresh(); });
    }
    connect(overwrite_, &QCheckBox::toggled, this, [this] { refresh(); });
    scope_->onChanged = [this] { refresh(); };
    connect(preview_, &QPushButton::clicked, this, [this] { run(true); });
    connect(run_, &QPushButton::clicked, this, [this] { run(false); });
    reload();
    refresh();
}

ZonalStatsDialog::~ZonalStatsDialog() = default;

void ZonalStatsDialog::reload()
{
    fillSources(*source_,
                terrainSources(context_, TerrainSourceKinds::Surfaces | TerrainSourceKinds::Rasters));
    if (context_.document != nullptr) {
        scope_->reload(context_.document->model());
    } else {
        scope_->reloadViews();
    }
}

ZonalForm ZonalStatsDialog::form() const
{
    ZonalForm form;
    form.source = source_->currentData().toString();
    for (int i = 0; i < stats_->count(); ++i) {
        const QListWidgetItem* item = stats_->item(i);
        if (item->checkState() == Qt::Checked) {
            form.stats << item->data(Qt::UserRole).toString();
        }
    }
    form.prefix = prefix_->text();
    form.pixels = pixels_->currentData().toString();
    form.csv = csv_->text();
    form.overwrite = overwrite_->isChecked();
    if (auto words = scope_->verbWords()) {
        form.scope = *words;
    } else {
        form.scopeError = QString::fromStdString(words.error().message);
    }
    return form;
}

Result<QString> ZonalStatsDialog::command() const
{
    return zonalLine(form());
}

void ZonalStatsDialog::refresh()
{
    overwrite_->setEnabled(!csv_->text().trimmed().isEmpty());
    const auto line = command();
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(line ? QString() : QString::fromStdString(line.error().message));
    preview_->setEnabled(line.ok());
    run_->setEnabled(line.ok());
}

void ZonalStatsDialog::run(bool preview)
{
    const auto line = command();
    if (!line) {
        reply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    runner_->run(preview ? *line + " PREVIEW" : *line);
}

void ZonalStatsDialog::showEvent(QShowEvent* event)
{
    reload();
    refresh();
    QDialog::showEvent(event);
}

} // namespace katana::qt
