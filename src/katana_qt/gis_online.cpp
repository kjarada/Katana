#include "gis_online.hpp"

#include <QAction>
#include <QDir>
#include <QKeySequence>
#include <QMainWindow>
#include <QMenu>
#include <QSettings>
#include <QStandardPaths>
#include <QTimer>

#include <cstdlib>
#include <fstream>
#include <memory>
#include <sstream>

#include "gis_online_dialog.hpp"
#include "jobs.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/gis/web_access.hpp"
#include "katana/interop/online_requests.hpp"
#include "view_workspace.hpp"
#include "viewport_widget.hpp"

namespace katana::qt {
namespace {

namespace interop = katana::interop;
namespace cmd = katana::commands;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

// A headless import that has not finished in this long is cancelled: a
// scripted run must end, and ten minutes is far past any import the limits
// allow on a working connection.
constexpr int kHeadlessDeadlineSeconds = 600;
// What the cache may hold before its oldest answers go, and how long any
// answer is kept at most: two gigabytes, three months.
constexpr std::uint64_t kCacheBytes = 2ull * 1024 * 1024 * 1024;
constexpr int kCacheDays = 90;

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

std::filesystem::path toPath(const QString& text)
{
    return std::filesystem::path(text.toStdU16String());
}

std::string readAll(const std::filesystem::path& path)
{
    std::ifstream in(path, std::ios::binary);
    std::ostringstream text;
    text << in.rdbuf();
    return text.str();
}

std::string errorLine(interop::OnlineVerb verb, const std::string& message)
{
    return "error verb=" + std::string(interop::toString(verb)) +
           " message=" + interop::replyValue(message) + "\n";
}

} // namespace

OnlineDataWorkbench::OnlineDataWorkbench(QMainWindow& window, OnlineServices services,
                                         QMenu& gisMenu)
    : window_(window), services_(std::move(services))
{
    reloadCatalogue();
    action_ = services_.makeAction(
        Icon::ImportRaster, "O&nline Data...",
        "Import imagery, elevation and features from public web services - Australian state "
        "and national, and global - into the project's coordinate system",
        {}, "onlineData");
    QObject::connect(action_, &QAction::triggered, &window_, [this] {
        OnlineDataDialog& shown = dialog();
        shown.refreshCatalogue();
        shown.show();
        shown.raise();
        shown.activateWindow();
    });
    gisMenu.addSection("Online - Web Services");
    gisMenu.addAction(action_);
}

OnlineDataWorkbench::~OnlineDataWorkbench()
{
    // The dialog is the window's child only so that it floats over it; it
    // calls back into this object, so it goes with it.
    delete dialog_.data();
}

OnlineDataDialog& OnlineDataWorkbench::dialog()
{
    if (dialog_ == nullptr) {
        OnlineDialogContext context;
        context.catalogue = [this]() -> const interop::OnlineCatalogue& { return catalogue_; };
        context.projectCrs = [this] { return projectCrs(); };
        context.hasKey = [this](const std::string& name) { return hasKey(name); };
        context.saveKey = [this](const std::string& name, const std::string& value) {
            saveKey(name, value);
        };
        context.run = [this](const interop::OnlineCommand& command) { return run(command); };
        context.addCustom = [this](const std::string& url) { return startDiscovery(url); };
        dialog_ = new OnlineDataDialog(std::move(context), &window_);
    }
    return *dialog_;
}

void OnlineDataWorkbench::reply(const std::string& text, bool isError) const
{
    std::string trimmed = text;
    while (!trimmed.empty() && trimmed.back() == '\n') {
        trimmed.pop_back();
    }
    services_.log(qs(trimmed), isError);
}

std::filesystem::path OnlineDataWorkbench::userCataloguePath() const
{
    if (const char* overridden = std::getenv("KATANA_ONLINE_CATALOGUE"); overridden != nullptr &&
                                                                          *overridden != '\0') {
        return std::filesystem::path(overridden);
    }
    return toPath(QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation)) /
           "online_sources.json";
}

std::filesystem::path OnlineDataWorkbench::cacheDirectory() const
{
    if (const char* overridden = std::getenv("KATANA_ONLINE_CACHE"); overridden != nullptr &&
                                                                       *overridden != '\0') {
        return std::filesystem::path(overridden);
    }
    return toPath(QStandardPaths::writableLocation(QStandardPaths::CacheLocation)) / "online";
}

void OnlineDataWorkbench::reloadCatalogue()
{
    auto builtIn = interop::builtInCatalogue();
    catalogue_ = builtIn ? *builtIn : interop::OnlineCatalogue{};
    if (!builtIn) {
        reply("the built-in online catalogue could not be read: " + builtIn.error().describe(), true);
    }
    const std::filesystem::path user = userCataloguePath();
    std::error_code error;
    if (std::filesystem::is_regular_file(user, error)) {
        auto parsed = interop::parseCatalogue(readAll(user), true);
        if (!parsed) {
            reply("the online catalogue " + user.string() + " was not read: " +
                      parsed.error().describe(),
                  true);
        } else {
            catalogue_ = interop::mergeCatalogues(std::move(catalogue_), *parsed);
        }
    }
    if (dialog_ != nullptr) {
        dialog_->refreshCatalogue();
    }
}

std::string OnlineDataWorkbench::projectCrs() const
{
    return services_.document->metadata().coordinateSystem;
}

bool OnlineDataWorkbench::hasKey(const std::string& name) const
{
    const QSettings settings;
    return !settings.value("online/keys/" + qs(name)).toString().isEmpty();
}

void OnlineDataWorkbench::saveKey(const std::string& name, const std::string& value)
{
    QSettings settings;
    if (value.empty()) {
        settings.remove("online/keys/" + qs(name));
    } else {
        settings.setValue("online/keys/" + qs(name), qs(value));
    }
}

interop::OnlineEnvironment OnlineDataWorkbench::environment() const
{
    interop::OnlineEnvironment environment;
    environment.cacheDirectory = cacheDirectory();
    environment.userAgent = interop::onlineUserAgent(services_.version);
    QSettings settings;
    settings.beginGroup("online/keys");
    for (const QString& name : settings.childKeys()) {
        environment.keys[name.toStdString()] = settings.value(name).toString().toStdString();
    }
    return environment;
}

Result<katana::gis::CrsBox> OnlineDataWorkbench::areaOf(const interop::OnlineCommand& command) const
{
    katana::geometry::Box2 box;
    switch (command.area) {
    case interop::OnlineAreaKind::Box:
        return command.box;
    case interop::OnlineAreaKind::View: {
        ViewportWidget* plan = services_.views->activePlanView();
        if (plan == nullptr) {
            return makeError(ErrorCode::InvalidState, "there is no plan view to take the area from");
        }
        const katana::cad::ViewTransform& view = plan->viewTransform();
        const double halfWidth = 0.5 * view.widthPixels / view.scale;
        const double halfHeight = 0.5 * view.heightPixels / view.scale;
        return katana::gis::CrsBox{view.center.x - halfWidth, view.center.y - halfHeight,
                                   view.center.x + halfWidth, view.center.y + halfHeight};
    }
    case interop::OnlineAreaKind::Drawing:
        box = services_.document->model().entities.bounds();
        if (box.empty()) {
            return makeError(ErrorCode::InvalidState,
                             "the drawing is empty, so it has no extents; use area=view or a box");
        }
        break;
    case interop::OnlineAreaKind::Selection: {
        const auto& entities = services_.document->model().entities;
        for (const auto id : services_.document->selection().ids()) {
            if (const auto* entity = entities.find(id)) {
                box.expand(katana::entity::boundingBox(entity->geometry));
            }
        }
        if (box.empty()) {
            return makeError(ErrorCode::InvalidState, "nothing is selected to take the area from");
        }
        break;
    }
    }
    // A single point or a straight line has no area; give it a margin of a
    // tenth of its size, and at least ten metres, so a selected lot or a
    // selected point asks for its surroundings rather than for nothing.
    const double margin =
        std::max(10.0, 0.1 * std::max(box.max.x - box.min.x, box.max.y - box.min.y));
    return katana::gis::CrsBox{box.min.x - margin, box.min.y - margin, box.max.x + margin,
                               box.max.y + margin};
}

QString OnlineDataWorkbench::loggedLine(const QString& line)
{
    const QStringList words = line.trimmed().split(' ', Qt::SkipEmptyParts);
    if (words.size() >= 4 && words[0].compare("ONLINE", Qt::CaseInsensitive) == 0 &&
        words[1].compare("KEY", Qt::CaseInsensitive) == 0) {
        return words[0] + " " + words[1] + " " + words[2] + " ***";
    }
    return line;
}

bool OnlineDataWorkbench::runLine(const QString& line)
{
    const QString first = line.trimmed().section(' ', 0, 0);
    if (first.compare("ONLINE", Qt::CaseInsensitive) != 0) {
        return false;
    }
    auto command = interop::parseOnlineCommand(line.toStdString());
    if (!command) {
        reply("error verb=ONLINE code=" + std::string(katana::core::toString(command.error().code)) +
                  " message=" + interop::replyValue(command.error().message) +
                  (command.error().context.empty()
                       ? std::string()
                       : " context=" + interop::replyValue(command.error().context)),
              true);
        return true;
    }
    if (const auto started = run(*command); !started) {
        reply(interop::formatError(command->verb, started.error()), true);
    }
    return true;
}

Status OnlineDataWorkbench::run(const interop::OnlineCommand& command)
{
    switch (command.verb) {
    case interop::OnlineVerb::Providers:
        reply(interop::formatProviders(catalogue_.filter(command.filter)));
        return {};
    case interop::OnlineVerb::Layers: {
        const interop::OnlineProvider* provider = catalogue_.findProvider(command.provider);
        if (provider == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "no provider " + command.provider + "; ONLINE PROVIDERS lists them");
        }
        const auto layers = provider->layers();
        const bool search = !command.filter.empty() && !layers.empty() &&
                            layers.front()->type == interop::OnlineServiceType::Ckan;
        if (!search) {
            reply(interop::formatLayers(*provider));
            return {};
        }
        // A catalogue search is a request, so it runs off the GUI thread
        // like every other.
        const interop::OnlineLayer layer = *layers.front();
        const std::string words = command.filter;
        const interop::OnlineEnvironment environment = this->environment();
        auto found = std::make_shared<std::vector<interop::CkanResource>>();
        JobRunner& runner = JobRunner::of(window_);
        const JobId id = runner.start(
            "Searching " + qs(layer.providerTitle),
            [layer, words, environment, found](JobControl& control) -> Result<JobRunner::Apply> {
                control.setStage("Searching");
                auto resources = interop::searchCatalogue(layer, words, environment, control.stopToken());
                if (!resources) {
                    return resources.error();
                }
                *found = std::move(*resources);
                return JobRunner::Apply([] {});
            },
            [this, found](const JobReport& report) {
                if (report.outcome == JobOutcome::Finished) {
                    reply(interop::formatCatalogueSearch(*found));
                } else {
                    reply(errorLine(interop::OnlineVerb::Layers,
                                    report.outcome == JobOutcome::Cancelled ? "cancelled" : report.error),
                          true);
                }
            });
        if (services_.headless()) {
            runner.waitFor(id);
        }
        return {};
    }
    case interop::OnlineVerb::Info: {
        const interop::OnlineProvider* provider = catalogue_.findProvider(command.provider);
        const interop::OnlineLayer* layer = provider != nullptr ? provider->findLayer(command.layer) : nullptr;
        if (layer == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "no layer " + command.provider + " " + command.layer +
                                 "; ONLINE LAYERS <provider> lists them");
        }
        reply(interop::formatInfo(*layer));
        return {};
    }
    case interop::OnlineVerb::Key: {
        saveKey(command.keyName, command.keyValue);
        // The key itself is never repeated, not even here.
        reply("key name=" + interop::replyValue(command.keyName) +
              (command.keyValue.empty() ? std::string(" stored=no")
                                        : " stored=yes length=" + std::to_string(command.keyValue.size())) +
              " where=settings\n");
        if (dialog_ != nullptr) {
            dialog_->refreshCatalogue();
        }
        return {};
    }
    case interop::OnlineVerb::Custom:
        return startDiscovery(command.url);
    case interop::OnlineVerb::Import:
        return startImport(command);
    }
    return {};
}

Status OnlineDataWorkbench::startDiscovery(const std::string& url)
{
    const interop::OnlineEnvironment environment = this->environment();
    auto found = std::make_shared<interop::OnlineProvider>();
    JobRunner& runner = JobRunner::of(window_);
    const JobId id = runner.start(
        "Asking " + qs(katana::gis::redactUrl(url)),
        [url, environment, found](JobControl& control) -> Result<JobRunner::Apply> {
            control.setStage("Reading the service's description");
            auto provider = interop::discoverOnline(url, environment, control.stopToken());
            if (!provider) {
                return provider.error();
            }
            *found = std::move(*provider);
            return JobRunner::Apply([] {});
        },
        [this, found](const JobReport& report) {
            if (report.outcome != JobOutcome::Finished) {
                const std::string why = report.outcome == JobOutcome::Cancelled ? "cancelled" : report.error;
                reply(errorLine(interop::OnlineVerb::Custom, why), true);
                if (dialog_ != nullptr) {
                    dialog_->setStatus(qs(why), true);
                }
                return;
            }
            // Written to the user catalogue, merged by id, so the service is
            // there next time - and a key it needs goes in the settings.
            const std::filesystem::path path = userCataloguePath();
            interop::OnlineCatalogue user;
            std::error_code error;
            if (std::filesystem::is_regular_file(path, error)) {
                auto parsed = interop::parseCatalogue(readAll(path), true);
                if (!parsed) {
                    // Writing now would replace the person's catalogue with
                    // this one provider.
                    reply(errorLine(interop::OnlineVerb::Custom,
                                    "the user catalogue " + path.string() +
                                        " could not be read, so nothing was added to it: " +
                                        parsed.error().describe()),
                          true);
                    return;
                }
                user = std::move(*parsed);
            }
            // A key in a discovered address (a WMTS template's ?api=, a
            // token) goes to the settings, and the address keeps {key}, so
            // the catalogue on disk never holds it.
            for (interop::OnlineService& service : found->services) {
                for (interop::OnlineLayer& layer : service.layers) {
                    auto [endpoint, secret] = interop::extractKey(layer.endpoint);
                    if (!secret.empty()) {
                        layer.endpoint = endpoint;
                        layer.keyRequired = true;
                        layer.keyName = found->id;
                        saveKey(found->id, secret);
                    }
                }
            }
            interop::OnlineCatalogue added;
            added.providers.push_back(*found);
            user = interop::mergeCatalogues(std::move(user), added);
            std::filesystem::create_directories(path.parent_path(), error);
            const std::filesystem::path partial = path.string() + ".part";
            {
                std::ofstream out(partial, std::ios::binary | std::ios::trunc);
                out << interop::catalogueToJson(user);
                if (!out) {
                    reply(errorLine(interop::OnlineVerb::Custom,
                                    "could not write the user catalogue " + path.string()),
                          true);
                    return;
                }
            }
            std::filesystem::rename(partial, path, error);
            if (error) {
                reply(errorLine(interop::OnlineVerb::Custom,
                                "could not write the user catalogue " + path.string()),
                      true);
                return;
            }
            reloadCatalogue();
            reply(interop::formatLayers(*found));
            if (dialog_ != nullptr) {
                dialog_->setStatus("Added " + qs(found->title) + " under Custom.");
                if (!found->layers().empty()) {
                    dialog_->selectLayer(found->id, found->layers().front()->id);
                }
            }
        });
    if (services_.headless()) {
        runner.waitFor(id);
    }
    return {};
}

Status OnlineDataWorkbench::startImport(const interop::OnlineCommand& command)
{
    const interop::OnlineProvider* provider = catalogue_.findProvider(command.provider);
    const interop::OnlineLayer* found = provider != nullptr ? provider->findLayer(command.layer) : nullptr;
    if (found == nullptr) {
        return makeError(ErrorCode::NotFound, "no layer " + command.provider + " " + command.layer +
                                                  "; ONLINE LAYERS <provider> lists them");
    }
    if (found->kind == interop::OnlineLayerKind::Catalogue) {
        return makeError(ErrorCode::Unsupported,
                         "a catalogue is searched, not imported: ONLINE LAYERS " + found->providerId +
                             " <words>");
    }
    // The project's CRS, or the one given for a project that has none -
    // which then becomes the project's, so the next import agrees with this
    // one.
    std::string crs = projectCrs();
    // Set on the project only when the import succeeds (in its Apply): a
    // failed or cancelled import changes nothing.
    std::string crsToSet;
    if (!command.crs.empty()) {
        auto wkt = katana::gis::crsToWkt(command.crs);
        if (!wkt) {
            return wkt.error();
        }
        if (crs.empty()) {
            crs = command.crs;
            crsToSet = command.crs;
        } else {
            // The same system however it is spelt: epsg:7856 is EPSG:7856.
            const auto given = katana::gis::crsEpsgCode(command.crs);
            const auto project = katana::gis::crsEpsgCode(crs);
            if (!given || !project || *given != *project) {
                return makeError(ErrorCode::InvalidCRS,
                                 "the project is in " + crs + "; crs= is for a project that has none");
            }
        }
    }
    if (crs.empty()) {
        return makeError(ErrorCode::InvalidCRS,
                         "the project has no coordinate system, so web data has nowhere to go; "
                         "add crs=EPSG:<code> (for Sydney in GDA2020, crs=EPSG:7856)");
    }
    auto area = areaOf(command);
    if (!area) {
        return area.error();
    }

    interop::OnlineRequestOptions options;
    options.area = *area;
    options.areaCrs = command.area == interop::OnlineAreaKind::Box && command.boxIsLonLat ? "EPSG:4326" : crs;
    options.targetCrs = crs;
    options.resolution = command.resolution;
    options.targetLayer = command.targetLayer;
    options.fromDate = command.fromDate;
    options.toDate = command.toDate;
    options.maxCloud = command.maxCloud;
    options.filter = command.tag;
    options.time = command.time;
    const std::string targetLayer = options.targetLayer.empty()
                                        ? "online/" + found->providerId + "/" + found->id
                                        : options.targetLayer;

    // Everything the job reads is copied here, on the GUI thread.
    const interop::OnlineLayer layer = *found;
    const interop::OnlineEnvironment environment = this->environment();
    auto result = std::make_shared<interop::OnlineImport>();
    JobRunner& runner = JobRunner::of(window_);
    const JobId id = runner.start(
        "Online: " + qs(layer.providerTitle + " - " + layer.title),
        [this, layer, options, environment, result, targetLayer, crsToSet](JobControl& control)
            -> Result<JobRunner::Apply> {
            (void)interop::pruneCache(environment.cacheDirectory, kCacheDays, kCacheBytes);
            auto fetched = interop::fetchOnlineLayer(
                layer, options, environment, control.stopToken(),
                [&control](double fraction, const std::string& stage) {
                    if (fraction >= 0.0) {
                        control.setProgress(fraction);
                    }
                    control.setStage(stage);
                });
            if (!fetched) {
                return fetched.error();
            }
            *result = std::move(*fetched);
            return JobRunner::Apply([this, layer, result, targetLayer, crsToSet] {
                if (!crsToSet.empty() && projectCrs().empty()) {
                    katana::storage::ProjectMetadata metadata = services_.document->metadata();
                    metadata.coordinateSystem = crsToSet;
                    services_.document->setMetadata(std::move(metadata));
                    reply("project coordinate_system=" + interop::replyValue(crsToSet) + " set=yes\n");
                }
                std::size_t added = 0;
                if (result->raster) {
                    // The pixels go to the reference data; the reply needs only
                    // the name, the size and the id, which are kept.
                    const std::string name = result->raster->name;
                    const katana::interop::ReferenceId reference =
                        services_.addRaster(std::move(*result->raster));
                    result->raster->name = name;
                    result->raster->id = reference;
                } else if (result->vectors) {
                    interop::VectorImportResult& vectors = *result->vectors;
                    auto transaction = std::make_unique<cmd::Transaction>("ONLINE IMPORT");
                    for (const std::string& name : vectors.layersNeeded) {
                        if (!services_.document->model().layers.contains(name)) {
                            katana::entity::Layer created;
                            created.name = name;
                            transaction->add(cmd::createLayer(created));
                        }
                    }
                    added = vectors.entities.size();
                    if (added != 0) {
                        transaction->add(cmd::createEntities(std::move(vectors.entities)));
                    }
                    // One command, one Ctrl+Z; an import of nothing adds nothing.
                    if (transaction->size() != 0) {
                        if (const auto status = services_.document->execute(std::move(transaction)); !status) {
                            reply(interop::formatError(interop::OnlineVerb::Import, status.error()), true);
                            return;
                        }
                    }
                }
                reply(interop::formatImport(layer, *result, added, targetLayer));
            });
        },
        [this](const JobReport& report) {
            if (report.outcome == JobOutcome::Cancelled) {
                reply(errorLine(interop::OnlineVerb::Import, "cancelled"), true);
            } else if (report.outcome == JobOutcome::Failed) {
                reply(errorLine(interop::OnlineVerb::Import, report.error), true);
            }
            if (dialog_ != nullptr) {
                dialog_->setStatus(report.outcome == JobOutcome::Finished
                                       ? "Imported - the command log has what and from whom."
                                       : qs(report.outcome == JobOutcome::Cancelled ? "Cancelled."
                                                                                    : report.error),
                                   report.outcome != JobOutcome::Finished);
            }
        });
    if (services_.headless()) {
        // Nobody can press Cancel: a deadline does it instead.
        const int seconds = command.timeoutSeconds.value_or(kHeadlessDeadlineSeconds);
        QTimer deadline;
        deadline.setSingleShot(true);
        QObject::connect(&deadline, &QTimer::timeout, &window_, [&runner, id, this, seconds] {
            if (runner.cancel(id)) {
                reply(errorLine(interop::OnlineVerb::Import,
                                "no answer within " + std::to_string(seconds) + " s; cancelled"),
                      true);
            }
        });
        deadline.start(seconds * 1000);
        runner.waitFor(id);
    }
    return {};
}

} // namespace katana::qt
