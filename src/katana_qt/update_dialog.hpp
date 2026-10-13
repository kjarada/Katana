#pragma once

// Help > Check for Updates: what the update system knows, in two sentences, and
// the buttons that fit it (docs/desktop.md, "Help > Check for Updates").
//
// THE DIALOG DOES NONE OF THE WORK and reads nothing of the update engine. It
// shows an UpdateView - plain data its owner sets (UpdateWorkbench in the
// window, a test's own) - and every button either builds the UPDATE line a
// person would type (src/katana_app/update_verbs.hpp) and hands it to the
// window's one executor (command_runner.hpp), or calls one callback of its
// context. What the line did comes back as the next view, never as a guess of
// the dialog's: a press changes no text here by itself.
//
// WHAT TURNS THE ENGINE'S RECORDS INTO A VIEW is beside it, at the end of this
// file ("THE DECISIONS"): functions of their arguments, with no window, no
// disk and no clock, which the owner calls and the widget tests drive with
// records written by hand.
//
// Non-modal, kept by its owner, and no button is a default.
//
// THE STATES, each with the first sentence in updateStateText, the rest in
// updateDetailText, and exactly these buttons shown (the others are hidden,
// which the headless --press refuses):
//
//   Idle         "Katana 0.2.0" / "Not checked for updates yet."
//                [Check now]. Before the first answer, and what a headless
//                run shows: it checks only when asked.
//   Checking     "Checking for updates..." / "You can keep working."  [Cancel]
//   Current      "Katana 0.2.0 is up to date." / "Last checked 14:02."
//                [Check now]
//   NotOffered   a newer release this copy does not offer, by `reason`:
//                "skipped": "Katana 0.3.0 is available, and you chose to skip
//                it." / "Settings > Updates offers it again."
//                "blocked_before": "Katana 0.3.0 could not be installed here
//                before, and is not offered again." / "A later release will
//                be offered, and Settings > Updates offers this one again."
//                (its button settingsUpdateUnblock runs UPDATE BLOCKED CLEAR)
//                [Check now]
//   Available    "Update Available" / "Katana 0.3.0 (you have 0.2.0). 23 of
//                411 files changed, 71 MB to download."
//                [Update] [Download only] [Skip this version] [Release notes]
//                [Later]. With `informationIsOld` the detail adds "The update
//                information is old: a newer release may exist." With
//                `notifyOnly` (a copy that cannot replace its own files: the
//                macOS bundle, Linux until its apply is written) the detail's
//                second sentence is "This copy cannot update itself: the
//                release notes say where to download it." and Update and
//                Download only are hidden.
//   Downloading  "Downloading 31 of 71 MB (12 of 23 files)." / "You can keep
//                working."  the progress bar, [Cancel]. The files are said
//                only when the owner knows how many are done.
//   Ready        "Katana 0.3.0 is ready." / "Katana will close, update and
//                reopen in a few seconds."  [Restart and update] [Later].
//                With `waitingForJobs` the detail is "Katana will close when
//                the running job has finished, then update and reopen."; with
//                `waitingForDialog`, "Katana will close when the dialog that
//                is open has been closed, then update and reopen."; and
//                Restart and update is disabled in both: the press is made.
//   Blocked      "Close these first: katana_mcp.exe (4412)." / "Then press
//                Retry."  [Retry]
//   RolledBack   what the last apply ended as, by `lastStatus`
//                (update::LastResult): "rolled_back": "Katana 0.3.0 could not
//                be installed, so 0.2.0 was restored." / "Reason: <reason>." -
//                and, when the reason is that the new version did not start
//                (health_exit, health_timeout), which is when the helper sets
//                the release aside, "A later release will be offered, and
//                Settings > Updates offers this one again." after it;
//                "refused": "Katana 0.3.0 could not be installed; nothing was
//                changed."; "rollback_incomplete": "Katana 0.3.0 could not be
//                installed, and 0.2.0 could not be put back completely." with
//                "Close Katana and run: <recoverCommand>" after the reason.
//                No button: it is read and closed.
//   CheckFailed  "Could not check for updates: <reason>."  [Retry]; for the
//                reason http_404, "No update information found." / "If the
//                releases are private, add an access token in Settings >
//                Updates."
//   Unsupported  "Updates are not available for this copy of Katana:
//                <reason>."  No button.
//
// <reason> is a person's words for the engine's token (updateReasonWords); a
// megabyte is a million bytes, rounded to the nearest (updateMegabytes).
//
// WHAT EACH BUTTON DOES:
//   updateCheckNow       the line UPDATE CHECK
//   updateRetry          CheckFailed: UPDATE CHECK; Blocked: UPDATE APPLY CONFIRM
//   updateInstall        context.update: download, then apply, then close - one
//                        click, which is more than one line
//   updateDownloadOnly   the line UPDATE DOWNLOAD CONFIRM
//   updateSkip           the line UPDATE SKIP <latest version>
//   updateNotes          context.openNotes with the release's notes URL
//   updateRestart        the line UPDATE APPLY CONFIRM
//   updateCancel         context.cancel: the running check or download
//   updateLater          context.later, then the dialog is hidden
// A button whose line has no executor, or whose callback is empty, is
// disabled. A line the executor refused is said under the two sentences, in
// the error colour (updateProblemText), as is UpdateView::problem - what the
// owner could not do at the last press; both go with the next view.
//
// Object names: updateDialog; updateStateText, updateDetailText,
// updateProblemText (hidden while empty), updateProgress (shown while checking,
// busy, and while downloading), and the nine buttons above.
//
// LIFETIME. The dialog copies what it shows, so the view it was given may go.
// Its callbacks are its owner's: an owner that goes first calls detach(), after
// which every button but Later is disabled and nothing is called.

#include <chrono>
#include <cstdint>
#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <QDialog>
#include <QString>

#include "command_runner.hpp"
#include "katana/update/install.hpp"
#include "katana/update/state.hpp"
#include "katana/update/version.hpp"
#include "update_verbs.hpp"

class QLabel;
class QProgressBar;
class QPushButton;

namespace katana::qt {

enum class UpdateDialogState {
    Idle,
    Checking,
    Current,
    NotOffered,
    Available,
    Downloading,
    Ready,
    Blocked,
    RolledBack,
    CheckFailed,
    Unsupported,
};

// What the dialog shows. Every member has a default; a state reads only the
// members its sentences name (the list above).
struct UpdateView {
    UpdateDialogState state = UpdateDialogState::Idle;
    QString installedVersion; // "0.2.0"
    QString latestVersion;    // "0.3.0": the release offered, staged, blocked or rolled back
    QString lastChecked;      // Current: the time as shown, "14:02"; empty leaves the sentence out
    // Available: how many of the install's files the release changes. Downloading: the
    // changed files are the "23" of "12 of 23 files".
    std::uint64_t changedFiles = 0;
    std::uint64_t totalFiles = 0;
    // Available: what Update would fetch. Downloading: the same, with what has arrived.
    std::uint64_t downloadBytes = 0;
    std::uint64_t downloadedBytes = 0;
    // Downloading: the files staged so far, when the download says. Unset: bytes alone.
    std::optional<std::uint64_t> downloadedFiles;
    QString notesUrl;
    // The engine's token (check.hpp, apply.hpp): CheckFailed, Unsupported, NotOffered,
    // RolledBack.
    QString reason;
    QString holders;        // Blocked: "katana_mcp.exe (4412)"
    QString lastStatus;     // RolledBack: "rolled_back", "refused" or "rollback_incomplete"
    QString recoverCommand; // RolledBack, rollback_incomplete: the command to run by hand
    // One sentence: what the owner could not do at the last press. Shown in
    // updateProblemText in any state; empty hides it.
    QString problem;
    bool informationIsOld = false; // Available
    bool notifyOnly = false;       // Available
    bool waitingForJobs = false;   // Ready: the close waits for a running job
    bool waitingForDialog = false; // Ready: the close waits for a dialog that is open
};

// What the dialog is given by whoever owns it. Every member may be empty, and
// a button with nothing to call is disabled, so a test builds the dialog alone.
struct UpdateDialogContext {
    // The window's one executor: the lines the buttons build go here.
    CommandRunner run{};
    // Update: download, apply and close, in one click.
    std::function<void()> update{};
    // Cancel: the check or the download that is running.
    std::function<void()> cancel{};
    // Release notes: the URL opened in the person's browser.
    std::function<void(const QString& url)> openNotes{};
    // Later: told before the dialog hides (an owner waiting to close stops waiting).
    std::function<void()> later{};
};

// The lines the buttons build, for the owner that must know them again when
// they come back through the executor.
inline constexpr const char* kUpdateCheckLine = "UPDATE CHECK";
inline constexpr const char* kUpdateDownloadLine = "UPDATE DOWNLOAD CONFIRM";
inline constexpr const char* kUpdateApplyLine = "UPDATE APPLY CONFIRM";
// "UPDATE SKIP 0.3.0"
[[nodiscard]] QString updateSkipLine(const QString& version);

// `line` as the command log may show it. There is no UPDATE line that carries
// an access token - the verbs have STATUS and CLEAR under TOKEN and nothing
// else - but a person may type one that does, expecting it to work (UPDATE
// TOKEN ghp_..., UPDATE TOKEN SET ghp_...), and a typed line is echoed in the
// log before any verb sees it. So every word after UPDATE TOKEN that is not
// STATUS or CLEAR is shown as "***"; the verbs are still handed the line as it
// was typed, and refuse it. Any other line comes back unchanged.
[[nodiscard]] QString updateLoggedLine(const QString& line);

// A person's words for a reason token of the update engine, to stand after a
// colon and before a full stop: "offline" is "this computer is offline, or the
// release server did not answer"; "http_503" is "the release server answered
// with error 503"; a token it does not know is given back as it came, which is
// still better than nothing said. The table is in update_dialog.cpp, a row a
// token of include/katana/update/check.hpp, feed.hpp (the transport's),
// stage.hpp (the download's and the preflight's), apply.hpp, and the verbs'
// own (src/katana_app/update_verbs.hpp). Where a person can do something about
// it the words say what: "elevated" is "Katana was started as administrator;
// start it the ordinary way, not as administrator, and update again - the
// download is kept".
[[nodiscard]] QString updateReasonWords(const QString& token);

// Bytes as whole megabytes of a million bytes, rounded to the nearest - "71"
// for 71 000 000 - and never "0" for a download that is not empty: one byte
// is "1".
[[nodiscard]] QString updateMegabytes(std::uint64_t bytes);

// The files a line of UPDATE DOWNLOAD's progress says are staged: the two
// numbers of a word `files=<done>/<all>`, wherever it stands in the line.
// nullopt for a line that has no such word, which leaves the files unsaid.
[[nodiscard]] std::optional<std::pair<std::uint64_t, std::uint64_t>>
updateFilesOfProgressLine(const QString& line);

// The two sentences of a view, as the dialog shows them: the owner logs the
// same words where a person is told outside the dialog.
[[nodiscard]] QString updateStateText(const UpdateView& view);
[[nodiscard]] QString updateDetailText(const UpdateView& view);

// ---- THE DECISIONS -----------------------------------------------------------------------------
//
// What the dialog's owner decides from the update engine's records: which view
// an answer is, what is offered when the window starts, when the window checks
// by itself, what the log may show, which helper copies a start may remove,
// whether a start confirms an update that was never said to start.
// They are here - beside the dialog, in the library the widget tests link -
// because their owner in the program (UpdateWorkbench) needs the whole window
// and cannot be built in a test. Each is a function of its arguments: the
// owner gathers the facts (the disk, the clock, the environment) and decides
// nothing of this itself.

// "0.3" and "0.3.0" are one release (update/version.hpp); text that is no
// version is the same only as itself.
[[nodiscard]] bool updateSameVersion(const QString& a, const QString& b);

// The value of `key` in a reply's records (`key=value`, blanks or line breaks
// between them) - of the LAST record that has it, since a download's reply
// ends with the record that says what is staged. Quotes around a value are
// taken off. Empty when no record of the reply is `key=...`.
[[nodiscard]] QString updateReplyValue(const QString& reply, const char* key);

// The version UPDATE DOWNLOAD CONFIRM's reply says is staged: the value of its
// last `staged=`, and empty for none at all and for `staged=none`.
[[nodiscard]] QString updateStagedOfReply(const QString& reply);

// What the verbs said back to `line`, as the command log may show it: every
// word of the line that updateLoggedLine hides is hidden in `reply` too, IN
// ANY CASE - a refusal may quote the line it refuses, and may have folded its
// case first. A reply to any other line comes back unchanged.
[[nodiscard]] QString updateLoggedReply(const QString& reply, const QString& line);

// The status bar's button for a view: "Update 0.3.0" while there is a release
// to act on - offered, on its way, downloaded, or waiting for other programs
// to close - and EMPTY when the button is hidden: in every other state, and
// for a view that names no version.
[[nodiscard]] QString updateStatusButtonText(const UpdateView& view);

// What the owner knows beside a check's answer.
struct UpdateOfferFacts {
    QString installedVersion;  // this copy's, for an answer that names none
    QString stagedVersion;     // the release downloaded and waiting; empty when none is
    QString checkedAt;         // the time as shown, "14:02"
    std::int64_t nowEpoch = 0; // UTC seconds, for "the update information is old"
    bool appliesHere = true;   // false where a copy cannot replace its own files
};

// What UPDATE CHECK's answer is shown as, by its status (update/check.hpp):
//   available    Available, with the files and the bytes - or Ready, when the
//                version offered is the one already downloaded. notifyOnly
//                for a notify-only manifest and where the copy cannot apply;
//                informationIsOld past the manifest's `expires`
//   current      Current with the time; NotOffered "skipped" when the reason
//                says the person skipped the release the feed has
//   blocked      NotOffered "blocked_before"
//   unreachable  CheckFailed with the reason
//   unsupported  Unsupported with the reason
[[nodiscard]] UpdateView updateViewOfOffer(const katana::app::UpdateOffer& offer,
                                           const UpdateOfferFacts& facts);

// What the AUTOMATIC check does with its answer, which nobody asked for.
enum class UpdateQuietAnswer {
    Nothing,  // it learnt nothing, or nothing that changes what is shown: no word anywhere
    Show,     // nothing to tell, something to show: the view changes and the log does not
    Announce, // a release to offer: logged as a typed check is, and shown
};
// `shown` is what the dialog shows now, `answer` the view of the check's
// answer (updateViewOfOffer, after update::applyUpdateState):
//   - a release to offer (Available, Ready) is announced - unless that very
//     version is already shown in that state, which is no news;
//   - a check that failed, or that this copy cannot make, says nothing;
//   - "up to date" and "not offered" are shown without a word - which is what
//     takes back a release the view still offers once the feed no longer has
//     it, or once it was skipped or rolled back here - except over a view
//     that must stay until it is read (RolledBack) or that a job owns
//     (Checking, Downloading).
[[nodiscard]] UpdateQuietAnswer updateQuietAnswer(const UpdateView& shown,
                                                  const UpdateView& answer);

// The downloaded release that is still one to install after a check answered
// `answer`: none once the feed has nothing above this copy (Current: the
// release was withdrawn), and none once the answer says that very version was
// skipped or rolled back here (NotOffered); otherwise `staged` as it was - a
// check that failed un-knows nothing.
[[nodiscard]] QString updateStagedAfter(const QString& staged, const UpdateView& answer);

// The release to offer as Ready when the window starts, from what UPDATE
// STATUS says is downloaded: its version, or empty when none is to be offered -
//   - nothing is staged, or what is staged is no version above `installed`
//     (the stage a finished update left behind);
//   - the person skipped that version (status.state.skippedVersion);
//   - a release that was ROLLED BACK HERE still has its stage. The apply keeps
//     the stage of a release it rolled back (update/apply.hpp) and STATUS
//     names what is staged by version alone, so the complete stage of a
//     blocked sequence in the update folder - which `stageIsThere(sequence)`
//     says - means what is staged is, or may be, the release that does not
//     start here. Offered as ready, it would be installed again.
[[nodiscard]] QString
updateStagedToOffer(const katana::app::UpdateStatus& status,
                    const katana::update::Version& installed,
                    const std::function<bool(std::uint64_t sequence)>& stageIsThere);

// How the last apply ended (U/last.json, update::LastResult) as the dialog
// shows it at the next start. nullopt for "committed": a success is one line
// in the log, not a dialog. "blocked" is the Blocked state with the programs
// to close; every other status is RolledBack with that status and the reason,
// and "rollback_incomplete" with the command that puts the install right by
// hand, which names `installRoot` (the folder as the platform writes it).
[[nodiscard]] std::optional<UpdateView>
updateViewOfLastResult(const katana::update::LastResult& last, const QString& installedVersion,
                       const QString& installRoot);

// What the window's automatic check depends on beside the state file.
struct UpdateAutoCheckFacts {
    bool headless = false;   // a headless run checks nothing by itself
    bool disabled = false;   // KATANA_UPDATE_DISABLE is set
    bool jobRunning = false; // an update job is running already
    katana::update::InstallKind kind = katana::update::InstallKind::Development;
    bool keepsState = false; // the session has a per-user update folder
};

// Whether the window checks by itself now. Only when ALL of these hold: the
// run is not headless; KATANA_UPDATE_DISABLE is not set; no update job runs;
// the copy is one packaging installed (a development tree stays offline); the
// session keeps per-user state (with none there is no saying when the last
// check was); and update::checkIsDue says so for `state` - its automatic flag
// on, and the last check 24 hours and `jitter` ago, or never, or in the future.
[[nodiscard]] bool updateAutomaticCheckIsDue(const UpdateAutoCheckFacts& facts,
                                             const katana::update::UpdateState& state,
                                             std::int64_t nowEpoch, std::chrono::seconds jitter);

// What a window knows when it asks whether its own start settles an update
// that was installed and never said to start (below).
struct UpdateConfirmFacts {
    bool appliesHere = false; // false where a copy cannot replace its own files
    bool headless = false;    // a headless run writes no per-user state
    bool keepsState = false;  // the session has a per-user update folder
    katana::update::InstallKind kind = katana::update::InstallKind::Development;
    // The install has a record of an update that committed (installed.json),
    // and the version that record names.
    bool hasInstalledRecord = false;
    katana::update::Version installed;
    // The version this program was built as (update::programVersion).
    katana::update::Version program;
};

// Whether this window, having reached its first idle turn, is the proof an
// update's health run was to give - so that it may settle an update whose
// helper died while it waited for that run (update::confirmStarted,
// include/katana/update/recover.hpp). Only when ALL of these hold: the copy
// can replace its own files; the run is not headless (the confirmation writes
// per-user state); the session has a per-user update folder; the copy is one
// packaging installed; and the install has a record of its own whose version
// is this program's own. The last is what tells the new version from a window
// of the OLD files that was started while the files were being swapped: that
// one runs old code over the new tree, its record names a version it is not,
// and its start proves nothing about the release.
[[nodiscard]] bool updateStartMayConfirm(const UpdateConfirmFacts& facts);

// The transactions whose helper copy U/run-<tx> the window's start lists for
// removal (update/state.hpp, cleanup.list). A swap that ends lists its own
// copy. One that a crash ended, or whose rollback failed, is put right later
// by `katana_updater --recover`, which writes nothing per user
// (update/recover.hpp), so nothing in U names its copy. The INSTALL names it:
// every run of --recover that found something appends a line
//   <UTC time> recover found=<tx,...> result=ok|failed left=<tx,...>
// to R/.update/recovery.log. `recoveryLog` is that file's text, and the answer
// is each id of a `found=` list, once, in the order first named, for which
// `isOver(id)` holds - the install has no folder R/.update/<id> any more. A
// transaction that a recovery found and whose folder is gone is over, rolled
// back or healthy, and nothing starts its helper again. One whose folder is
// still there keeps its copy: a rollback that failed runs from it at the next
// try. `isOver` is asked once for an id however often it is named, and with
// none given nothing is known to be over.
//
// WHY THE INSTALL'S LOG AND NOT THE NAMES IN U. U is one folder for every copy
// of Katana a person runs, and a `run-<tx>` with no transaction folder in THIS
// install is also what UPDATE APPLY CONFIRM has just made, here or in another
// install, for a helper that has not started yet. Only an id that this
// install's own recovery named is known to be this install's, and begun.
// (What defeats it is a COPY of an install folder made while an update in it
// was half done: the copy carries the transaction with it, and putting the
// copy right removes the helper copy the original would have run. The
// original then runs its own bin/katana_updater, as update::recover does
// whenever that copy is gone.)
//
// THE LOG IS NOT TRUSTED: a process of the same user can write R/.update. A
// line counts only when it is whole - ended by its line feed, so a torn last
// write names nothing - and has the form above: its second word `recover`, its
// third `found=...`, its fourth `result=...`. The same file quotes journal
// records a rollback refused (`<tx> journal record refused: ...`), whose text
// is anybody's and therefore names nothing. And an id counts only when
// update::isValidTransactionId says it is one, so it names no place but
// U/run-<id>.
[[nodiscard]] std::vector<std::string>
updateRecoveredTransactions(std::string_view recoveryLog,
                            const std::function<bool(const std::string& transaction)>& isOver);

class UpdateDialog final : public QDialog {
  public:
    explicit UpdateDialog(UpdateDialogContext context, QWidget* parent = nullptr);
    ~UpdateDialog() override;

    UpdateDialog(const UpdateDialog&) = delete;
    UpdateDialog& operator=(const UpdateDialog&) = delete;

    // Shows `view`: its sentences, its buttons, its progress. The dialog keeps
    // a copy. Whether the dialog is on screen is the owner's to say (show()).
    void setView(UpdateView view);
    [[nodiscard]] const UpdateView& view() const { return view_; }

    // Drops the callbacks: what an owner that goes before the dialog does.
    void detach();

  private:
    void refresh();
    // `line` through the executor; a refusal is said in updateProblemText.
    void runLine(const QString& line);

    UpdateDialogContext context_;
    UpdateView view_;
    // What the executor refused at the last press; cleared by the next view.
    QString refusal_;

    QLabel* state_ = nullptr;
    QLabel* detail_ = nullptr;
    QLabel* problem_ = nullptr;
    QProgressBar* progress_ = nullptr;
    QPushButton* checkNow_ = nullptr;
    QPushButton* retry_ = nullptr;
    QPushButton* install_ = nullptr;
    QPushButton* downloadOnly_ = nullptr;
    QPushButton* skip_ = nullptr;
    QPushButton* notes_ = nullptr;
    QPushButton* restart_ = nullptr;
    QPushButton* cancel_ = nullptr;
    QPushButton* later_ = nullptr;
};

} // namespace katana::qt
