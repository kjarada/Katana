#pragma once

// What the geoprocessing dialogs share (docs/geoprocessing.md, "The window";
// CLAUDE.md section 1): the Grid Points to DEM dialog, the DEM tools and the
// GDAL Toolbox. None of them does the work: each builds the verb line its
// fields describe - shown, as it will run, in its Command field - and Run
// hands that line to the window's one executor (GeoServices::run, which is
// MainWindow::runVerbLine) exactly as if it had been typed. So it is logged,
// kept in the history and undone like a typed line, and the line the dialog
// shows is the line an agent types.
//
// A geoprocessing line runs as a background job. Headless the executor waits
// for it, so the runner's outcome is the reply; interactively the runner
// answers `job id=<n> ... state=started` at once and the reply arrives when
// the job ends (GeoWorkbench::addFinishedListener). GeoRunPanel hides the
// difference: it shows "running" and then the reply, whichever way it came.

#include <QDialog>
#include <QString>
#include <QWidget>

#include <functional>
#include <map>
#include <memory>
#include <optional>
#include <vector>

#include "command_runner.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "jobs.hpp"
#include "katana/core/error.hpp"

class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;

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

// What a geoprocessing dialog asks of whoever shows it. The window's comes
// from its GeoWorkbench (geoDialogContext); a test supplies its own - a stub
// runner is how a dialog is tested without a window.
struct GeoDialogContext {
    // The window's one executor.
    CommandRunner run;
    // Hears the end of every geoprocessing job, for as long as the returned
    // object lives. Unset: the runner's outcome is taken as the reply, as a
    // headless run's is.
    std::function<std::shared_ptr<void>(std::function<void(JobId, const VerbOutcome&)>)> listen;
    // A headless session opens no file dialog: Browse says to fill the field.
    std::function<bool()> headless;
    // What the pickers list: the drawing's layers and properties, the
    // reference rasters, the surfaces. Any may be null (a test's); the lists
    // are then empty. Read only while the window lives - a dialog outlives
    // none of them in use, since they are the window's.
    katana::cad::Document* document = nullptr;
    const katana::interop::ReferenceData* reference = nullptr;
    const katana::terrain::SurfaceStore* surfaces = nullptr;
    // The workspace's open views, for a scope's View choice. May be unset.
    std::function<std::vector<ScopeFilterView>()> views;
};

// The window's context for a dialog: its executor, its jobs, its drawing,
// reference rasters, surfaces and views.
[[nodiscard]] GeoDialogContext geoDialogContext(GeoWorkbench& workbench);

// `text` as one word of a line, as the command line reads it: double-quoted
// when it holds a blank. InvalidArgument naming `field` for a double quote,
// which no word of a line can carry, and for an empty word.
[[nodiscard]] katana::core::Result<QString> lineWord(const QString& text, const QString& field);

// The job a runner's reply says it started: `job id=<n> title="..."
// state=started`, the executor's answer to an interactive run.
[[nodiscard]] std::optional<JobId> startedJob(const QString& reply);

// A reply's lines that are records of `kind` ("output", "scope" ...): what a
// dialog reads its status from.
[[nodiscard]] QString firstRecord(const QString& reply, const QString& kind);

// The dialog, `name`, a child of `window` (how --dialog finds it), made by
// `make` the first time and kept: a dialog keeps its fields between uses.
// Shown, raised and activated.
template <class Dialog, class Make>
Dialog& showKeptDialog(QWidget& window, const QString& name, Make&& make)
{
    // No moc in the window, so the kept dialog is found as a QDialog.
    auto* dialog =
        dynamic_cast<Dialog*>(window.findChild<QDialog*>(name, Qt::FindDirectChildrenOnly));
    if (dialog == nullptr) {
        dialog = make();
    }
    dialog->show();
    dialog->raise();
    dialog->activateWindow();
    return *dialog;
}

// The Command, Preview, Run, status and Reply of a geoprocessing dialog, or
// of one tab of one. Its object names are the prefix given, then:
//   <prefix>Command   the exact line Run runs (read-only)
//   <prefix>Preview   run the line with PREVIEW: what it would take, checked
//                     by GDAL, nothing changed (absent when not offered)
//   <prefix>Run       run it
//   <prefix>Status    what happened last, one line
//   <prefix>Reply     the reply records, or why the line failed
class GeoRunPanel final : public QWidget {
  public:
    GeoRunPanel(const QString& prefix, GeoDialogContext context, bool preview,
                QWidget* parent = nullptr);
    ~GeoRunPanel() override;
    GeoRunPanel(const GeoRunPanel&) = delete;
    GeoRunPanel& operator=(const GeoRunPanel&) = delete;

    // The line the dialog's fields describe, or why there is none (which
    // names the field). Asked by refresh() and by Run.
    std::function<katana::core::Result<QString>()> line;
    // Told the outcome of every run, when its reply is in.
    std::function<void(const VerbOutcome& outcome)> onFinished;

    // Reads `line` again into the Command field; Run and Preview are enabled
    // only when there is a line.
    void refresh();
    // What Run (preview false) and Preview do: the line handed to the
    // executor and its reply shown when it comes. A line that cannot be
    // written runs nothing, and the status says why.
    void run(bool preview = false);

    // A job this panel started and has not heard the end of.
    [[nodiscard]] bool running() const { return pending_ != kNoJob; }
    [[nodiscard]] QString reply() const;
    [[nodiscard]] QString status() const;
    void setStatus(const QString& text, bool isError = false);

  private:
    void finish(const VerbOutcome& outcome);
    void heard(JobId id, const VerbOutcome& outcome);

    GeoDialogContext context_;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QLabel* status_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    JobId pending_ = kNoJob;
    // Jobs heard to end while their run was still being handed over: a
    // headless run's job ends inside the runner's call.
    std::map<JobId, VerbOutcome> early_;
    bool handing_ = false;
    std::shared_ptr<void> listening_;
};

} // namespace katana::qt
