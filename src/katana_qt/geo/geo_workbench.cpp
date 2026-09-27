// The geoprocessing verbs in the window (geo_workbench.hpp).

#include "geo/geo_workbench.hpp"

#include <QAction>
#include <QMainWindow>
#include <QMenu>

#include <memory>
#include <optional>
#include <string>
#include <utility>

#include "geo/replies.hpp"
#include "katana/core/text.hpp"

namespace katana::qt {

using katana::core::Result;

namespace {

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

} // namespace

GeoWorkbench::GeoWorkbench(QMainWindow& window, GeoServices services)
    : window_(window), services_(std::move(services)),
      context_{*services_.document,
               *services_.interpreter,
               *services_.reference,
               *services_.surfaces,
               services_.scratch,
               services_.frame,
               services_.changed}
{
    if (!services_.pickPoint && services_.views != nullptr) {
        services_.pickPoint = planPointPicker(*services_.views);
    }
}

GeoWorkbench::~GeoWorkbench() = default;

int GeoWorkbench::addFinishedListener(std::function<void(JobId, const VerbOutcome&)> listener)
{
    const int key = nextListener_++;
    listeners_.emplace(key, std::move(listener));
    return key;
}

void GeoWorkbench::removeFinishedListener(int key)
{
    listeners_.erase(key);
}

void GeoWorkbench::notify(JobId id, const VerbOutcome& outcome)
{
    if (services_.finished) {
        services_.finished(id, outcome);
    }
    // A copy: a listener may remove itself.
    const auto listeners = listeners_;
    for (const auto& [key, listener] : listeners) {
        listener(id, outcome);
    }
}

bool GeoWorkbench::runLine(const QString& line)
{
    const std::string text = line.trimmed().toStdString();
    if (!katana::app::geo::handles(text)) {
        return false;
    }
    lastJob_ = kNoJob;
    auto prepared = katana::app::geo::prepare(context_, text);
    if (!prepared) {
        services_.log("error: " + qs(prepared.error().describe()), true);
        return true;
    }
    if (prepared->reply) {
        services_.log(qs(*prepared->reply), false);
        return true;
    }

    // The job owns what the work reads - copied at prepare - and hands back
    // the executor's Apply, which runs here, on the GUI thread, as one step.
    const katana::app::geo::Work work = prepared->work;
    auto reply = std::make_shared<VerbOutcome>();
    JobRunner& runner = JobRunner::of(window_);
    const JobId id = runner.start(
        qs(prepared->title),
        [this, work, reply](JobControl& control) -> Result<JobRunner::Apply> {
            auto apply = work(control.stopToken(),
                              [&control](double fraction) { control.setProgress(fraction); });
            if (!apply) {
                return apply.error();
            }
            katana::app::geo::Apply step = std::move(apply).value();
            return JobRunner::Apply([this, step, reply] {
                auto applied = step(context_);
                if (!applied) {
                    reply->ok = false;
                    reply->error = "error: " + qs(applied.error().describe());
                    services_.log(reply->error, true);
                    return;
                }
                reply->ok = true;
                reply->reply = qs(*applied);
                if (!applied->empty()) {
                    services_.log(reply->reply, false);
                }
            });
        },
        [this, reply](const JobReport& report) {
            if (report.outcome == JobOutcome::Cancelled) {
                // The executor's word for it, as katana_cli says it.
                reply->ok = false;
                reply->error = "error: InvalidState: cancelled";
                services_.log(reply->error, true);
            } else if (report.outcome == JobOutcome::Failed) {
                reply->ok = false;
                reply->error = "error: " + qs(report.error);
                services_.log(reply->error, true);
            }
            notify(report.id, *reply);
        });
    lastJob_ = id;
    if (services_.headless && services_.headless()) {
        // Nobody to wait for a result, and the next line may read it.
        runner.waitFor(id);
    } else {
        services_.log(startedRecord(id, qs(prepared->title)), false);
    }
    return true;
}

// ---- the menus -----------------------------------------------------------------------------

void GeoMenus::addToGis(const QString& title, QAction* action)
{
    const auto found = gisSections_.find(title);
    if (found == gisSections_.end()) {
        gis_.addSection(title);
        gis_.addAction(action);
        gisSections_.emplace(title, action);
        return;
    }
    // After the section's last item, so a package's items stay together
    // whatever order the packages add them in.
    const QList<QAction*> actions = gis_.actions();
    const qsizetype at = actions.indexOf(found->second);
    gis_.insertAction(at + 1 < actions.size() ? actions[at + 1] : nullptr, action);
    found->second = action;
}

void GeoMenus::addToTerrain(const QString& title, const QString& name, Icon icon,
                            const QString& tip, QAction* action)
{
    QMenu*& menu = terrainMenus_[name];
    if (menu == nullptr) {
        menu = terrain_.addMenu(title);
        menu->setObjectName(name);
        // A submenu is a menu item too: the window's --check-menus holds it
        // to an icon and a status tip like every other.
        menu->setIcon(katana::qt::icon(icon));
        menu->menuAction()->setStatusTip(tip);
    }
    menu->addAction(action);
}

QString startedRecord(JobId id, const QString& title)
{
    return "job id=" + QString::number(id) +
           " title=" + QString::fromStdString(katana::app::geo::value(title.toStdString())) +
           " state=started";
}

std::optional<JobId> startedJob(const QString& reply)
{
    for (const katana::app::geo::Record& record :
         katana::app::geo::parseRecords(reply.toStdString())) {
        if (record.kind != "job" || record.get("state") != std::optional<std::string>("started")) {
            continue;
        }
        if (const auto id = katana::core::parseInteger(record.get("id").value_or(""));
            id && *id > 0) {
            return static_cast<JobId>(*id);
        }
    }
    return std::nullopt;
}

} // namespace katana::qt
