#pragma once

// GIS > Online - Web Services: the online data workbench (docs/gis_online.md).
//
// Everything online the window does belongs to this class, which MainWindow
// only constructs, hands a GIS menu section and a few callbacks
// (OnlineServices), and asks to run any line that starts with ONLINE. That
// keeps the work out of main_window.cpp - which other work changes at the same
// time - and gives the dialog and the verbs ONE executor: the dialog builds an
// interop::OnlineCommand and calls run(), the command line parses one and
// calls run(), and run() is the only place an import is started, applied and
// reported.
//
// Threading follows src/katana_qt/jobs.hpp: the download, the warp and the
// reading run in a background job on inputs copied on the GUI thread (the
// layer, the area already in coordinates, the keys); the job returns an Apply
// step that runs on the GUI thread, where the entities are added as ONE
// command (so one Ctrl+Z removes an import) or the raster is added to the
// reference data. Cancelling the job from the status bar stops the transfer;
// nothing is applied. A headless session (--command) blocks on the job, with
// a deadline (timeout=, 600 s by default) after which the job is cancelled.
//
// Settings, never the project: keys under online/keys/<name> in Katana's
// QSettings, the user catalogue as online_sources.json in the application's
// configuration folder, the cache in its cache folder (KATANA_ONLINE_CACHE
// overrides it, for tests and for a shared cache).

#include <QPointer>
#include <QString>

#include <filesystem>
#include <functional>
#include <memory>
#include <optional>
#include <string>

#include "icons.hpp"
#include "katana/core/error.hpp"
#include "katana/interop/online_catalogue.hpp"
#include "katana/interop/online_fetch.hpp"
#include "katana/interop/online_verbs.hpp"
#include "katana/interop/reference_data.hpp"

class QAction;
class QKeySequence;
class QMainWindow;
class QMenu;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class OnlineDataDialog;
class ViewWorkspace;

struct OnlineServices {
    katana::cad::Document* document = nullptr;
    ViewWorkspace* views = nullptr;
    std::function<QAction*(Icon icon, const QString& text, const QString& tip,
                           const QKeySequence& shortcut, const QString& objectName)>
        makeAction;
    std::function<void(const QString& text, bool isError)> log;
    std::function<bool()> headless;
    // Adds a raster to the window's reference data and shows it; returns its id.
    std::function<katana::interop::ReferenceId(katana::interop::RasterOverlay)> addRaster;
    // The version the User-Agent names.
    std::string version;
    // Opens File > Project Coordinate System, suggesting systems for a
    // longitude and latitude when there is one; true when the project's
    // coordinate system changed.
    std::function<bool(std::optional<std::pair<double, double>> place)> chooseProjectCrs;
};

class OnlineDataWorkbench {
  public:
    // Adds the "Online - Web Services" section and its action (onlineData)
    // to `gisMenu`.
    OnlineDataWorkbench(QMainWindow& window, OnlineServices services, QMenu& gisMenu);
    ~OnlineDataWorkbench();

    OnlineDataWorkbench(const OnlineDataWorkbench&) = delete;
    OnlineDataWorkbench& operator=(const OnlineDataWorkbench&) = delete;

    [[nodiscard]] QAction* action() const { return action_; }

    // True when `line` is an ONLINE verb, which it then runs, replying to
    // the log; false leaves the line to whoever asked.
    bool runLine(const QString& line);
    // `line` as the command log may show it: an ONLINE KEY's value replaced
    // by ***, everything else as typed.
    [[nodiscard]] static QString loggedLine(const QString& line);
    // Runs one command. An Import starts a background job (and in a headless
    // session waits for it); the others answer at once. Fails only when the
    // command cannot start; how it ended is logged.
    katana::core::Status run(const katana::interop::OnlineCommand& command);

    [[nodiscard]] const katana::interop::OnlineCatalogue& catalogue() const { return catalogue_; }
    // The built-in catalogue with the user's merged over it, read again.
    void reloadCatalogue();

    // The dialog, made the first time it is asked for.
    OnlineDataDialog& dialog();

  private:
    [[nodiscard]] katana::interop::OnlineEnvironment environment() const;
    [[nodiscard]] std::filesystem::path userCataloguePath() const;
    [[nodiscard]] std::filesystem::path cacheDirectory() const;
    [[nodiscard]] std::string projectCrs() const;
    [[nodiscard]] katana::core::Result<katana::gis::CrsBox>
    areaOf(const katana::interop::OnlineCommand& command) const;
    katana::core::Status startImport(const katana::interop::OnlineCommand& command);
    katana::core::Status startDiscovery(const std::string& url);
    void reply(const std::string& text, bool isError = false) const;
    void saveKey(const std::string& name, const std::string& value);
    [[nodiscard]] bool hasKey(const std::string& name) const;

    QMainWindow& window_;
    OnlineServices services_;
    QAction* action_ = nullptr;
    katana::interop::OnlineCatalogue catalogue_;
    QPointer<OnlineDataDialog> dialog_;
};

} // namespace katana::qt
