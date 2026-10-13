#pragma once

// The update system in the window (docs/desktop.md, "Help > Check for
// Updates"): what only a window has beside the UPDATE verbs it shares with
// katana_cli (src/katana_app/update_verbs.hpp).
//
// Built as the geoprocessing and utility workbenches are: MainWindow makes it,
// hands it a few callbacks (UpdateServices) and asks it to run any line whose
// verb is UPDATE - typed, or a dialog's through the window's one executor
// (MainWindow::runVerbLine). It adds:
//   - Help > "Check for Updates..." (helpCheckForUpdates, whose data names
//     updateDialog) and the status bar's button (statusUpdateAvailable,
//     "Update 0.3.0"), hidden until a release is known; both open the dialog;
//   - the dialog (update_dialog.hpp), which it owns and feeds: every sentence
//     the dialog shows is an UpdateView this object made from a verb's reply;
//   - the verbs' context (app::UpdateContext): made once, at first use, for the
//     surface the window turned out to be - Window, or HeadlessWindow - with
//     this process's id and this program's path, which the helper waits for
//     and starts again;
//   - the lines that reach the network as BACKGROUND JOBS (jobs.hpp): UPDATE
//     CHECK, UPDATE DOWNLOAD CONFIRM and UPDATE APPLY CONFIRM (whose preflight
//     runs the helper's self-test) are run by app::runUpdateLine on a worker,
//     with a copy of the context, and their reply is logged on the interface
//     thread when they end, as a typed line's is. The other lines (STATUS,
//     SKIP, BLOCKED, SET, TOKEN) read and write small files and run at once;
//   - the AUTOMATIC CHECK (start(), then once an hour to see whether one is
//     due): only in a window a person uses - never headless - of a copy that
//     packaging installed (not a development tree), with the state's
//     automatic flag on, when update::checkIsDue says so and
//     KATANA_UPDATE_DISABLE is not set. It is quiet when it learns nothing, and
//     a release the person skipped, or that was rolled back here, is not
//     offered (update::applyUpdateState) - and is taken back if the window
//     still offered it. A person who asks for a check while it is on its way
//     is given that one, aloud;
//   - ONE CLICK (the dialog's Update): the DOWNLOAD line, then the APPLY line,
//     then the window's ordinary close - its unsaved-changes question
//     included. Cancel there, and the window stays, the release stays staged
//     and the dialog says Ready. A close that would cut a running job short
//     waits for the job, and one that would close the program around a modal
//     dialog the person opened meanwhile waits for that. ONLY when the close
//     this object asked for succeeded does takePendingApply() hand main() the
//     request to start the helper with, after the event loop has ended;
//   - what the last apply ended as (U/last.json), shown once at the next start
//     and then cleared; and a release downloaded in an earlier session offered
//     again as ready - unless it was skipped since, or rolled back here;
//   - THE SWEEP at a start, on Windows, where an update can have run: what a
//     lock kept an earlier update from deleting (U/cleanup.list), and with it
//     the helper's copy U/run-<tx> of each update of THIS install that needed
//     a recovery and is over - which only the install's own
//     R/.update/recovery.log names, since a recovery writes nothing per user;
//   - THE CONFIRMATION at a start's first idle turn, of an update that was
//     installed and never said to start: its helper died - power, a shutdown,
//     the process ended - while it waited for the new version to prove that it
//     starts, so nothing recorded that it does. A window of that very version
//     which has come idle IS that proof, and settles the update
//     (update::confirmStarted, include/katana/update/recover.hpp). While a
//     helper is alive - it holds the update lock, and settles its own update -
//     the window asks again, every five seconds for a minute, and then leaves
//     it to the next start or the next logon.
//
// WHAT IT DECIDES IS NOT HERE. Which view an answer is, what a line that was
// run at once does to what is shown, what is offered at a start, when the
// automatic check is due, what its answer does to what is shown, what the log
// may show, what Settings > Updates is told of the state file, which helper
// copies a start may remove and whether a start may confirm an update are
// functions of their arguments beside the dialog (update_dialog.hpp, "THE
// DECISIONS"), where the widget tests drive them: this object needs the whole
// window and is built in no test. It gathers the facts - the disk, the clock,
// the environment, the job runner - and does what they say.
//
// A HEADLESS RUN (docs/headless.md): no automatic check, no per-user state
// read or written (the context has no state folder), nothing shown unless
// --dialog asks for the dialog. A typed UPDATE CHECK waits for its job, so its
// reply is logged before the next step; DOWNLOAD and APPLY are handed to the
// verbs as they are, which refuse them for this surface (reason=headless).

#include <QPointer>
#include <QString>
#include <QStringList>

#include <chrono>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <optional>

#include "command_runner.hpp"
#include "customisation/settings_dialog.hpp"
#include "icons.hpp"
#include "jobs.hpp"
#include "katana/core/error.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/feed.hpp"
#include "update_dialog.hpp"
#include "update_verbs.hpp"

class QAction;
class QKeySequence;
class QObject;
class QTimer;
class QToolButton;

namespace katana::qt {

class MainWindow;

struct UpdateServices {
    // The window's action factory, so the Help item looks and reads like
    // every other: icon, status tip, tooltip.
    std::function<QAction*(Icon icon, const QString& text, const QString& tip,
                           const QKeySequence& shortcut, const QString& objectName)>
        makeAction;
    // The command log. isError also counts it as a refusal of the line.
    std::function<void(const QString& text, bool isError)> log;
    // Asked each time: the window learns it after it is built.
    std::function<bool()> headless;
};

class UpdateWorkbench {
  public:
    // Makes the action and the status-bar button; the window puts them in
    // Help and in its status bar.
    UpdateWorkbench(MainWindow& window, UpdateServices services);
    // Deletes the dialog, which calls back into this object.
    ~UpdateWorkbench();

    UpdateWorkbench(const UpdateWorkbench&) = delete;
    UpdateWorkbench& operator=(const UpdateWorkbench&) = delete;

    [[nodiscard]] QAction* action() const { return action_; }
    [[nodiscard]] QToolButton* statusButton() const { return statusButton_; }

    // True when `line` is an UPDATE line, which it then runs: answered in the
    // log at once, or started as a job. False leaves the line to whoever asked.
    bool runLine(const QString& line);

    // What an interactive window does about updates as it starts, once, after
    // it is shown: the leftovers of an earlier update removed - the helper's
    // copy of one that a recovery put right among them - the last apply's
    // result shown and cleared, a release already downloaded offered again,
    // and, once the window has come idle, an update that was never said to
    // start confirmed and the automatic check when one is due. Nothing in a
    // headless run, and nothing for a development copy, which shares the
    // per-user update folder with an installed copy and must leave it alone.
    void start();

    // The dialog, made the first time it is asked for.
    UpdateDialog& dialog();
    // Shows the dialog; with nothing known yet it also says so, or asks: a
    // development copy is told it cannot update, an interactive window runs
    // UPDATE CHECK, a headless one waits to be asked.
    void open();

    // What Settings > Updates shows and writes to, as this session has it.
    [[nodiscard]] SettingsUpdates settingsUpdates();

    // The request UPDATE APPLY CONFIRM returned - once, and only when the
    // close this object asked the window for succeeded. main() calls it after
    // the event loop has ended and starts the helper with it.
    [[nodiscard]] std::optional<katana::update::ApplyRequest> takePendingApply();

  private:
    enum class JobKind { None, Check, Download, Apply };
    // What a job's worker and this object share (update_workbench.cpp).
    struct Shared;

    [[nodiscard]] bool headless() const;
    void log(const QString& text, bool isError = false) const;
    // The verbs' context, made at first use.
    [[nodiscard]] const katana::app::UpdateContext& context();
    // The version this copy is, as the dialog names it.
    [[nodiscard]] QString installedVersion();
    [[nodiscard]] UpdateView viewOf(UpdateDialogState state);
    [[nodiscard]] UpdateView readyView();
    // What a check's answer is shown as.
    [[nodiscard]] UpdateView viewOfOffer(const katana::app::UpdateOffer& offer);
    // Makes `view` what the dialog shows and the status-bar button follows.
    void show(UpdateView view);

    // A line that answers at once, on the interface thread.
    void runNow(const QString& line);
    // The reply, or the refusal, in the log. `line` is the line it answers,
    // when that was typed by a person: what stands in it where someone hoped
    // UPDATE TOKEN would take a token is shown as "***" wherever the verbs'
    // words repeat it, in any case (updateLoggedReply).
    void logReply(const katana::core::Result<katana::app::UpdateReply>& reply,
                  const QString& line = {}) const;
    // False, said in the log, while another update job runs.
    [[nodiscard]] bool mayStart(const QString& line);
    void startCheck(bool automatic);
    void startDownload(bool thenApply);
    void startApply();
    void beginJob(JobKind kind, const QString& title, const char* line);
    void finished(JobKind kind, Shared& shared, const JobReport& report);
    void checkFinished(const katana::core::Result<katana::app::UpdateReply>& reply,
                       bool automatic);
    void downloadFinished(const katana::core::Result<katana::app::UpdateReply>& reply,
                          bool thenApply);
    void applyFinished(const katana::core::Result<katana::app::UpdateReply>& reply);
    // The download's bytes and files into the dialog, a few times a second.
    void showProgress();

    // The dialog's Update, Cancel, Release notes and Later.
    void installUpdate();
    void cancelJob();
    void openNotes(const QString& url);
    void later();

    // A request UPDATE APPLY CONFIRM prepared and nobody will start - Later,
    // or Cancel at the window's question: the helper's copy made for it is
    // listed for the next start's sweep.
    void dropPrepared(const katana::update::ApplyRequest& request);
    // The window's close for a prepared apply, once no job would be cut short
    // and no modal dialog is open over the window.
    void closeWhenIdle();
    // The automatic check, when every condition for one holds.
    void checkIfDue();
    // Settles an update of this install that was installed and never said to
    // start, when this window is the version it installed; asks again a few
    // times while a live helper holds the update lock.
    void confirmStart();
    void showLastResult();
    void restoreStaged();

    MainWindow& window_;
    UpdateServices services_;
    // The receiver of every connection and timer here, so that none outlives
    // this object: declared before what uses it, destroyed after.
    std::unique_ptr<QObject> scope_;
    QAction* action_ = nullptr;
    // A child of the window (its status bar's, once it is put there), which
    // outlives this object as the window's other children do.
    QToolButton* statusButton_ = nullptr;
    QPointer<UpdateDialog> dialog_;

    std::optional<katana::app::UpdateContext> context_;
    // The route the last check's answer came by (update/feed.hpp): put into
    // every context made after it, so the download asks the way the check did.
    std::optional<katana::update::FeedRoute> route_;
    // What the dialog shows, kept here so that it is there before the dialog is.
    UpdateView view_;
    // What it showed when the running job started: where a cancelled or
    // failed job goes back to.
    UpdateView viewBeforeJob_;
    // The release the last check offered, for the download's sentence.
    std::optional<katana::app::UpdateOffer> offer_;
    // The version that is downloaded and waiting, empty when none is.
    QString stagedVersion_;

    JobKind running_ = JobKind::None;
    JobId job_ = kNoJob;
    QString jobTitle_;
    std::shared_ptr<Shared> shared_;
    bool automaticCheck_ = false;     // the running check is the automatic one
    bool applyAfterDownload_ = false; // the next DOWNLOAD line is the one click's
    bool thenApply_ = false;          // the running download is followed by APPLY
    QTimer* progressTimer_ = nullptr; // children of scope_
    QTimer* dueTimer_ = nullptr;
    QTimer* waitTimer_ = nullptr;
    // Drawn once for the process (update/state.hpp, checkIsDue).
    std::chrono::seconds jitter_{0};
    bool started_ = false;
    // How often confirmStart has asked again because the update lock was held.
    int confirmRetries_ = 0;

    std::optional<katana::update::ApplyRequest> pendingApply_;
    bool closedForApply_ = false;
    // Gone with this object: what a callback handed to another owner (the
    // Settings dialog's) asks before it touches this one.
    std::shared_ptr<int> alive_ = std::make_shared<int>(0);
};

} // namespace katana::qt
