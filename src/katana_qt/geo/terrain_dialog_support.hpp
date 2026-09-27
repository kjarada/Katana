#pragma once

// What the terrain dialogs share (docs/terrain.md; docs/geoprocessing.md T0 to
// T3): Terrain > Surface From, GIS > Export Surface as DEM, Terrain >
// Analysis > Contours, Terrain Shading, and Slope and Aspect.
//
// Each dialog writes the line its fields describe - shown, as it will run, in
// its <d>Command field - and Run hands that line to the window's one executor
// (MainWindow::runVerbLine through GeoServices::run), exactly as if it had
// been typed. The geoprocessing executor runs it as a background job: headless
// the line waits for its job and the reply comes straight back; interactively
// it answers `job id=<n> ... state=started`, and the reply is shown when the
// job ends (GeoWorkbench::addFinishedListener). So there is nothing a dialog
// does that an agent cannot do by typing the same line.
//
// No moc, as the rest of the window: the pieces report through callbacks.

#include <QString>

#include <functional>
#include <memory>
#include <optional>
#include <vector>

#include "command_runner.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "geo/point_pick.hpp"
#include "jobs.hpp"

class QComboBox;
class QDialog;
class QLineEdit;
class QPlainTextEdit;
class QWidget;

namespace katana::cad {
class Document;
}
namespace katana::interop {
class ReferenceData;
}
namespace katana::terrain {
class SurfaceStore;
}

namespace katana::qt {

class GeoWorkbench;

struct TerrainDialogContext {
    // The window's one executor; a test hands its own.
    CommandRunner run;
    // True in a headless session, which opens no file dialog.
    std::function<bool()> headless;
    // What the source pickers list. May be null (a test's): the lists are
    // then empty.
    const katana::interop::ReferenceData* reference = nullptr;
    const katana::terrain::SurfaceStore* surfaces = nullptr;
    // The drawing a scope's layer list reads. May be null.
    katana::cad::Document* document = nullptr;
    // The workspace's open views, for a scope's View choice. May be unset.
    std::function<std::vector<ScopeFilterView>()> views;
    // Told when a job ends: the listener gets the job's id and what it said.
    // May be unset (a test's): a line that starts a job then shows only that
    // it started. A listener is never taken back: the dialogs are children
    // of the window, which destroys the workbench before its children, so a
    // dialog may not call into it as it goes (TerrainRun holds its state
    // weakly instead).
    std::function<void(std::function<void(JobId, const VerbOutcome&)>)> listen;
    // A point picked in a plan view (GeoServices::pickPoint). May be unset:
    // a dialog then offers no Pick, and the point is typed.
    PointPicker pickPoint;
};

// The context the window's geoprocessing workbench gives a terrain dialog.
[[nodiscard]] TerrainDialogContext terrainDialogContext(GeoWorkbench& workbench);

// What a source picker offers: a surface, a reference raster or a point
// cloud, and the words a line names it by ("SURFACE ground", "RASTER 3",
// "CLOUD 2").
struct TerrainSourceChoice {
    QString label; // "ground (surface)", "terrain.asc (raster 3)"
    QString words;
};

enum TerrainSourceKinds : unsigned { Surfaces = 1, Rasters = 2, Clouds = 4 };

[[nodiscard]] std::vector<TerrainSourceChoice> terrainSources(const TerrainDialogContext& context,
                                                              unsigned kinds);

// Refills `combo` with `choices`, keeping the one chosen by its words when it
// is still there. Each item's data is its words.
void fillSources(QComboBox& combo, const std::vector<TerrainSourceChoice>& choices);

// A word of a line: as it is, or double-quoted when it holds a blank. Empty
// when it holds a double quote, which no line can say.
[[nodiscard]] std::optional<QString> lineWord(const QString& text);

// A list of `count` numbers separated by commas, as a line writes them
// ("0,0,40,30"; 0: any count); each must read as a number, whole when
// `whole`, locale-independently.
[[nodiscard]] bool numberList(const QString& text, int count, bool whole);

// The fields every terrain dialog has: the line it will run, read-only, and
// what the line said, in a fixed-width font.
[[nodiscard]] QLineEdit* terrainCommandField(QWidget* parent, const QString& name);
[[nodiscard]] QPlainTextEdit* terrainReplyField(QWidget* parent, const QString& name);

// The dialog `name` under the workbench's window, made by `make` the first
// time - a child of the window, so --dialog finds it by name - then shown and
// raised. What a Terrain > Analysis item does.
QDialog& showTerrainDialog(GeoWorkbench& workbench, const QString& name,
                           const std::function<QDialog*(TerrainDialogContext, QWidget*)>& make);

// Runs a dialog's line and shows what it said in `reply`: at once when it
// answered (a refusal, a PREVIEW, a headless run), or when its job ends -
// "Running ..." until then. What the listener reaches is held weakly, so a
// job ending after the dialog has gone finds nothing to tell.
class TerrainRun {
  public:
    TerrainRun(TerrainDialogContext& context, QPlainTextEdit& reply);
    ~TerrainRun();
    TerrainRun(const TerrainRun&) = delete;
    TerrainRun& operator=(const TerrainRun&) = delete;

    // The outcome the executor gave back at once.
    VerbOutcome run(const QString& line);
    // Called with what the line said in the end: at once for a line that
    // answered at once, else when its job ends.
    std::function<void(const VerbOutcome&)> done;

  private:
    struct Waiting;
    void show(const VerbOutcome& outcome);

    TerrainDialogContext& context_;
    QPlainTextEdit& reply_;
    std::shared_ptr<Waiting> waiting_;
};

} // namespace katana::qt
