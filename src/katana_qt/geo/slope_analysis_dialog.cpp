// Terrain > Analysis > Slope and Aspect (slope_analysis_dialog.hpp).

#include "geo/slope_analysis_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QShowEvent>
#include <QVBoxLayout>

#include <initializer_list>
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

} // namespace

Result<QString> slopeLine(const SlopeForm& form)
{
    if (form.source.trimmed().isEmpty()) {
        return invalid("Source: there is no surface or raster to analyse; build a surface or "
                       "import a DEM first");
    }
    QString line = QString(form.aspect ? "RASTER ASPECT " : "RASTER SLOPE ") + form.source.trimmed();
    if (!form.aspect) {
        line += " unit=" + form.unit;
        const QString classes = QString(form.classes).remove(' ');
        if (!classes.isEmpty()) {
            if (!numberList(classes, 0, false)) {
                return invalid("Classes: the slopes between classes, separated by commas, as "
                               "5,10,25");
            }
            line += " classes=" + classes;
            const QString layer = form.areas.trimmed();
            if (!layer.isEmpty()) {
                const auto word = lineWord(layer);
                if (!word) {
                    return invalid("Areas on: a layer name cannot hold a double quote");
                }
                line += " areas=" + *word;
            }
            const QString least = form.minArea.trimmed();
            if (!least.isEmpty()) {
                if (!numberList(least, 1, false) || !(least.toDouble() > 0.0)) {
                    return invalid("Least area: a positive area, as 25");
                }
                line += " min_area=" + least;
            }
        }
    }
    if (!form.name.trimmed().isEmpty()) {
        const auto name = lineWord(form.name.trimmed());
        if (!name) {
            return invalid("Name: a name cannot hold a double quote");
        }
        line += " NAME " + *name;
    }
    if (form.clip) {
        if (!form.scopeError.isEmpty()) {
            return invalid("Keep inside: " + form.scopeError);
        }
        line += " " + form.scope.trimmed();
    }
    return line;
}

SlopeAnalysisDialog::SlopeAnalysisDialog(TerrainDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("slopeAnalysisDialog");
    setWindowTitle("Slope and Aspect");
    auto* layout = new QVBoxLayout(this);
    auto* form = new QFormLayout();

    source_ = new QComboBox(this);
    source_->setObjectName("slopeSource");
    form->addRow("Source:", source_);
    kind_ = new QComboBox(this);
    kind_->setObjectName("slopeKind");
    kind_->addItem("Slope", false);
    kind_->addItem("Aspect", true);
    form->addRow("Analysis:", kind_);
    unit_ = new QComboBox(this);
    unit_->setObjectName("slopeUnit");
    unit_->addItem("Percent", "percent");
    unit_->addItem("Degrees", "degree");
    form->addRow("Unit:", unit_);
    classes_ = new QLineEdit(this);
    classes_->setObjectName("slopeClasses");
    classes_->setPlaceholderText("5,10,25 - blank: the slope alone");
    classes_->setToolTip("The slopes between classes: 5,10,25 makes 0-5, 5-10, 10-25 and 25+.");
    form->addRow("Classes:", classes_);
    areas_ = new QLineEdit("terrain/slope", this);
    areas_->setObjectName("slopeAreasLayer");
    areas_->setToolTip("Each class's areas go to <layer>/<class>.");
    form->addRow("Areas on:", areas_);
    minArea_ = new QLineEdit(this);
    minArea_->setObjectName("slopeMinArea");
    minArea_->setPlaceholderText("blank: keep every region");
    minArea_->setToolTip("Regions smaller than this are merged into their largest neighbour.");
    form->addRow("Least area:", minArea_);
    name_ = new QLineEdit(this);
    name_->setObjectName("slopeName");
    name_->setPlaceholderText("the source's");
    form->addRow("Name:", name_);
    layout->addLayout(form);

    clip_ = new QCheckBox("Keep inside closed shapes of the drawing", this);
    clip_->setObjectName("slopeClip");
    layout->addWidget(clip_);
    scope_ = new ScopeFilterWidget("slope", this);
    scope_->views = context_.views;
    scope_->setChoice(ScopeChoice::Selection);
    layout->addWidget(scope_);

    auto* commandRow = new QFormLayout();
    command_ = terrainCommandField(this, "slopeCommand");
    commandRow->addRow("Command:", command_);
    layout->addLayout(commandRow);

    auto* buttons = new QHBoxLayout();
    buttons->addStretch();
    preview_ = new QPushButton("Preview", this);
    preview_->setObjectName("slopePreview");
    run_ = new QPushButton("Analyse", this);
    run_->setObjectName("slopeRun");
    run_->setDefault(true);
    buttons->addWidget(preview_);
    buttons->addWidget(run_);
    layout->addLayout(buttons);
    reply_ = terrainReplyField(this, "slopeReply");
    layout->addWidget(reply_);
    runner_ = std::make_unique<TerrainRun>(context_, *reply_);

    for (QComboBox* combo : {source_, kind_, unit_}) {
        connect(combo, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    }
    for (QLineEdit* field : {classes_, areas_, minArea_, name_}) {
        connect(field, &QLineEdit::textChanged, this, [this] { refresh(); });
    }
    connect(clip_, &QCheckBox::toggled, this, [this] { refresh(); });
    scope_->onChanged = [this] { refresh(); };
    connect(preview_, &QPushButton::clicked, this, [this] { run(true); });
    connect(run_, &QPushButton::clicked, this, [this] { run(false); });
    reload();
    refresh();
}

SlopeAnalysisDialog::~SlopeAnalysisDialog() = default;

void SlopeAnalysisDialog::reload()
{
    fillSources(*source_,
                terrainSources(context_, TerrainSourceKinds::Surfaces | TerrainSourceKinds::Rasters));
    if (context_.document != nullptr) {
        scope_->reload(context_.document->model());
    } else {
        scope_->reloadViews();
    }
}

SlopeForm SlopeAnalysisDialog::form() const
{
    SlopeForm form;
    form.source = source_->currentData().toString();
    form.aspect = kind_->currentData().toBool();
    form.unit = unit_->currentData().toString();
    form.classes = classes_->text();
    form.areas = areas_->text();
    form.minArea = minArea_->text();
    form.name = name_->text();
    form.clip = clip_->isChecked();
    if (auto words = scope_->verbWords()) {
        form.scope = *words;
    } else {
        form.scopeError = QString::fromStdString(words.error().message);
    }
    return form;
}

Result<QString> SlopeAnalysisDialog::command() const
{
    return slopeLine(form());
}

void SlopeAnalysisDialog::refresh()
{
    const bool slope = !kind_->currentData().toBool();
    unit_->setEnabled(slope);
    classes_->setEnabled(slope);
    const bool classed = slope && !classes_->text().trimmed().isEmpty();
    for (QWidget* field : std::initializer_list<QWidget*>{areas_, minArea_}) {
        field->setEnabled(classed);
    }
    scope_->setEnabled(clip_->isChecked());
    const auto line = command();
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(line ? QString() : QString::fromStdString(line.error().message));
    preview_->setEnabled(line.ok());
    run_->setEnabled(line.ok());
}

void SlopeAnalysisDialog::run(bool preview)
{
    const auto line = command();
    if (!line) {
        reply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    runner_->run(preview ? *line + " PREVIEW" : *line);
}

void SlopeAnalysisDialog::showEvent(QShowEvent* event)
{
    reload();
    refresh();
    QDialog::showEvent(event);
}

} // namespace katana::qt
