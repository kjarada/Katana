// Terrain > Analysis > Drape and Sample Heights (drape_dialog.hpp).

#include "geo/drape_dialog.hpp"

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

void addMethods(QComboBox& combo)
{
    combo.addItem("Bilinear", "bilinear");
    combo.addItem("Nearest cell", "nearest");
    combo.addItem("Cubic", "cubic");
    combo.addItem("Cubic spline", "cubicspline");
}

bool isSurface(const QString& source)
{
    return source.trimmed().startsWith("SURFACE ", Qt::CaseInsensitive);
}

} // namespace

QString pointWords(double x, double y)
{
    return QString::number(x, 'f', 3) + "," + QString::number(y, 'f', 3);
}

Result<QString> drapeLine(const DrapeForm& form)
{
    if (form.source.trimmed().isEmpty()) {
        return invalid("Source: there is no surface or raster to drape on; build a surface or "
                       "import a DEM first");
    }
    if (!form.scopeError.isEmpty()) {
        return invalid("Drape: " + form.scopeError);
    }
    QString line = "DRAPE " + form.source.trimmed();
    if (!form.scope.trimmed().isEmpty()) {
        line += " " + form.scope.trimmed();
    }
    if (!isSurface(form.source)) {
        line += " method=" + form.method;
    }
    return line;
}

Result<QString> sampleLine(const SampleForm& form)
{
    if (form.source.trimmed().isEmpty()) {
        return invalid("Source: there is no surface or raster to sample; build a surface or "
                       "import a DEM first");
    }
    if (form.points.isEmpty()) {
        return invalid("Points: add a point to sample, typed or picked");
    }
    QString line = "RASTER SAMPLE " + form.source.trimmed();
    for (const QString& point : form.points) {
        const QString words = QString(point).remove(' ');
        if (!numberList(words, 2, false)) {
            return invalid("Points: " + point + " is not x,y");
        }
        line += " AT " + words;
    }
    if (!isSurface(form.source)) {
        line += " method=" + form.method;
    }
    return line;
}

DrapeDialog::DrapeDialog(TerrainDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("drapeDialog");
    setWindowTitle("Drape and Sample Heights");
    auto* layout = new QVBoxLayout(this);
    tabs_ = new QTabWidget(this);
    tabs_->setObjectName("drapeTabs");
    layout->addWidget(tabs_);

    // ---- Drape ----
    auto* drapePage = new QWidget(tabs_);
    auto* drapeLayout = new QVBoxLayout(drapePage);
    auto* drapeFields = new QFormLayout();
    drapeSource_ = new QComboBox(drapePage);
    drapeSource_->setObjectName("drapeSource");
    drapeFields->addRow("Ground:", drapeSource_);
    drapeMethod_ = new QComboBox(drapePage);
    drapeMethod_->setObjectName("drapeMethod");
    addMethods(*drapeMethod_);
    drapeMethod_->setToolTip("How a raster is read between cell centres. A surface is read on its "
                             "own triangles.");
    drapeFields->addRow("Between cells:", drapeMethod_);
    drapeLayout->addLayout(drapeFields);
    scope_ = new ScopeFilterWidget("drape", drapePage);
    scope_->views = context_.views;
    scope_->setChoice(ScopeChoice::Selection);
    drapeLayout->addWidget(scope_);
    auto* drapeCommandRow = new QFormLayout();
    drapeCommand_ = terrainCommandField(drapePage, "drapeCommand");
    drapeCommandRow->addRow("Command:", drapeCommand_);
    drapeLayout->addLayout(drapeCommandRow);
    auto* drapeButtons = new QHBoxLayout();
    drapeButtons->addStretch();
    drapePreview_ = new QPushButton("Preview", drapePage);
    drapePreview_->setObjectName("drapePreview");
    drapeRun_ = new QPushButton("Drape", drapePage);
    drapeRun_->setObjectName("drapeRun");
    drapeButtons->addWidget(drapePreview_);
    drapeButtons->addWidget(drapeRun_);
    drapeLayout->addLayout(drapeButtons);
    drapeReply_ = terrainReplyField(drapePage, "drapeReply");
    drapeLayout->addWidget(drapeReply_);
    tabs_->addTab(drapePage, "Drape");

    // ---- Sample ----
    auto* samplePage = new QWidget(tabs_);
    auto* sampleLayout = new QVBoxLayout(samplePage);
    auto* sampleFields = new QFormLayout();
    sampleSource_ = new QComboBox(samplePage);
    sampleSource_->setObjectName("sampleSource");
    sampleFields->addRow("Ground:", sampleSource_);
    sampleMethod_ = new QComboBox(samplePage);
    sampleMethod_->setObjectName("sampleMethod");
    addMethods(*sampleMethod_);
    sampleFields->addRow("Between cells:", sampleMethod_);
    auto* pointRow = new QHBoxLayout();
    samplePoint_ = new QLineEdit(samplePage);
    samplePoint_->setObjectName("samplePoint");
    samplePoint_->setPlaceholderText("x,y");
    sampleAdd_ = new QPushButton("Add", samplePage);
    sampleAdd_->setObjectName("sampleAdd");
    samplePick_ = new QPushButton("Pick", samplePage);
    samplePick_->setObjectName("samplePick");
    samplePick_->setToolTip("Click the point in a plan view; Esc or a right click cancels.");
    samplePick_->setEnabled(static_cast<bool>(context_.pickPoint));
    pointRow->addWidget(samplePoint_);
    pointRow->addWidget(sampleAdd_);
    pointRow->addWidget(samplePick_);
    sampleFields->addRow("Point:", pointRow);
    sampleLayout->addLayout(sampleFields);
    samplePoints_ = new QListWidget(samplePage);
    samplePoints_->setObjectName("samplePoints");
    samplePoints_->setMaximumHeight(110);
    sampleLayout->addWidget(samplePoints_);
    auto* listButtons = new QHBoxLayout();
    listButtons->addStretch();
    sampleRemove_ = new QPushButton("Remove", samplePage);
    sampleRemove_->setObjectName("sampleRemove");
    listButtons->addWidget(sampleRemove_);
    sampleLayout->addLayout(listButtons);
    auto* sampleCommandRow = new QFormLayout();
    sampleCommand_ = terrainCommandField(samplePage, "sampleCommand");
    sampleCommandRow->addRow("Command:", sampleCommand_);
    sampleLayout->addLayout(sampleCommandRow);
    auto* sampleButtons = new QHBoxLayout();
    sampleButtons->addStretch();
    sampleRun_ = new QPushButton("Sample", samplePage);
    sampleRun_->setObjectName("sampleRun");
    sampleButtons->addWidget(sampleRun_);
    sampleLayout->addLayout(sampleButtons);
    sampleReply_ = terrainReplyField(samplePage, "sampleReply");
    sampleLayout->addWidget(sampleReply_);
    tabs_->addTab(samplePage, "Sample");

    drapeRunner_ = std::make_unique<TerrainRun>(context_, *drapeReply_);
    sampleRunner_ = std::make_unique<TerrainRun>(context_, *sampleReply_);

    for (QComboBox* combo : {drapeSource_, drapeMethod_, sampleSource_, sampleMethod_}) {
        connect(combo, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    }
    scope_->onChanged = [this] { refresh(); };
    connect(drapePreview_, &QPushButton::clicked, this, [this] { runDrape(true); });
    connect(drapeRun_, &QPushButton::clicked, this, [this] { runDrape(false); });
    connect(sampleAdd_, &QPushButton::clicked, this, [this] {
        if (addSamplePoint(samplePoint_->text())) {
            samplePoint_->clear();
        } else {
            sampleReply_->setPlainText("Point: " + samplePoint_->text() + " is not x,y");
        }
    });
    connect(samplePoint_, &QLineEdit::returnPressed, sampleAdd_, &QPushButton::click);
    connect(samplePick_, &QPushButton::clicked, this, [this] {
        if (!context_.pickPoint) {
            return;
        }
        sampleReply_->setPlainText("Click the point in a plan view; Esc or a right click cancels.");
        QPointer<DrapeDialog> self(this);
        context_.pickPoint([self](std::optional<katana::geometry::Point2> picked) {
            if (!self) {
                return;
            }
            if (!picked) {
                self->sampleReply_->setPlainText("No point was picked.");
                return;
            }
            (void)self->addSamplePoint(pointWords(picked->x, picked->y));
            self->sampleReply_->clear();
        });
    });
    connect(sampleRemove_, &QPushButton::clicked, this, [this] {
        delete samplePoints_->currentItem();
        refresh();
    });
    connect(sampleRun_, &QPushButton::clicked, this, [this] { runSample(); });
    reload();
    refresh();
}

DrapeDialog::~DrapeDialog() = default;

void DrapeDialog::reload()
{
    const auto choices =
        terrainSources(context_, TerrainSourceKinds::Surfaces | TerrainSourceKinds::Rasters);
    fillSources(*drapeSource_, choices);
    fillSources(*sampleSource_, choices);
    if (context_.document != nullptr) {
        scope_->reload(context_.document->model());
    } else {
        scope_->reloadViews();
    }
}

DrapeForm DrapeDialog::drapeForm() const
{
    DrapeForm form;
    form.source = drapeSource_->currentData().toString();
    form.method = drapeMethod_->currentData().toString();
    if (auto words = scope_->verbWords()) {
        form.scope = *words;
    } else {
        form.scopeError = QString::fromStdString(words.error().message);
    }
    return form;
}

SampleForm DrapeDialog::sampleForm() const
{
    SampleForm form;
    form.source = sampleSource_->currentData().toString();
    form.method = sampleMethod_->currentData().toString();
    for (int i = 0; i < samplePoints_->count(); ++i) {
        form.points << samplePoints_->item(i)->text();
    }
    return form;
}

bool DrapeDialog::addSamplePoint(const QString& text)
{
    const QString words = QString(text).remove(' ');
    if (words.isEmpty() || !numberList(words, 2, false)) {
        return false;
    }
    samplePoints_->addItem(words);
    refresh();
    return true;
}

void DrapeDialog::refresh()
{
    drapeMethod_->setEnabled(!isSurface(drapeSource_->currentData().toString()));
    sampleMethod_->setEnabled(!isSurface(sampleSource_->currentData().toString()));
    sampleRemove_->setEnabled(samplePoints_->count() > 0);
    const auto drape = drapeLine(drapeForm());
    drapeCommand_->setText(drape ? *drape : QString());
    drapeCommand_->setPlaceholderText(drape ? QString()
                                            : QString::fromStdString(drape.error().message));
    drapePreview_->setEnabled(drape.ok());
    drapeRun_->setEnabled(drape.ok());
    const auto sample = sampleLine(sampleForm());
    sampleCommand_->setText(sample ? *sample : QString());
    sampleCommand_->setPlaceholderText(sample ? QString()
                                              : QString::fromStdString(sample.error().message));
    sampleRun_->setEnabled(sample.ok());
}

void DrapeDialog::runDrape(bool preview)
{
    const auto line = drapeLine(drapeForm());
    if (!line) {
        drapeReply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    drapeRunner_->run(preview ? *line + " PREVIEW" : *line);
}

void DrapeDialog::runSample()
{
    const auto line = sampleLine(sampleForm());
    if (!line) {
        sampleReply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    sampleRunner_->run(*line);
}

void DrapeDialog::showEvent(QShowEvent* event)
{
    reload();
    refresh();
    QDialog::showEvent(event);
}

} // namespace katana::qt
