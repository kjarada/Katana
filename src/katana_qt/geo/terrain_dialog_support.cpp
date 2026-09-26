// What the terrain dialogs share (terrain_dialog_support.hpp).

#include "geo/terrain_dialog_support.hpp"

#include <QComboBox>
#include <QDialog>
#include <QFontDatabase>
#include <QLineEdit>
#include <QMainWindow>
#include <QPlainTextEdit>
#include <QRegularExpression>
#include <QStringList>

#include <algorithm>
#include <string>
#include <utility>

#include "geo/geo_workbench.hpp"
#include "katana/core/text.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

namespace {

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// The job an interactive run started, from the line the workbench logs:
// `job id=<n> title="..." state=started`.
JobId startedJob(const QString& reply)
{
    static const QRegularExpression started(QStringLiteral("^job id=(\\d+) .*state=started$"),
                                            QRegularExpression::MultilineOption);
    const QRegularExpressionMatch match = started.match(reply);
    return match.hasMatch() ? match.captured(1).toULongLong() : kNoJob;
}

} // namespace

TerrainDialogContext terrainDialogContext(GeoWorkbench& workbench)
{
    const GeoServices& services = workbench.services();
    TerrainDialogContext context;
    context.run = services.run;
    context.headless = services.headless;
    context.reference = services.reference;
    context.surfaces = services.surfaces;
    context.document = services.document;
    if (services.views != nullptr) {
        ViewWorkspace* views = services.views;
        context.views = [views] { return scopeFilterViews(views->viewSet()); };
    }
    context.listen = [&workbench](std::function<void(JobId, const VerbOutcome&)> listener) {
        (void)workbench.addFinishedListener(std::move(listener));
    };
    return context;
}

std::vector<TerrainSourceChoice> terrainSources(const TerrainDialogContext& context, unsigned kinds)
{
    std::vector<TerrainSourceChoice> choices;
    const auto word = [](const std::string& text) { return lineWord(qs(text)).value_or(QString()); };
    if ((kinds & TerrainSourceKinds::Surfaces) != 0 && context.surfaces != nullptr) {
        for (const katana::terrain::NamedSurface& surface : context.surfaces->all()) {
            const QString name = word(surface.name);
            if (!name.isEmpty()) {
                choices.push_back({qs(surface.name) + " (surface)", "SURFACE " + name});
            }
        }
    }
    if ((kinds & TerrainSourceKinds::Rasters) != 0 && context.reference != nullptr) {
        // By id: two imports of one file have one name.
        for (const katana::interop::RasterOverlay& raster : context.reference->rasters()) {
            choices.push_back({QString("%1 (raster %2)").arg(qs(raster.name)).arg(raster.id),
                               QString("RASTER %1").arg(raster.id)});
        }
    }
    if ((kinds & TerrainSourceKinds::Clouds) != 0 && context.reference != nullptr) {
        for (const katana::interop::PointCloudLayer& cloud : context.reference->pointClouds()) {
            choices.push_back({QString("%1 (point cloud %2)").arg(qs(cloud.name)).arg(cloud.id),
                               QString("CLOUD %1").arg(cloud.id)});
        }
    }
    return choices;
}

void fillSources(QComboBox& combo, const std::vector<TerrainSourceChoice>& choices)
{
    const QString kept = combo.currentData().toString();
    combo.clear();
    for (const TerrainSourceChoice& choice : choices) {
        combo.addItem(choice.label, choice.words);
    }
    const int again = combo.findData(kept);
    if (again >= 0) {
        combo.setCurrentIndex(again);
    }
}

std::optional<QString> lineWord(const QString& text)
{
    if (text.contains('"')) {
        return std::nullopt;
    }
    const bool blank = text.isEmpty() || text.contains(' ') || text.contains('\t');
    return blank ? "\"" + text + "\"" : text;
}

bool numberList(const QString& text, int count, bool whole)
{
    const QStringList items = text.split(',');
    if (count > 0 && items.size() != count) {
        return false;
    }
    return std::ranges::all_of(items, [whole](const QString& item) {
        const std::string word = item.trimmed().toStdString();
        return whole ? katana::core::parseInteger(word).has_value()
                     : katana::core::parseFiniteDouble(word).has_value();
    });
}

QLineEdit* terrainCommandField(QWidget* parent, const QString& name)
{
    auto* command = new QLineEdit(parent);
    command->setObjectName(name);
    command->setReadOnly(true);
    command->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    return command;
}

QPlainTextEdit* terrainReplyField(QWidget* parent, const QString& name)
{
    auto* reply = new QPlainTextEdit(parent);
    reply->setObjectName(name);
    reply->setReadOnly(true);
    reply->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    reply->setMinimumHeight(110);
    return reply;
}

QDialog& showTerrainDialog(GeoWorkbench& workbench, const QString& name,
                           const std::function<QDialog*(TerrainDialogContext, QWidget*)>& make)
{
    // By the base class: the window has no moc, so a dialog's own class has
    // no meta-object for findChild to tell it by; the name is unique.
    auto* dialog = workbench.window().findChild<QDialog*>(name);
    if (dialog == nullptr) {
        dialog = make(terrainDialogContext(workbench), &workbench.window());
    }
    dialog->show();
    dialog->raise();
    dialog->activateWindow();
    return *dialog;
}

// What a job's end is matched against: the job the dialog waits for, and
// the dialog, while it lives.
struct TerrainRun::Waiting {
    TerrainRun* run = nullptr;
    JobId job = kNoJob;
};

TerrainRun::TerrainRun(TerrainDialogContext& context, QPlainTextEdit& reply)
    : context_(context), reply_(reply), waiting_(std::make_shared<Waiting>())
{
    waiting_->run = this;
    if (context_.listen) {
        context_.listen([weak = std::weak_ptr<Waiting>(waiting_)](JobId id,
                                                                   const VerbOutcome& outcome) {
            const std::shared_ptr<Waiting> waiting = weak.lock();
            if (waiting && id != kNoJob && id == waiting->job) {
                waiting->job = kNoJob;
                waiting->run->show(outcome);
            }
        });
    }
}

TerrainRun::~TerrainRun() = default;

VerbOutcome TerrainRun::run(const QString& line)
{
    if (!context_.run) {
        VerbOutcome refused{false, {}, "nothing here can run a command"};
        show(refused);
        return refused;
    }
    const VerbOutcome outcome = context_.run(line);
    const JobId job = outcome.ok ? startedJob(outcome.reply) : kNoJob;
    if (job != kNoJob && context_.listen) {
        waiting_->job = job;
        reply_.setPlainText(outcome.reply + "\nRunning - the result is shown here when the job "
                                            "ends, and in the command log.");
        return outcome;
    }
    show(outcome);
    return outcome;
}

void TerrainRun::show(const VerbOutcome& outcome)
{
    QString text = outcome.reply;
    if (!outcome.error.isEmpty()) {
        text += (text.isEmpty() ? "" : "\n") + outcome.error;
    }
    reply_.setPlainText(text);
    if (done) {
        done(outcome);
    }
}

} // namespace katana::qt
