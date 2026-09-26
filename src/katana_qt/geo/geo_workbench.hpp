#pragma once

// The geoprocessing verbs in the window (docs/desktop.md, "Geoprocessing
// jobs"; docs/geoprocessing.md): GDAL and the families after it, run by the
// ONE executor katana_cli and katana_mcp run (src/katana_app/geo/geo_verbs.hpp).
//
// Built as the online and utility workbenches are: MainWindow constructs it,
// hands it the window's drawing, interpreter, reference rasters, surfaces and
// a few callbacks (GeoServices), and asks it to run any line the executor
// handles - typed, or a dialog's line through the window's one executor
// (MainWindow::runVerbLine).
//
// A line is prepared on the GUI thread. What answers at once - GDAL LIST,
// HELP, VERSION, a PREVIEW, a refusal - is logged there. Anything that runs is
// a background job (jobs.hpp): the algorithm on its own thread with progress
// and Cancel in the status bar, and the result applied on the GUI thread as
// one undo step. A headless run (--command, --run-line, a script) waits for
// the job, so its outcome is logged, and testable, before the next line; an
// interactive one logs `job id=<n> title="..." state=started` and goes on.
// When the job ends, `finished` is told, which is how a dialog learns what its
// line did.

#include <QString>

#include <filesystem>
#include <functional>
#include <map>
#include <memory>

#include "command_runner.hpp"
#include "geo/geo_verbs.hpp"
#include "geo/point_pick.hpp"
#include "icons.hpp"
#include "jobs.hpp"
#include "katana/geometry/primitives2d.hpp"

class QAction;
class QKeySequence;
class QMainWindow;
class QMenu;

namespace katana::qt {

class ViewWorkspace;

struct GeoServices {
    katana::cad::Document* document = nullptr;
    katana::cad::CommandInterpreter* interpreter = nullptr;
    katana::interop::ReferenceData* reference = nullptr;
    katana::terrain::SurfaceStore* surfaces = nullptr;
    // Derived rasters of a drawing with no project.
    std::filesystem::path scratch;
    // The command log. isError also counts it as a refusal of the line.
    std::function<void(const QString& text, bool isError)> log;
    std::function<bool()> headless;
    // Frame what an apply added in the plan views; may be empty.
    std::function<void(const katana::geometry::Box2&)> frame;
    // The reference rasters or the surfaces changed: redraw them.
    std::function<void()> changed;
    // The window's one executor, for the dialogs the lanes add.
    CommandRunner run;
    // A job ended: its id and what it logged. May be empty.
    std::function<void(JobId, const VerbOutcome&)> finished;
    // The workspace's views, for a dialog's scope and filter controls (the
    // View choice). May be null.
    ViewWorkspace* views = nullptr;
    // A point picked in a plan view, for a dialog's line (point_pick.hpp):
    // the sample points, a viewshed's observers, a sight line's ends. Left
    // empty, the workbench picks through `views` (planPointPicker); with no
    // views either, it stays empty and the dialogs offer no Pick.
    PointPicker pickPoint;
    std::function<QAction*(Icon icon, const QString& text, const QString& tip,
                           const QKeySequence& shortcut, const QString& objectName)>
        makeAction;
};

class GeoWorkbench {
  public:
    GeoWorkbench(QMainWindow& window, GeoServices services);
    ~GeoWorkbench();
    GeoWorkbench(const GeoWorkbench&) = delete;
    GeoWorkbench& operator=(const GeoWorkbench&) = delete;

    // True when `line` is the executor's, which it then runs: answered in the
    // log, or started as a job. False leaves the line to whoever asked.
    bool runLine(const QString& line);

    // The job the last line started; kNoJob when it started none.
    [[nodiscard]] JobId lastJob() const { return lastJob_; }

    // Told when any of the workbench's jobs ends, after `finished`. For a
    // dialog, which listens while it is open; the returned key stops it.
    int addFinishedListener(std::function<void(JobId, const VerbOutcome&)> listener);
    void removeFinishedListener(int key);

    [[nodiscard]] katana::app::geo::Context& context() { return context_; }
    [[nodiscard]] const GeoServices& services() const { return services_; }
    // The window the packages' dialogs are children of, so --dialog finds
    // them by object name.
    [[nodiscard]] QMainWindow& window() const { return window_; }

  private:
    void notify(JobId id, const VerbOutcome& outcome);

    QMainWindow& window_;
    GeoServices services_;
    katana::app::geo::Context context_;
    JobId lastJob_ = kNoJob;
    std::map<int, std::function<void(JobId, const VerbOutcome&)>> listeners_;
    int nextListener_ = 1;
};

// The menus the geoprocessing packages add their items to, made the first
// time an item is added (menu_table.cpp), so a menu shows no heading with
// nothing under it:
//   GIS      "Processing - GDAL", "Analysis - GDAL", "Check - GDAL" sections
//   Terrain  "Analysis" (terrainAnalysisMenu) and "DEM" (terrainDemMenu)
class GeoMenus {
  public:
    GeoMenus(QMenu& gis, QMenu& terrain) : gis_(gis), terrain_(terrain) {}

    // Adds `action` under the GIS menu's section `title`, made at the end of
    // the menu the first time.
    void addToGis(const QString& title, QAction* action);
    // Adds `action` to the Terrain submenu `title` (object name `name`).
    void addToTerrain(const QString& title, const QString& name, QAction* action);

  private:
    QMenu& gis_;
    QMenu& terrain_;
    std::map<QString, QAction*> gisSections_; // title -> its last action
    std::map<QString, QMenu*> terrainMenus_;
};

// Every package's menu items, from one table with a block per package
// (menu_table.cpp).
void buildGeoMenus(GeoMenus& menus, GeoWorkbench& workbench);

} // namespace katana::qt
