// Terrain > Analysis > Viewshed and Line of Sight (viewshed_dialog.hpp).

#include "geo/viewshed_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPointer>
#include <QPushButton>
#include <QShowEvent>
#include <QTabWidget>
#include <QVBoxLayout>

#include <initializer_list>
#include <utility>

#include "geo/drape_dialog.hpp"
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

// A point as the line takes it, blanks dropped; none when it is not x,y.
std::optional<QString> pointText(const QString& text)
{
    const QString words = QString(text).remove(' ');
    if (words.isEmpty() || !numberList(words, 2, false)) {
        return std::nullopt;
    }
    return words;
}

// A number field: blank is the default, the text when it reads.
Result<QString> numberField(const QString& label, const QString& text, bool allowBlank)
{
    const QString value = text.trimmed();
    if (value.isEmpty() && allowBlank) {
        return QString();
    }
    if (!numberList(value, 1, false) || value.toDouble() < 0.0) {
        return invalid(label + ": a number, 0 or more");
    }
    return value;
}

// blank (GDAL's own coefficient), none, or a coefficient.
Result<QString> curvatureField(const QString& text)
{
    const QString value = text.trimmed();
    if (value.isEmpty() || value.compare("none", Qt::CaseInsensitive) == 0) {
        return value.toLower();
    }
    if (!numberList(value, 1, false) || value.toDouble() < 0.0) {
        return invalid("Curvature: blank for 0.85714, none, or a coefficient of 0 or more");
    }
    return value;
}

} // namespace

Result<QString> viewshedLine(const ViewshedForm& form)
{
    if (form.source.trimmed().isEmpty()) {
        return invalid("Source: there is no surface or raster to look over; build a surface or "
                       "import a DEM first");
    }
    QString line = "RASTER VIEWSHED " + form.source.trimmed();
    if (form.useScope) {
        if (!form.scopeError.isEmpty()) {
            return invalid("Observers: " + form.scopeError);
        }
        line += " OBSERVERS " + form.scope.trimmed();
    } else {
        if (form.observers.isEmpty()) {
            return invalid("Observers: add an observer, typed or picked, or take the points of a "
                           "scope");
        }
        for (const QString& observer : form.observers) {
            const auto point = pointText(observer);
            if (!point) {
                return invalid("Observers: " + observer + " is not x,y");
            }
            line += " OBSERVER " + *point;
        }
    }
    auto height = numberField("Eye height", form.height, false);
    if (!height) {
        return height.error();
    }
    auto target = numberField("Target height", form.target, false);
    if (!target) {
        return target.error();
    }
    line += " height=" + *height + " target=" + *target;
    auto reach = numberField("Look as far as", form.max, true);
    if (!reach) {
        return reach.error();
    }
    if (!reach->isEmpty()) {
        if (!(reach->toDouble() > 0.0)) {
            return invalid("Look as far as: a distance above 0, or blank for the whole raster");
        }
        line += " max=" + *reach;
    }
    auto curvature = curvatureField(form.curvature);
    if (!curvature) {
        return curvature.error();
    }
    if (!curvature->isEmpty()) {
        line += " curvature=" + *curvature;
    }
    if (!form.areas.trimmed().isEmpty()) {
        const auto word = lineWord("areas=" + form.areas.trimmed());
        if (!word) {
            return invalid("Areas on: a layer name cannot hold a double quote");
        }
        line += " " + *word;
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

Result<QString> losLine(const LosForm& form)
{
    if (form.source.trimmed().isEmpty()) {
        return invalid("Source: there is no surface or raster to look over; build a surface or "
                       "import a DEM first");
    }
    const auto observer = pointText(form.observer);
    if (!observer) {
        return invalid("Observer: x,y, typed or picked");
    }
    const auto target = pointText(form.target);
    if (!target) {
        return invalid("Target: x,y, typed or picked");
    }
    auto height = numberField("Eye height", form.height, false);
    if (!height) {
        return height.error();
    }
    auto targetHeight = numberField("Target height", form.targetHeight, false);
    if (!targetHeight) {
        return targetHeight.error();
    }
    QString line = "LOS " + form.source.trimmed() + " OBSERVER " + *observer + " TARGET " + *target +
                   " height=" + *height + " target=" + *targetHeight;
    auto curvature = curvatureField(form.curvature);
    if (!curvature) {
        return curvature.error();
    }
    if (!curvature->isEmpty()) {
        line += " curvature=" + *curvature;
    }
    return line;
}

ViewshedDialog::ViewshedDialog(TerrainDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("viewshedDialog");
    setWindowTitle("Viewshed and Line of Sight");
    auto* layout = new QVBoxLayout(this);
    tabs_ = new QTabWidget(this);
    tabs_->setObjectName("viewshedTabs");
    layout->addWidget(tabs_);
    const bool canPick = static_cast<bool>(context_.pickPoint);
    const QString pickTip = "Click the point in a plan view; Esc or a right click cancels.";

    // ---- Viewshed ----
    auto* page = new QWidget(tabs_);
    auto* pageLayout = new QVBoxLayout(page);
    auto* fields = new QFormLayout();
    source_ = new QComboBox(page);
    source_->setObjectName("viewshedSource");
    fields->addRow("Ground:", source_);
    auto* observerRow = new QHBoxLayout();
    observer_ = new QLineEdit(page);
    observer_->setObjectName("viewshedObserver");
    observer_->setPlaceholderText("x,y");
    add_ = new QPushButton("Add", page);
    add_->setObjectName("viewshedAdd");
    pick_ = new QPushButton("Pick", page);
    pick_->setObjectName("viewshedPick");
    pick_->setToolTip(pickTip);
    pick_->setEnabled(canPick);
    observerRow->addWidget(observer_);
    observerRow->addWidget(add_);
    observerRow->addWidget(pick_);
    fields->addRow("Observer:", observerRow);
    pageLayout->addLayout(fields);
    observers_ = new QListWidget(page);
    observers_->setObjectName("viewshedObservers");
    observers_->setMaximumHeight(90);
    pageLayout->addWidget(observers_);
    auto* listRow = new QHBoxLayout();
    listRow->addStretch();
    remove_ = new QPushButton("Remove", page);
    remove_->setObjectName("viewshedRemove");
    listRow->addWidget(remove_);
    pageLayout->addLayout(listRow);
    useScope_ = new QCheckBox("The observers are the points these take instead", page);
    useScope_->setObjectName("viewshedUseScope");
    pageLayout->addWidget(useScope_);
    scope_ = new ScopeFilterWidget("viewshed", page);
    scope_->views = context_.views;
    scope_->setChoice(ScopeChoice::Selection);
    pageLayout->addWidget(scope_);
    auto* options = new QFormLayout();
    height_ = new QLineEdit("1.7", page);
    height_->setObjectName("viewshedHeight");
    options->addRow("Eye height:", height_);
    target_ = new QLineEdit("0", page);
    target_->setObjectName("viewshedTarget");
    target_->setToolTip("How high above the ground a cell counts as seen.");
    options->addRow("Target height:", target_);
    max_ = new QLineEdit(page);
    max_->setObjectName("viewshedMax");
    max_->setPlaceholderText("blank: the whole raster");
    options->addRow("Look as far as:", max_);
    curvature_ = new QLineEdit(page);
    curvature_->setObjectName("viewshedCurvature");
    curvature_->setPlaceholderText("blank: 0.85714 (curvature and refraction); none");
    options->addRow("Curvature:", curvature_);
    areas_ = new QLineEdit(page);
    areas_->setObjectName("viewshedAreas");
    areas_->setPlaceholderText("blank: no areas drawn");
    options->addRow("Draw visible area on:", areas_);
    name_ = new QLineEdit(page);
    name_->setObjectName("viewshedName");
    name_->setPlaceholderText("the source's");
    options->addRow("Name:", name_);
    pageLayout->addLayout(options);
    auto* commandRow = new QFormLayout();
    command_ = terrainCommandField(page, "viewshedCommand");
    commandRow->addRow("Command:", command_);
    pageLayout->addLayout(commandRow);
    auto* buttons = new QHBoxLayout();
    buttons->addStretch();
    preview_ = new QPushButton("Preview", page);
    preview_->setObjectName("viewshedPreview");
    run_ = new QPushButton("Compute", page);
    run_->setObjectName("viewshedRun");
    buttons->addWidget(preview_);
    buttons->addWidget(run_);
    pageLayout->addLayout(buttons);
    reply_ = terrainReplyField(page, "viewshedReply");
    pageLayout->addWidget(reply_);
    tabs_->addTab(page, "Viewshed");

    // ---- Line of Sight ----
    auto* losPage = new QWidget(tabs_);
    auto* losLayout = new QVBoxLayout(losPage);
    auto* losFields = new QFormLayout();
    losSource_ = new QComboBox(losPage);
    losSource_->setObjectName("losSource");
    losFields->addRow("Ground:", losSource_);
    auto* observerLine = new QHBoxLayout();
    losObserver_ = new QLineEdit(losPage);
    losObserver_->setObjectName("losObserver");
    losObserver_->setPlaceholderText("x,y");
    losPickObserver_ = new QPushButton("Pick", losPage);
    losPickObserver_->setObjectName("losPickObserver");
    losPickObserver_->setToolTip(pickTip);
    losPickObserver_->setEnabled(canPick);
    observerLine->addWidget(losObserver_);
    observerLine->addWidget(losPickObserver_);
    losFields->addRow("Observer:", observerLine);
    auto* targetLine = new QHBoxLayout();
    losTarget_ = new QLineEdit(losPage);
    losTarget_->setObjectName("losTarget");
    losTarget_->setPlaceholderText("x,y");
    losPickTarget_ = new QPushButton("Pick", losPage);
    losPickTarget_->setObjectName("losPickTarget");
    losPickTarget_->setToolTip(pickTip);
    losPickTarget_->setEnabled(canPick);
    targetLine->addWidget(losTarget_);
    targetLine->addWidget(losPickTarget_);
    losFields->addRow("Target:", targetLine);
    losHeight_ = new QLineEdit("1.7", losPage);
    losHeight_->setObjectName("losHeight");
    losFields->addRow("Eye height:", losHeight_);
    losTargetHeight_ = new QLineEdit("0", losPage);
    losTargetHeight_->setObjectName("losTargetHeight");
    losFields->addRow("Target height:", losTargetHeight_);
    losCurvature_ = new QLineEdit(losPage);
    losCurvature_->setObjectName("losCurvature");
    losCurvature_->setPlaceholderText("blank: 0.85714; none");
    losFields->addRow("Curvature:", losCurvature_);
    losLayout->addLayout(losFields);
    auto* losCommandRow = new QFormLayout();
    losCommand_ = terrainCommandField(losPage, "losCommand");
    losCommandRow->addRow("Command:", losCommand_);
    losLayout->addLayout(losCommandRow);
    auto* losButtons = new QHBoxLayout();
    losButtons->addStretch();
    losRun_ = new QPushButton("Check", losPage);
    losRun_->setObjectName("losRun");
    losButtons->addWidget(losRun_);
    losLayout->addLayout(losButtons);
    losReply_ = terrainReplyField(losPage, "losReply");
    losLayout->addWidget(losReply_);
    tabs_->addTab(losPage, "Line of Sight");

    viewshedRunner_ = std::make_unique<TerrainRun>(context_, *reply_);
    losRunner_ = std::make_unique<TerrainRun>(context_, *losReply_);

    for (QComboBox* combo : {source_, losSource_}) {
        connect(combo, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    }
    for (QLineEdit* field : {height_, target_, max_, curvature_, areas_, name_, losObserver_,
                             losTarget_, losHeight_, losTargetHeight_, losCurvature_}) {
        connect(field, &QLineEdit::textChanged, this, [this] { refresh(); });
    }
    connect(useScope_, &QCheckBox::toggled, this, [this] { refresh(); });
    scope_->onChanged = [this] { refresh(); };
    connect(add_, &QPushButton::clicked, this, [this] {
        if (addObserver(observer_->text())) {
            observer_->clear();
        } else {
            reply_->setPlainText("Observer: " + observer_->text() + " is not x,y");
        }
    });
    connect(observer_, &QLineEdit::returnPressed, add_, &QPushButton::click);
    connect(pick_, &QPushButton::clicked, this, [this] { pickInto(nullptr, reply_); });
    connect(losPickObserver_, &QPushButton::clicked, this,
            [this] { pickInto(losObserver_, losReply_); });
    connect(losPickTarget_, &QPushButton::clicked, this, [this] { pickInto(losTarget_, losReply_); });
    connect(remove_, &QPushButton::clicked, this, [this] {
        delete observers_->currentItem();
        refresh();
    });
    connect(preview_, &QPushButton::clicked, this, [this] { runViewshed(true); });
    connect(run_, &QPushButton::clicked, this, [this] { runViewshed(false); });
    connect(losRun_, &QPushButton::clicked, this, [this] { runLos(); });
    reload();
    refresh();
}

ViewshedDialog::~ViewshedDialog() = default;

void ViewshedDialog::reload()
{
    const auto choices =
        terrainSources(context_, TerrainSourceKinds::Surfaces | TerrainSourceKinds::Rasters);
    fillSources(*source_, choices);
    fillSources(*losSource_, choices);
    if (context_.document != nullptr) {
        scope_->reload(context_.document->model());
    } else {
        scope_->reloadViews();
    }
}

ViewshedForm ViewshedDialog::viewshedForm() const
{
    ViewshedForm form;
    form.source = source_->currentData().toString();
    for (int i = 0; i < observers_->count(); ++i) {
        form.observers << observers_->item(i)->text();
    }
    form.useScope = useScope_->isChecked();
    if (auto words = scope_->verbWords()) {
        form.scope = *words;
    } else {
        form.scopeError = QString::fromStdString(words.error().message);
    }
    form.height = height_->text();
    form.target = target_->text();
    form.max = max_->text();
    form.curvature = curvature_->text();
    form.areas = areas_->text();
    form.name = name_->text();
    return form;
}

LosForm ViewshedDialog::losForm() const
{
    LosForm form;
    form.source = losSource_->currentData().toString();
    form.observer = losObserver_->text();
    form.target = losTarget_->text();
    form.height = losHeight_->text();
    form.targetHeight = losTargetHeight_->text();
    form.curvature = losCurvature_->text();
    return form;
}

bool ViewshedDialog::addObserver(const QString& text)
{
    const auto point = pointText(text);
    if (!point) {
        return false;
    }
    observers_->addItem(*point);
    refresh();
    return true;
}

void ViewshedDialog::pickInto(QLineEdit* field, QPlainTextEdit* reply)
{
    if (!context_.pickPoint) {
        return;
    }
    reply->setPlainText("Click the point in a plan view; Esc or a right click cancels.");
    QPointer<ViewshedDialog> self(this);
    QPointer<QLineEdit> into(field);
    QPointer<QPlainTextEdit> said(reply);
    context_.pickPoint([self, into, said, field](std::optional<katana::geometry::Point2> picked) {
        if (!self || !said) {
            return;
        }
        if (!picked) {
            said->setPlainText("No point was picked.");
            return;
        }
        const QString words = pointWords(picked->x, picked->y);
        if (field == nullptr) {
            (void)self->addObserver(words);
        } else if (into) {
            into->setText(words);
        }
        said->clear();
    });
}

void ViewshedDialog::refresh()
{
    const bool fromScope = useScope_->isChecked();
    for (QWidget* widget : std::initializer_list<QWidget*>{observer_, add_, observers_}) {
        widget->setEnabled(!fromScope);
    }
    pick_->setEnabled(!fromScope && static_cast<bool>(context_.pickPoint));
    remove_->setEnabled(!fromScope && observers_->count() > 0);
    scope_->setEnabled(fromScope);
    const auto viewshed = viewshedLine(viewshedForm());
    command_->setText(viewshed ? *viewshed : QString());
    command_->setPlaceholderText(viewshed ? QString()
                                          : QString::fromStdString(viewshed.error().message));
    preview_->setEnabled(viewshed.ok());
    run_->setEnabled(viewshed.ok());
    const auto los = losLine(losForm());
    losCommand_->setText(los ? *los : QString());
    losCommand_->setPlaceholderText(los ? QString() : QString::fromStdString(los.error().message));
    losRun_->setEnabled(los.ok());
}

void ViewshedDialog::runViewshed(bool preview)
{
    const auto line = viewshedLine(viewshedForm());
    if (!line) {
        reply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    viewshedRunner_->run(preview ? *line + " PREVIEW" : *line);
}

void ViewshedDialog::runLos()
{
    const auto line = losLine(losForm());
    if (!line) {
        losReply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    losRunner_->run(*line);
}

void ViewshedDialog::showEvent(QShowEvent* event)
{
    reload();
    refresh();
    QDialog::showEvent(event);
}

} // namespace katana::qt
