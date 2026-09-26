#pragma once

// The frame every GIS analysis and check dialog is built on (GIS > Analysis -
// GDAL, GIS > Check - GDAL; docs/geoprocessing.md "V1" to "V5"): Buffer,
// Dissolve, Overlay, Boundary Around Features, Clip to Boundary, Query with
// SQL, Check Geometry, Repair Geometry, Gaps and Overlaps.
//
// Such a dialog runs nothing of GDAL's itself. Its fields describe a GIS line
// - shown, exactly as it will run, in <d>Command - and Run hands that line to
// the window's one executor (MainWindow::runVerbLine, the CommandRunner), as
// if it had been typed: echoed in the command log, kept in the history, and
// one undo step. So there is nothing the dialog does that an agent cannot do
// by typing the same line into katana_cli or katana_mcp. Preview runs the
// same line with PREVIEW, which changes nothing and says what the scope took.
//
// The window runs a GIS line as a background job (geo_workbench.hpp): typed
// interactively, the runner answers `job id=<n> ... state=started` at once,
// and the dialog shows the job's reply when the workbench says it ended.
// Headless (--dialog ... --press <d>Run) the job is waited for, and the reply
// is there when Run returns.
//
// What the drawing's data is taken from is the shared "Apply to" and "Only
// those that match" controls (ScopeFilterWidget, CLAUDE.md section 1.1),
// named <d>Scope, <d>ScopeSelection, <d>ScopeView, <d>ScopeLayers,
// <d>ScopeDrawing, <d>View, <d>OnScreen, <d>Layers, <d>Sublayers,
// <d>Type<Point|Line|...>, <d>FilterLayer, <d>FilterStyle, <d>FilterColour,
// <d>FilterProperty, <d>FilterValue, <d>FilterText, <d>DrawnOnly.
//
// Every dialog has, besides its own fields (each dialog's header lists them):
//   <d>Dialog    the dialog
//   <d>Command   the line Run will run (read-only)
//   <d>Preview   run it with PREVIEW: what the scope takes, nothing changed
//   <d>Run       run it
//   <d>Status    what happened last
//   <d>Reply     the reply records, or why the line was refused (read-only)
//   <d>Close     close

#include <QDialog>
#include <QPointer>
#include <QString>

#include <functional>
#include <memory>
#include <vector>

#include "command_runner.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "icons.hpp"
#include "jobs.hpp"
#include "katana/core/error.hpp"

class QFormLayout;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QShowEvent;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class DocumentWatcher;
class GeoMenus;
class GeoWorkbench;

// What a GIS dialog asks of the window; the menu table supplies the window's,
// a test its own.
struct GisDialogContext {
    // The window's one executor.
    CommandRunner run;
    // The drawing whose layers the scope offers. May be null (a test's).
    katana::cad::Document* document = nullptr;
    // The workspace's open views, for the scope's View choice. May be unset.
    std::function<std::vector<ScopeFilterView>()> views;
    std::function<bool()> headless;
};

// The scope words a form carries: ScopeFilterWidget::verbWords, or why the
// controls say none.
struct GisScopeWords {
    QString words;
    QString error;
};

// A word as the command line reads it: double-quoted when it holds a blank.
// InvalidArgument naming `field` for a double quote, which no line can carry.
[[nodiscard]] katana::core::Result<QString> gisWord(const QString& text, const QString& field);
// The scope words, or InvalidArgument saying why there are none.
[[nodiscard]] katana::core::Result<QString> gisScope(const GisScopeWords& scope);
// " key=<number>" for a number given, nothing for a blank; InvalidArgument
// naming `field` for one that does not read.
[[nodiscard]] katana::core::Result<QString> gisNumberOption(const QString& key, const QString& text,
                                                            const QString& field);
// " TO LAYER <layer>" for a layer given, nothing for a blank (the verb's
// default applies).
[[nodiscard]] katana::core::Result<QString> gisLayerTarget(const QString& layer);

class GisToolDialog : public QDialog {
  public:
    // `name` is the prefix of every object name ("gisBuffer"); `scoped`
    // says whether the dialog reads the drawing through the scope controls.
    GisToolDialog(const QString& name, const QString& title, GisDialogContext context,
                  QWidget* parent, bool scoped = true);
    ~GisToolDialog() override;
    GisToolDialog(const GisToolDialog&) = delete;
    GisToolDialog& operator=(const GisToolDialog&) = delete;

    // The line Run would run, or why there is none.
    [[nodiscard]] virtual katana::core::Result<QString> command() const = 0;

    // What Run does: the line handed to the executor; its reply - at once
    // headless, when its job ends interactively - into <d>Reply. A line that
    // cannot be written runs nothing, and <d>Status says why.
    void run();
    // Run with PREVIEW: nothing changes.
    void preview();
    // The workbench's word that a job ended; the dialog shows it when it is
    // the job its last line started.
    void jobFinished(JobId id, const VerbOutcome& outcome);

    // The scope controls; null for a dialog that takes no scope.
    [[nodiscard]] ScopeFilterWidget* scopeControls() const { return scope_; }
    // Refills the scope's layers and views, keeping what is chosen.
    virtual void reload();
    [[nodiscard]] QString replyText() const;
    [[nodiscard]] QString statusText() const;

  protected:
    void showEvent(QShowEvent* event) override;

    // Where a dialog adds its own fields.
    [[nodiscard]] QFormLayout& fields() const { return *fields_; }
    // A line edit named <name> with a placeholder and a tip, which refreshes
    // the command as it is typed in.
    [[nodiscard]] QLineEdit* addField(const QString& name, const QString& label,
                                      const QString& placeholder, const QString& tip);
    // The scope words as the controls say them.
    [[nodiscard]] GisScopeWords scopeWords() const;
    // Shows the line the fields make now, or why there is none. Called
    // whenever a field changes.
    void refreshCommand();
    // What a reply says, in a sentence for <d>Status; a dialog may say more.
    [[nodiscard]] virtual QString summary(const QString& reply) const;
    [[nodiscard]] const GisDialogContext& context() const { return context_; }
    [[nodiscard]] bool documentAlive() const;

  private:
    void runLine(const QString& line);
    void showOutcome(const VerbOutcome& outcome);
    void setStatus(const QString& text, bool isError);

    QString name_;
    GisDialogContext context_;
    ScopeFilterWidget* scope_ = nullptr;
    QFormLayout* fields_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QLabel* status_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    JobId pending_ = kNoJob;
    // Last, so it goes first: no delivery reaches a half-destroyed dialog.
    std::unique_ptr<DocumentWatcher> watcher_;
};

// Makes the GIS menu action `name` in `section`, which opens the dialog
// `make` builds - one per window, made the first time and kept - and tells it
// when the workbench's jobs end. The action's data names the dialog
// (<name>Dialog), which is how --dialog finds it.
using GisDialogMaker = std::function<GisToolDialog*(GisDialogContext context, QWidget* parent)>;
void addGisToolAction(GeoMenus& menus, GeoWorkbench& workbench, const QString& section,
                      Icon icon, const QString& text, const QString& tip, const QString& name,
                      GisDialogMaker make);

} // namespace katana::qt
