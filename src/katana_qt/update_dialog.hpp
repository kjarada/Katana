#pragma once

// Help > Check for Updates: what the update system knows, in two sentences, and
// the buttons that fit it (docs/desktop.md, "Help > Check for Updates").
//
// THE DIALOG DOES NONE OF THE WORK and knows nothing of the update engine. It
// shows an UpdateView - plain data its owner sets (UpdateWorkbench in the
// window, a test's own) - and every button either builds the UPDATE line a
// person would type (src/katana_app/update_verbs.hpp) and hands it to the
// window's one executor (command_runner.hpp), or calls one callback of its
// context. What the line did comes back as the next view, never as a guess of
// the dialog's: a press changes no text here by itself.
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
//                be offered."   [Check now]
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
//                the running job has finished, then update and reopen." and
//                Restart and update is disabled.
//   Blocked      "Close these first: katana_mcp.exe (4412)." / "Then press
//                Retry."  [Retry]
//   RolledBack   what the last apply ended as, by `lastStatus`
//                (update::LastResult): "rolled_back": "Katana 0.3.0 could not
//                be installed, so 0.2.0 was restored." / "Reason: <reason>.";
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

#include <cstdint>
#include <functional>
#include <optional>
#include <utility>

#include <QDialog>
#include <QString>

#include "command_runner.hpp"

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
    bool waitingForJobs = false;   // Ready
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

// A person's words for a reason token of the update engine, to stand after a
// colon and before a full stop: "offline" is "this computer is offline, or the
// release server did not answer"; "http_503" is "the release server answered
// with error 503"; a token it does not know is given back as it came, which is
// still better than nothing said. The table is in update_dialog.cpp, a row a
// token of include/katana/update/check.hpp and apply.hpp.
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
