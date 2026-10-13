// UpdateDialog (Help > Check for Updates) and the Updates page of Settings,
// driven by object name as a person - or the headless driver - drives them.
//
// Neither does any work: a button builds an UPDATE line and hands it to the
// runner it was given, or calls one callback. So every test gives a RECORDING
// runner and callbacks that count, and asserts on exactly what was handed over.
//
// WHERE THE EXPECTATIONS COME FROM. Every sentence is written out by hand from
// the update system's design, section 7 ("The three surfaces", Window), which
// gives each state its words with the example a release would show:
//
//   Current      "Katana 0.2.0 is up to date. Last checked 14:02."  [Check now]
//   Available    "Update Available" / "Katana 0.3.0 (you have 0.2.0). 23 of 411
//                files changed, 71 MB to download."  [Update] [Download only]
//                [Skip this version] [Release notes] [Later]
//   Downloading  "Downloading 31 of 71 MB (12 of 23 files). You can keep
//                working."  [Cancel]
//   Ready        "Katana 0.3.0 is ready. Katana will close, update and reopen in
//                a few seconds."  [Restart and update] [Later]
//   Blocked      "Close these first: katana_mcp.exe (4412). Then press Retry."
//   Rolled back  "Katana 0.3.0 could not be installed, so 0.2.0 was restored.
//                Reason: <reason>."
//   check failed "Could not check for updates: <reason>."  [Retry]; a private
//                feed's 404: "No update information found. If the releases are
//                private, add an access token in Settings > Updates."
//   Unsupported  "Updates are not available for this copy of Katana: <reason>."
//
// The lines are the verbs' grammar (src/katana_app/update_verbs.hpp): UPDATE
// CHECK, UPDATE DOWNLOAD CONFIRM, UPDATE APPLY CONFIRM, UPDATE SKIP <version>,
// UPDATE SKIP NONE, UPDATE BLOCKED CLEAR, UPDATE SET AUTO ON|OFF, UPDATE TOKEN
// CLEAR. The words for a reason token are the dialog's own table, of which the
// header of update_dialog.cpp says the rule (a row a token of the engine's
// check.hpp and apply.hpp, and of the verbs' header); the tokens listed here
// are copied from those headers.
//
// The numbers: 71 000 000 bytes is the design's "71 MB" and 31 000 000 its
// "31"; 31 / 71 of the progress bar's thousand steps is 436.6, so 437.
//
// THE DECISIONS (the UpdateDecisions tests, at the end of the first half): what
// the window's update workbench decides from the engine's records is a set of
// functions beside the dialog, because the workbench itself needs the whole
// window. Their expectations are the task's rules, written out case by case:
// a skipped or blocked release is not offered; the automatic check runs only
// when the install is not a development one, the automatic flag is on, a check
// is due (include/katana/update/state.hpp states when: every 24 hours plus the
// jitter, or never checked, or a last check in the future), the run is not
// headless and KATANA_UPDATE_DISABLE is not set, and is quiet when it learns
// nothing; the status bar's button reads "Update <version>" and is hidden
// until an update is known; a start confirms an update that was never said to
// start only in a window of the version it installed (the hardening pass's
// H1, 2026-10-14); a line that empties a list takes back what the dialog said
// that list set aside, and no other (H5); a state file that does not read is
// "nobody can say" and never "none" (H5: the verb refuses such a file and
// says why, which only a button that is on lets a person read); a build that
// was put back is told of there only while it can still be offered to this
// install - made after the build installed, or the one the dialog is telling
// of - and an apply that was refused over an earlier update that is not
// settled carries the command that settles it (both the pass's integration
// review); and an update that could not be started is said in a message box
// only in a window nobody drove (the same pass, for main()). The records are
// written by hand: the design's 0.2.0 and 0.3.0, its sequence 1792312800, its
// `expires` of 2027-04-18T08:00:00Z - which is 1 808 035 200 seconds after
// 1970 (20 926 days: 57 years of 365, 14 leap days, 31 + 28 + 31 + 17; and 8
// hours).
//
// The recovery log's lines (the last UpdateDecisions tests) are written by hand
// in the two forms the engine states: include/katana/update/recover.hpp for a
// run of `katana_updater --recover` - "<UTC time> recover found=<tx,...>
// result=ok|failed left=<tx,...>", and `detail=` after a failure - and
// src/katana_update/apply.cpp (reportRefusals) for a journal record a
// rollback would not follow, "<tx> journal record refused: <record>".
// A transaction id is 1 to 64 characters, each 0-9 or a-f
// (include/katana/update/apply.hpp, isValidTransactionId).

#include <gtest/gtest.h>

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <QAbstractButton>
#include <QCheckBox>
#include <QLabel>
#include <QLayout>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPoint>
#include <QProgressBar>
#include <QPushButton>
#include <QStackedWidget>
#include <QStringList>

#include "command_runner.hpp"
#include "customisation/settings_dialog.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/update/check.hpp"
#include "katana/update/credential.hpp"
#include "katana/update/install.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/state.hpp"
#include "katana/update/version.hpp"
#include "theme.hpp"
#include "update_dialog.hpp"
#include "update_verbs.hpp"
#include "widget_harness.hpp"

using katana::qt::SettingsContext;
using katana::qt::SettingsDialog;
using katana::qt::UpdateDialog;
using katana::qt::UpdateDialogContext;
using katana::qt::UpdateDialogState;
using katana::qt::UpdateView;
using katana::qt::VerbOutcome;

namespace {

VerbOutcome carriedOut() { return VerbOutcome{true, {}, {}}; }

template <typename T> T* child(const QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

// A click, as a person's or the headless driver's --press.
void press(const QWidget& dialog, const char* button)
{
    if (QAbstractButton* found = child<QAbstractButton>(dialog, button)) {
        found->click();
    }
}

QString shown(const QWidget& dialog, const char* label)
{
    const QLabel* found = child<QLabel>(dialog, label);
    return found != nullptr ? found->text() : QString();
}

bool enabled(const QWidget& dialog, const char* name)
{
    return child<QWidget>(dialog, name)->isEnabled();
}

// The buttons the dialog shows, by name, in the order of the alphabet: the
// ones a person can see once the dialog is on screen, whether it is yet or not.
QStringList shownButtons(const QWidget& dialog)
{
    QStringList names;
    for (const QPushButton* button : dialog.findChildren<QPushButton*>()) {
        if (button->isVisibleTo(&dialog)) {
            names << button->objectName();
        }
    }
    names.sort();
    return names;
}

// The two sentences as a person reads them, one after the other.
QString said(const QWidget& dialog)
{
    const QString state = shown(dialog, "updateStateText");
    const QString detail = shown(dialog, "updateDetailText");
    return detail.isEmpty() ? state : state + QLatin1Char(' ') + detail;
}

// Everything a press can reach, recorded.
struct DialogFixture {
    UpdateDialogContext context;
    QStringList ran; // every line handed to the executor, in order
    int updates = 0;
    int cancels = 0;
    int laters = 0;
    QStringList opened; // every URL handed to openNotes
    // What a line answers. Unset: that it was carried out.
    std::function<VerbOutcome(const QString& line)> answer;

    DialogFixture()
    {
        context.run = [this](const QString& line) {
            ran << line;
            return answer ? answer(line) : carriedOut();
        };
        context.update = [this] { ++updates; };
        context.cancel = [this] { ++cancels; };
        context.openNotes = [this](const QString& url) { opened << url; };
        context.later = [this] { ++laters; };
    }

    // What has been called so far, as one text to compare: the lines, then
    // each callback that was called with how often.
    [[nodiscard]] QString called() const
    {
        QStringList parts = ran;
        if (updates != 0) {
            parts << QStringLiteral("update x%1").arg(updates);
        }
        if (cancels != 0) {
            parts << QStringLiteral("cancel x%1").arg(cancels);
        }
        if (laters != 0) {
            parts << QStringLiteral("later x%1").arg(laters);
        }
        for (const QString& url : opened) {
            parts << QStringLiteral("notes ") + url;
        }
        return parts.join(QStringLiteral(" | "));
    }
};

// The design's examples: Katana 0.2.0 installed, 0.3.0 released.
UpdateView viewOf(UpdateDialogState state)
{
    UpdateView view;
    view.state = state;
    view.installedVersion = QStringLiteral("0.2.0");
    view.latestVersion = QStringLiteral("0.3.0");
    return view;
}

UpdateView available()
{
    UpdateView view = viewOf(UpdateDialogState::Available);
    view.changedFiles = 23;
    view.totalFiles = 411;
    view.downloadBytes = 71'000'000;
    view.notesUrl = QStringLiteral("https://github.com/kjarada/Katana/releases/tag/v0.3.0");
    return view;
}

UpdateView downloading()
{
    UpdateView view = viewOf(UpdateDialogState::Downloading);
    view.changedFiles = 23;
    view.totalFiles = 411;
    view.downloadBytes = 71'000'000;
    view.downloadedBytes = 31'000'000;
    view.downloadedFiles = 12;
    return view;
}

} // namespace

// ---- what it is ---------------------------------------------------------------------------------

TEST(UpdateDialog, ItIsANonModalDialogWithANameForEveryControl)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    EXPECT_EQ(dialog.objectName(), QStringLiteral("updateDialog"));
    EXPECT_EQ(dialog.windowTitle(), QStringLiteral("Check for Updates"));
    EXPECT_FALSE(dialog.isModal());

    for (const char* name : {"updateStateText", "updateDetailText", "updateProblemText"}) {
        EXPECT_NE(dialog.findChild<QLabel*>(QString::fromLatin1(name)), nullptr) << name;
    }
    EXPECT_NE(dialog.findChild<QProgressBar*>(QStringLiteral("updateProgress")), nullptr);
    // The nine buttons and the words on them, as the design names them.
    const std::vector<std::pair<const char*, const char*>> buttons{
        {"updateCheckNow", "Check now"},
        {"updateInstall", "Update"},
        {"updateDownloadOnly", "Download only"},
        {"updateSkip", "Skip this version"},
        {"updateNotes", "Release notes"},
        {"updateLater", "Later"},
        {"updateCancel", "Cancel"},
        {"updateRestart", "Restart and update"},
        {"updateRetry", "Retry"},
    };
    for (const auto& [name, words] : buttons) {
        const auto* button = dialog.findChild<QPushButton*>(QString::fromLatin1(name));
        ASSERT_NE(button, nullptr) << name;
        EXPECT_EQ(button->text(), QString::fromLatin1(words)) << name;
        EXPECT_FALSE(button->toolTip().isEmpty()) << name << " says what it does";
    }
    EXPECT_EQ(dialog.findChildren<QPushButton*>().size(), 9) << "and no other button";
    katana::qt::test::paint(dialog);
}

TEST(UpdateDialog, NoButtonIsADefaultSoEnterPressesNothing)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    dialog.setView(viewOf(UpdateDialogState::Ready));
    ASSERT_FALSE(dialog.findChildren<QPushButton*>().isEmpty());
    for (const QPushButton* button : dialog.findChildren<QPushButton*>()) {
        EXPECT_FALSE(button->isDefault()) << button->objectName().toStdString();
        EXPECT_FALSE(button->autoDefault()) << button->objectName().toStdString();
    }
}

// ---- every state shows exactly its sentences and its buttons -----------------------------------

TEST(UpdateDialog, BeforeAnythingIsKnownItSaysSoAndOffersACheck)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    // As made, and with the installed version given.
    EXPECT_EQ(said(dialog), QStringLiteral("Katana Not checked for updates yet."));
    dialog.setView(viewOf(UpdateDialogState::Idle));
    EXPECT_EQ(shown(dialog, "updateStateText"), QStringLiteral("Katana 0.2.0"));
    EXPECT_EQ(shown(dialog, "updateDetailText"), QStringLiteral("Not checked for updates yet."));
    EXPECT_EQ(shownButtons(dialog), (QStringList{QStringLiteral("updateCheckNow")}));
    EXPECT_FALSE(child<QProgressBar>(dialog, "updateProgress")->isVisibleTo(&dialog));
    EXPECT_FALSE(child<QLabel>(dialog, "updateProblemText")->isVisibleTo(&dialog));
}

TEST(UpdateDialog, WhileCheckingItSaysSoAndOffersOnlyCancel)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    dialog.setView(viewOf(UpdateDialogState::Checking));
    EXPECT_EQ(said(dialog), QStringLiteral("Checking for updates... You can keep working."));
    EXPECT_EQ(shownButtons(dialog), (QStringList{QStringLiteral("updateCancel")}));
    // Busy: a check cannot say how far it is.
    const auto* progress = child<QProgressBar>(dialog, "updateProgress");
    EXPECT_TRUE(progress->isVisibleTo(&dialog));
    EXPECT_EQ(progress->minimum(), 0);
    EXPECT_EQ(progress->maximum(), 0);
}

TEST(UpdateDialog, CurrentSaysTheVersionIsUpToDateAndWhenItWasChecked)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = viewOf(UpdateDialogState::Current);
    view.lastChecked = QStringLiteral("14:02");
    dialog.setView(view);
    EXPECT_EQ(said(dialog), QStringLiteral("Katana 0.2.0 is up to date. Last checked 14:02."));
    EXPECT_EQ(shown(dialog, "updateStateText"), QStringLiteral("Katana 0.2.0 is up to date."));
    EXPECT_EQ(shownButtons(dialog), (QStringList{QStringLiteral("updateCheckNow")}));

    // With no time to say, the sentence about it is left out, not left empty.
    view.lastChecked.clear();
    dialog.setView(view);
    EXPECT_EQ(said(dialog), QStringLiteral("Katana 0.2.0 is up to date."));
}

TEST(UpdateDialog, AvailableSaysWhatChangedAndHowMuchThereIsToDownload)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    dialog.setView(available());
    EXPECT_EQ(shown(dialog, "updateStateText"), QStringLiteral("Update Available"));
    EXPECT_EQ(shown(dialog, "updateDetailText"),
              QStringLiteral("Katana 0.3.0 (you have 0.2.0). 23 of 411 files changed, 71 MB to "
                             "download."));
    EXPECT_EQ(shownButtons(dialog),
              (QStringList{QStringLiteral("updateDownloadOnly"), QStringLiteral("updateInstall"),
                           QStringLiteral("updateLater"), QStringLiteral("updateNotes"),
                           QStringLiteral("updateSkip")}));
    EXPECT_FALSE(child<QProgressBar>(dialog, "updateProgress")->isVisibleTo(&dialog));
    for (const char* name : {"updateDownloadOnly", "updateInstall", "updateLater", "updateNotes",
                             "updateSkip"}) {
        EXPECT_TRUE(enabled(dialog, name)) << name;
    }
    katana::qt::test::paint(dialog);
}

TEST(UpdateDialog, OldUpdateInformationIsSaidAndTheUpdateIsStillOffered)
{
    // The manifest's `expires` is advisory (design section 1): the dialog says
    // the information is old, and refuses nothing.
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = available();
    view.informationIsOld = true;
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateDetailText"),
              QStringLiteral("Katana 0.3.0 (you have 0.2.0). 23 of 411 files changed, 71 MB to "
                             "download. The update information is old: a newer release may "
                             "exist."));
    EXPECT_TRUE(shownButtons(dialog).contains(QStringLiteral("updateInstall")));
    EXPECT_TRUE(enabled(dialog, "updateInstall"));
}

TEST(UpdateDialog, ACopyThatCannotUpdateItselfIsToldOfTheReleaseAndSentToItsNotes)
{
    // Check and notify (design section 6): nothing here can install it, so
    // neither button that would is offered.
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = available();
    view.notifyOnly = true;
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateStateText"), QStringLiteral("Update Available"));
    EXPECT_EQ(shown(dialog, "updateDetailText"),
              QStringLiteral("Katana 0.3.0 (you have 0.2.0). This copy cannot update itself: the "
                             "release notes say where to download it."));
    EXPECT_EQ(shownButtons(dialog),
              (QStringList{QStringLiteral("updateLater"), QStringLiteral("updateNotes"),
                           QStringLiteral("updateSkip")}));
}

TEST(UpdateDialog, DownloadingSaysHowFarItIsAndThatWorkCanGoOn)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    dialog.setView(downloading());
    EXPECT_EQ(said(dialog), QStringLiteral("Downloading 31 of 71 MB (12 of 23 files). You can "
                                           "keep working."));
    EXPECT_EQ(shownButtons(dialog), (QStringList{QStringLiteral("updateCancel")}));
    const auto* progress = child<QProgressBar>(dialog, "updateProgress");
    EXPECT_TRUE(progress->isVisibleTo(&dialog));
    EXPECT_EQ(progress->minimum(), 0);
    EXPECT_EQ(progress->maximum(), 1000);
    // 31 of 71 is 0.4366: 437 of the bar's thousand steps.
    EXPECT_EQ(progress->value(), 437);
}

TEST(UpdateDialog, ADownloadThatDoesNotSayItsFilesIsShownInBytesAlone)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = downloading();
    view.downloadedFiles.reset();
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateStateText"), QStringLiteral("Downloading 31 of 71 MB."));

    // Nothing has arrived yet; and all of it has, which is never said as more
    // than the whole, nor the files as more than there are.
    view.downloadedBytes = 0;
    view.downloadedFiles = 0;
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateStateText"),
              QStringLiteral("Downloading 0 of 71 MB (0 of 23 files)."));
    EXPECT_EQ(child<QProgressBar>(dialog, "updateProgress")->value(), 0);
    view.downloadedBytes = 80'000'000;
    view.downloadedFiles = 30;
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateStateText"),
              QStringLiteral("Downloading 71 of 71 MB (23 of 23 files)."));
    EXPECT_EQ(child<QProgressBar>(dialog, "updateProgress")->value(), 1000);

    // A download whose size nobody gave has no measure: the bar is busy.
    view.downloadBytes = 0;
    view.downloadedBytes = 0;
    dialog.setView(view);
    EXPECT_EQ(child<QProgressBar>(dialog, "updateProgress")->maximum(), 0);
}

TEST(UpdateDialog, ReadySaysKatanaWillCloseUpdateAndReopen)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    dialog.setView(viewOf(UpdateDialogState::Ready));
    EXPECT_EQ(said(dialog), QStringLiteral("Katana 0.3.0 is ready. Katana will close, update and "
                                           "reopen in a few seconds."));
    EXPECT_EQ(shownButtons(dialog),
              (QStringList{QStringLiteral("updateLater"), QStringLiteral("updateRestart")}));
    EXPECT_TRUE(enabled(dialog, "updateRestart"));
}

TEST(UpdateDialog, WhileTheWindowWaitsForAJobRestartIsOffAndTheDetailSaysWhatItWaitsFor)
{
    // "It waits while a job runs" (design section 7): the press is made, and
    // only Later, which takes it back, is left to press.
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = viewOf(UpdateDialogState::Ready);
    view.waitingForJobs = true;
    dialog.setView(view);
    EXPECT_EQ(said(dialog), QStringLiteral("Katana 0.3.0 is ready. Katana will close when the "
                                           "running job has finished, then update and reopen."));
    EXPECT_EQ(shownButtons(dialog),
              (QStringList{QStringLiteral("updateLater"), QStringLiteral("updateRestart")}));
    EXPECT_FALSE(enabled(dialog, "updateRestart"));
    EXPECT_TRUE(enabled(dialog, "updateLater"));
}

TEST(UpdateDialog, WhileTheWindowWaitsForAnOpenDialogItSaysThatAndRestartIsOff)
{
    // The close comes from a timer, not from a press: under a modal dialog
    // the person opened while the release was downloading it would close the
    // program around that dialog, so it waits for it - and says so.
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = viewOf(UpdateDialogState::Ready);
    view.waitingForDialog = true;
    dialog.setView(view);
    EXPECT_EQ(said(dialog), QStringLiteral("Katana 0.3.0 is ready. Katana will close when the "
                                           "dialog that is open has been closed, then update and "
                                           "reopen."));
    EXPECT_EQ(shownButtons(dialog),
              (QStringList{QStringLiteral("updateLater"), QStringLiteral("updateRestart")}));
    EXPECT_FALSE(enabled(dialog, "updateRestart"));
    EXPECT_TRUE(enabled(dialog, "updateLater"));

    // A job is what it waits for first, when there are both.
    view.waitingForJobs = true;
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateDetailText"),
              QStringLiteral("Katana will close when the running job has finished, then update "
                             "and reopen."));
}

TEST(UpdateDialog, BlockedNamesWhatToCloseAndOffersRetry)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = viewOf(UpdateDialogState::Blocked);
    view.holders = QStringLiteral("katana_mcp.exe (4412)");
    dialog.setView(view);
    EXPECT_EQ(said(dialog),
              QStringLiteral("Close these first: katana_mcp.exe (4412). Then press Retry."));
    EXPECT_EQ(shownButtons(dialog), (QStringList{QStringLiteral("updateRetry")}));

    // A record that names nobody still says what to do.
    view.holders.clear();
    dialog.setView(view);
    EXPECT_EQ(said(dialog), QStringLiteral("Close the other Katana programs that are open. Then "
                                           "press Retry."));
}

TEST(UpdateDialog, RolledBackSaysWhatWasRestoredAndWhy)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = viewOf(UpdateDialogState::RolledBack);
    view.lastStatus = QStringLiteral("rolled_back");
    view.reason = QStringLiteral("health_exit");
    dialog.setView(view);
    // The design's sentence, and after it - the hardening pass's ruling H5 of
    // 2026-10-14, "the dialog's sentence for a blocked release says where that
    // button is" - where the release that was put back is offered again: a
    // release that did not start is set aside for good otherwise.
    EXPECT_EQ(said(dialog),
              QStringLiteral("Katana 0.3.0 could not be installed, so 0.2.0 was restored. Reason: "
                             "the new version did not start. A later release will be offered, "
                             "and Settings > Updates offers this one again."));
    EXPECT_EQ(shown(dialog, "updateStateText"),
              QStringLiteral("Katana 0.3.0 could not be installed, so 0.2.0 was restored."));
    // It is read and closed: nothing here tries the same release again.
    EXPECT_TRUE(shownButtons(dialog).isEmpty())
        << shownButtons(dialog).join(QLatin1Char(' ')).toStdString();
}

TEST(UpdateDialog, OnlyAReleaseThatWasPutBackForNotStartingIsSaidToBeOfferedAgainInSettings)
{
    // Which endings set a release aside is the helper's rule
    // (src/katana_update/apply.cpp, Applier::fail: "A release that does not
    // start must not be offered again as it was" - a rollback whose reason is
    // health_exit or health_timeout, and no other ending). Only there is
    // there something for Settings > Updates to offer again; said after any
    // other ending the sentence would send the person to a button that is off.
    DialogFixture f;
    UpdateDialog dialog(f.context);
    const QString again = QStringLiteral("A later release will be offered, and Settings > "
                                         "Updates offers this one again.");
    UpdateView view = viewOf(UpdateDialogState::RolledBack);
    view.lastStatus = QStringLiteral("rolled_back");
    view.reason = QStringLiteral("health_timeout");
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateDetailText"),
              QStringLiteral("Reason: the new version did not finish starting in time. ") + again);

    // Put back because a file could not be replaced: nothing was set aside.
    view.reason = QStringLiteral("rename_failed");
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateDetailText"),
              QStringLiteral("Reason: a file was in use and could not be replaced."));

    // The same reasons under the two endings that put nothing back, or not
    // all of it: no release was set aside by either.
    for (const char* status : {"refused", "rollback_incomplete"}) {
        view.lastStatus = QString::fromLatin1(status);
        view.reason = QStringLiteral("health_exit");
        dialog.setView(view);
        EXPECT_FALSE(shown(dialog, "updateDetailText").contains(QStringLiteral("Settings")))
            << status;
    }
    // And with no reason given there is no sentence to hang it on, and none
    // that says it was the health check.
    view.lastStatus = QStringLiteral("rolled_back");
    view.reason.clear();
    dialog.setView(view);
    EXPECT_TRUE(shown(dialog, "updateDetailText").isEmpty());
}

TEST(UpdateDialog, AnApplyThatChangedNothingAndOneThatCouldNotBePutBackAreNotCalledRestored)
{
    // update::LastResult::status has two more endings than the design's
    // sentence covers (apply.hpp, ApplyOutcome): refused, where nothing was
    // touched, and rollback_incomplete, where the old version is NOT back.
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = viewOf(UpdateDialogState::RolledBack);
    view.lastStatus = QStringLiteral("refused");
    view.reason = QStringLiteral("space");
    dialog.setView(view);
    EXPECT_EQ(said(dialog), QStringLiteral("Katana 0.3.0 could not be installed; nothing was "
                                           "changed. Reason: there is not enough free disk "
                                           "space."));

    view.lastStatus = QStringLiteral("rollback_incomplete");
    view.reason = QStringLiteral("rollback_failed");
    view.recoverCommand = QStringLiteral("katana_updater --recover \"C:/Programs/Katana\"");
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateStateText"),
              QStringLiteral("Katana 0.3.0 could not be installed, and 0.2.0 could not be put "
                             "back completely."));
    EXPECT_EQ(shown(dialog, "updateDetailText"),
              QStringLiteral("Reason: the old version could not be put back completely. Close "
                             "Katana and run: katana_updater --recover \"C:/Programs/Katana\""));
    EXPECT_TRUE(shownButtons(dialog).isEmpty());
}

TEST(UpdateDialog, AFailedCheckSaysWhyAndOffersRetry)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = viewOf(UpdateDialogState::CheckFailed);
    view.reason = QStringLiteral("offline");
    dialog.setView(view);
    EXPECT_EQ(said(dialog),
              QStringLiteral("Could not check for updates: this computer is offline, or the "
                             "release server did not answer."));
    EXPECT_EQ(shownButtons(dialog), (QStringList{QStringLiteral("updateRetry")}));

    // A reason nobody gave leaves the colon out with it.
    view.reason.clear();
    dialog.setView(view);
    EXPECT_EQ(said(dialog), QStringLiteral("Could not check for updates."));
}

TEST(UpdateDialog, ACheckThatFindsNoUpdateInformationPointsAtTheAccessTokenInSettings)
{
    // A private repository answers an anonymous read with 404 (design
    // section 3): the one failure whose remedy the dialog can name.
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = viewOf(UpdateDialogState::CheckFailed);
    view.reason = QStringLiteral("http_404");
    dialog.setView(view);
    EXPECT_EQ(said(dialog),
              QStringLiteral("No update information found. If the releases are private, add an "
                             "access token in Settings > Updates."));
    EXPECT_EQ(shownButtons(dialog), (QStringList{QStringLiteral("updateRetry")}));
}

TEST(UpdateDialog, UnsupportedSaysWhyThisCopyCannotUpdateAndOffersNothing)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = viewOf(UpdateDialogState::Unsupported);
    view.reason = QStringLiteral("development_tree");
    dialog.setView(view);
    EXPECT_EQ(said(dialog),
              QStringLiteral("Updates are not available for this copy of Katana: it is a "
                             "development build, not an installed release."));
    EXPECT_TRUE(shownButtons(dialog).isEmpty())
        << shownButtons(dialog).join(QLatin1Char(' ')).toStdString();

    view.reason = QStringLiteral("root_not_writable");
    dialog.setView(view);
    EXPECT_EQ(said(dialog), QStringLiteral("Updates are not available for this copy of Katana: "
                                           "its folder cannot be written by you."));
}

TEST(UpdateDialog, AReleaseThatWasSkippedOrRolledBackHereBeforeIsSaidAndNotOffered)
{
    // The two answers of UPDATE CHECK that name a newer release without
    // offering it (check.hpp: reason "skipped", "blocked_before").
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = viewOf(UpdateDialogState::NotOffered);
    view.reason = QStringLiteral("skipped");
    dialog.setView(view);
    EXPECT_EQ(said(dialog), QStringLiteral("Katana 0.3.0 is available, and you chose to skip it. "
                                           "Settings > Updates offers it again."));
    EXPECT_EQ(shownButtons(dialog), (QStringList{QStringLiteral("updateCheckNow")}));

    // And, since the hardening pass's ruling H5 (2026-10-14), where the way
    // back is: the sentence ended at "A later release will be offered." while
    // nothing but deleting the state file by hand offered the release again.
    view.reason = QStringLiteral("blocked_before");
    dialog.setView(view);
    EXPECT_EQ(said(dialog), QStringLiteral("Katana 0.3.0 could not be installed here before, and "
                                           "is not offered again. A later release will be "
                                           "offered, and Settings > Updates offers this one "
                                           "again."));
    EXPECT_EQ(shownButtons(dialog), (QStringList{QStringLiteral("updateCheckNow")}));
}

// ---- every button calls exactly its callback, or builds exactly its line -----------------------

TEST(UpdateDialog, EachButtonBuildsExactlyItsLineOrCallsExactlyItsCallback)
{
    struct Press {
        UpdateView view;
        const char* button;
        const char* called; // DialogFixture::called() after the one press
    };
    UpdateView blocked = viewOf(UpdateDialogState::Blocked);
    blocked.holders = QStringLiteral("katana_mcp.exe (4412)");
    UpdateView failed = viewOf(UpdateDialogState::CheckFailed);
    failed.reason = QStringLiteral("offline");
    const std::vector<Press> presses{
        {viewOf(UpdateDialogState::Idle), "updateCheckNow", "UPDATE CHECK"},
        {viewOf(UpdateDialogState::Current), "updateCheckNow", "UPDATE CHECK"},
        {viewOf(UpdateDialogState::NotOffered), "updateCheckNow", "UPDATE CHECK"},
        {viewOf(UpdateDialogState::Checking), "updateCancel", "cancel x1"},
        // One click that is more than one line: the owner's, not a line.
        {available(), "updateInstall", "update x1"},
        {available(), "updateDownloadOnly", "UPDATE DOWNLOAD CONFIRM"},
        {available(), "updateSkip", "UPDATE SKIP 0.3.0"},
        {available(), "updateNotes",
         "notes https://github.com/kjarada/Katana/releases/tag/v0.3.0"},
        {available(), "updateLater", "later x1"},
        {downloading(), "updateCancel", "cancel x1"},
        {viewOf(UpdateDialogState::Ready), "updateRestart", "UPDATE APPLY CONFIRM"},
        {viewOf(UpdateDialogState::Ready), "updateLater", "later x1"},
        // Retry tries again what failed: the swap for a blocked copy, the
        // check for a failed check.
        {blocked, "updateRetry", "UPDATE APPLY CONFIRM"},
        {failed, "updateRetry", "UPDATE CHECK"},
    };
    for (const Press& each : presses) {
        DialogFixture f;
        UpdateDialog dialog(f.context);
        dialog.setView(each.view);
        ASSERT_TRUE(shownButtons(dialog).contains(QString::fromLatin1(each.button)))
            << each.button << " is not shown in state " << static_cast<int>(each.view.state);
        ASSERT_TRUE(enabled(dialog, each.button)) << each.button;
        EXPECT_TRUE(f.called().isEmpty()) << "showing a state calls nothing";
        press(dialog, each.button);
        EXPECT_EQ(f.called(), QString::fromLatin1(each.called))
            << each.button << " in state " << static_cast<int>(each.view.state);
    }
}

TEST(UpdateDialog, APressChangesNoSentenceByItselfTheNextViewDoes)
{
    // The dialog shows what its owner says happened, never what a button
    // hoped for: a line that was carried out leaves the state as it was.
    DialogFixture f;
    UpdateDialog dialog(f.context);
    dialog.setView(available());
    const QString before = said(dialog);
    press(dialog, "updateDownloadOnly");
    EXPECT_EQ(said(dialog), before);
    EXPECT_EQ(dialog.view().state, UpdateDialogState::Available);
    EXPECT_EQ(shownButtons(dialog).size(), 5);
}

TEST(UpdateDialog, LaterTellsTheOwnerAndHidesTheDialog)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    dialog.setView(viewOf(UpdateDialogState::Ready));
    dialog.show();
    ASSERT_TRUE(dialog.isVisible());
    press(dialog, "updateLater");
    EXPECT_FALSE(dialog.isVisible());
    EXPECT_EQ(f.called(), QStringLiteral("later x1")) << "and no line: the release stays staged";
    // Kept, with what it showed: opened again it says the same.
    EXPECT_EQ(shown(dialog, "updateStateText"), QStringLiteral("Katana 0.3.0 is ready."));
}

TEST(UpdateDialog, ALineTheExecutorRefusedIsSaidUntilTheNextView)
{
    DialogFixture f;
    f.answer = [](const QString&) {
        return VerbOutcome{false, {}, QStringLiteral("InvalidState: an update job is running")};
    };
    UpdateDialog dialog(f.context);
    dialog.setView(available());
    press(dialog, "updateDownloadOnly");
    const auto* problem = child<QLabel>(dialog, "updateProblemText");
    EXPECT_TRUE(problem->isVisibleTo(&dialog));
    EXPECT_EQ(problem->text(), QStringLiteral("InvalidState: an update job is running"));
    EXPECT_TRUE(problem->styleSheet().contains(katana::qt::theme::error().name()));
    EXPECT_EQ(shown(dialog, "updateStateText"), QStringLiteral("Update Available"));

    // A refusal with no words of its own still says it was one.
    f.answer = [](const QString&) { return VerbOutcome{false, {}, {}}; };
    press(dialog, "updateSkip");
    EXPECT_EQ(problem->text(), QStringLiteral("The command was refused."));

    dialog.setView(available());
    EXPECT_FALSE(problem->isVisibleTo(&dialog));
    EXPECT_TRUE(problem->text().isEmpty());
}

TEST(UpdateDialog, WhatTheOwnerCouldNotDoIsShownWithTheStateAndGoesWithTheNextView)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = available();
    view.problem = QStringLiteral("The download did not finish: this network does not allow "
                                  "partial downloads.");
    dialog.setView(view);
    const auto* problem = child<QLabel>(dialog, "updateProblemText");
    EXPECT_TRUE(problem->isVisibleTo(&dialog));
    EXPECT_EQ(problem->text(), view.problem);
    EXPECT_EQ(shown(dialog, "updateStateText"), QStringLiteral("Update Available"));
    EXPECT_TRUE(enabled(dialog, "updateInstall")) << "it can be tried again";

    dialog.setView(available());
    EXPECT_FALSE(problem->isVisibleTo(&dialog));
}

TEST(UpdateDialog, AButtonWithNothingToCallIsDisabled)
{
    // Every member of the context at its default: nothing can be run.
    UpdateDialog bare(UpdateDialogContext{});
    for (const UpdateView& view :
         {viewOf(UpdateDialogState::Idle), available(), downloading(),
          viewOf(UpdateDialogState::Ready), viewOf(UpdateDialogState::CheckFailed)}) {
        bare.setView(view);
        for (const QString& name : shownButtons(bare)) {
            EXPECT_EQ(bare.findChild<QPushButton*>(name)->isEnabled(),
                      name == QStringLiteral("updateLater"))
                << name.toStdString() << " in state " << static_cast<int>(view.state);
        }
    }
    // Later needs nobody: it only hides the dialog.
    bare.setView(available());
    bare.show();
    press(bare, "updateLater");
    EXPECT_FALSE(bare.isVisible());

    // With the executor alone, the lines can be built and the callbacks cannot.
    QStringList ran;
    UpdateDialogContext lines;
    lines.run = [&ran](const QString& line) {
        ran << line;
        return carriedOut();
    };
    UpdateDialog partial(lines);
    partial.setView(available());
    EXPECT_TRUE(enabled(partial, "updateDownloadOnly"));
    EXPECT_TRUE(enabled(partial, "updateSkip"));
    EXPECT_FALSE(enabled(partial, "updateInstall"));
    EXPECT_FALSE(enabled(partial, "updateNotes"));
}

TEST(UpdateDialog, ReleaseNotesNeedAUrlAndSkipNeedsAVersion)
{
    DialogFixture f;
    UpdateDialog dialog(f.context);
    UpdateView view = available();
    view.notesUrl.clear();
    view.latestVersion.clear();
    dialog.setView(view);
    EXPECT_FALSE(enabled(dialog, "updateNotes"));
    EXPECT_FALSE(enabled(dialog, "updateSkip"));
    press(dialog, "updateNotes");
    press(dialog, "updateSkip");
    EXPECT_TRUE(f.called().isEmpty()) << f.called().toStdString();
}

// ---- lifetime -----------------------------------------------------------------------------------

TEST(UpdateDialog, TheDialogOutlivesTheViewItWasGivenAndTheOwnerThatGaveIt)
{
    auto owner = std::make_unique<DialogFixture>();
    UpdateDialog dialog(owner->context);
    {
        // What it shows is a copy: the view it was given goes out of scope.
        const auto view = std::make_unique<UpdateView>(available());
        dialog.setView(*view);
    }
    EXPECT_EQ(shown(dialog, "updateDetailText"),
              QStringLiteral("Katana 0.3.0 (you have 0.2.0). 23 of 411 files changed, 71 MB to "
                             "download."));
    EXPECT_EQ(dialog.view().notesUrl,
              QStringLiteral("https://github.com/kjarada/Katana/releases/tag/v0.3.0"));

    // The owner goes first, and says so. Its callbacks hold a pointer to it:
    // none may be called again.
    dialog.detach();
    owner.reset();
    for (const UpdateView& view :
         {viewOf(UpdateDialogState::Idle), available(), downloading(),
          viewOf(UpdateDialogState::Ready), viewOf(UpdateDialogState::Blocked),
          viewOf(UpdateDialogState::CheckFailed)}) {
        dialog.setView(view);
        for (const QString& name : shownButtons(dialog)) {
            auto* button = dialog.findChild<QPushButton*>(name);
            EXPECT_EQ(button->isEnabled(), name == QStringLiteral("updateLater"))
                << name.toStdString();
            // Even a click that reaches a disabled button's slot calls nothing.
            button->setEnabled(true);
            button->click();
        }
    }
    // It still says what it was last told.
    EXPECT_EQ(shown(dialog, "updateStateText"), QStringLiteral("Could not check for updates."));
    katana::qt::test::paint(dialog);
}

// ---- the words and the numbers ------------------------------------------------------------------

TEST(UpdateWords, MegabytesAreMillionsOfBytesRoundedToTheNearestAndNeverNoneForADownload)
{
    using katana::qt::updateMegabytes;
    EXPECT_EQ(updateMegabytes(0), QStringLiteral("0"));
    EXPECT_EQ(updateMegabytes(1), QStringLiteral("1")) << "a download is never said to be nothing";
    EXPECT_EQ(updateMegabytes(499'999), QStringLiteral("1"));
    EXPECT_EQ(updateMegabytes(1'000'000), QStringLiteral("1"));
    EXPECT_EQ(updateMegabytes(1'499'999), QStringLiteral("1"));
    EXPECT_EQ(updateMegabytes(1'500'000), QStringLiteral("2"));
    EXPECT_EQ(updateMegabytes(71'000'000), QStringLiteral("71"));
    // The largest payload a manifest may name, 4 GiB (manifest.hpp): 4294.967296 MB.
    EXPECT_EQ(updateMegabytes(4'294'967'296ull), QStringLiteral("4295"));
}

TEST(UpdateWords, AReasonTokenIsSaidInWordsAndOneNobodyKnowsAsItCame)
{
    using katana::qt::updateReasonWords;
    EXPECT_EQ(updateReasonWords(QStringLiteral("partial_downloads")),
              QStringLiteral("this network does not allow partial downloads"));
    EXPECT_EQ(updateReasonWords(QStringLiteral("http_403")),
              QStringLiteral("the release server refused the request (error 403)"));
    // Any other status the server answers with: the engine writes http_<n>.
    EXPECT_EQ(updateReasonWords(QStringLiteral("http_503")),
              QStringLiteral("the release server answered with error 503"));
    // A token of a later engine is still said, as it came.
    EXPECT_EQ(updateReasonWords(QStringLiteral("something_new")), QStringLiteral("something_new"));
    EXPECT_EQ(updateReasonWords(QStringLiteral("http_5033")), QStringLiteral("http_5033"));
    EXPECT_TRUE(updateReasonWords(QString()).isEmpty());
}

TEST(UpdateWords, EveryReasonTheEngineGivesHasWordsOfItsOwn)
{
    // The tokens of include/katana/update/check.hpp (a check's `reason`), of
    // apply.hpp (ApplyResult::reason) and the verbs' own two
    // (update_verbs.hpp), copied from those headers' comments. Each must read
    // as words: none comes back as the token it is.
    //
    // And the tokens the network engine adds, copied from the comments of its
    // headers (branch upd-t8: check.hpp "Added by check() itself", feed.hpp
    // fetchFailureReason, stage.hpp "The Error of a failure carries a
    // machine-readable token" and "preflight's"): the rows from html_page to
    // helper_failed.
    //
    // And the two refusals of a copy that may not install, which the verbs'
    // header lists for UPDATE APPLY CONFIRM (update_verbs.hpp: "platform" where
    // this version installs nothing, "elevated" - the hardening pass's ruling
    // H4 - for a Katana started as administrator): the last row.
    for (const char* token :
         {"offline",          "http_403",        "http_404",         "http_429",
          "bad_signature",    "wrong_product",   "wrong_target",     "malformed",
          "untrusted_key",    "placeholder_keys", "partial_downloads", "cancelled",
          "disabled_build",   "development_tree", "root_not_writable", "headless",
          "skipped",          "blocked_before",  "no_transport",     "no_store",
          "locked",           "interrupted_update", "invalid_request", "install_record",
          "staged_manifest",  "notify_only",     "applier_too_new",  "downgrade",
          "blocked",          "scan_failed",     "staged_missing",   "staged_hash",
          "reparse_point",    "path_conflict",   "copy_failed",      "space",
          "journal",          "io_error",        "transaction_exists", "rename_failed",
          "health_timeout",   "health_exit",     "rollback_failed",  "internal_error",
          "html_page",        "too_large",       "wrong_channel",    "asset_missing",
          "verify_unavailable", "bad_range",     "bad_redirect",     "too_many_redirects",
          "error",            "staged_corrupt",  "write_failed",     "stage_busy",
          "low_disk_space",   "short_range",     "unverified_manifest", "no_token",
          "payload_mismatch", "helper_missing",  "helper_not_started", "helper_timeout",
          "helper_failed",
          "platform",         "elevated"}) {
        const QString words = katana::qt::updateReasonWords(QString::fromLatin1(token));
        EXPECT_NE(words, QString::fromLatin1(token)) << token;
        EXPECT_TRUE(words.contains(QLatin1Char(' '))) << token << ": " << words.toStdString();
        EXPECT_FALSE(words.endsWith(QLatin1Char('.'))) << token << " ends the sentence itself";
    }
}

TEST(UpdateWords, ACopyThatMayNotInstallIsToldWhatItCanDoAboutIt)
{
    using katana::qt::updateReasonWords;
    // Started as administrator (the hardening pass's ruling H4: "The sentence
    // tells the person to start Katana the ordinary way and update again; the
    // download stays"). Each of the three things is said, in a person's
    // words, and the token is not.
    const QString elevated = updateReasonWords(QStringLiteral("elevated"));
    EXPECT_EQ(elevated,
              QStringLiteral("Katana was started as administrator; start it the ordinary way, "
                             "not as administrator, and update again - the download is kept"));
    for (const char* part : {"start it the ordinary way", "not as administrator", "update again",
                             "the download is kept"}) {
        EXPECT_TRUE(elevated.contains(QLatin1String(part))) << part;
    }
    EXPECT_FALSE(elevated.contains(QStringLiteral("elevated")));

    // A copy on a platform this version installs nothing on (the update
    // document's platform table: such a copy is told of a release and sent to
    // its notes, which is the Available state's sentence for it too).
    const QString platform = updateReasonWords(QStringLiteral("platform"));
    EXPECT_EQ(platform,
              QStringLiteral("this copy of Katana cannot install an update on this kind of "
                             "computer; the release notes say where to download it"));
    EXPECT_TRUE(platform.contains(QStringLiteral("the release notes say where to download it")));

    // As the dialog says it: how the last apply ended, when the helper
    // refused it for the platform. Of the two, only `platform` is among an
    // apply's reasons (include/katana/update/apply.hpp, ApplyResult::reason).
    // `elevated` is the verbs' own refusal of UPDATE APPLY CONFIRM
    // (src/katana_app/update_verbs.hpp), made before any helper is started:
    // it is never how an apply ended, and is said where that line's refusal
    // is said.
    UpdateDialog dialog(UpdateDialogContext{});
    UpdateView view = viewOf(UpdateDialogState::RolledBack);
    view.lastStatus = QStringLiteral("refused");
    view.reason = QStringLiteral("platform");
    dialog.setView(view);
    EXPECT_EQ(said(dialog),
              QStringLiteral("Katana 0.3.0 could not be installed; nothing was changed. Reason: "
                             "this copy of Katana cannot install an update on this kind of "
                             "computer; the release notes say where to download it."));
}

TEST(UpdateWords, AnEarlierUpdateThatIsStillOpenIsSaidToBeUnsettledNotFailed)
{
    // The helper refuses an apply as interrupted_update while an earlier one
    // is neither confirmed nor put back (the hardening pass's H1: an update
    // whose helper died during the health check is settled by a later start,
    // and until then "an earlier update is not settled yet"). The words said
    // it "could not be put back", which is not what has happened.
    EXPECT_EQ(katana::qt::updateReasonWords(QStringLiteral("interrupted_update")),
              QStringLiteral("an earlier update is not settled yet"));
    UpdateDialog dialog(UpdateDialogContext{});
    UpdateView view = viewOf(UpdateDialogState::RolledBack);
    view.lastStatus = QStringLiteral("refused");
    view.reason = QStringLiteral("interrupted_update");
    dialog.setView(view);
    EXPECT_EQ(said(dialog),
              QStringLiteral("Katana 0.3.0 could not be installed; nothing was changed. Reason: "
                             "an earlier update is not settled yet."));

    // With the command the owner gives it for this ending
    // (updateViewOfLastResult), the dialog says what to do, in the order the
    // helper's own `detail` gives the two steps (src/katana_update/apply.cpp,
    // Applier::begin: "Katana's window confirms it when it starts - or run:
    // katana_updater --recover ... - then update again"): the start that
    // shows this has confirmed it or not, so update again first, and the
    // command by hand where that is refused again. Nothing to press: it is
    // read and closed.
    view.recoverCommand = QStringLiteral("katana_updater --recover \"C:/Programs/Katana\"");
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateStateText"),
              QStringLiteral("Katana 0.3.0 could not be installed; nothing was changed."));
    EXPECT_EQ(shown(dialog, "updateDetailText"),
              QStringLiteral("Reason: an earlier update is not settled yet. Update again; if this "
                             "is said again, close Katana and run: katana_updater --recover "
                             "\"C:/Programs/Katana\""));
    EXPECT_TRUE(shownButtons(dialog).isEmpty());

    // The command is for that refusal alone: another one names none, whatever
    // the view carries.
    view.reason = QStringLiteral("space");
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateDetailText"),
              QStringLiteral("Reason: there is not enough free disk space."));
}

TEST(UpdateWords, ACaptivePortalAndAFullDiskAreSaidInWordsNotAsTheirTokens)
{
    // The two a person meets most of the network engine's reasons: a hotel's
    // or a site office's sign-in page answering in the place of the release
    // server (html_page), and a download that does not fit (low_disk_space).
    // Neither sentence may show the token.
    UpdateDialog dialog(UpdateDialogContext{});
    UpdateView view = viewOf(UpdateDialogState::CheckFailed);
    view.reason = QStringLiteral("html_page");
    dialog.setView(view);
    EXPECT_EQ(shown(dialog, "updateStateText"),
              QStringLiteral("Could not check for updates: a web page answered in the place of "
                             "the release server; this network may want you to sign in first."));
    EXPECT_EQ(katana::qt::updateReasonWords(QStringLiteral("low_disk_space")),
              QStringLiteral("there is not enough free disk space for the download"));
    EXPECT_EQ(katana::qt::updateReasonWords(QStringLiteral("stage_busy")),
              QStringLiteral("another Katana is downloading this update now"));
}

TEST(UpdateWords, TheFilesOfAProgressLineAreReadWhereverTheWordStands)
{
    using katana::qt::updateFilesOfProgressLine;
    for (const char* line : {"files=12/23", "progress bytes=31000000/71000000 files=12/23",
                             "progress files=12/23 file=bin/a.dll", "files=12/23 staged=none"}) {
        const auto files = updateFilesOfProgressLine(QString::fromLatin1(line));
        ASSERT_TRUE(files.has_value()) << line;
        EXPECT_EQ(files->first, 12u) << line;
        EXPECT_EQ(files->second, 23u) << line;
    }
    // Not a word of its own, not two numbers, or not there: the files are left unsaid.
    for (const char* line : {"progress profiles=12/23", "progress files=12", "progress files=a/b",
                             "progress files=12/23x", "staged=0.3.0", ""}) {
        EXPECT_FALSE(updateFilesOfProgressLine(QString::fromLatin1(line)).has_value()) << line;
    }
}

TEST(UpdateWords, TheSkipLineNamesTheVersion)
{
    EXPECT_EQ(katana::qt::updateSkipLine(QStringLiteral("0.3.0")),
              QStringLiteral("UPDATE SKIP 0.3.0"));
}

TEST(UpdateWords, WhatIsTypedWhereATokenWasHopedToGoIsNeverEchoed)
{
    // The verbs have UPDATE TOKEN STATUS and UPDATE TOKEN CLEAR and no line
    // that takes a token (update_verbs.hpp): a typed line is echoed in the
    // command log before any verb refuses it, so the echo hides every other
    // word after TOKEN.
    using katana::qt::updateLoggedLine;
    const QString token = QStringLiteral("ghp_NotARealTokenOnlyATestsWord0123456789");
    EXPECT_EQ(updateLoggedLine(QStringLiteral("UPDATE TOKEN ") + token),
              QStringLiteral("UPDATE TOKEN ***"));
    EXPECT_EQ(updateLoggedLine(QStringLiteral("update token SET ") + token),
              QStringLiteral("update token *** ***"));
    EXPECT_EQ(updateLoggedLine(QStringLiteral("  UPDATE   TOKEN CLEAR ") + token),
              QStringLiteral("UPDATE TOKEN CLEAR ***"));
    // The two lines there are, and every other line, are echoed as typed.
    for (const char* line : {"UPDATE TOKEN STATUS", "update token clear", "UPDATE TOKEN",
                             "UPDATE CHECK", "UPDATE SKIP 0.3.0", "UPDATE  SET AUTO  ON",
                             "ONLINE KEY provider secret", "TOKEN one two", "LINE 0,0 10,0", ""}) {
        EXPECT_EQ(updateLoggedLine(QString::fromLatin1(line)), QString::fromLatin1(line)) << line;
    }
}

// ---- the decisions ------------------------------------------------------------------------------

namespace {

using katana::app::UpdateOffer;
using katana::app::UpdateStatus;
using katana::qt::UpdateAutoCheckFacts;
using katana::qt::UpdateOfferFacts;
using katana::qt::UpdateQuietAnswer;
using katana::update::CheckStatus;

// 2026-10-20T08:00:00Z: 20 746 days after 1970 (56 years of 365, 14 leap
// days, 273 days to the first of October, 19 more) and 8 hours.
constexpr std::int64_t kNow = 1'792'483'200;
// The design's `sequence` for the build of 0.3.0.
constexpr std::uint64_t kSequence = 1'792'312'800;

// 0.2.0 as the install's record has it (update/version.hpp).
katana::update::Version installedVersion()
{
    katana::update::Version version;
    version.components = {0, 2, 0, 0};
    version.written = 3;
    return version;
}

// What UPDATE CHECK answers for the design's example: 0.2.0 installed, 0.3.0
// released, 23 of 411 files and 71 MB when it is offered.
UpdateOffer offerOf(CheckStatus status)
{
    UpdateOffer offer;
    offer.check.status = status;
    offer.check.installedVersion = "0.2.0";
    offer.check.latestVersion = "0.3.0";
    offer.check.notesUrl = "https://github.com/kjarada/Katana/releases/tag/v0.3.0";
    if (status == CheckStatus::Available) {
        offer.changedFiles = 23;
        offer.totalFiles = 411;
        offer.downloadBytes = 71'000'000;
    }
    return offer;
}

UpdateOfferFacts factsOf()
{
    UpdateOfferFacts facts;
    facts.installedVersion = QStringLiteral("0.2.0");
    facts.checkedAt = QStringLiteral("14:02");
    facts.nowEpoch = kNow;
    return facts;
}

// A view's two sentences, as the dialog shows them and the log says them.
QString saidBy(const UpdateView& view)
{
    const QString state = katana::qt::updateStateText(view);
    const QString detail = katana::qt::updateDetailText(view);
    return detail.isEmpty() ? state : state + QLatin1Char(' ') + detail;
}

UpdateView viewOfVersion(UpdateDialogState state, const char* version)
{
    UpdateView view = viewOf(state);
    view.latestVersion = QString::fromLatin1(version);
    return view;
}

} // namespace

TEST(UpdateDecisions, AReleaseThatWasRolledBackHereIsNotOfferedAsReadyAtTheNextStart)
{
    // THE REGRESSION. 0.3.0 was installed, did not start, and was rolled
    // back: the helper put its sequence on the blocked list and KEPT its stage
    // (include/katana/update/apply.hpp, step 7: "the sequence is added to
    // `blocked`"; only a healthy update deletes the stage). UPDATE STATUS
    // then still names 0.3.0 as downloaded. Offered as ready, with the status
    // bar's button, "Restart and update" installs the release known not to
    // start here - and after each rollback it is offered again.
    using katana::qt::updateStagedToOffer;
    UpdateStatus status;
    status.installedVersion = "0.2.0";
    status.stagedVersion = "0.3.0";
    status.state.blockedSequences = {kSequence};
    const auto stageOfTheReleaseThatFailed = [](std::uint64_t sequence) {
        return sequence == kSequence;
    };
    EXPECT_TRUE(updateStagedToOffer(status, installedVersion(), stageOfTheReleaseThatFailed)
                    .isEmpty());

    // The same copy BEFORE that release failed: it is the one to offer.
    status.state.blockedSequences.clear();
    EXPECT_EQ(updateStagedToOffer(status, installedVersion(), stageOfTheReleaseThatFailed),
              QStringLiteral("0.3.0"));

    // And once a later build has been downloaded: the stage of the build that
    // failed is gone (a download removes the stages its release outranks), so
    // what STATUS names is not the release that does not start here.
    status.state.blockedSequences = {kSequence};
    status.stagedVersion = "0.3.1";
    const auto noStageOfIt = [](std::uint64_t) { return false; };
    EXPECT_EQ(updateStagedToOffer(status, installedVersion(), noStageOfIt),
              QStringLiteral("0.3.1"));
}

TEST(UpdateDecisions, ADownloadedReleaseIsOfferedAtAStartOnlyWhenItIsOneToInstall)
{
    using katana::qt::updateStagedToOffer;
    const auto noBlockedStage = [](std::uint64_t) { return false; };
    UpdateStatus status;
    status.installedVersion = "0.2.0";
    // Nothing is downloaded.
    EXPECT_TRUE(updateStagedToOffer(status, installedVersion(), noBlockedStage).isEmpty());

    // Downloaded, and skipped since ("0.3" and "0.3.0" are one release): a
    // skipped release is not offered.
    status.stagedVersion = "0.3.0";
    status.state.skippedVersion = "0.3";
    EXPECT_TRUE(updateStagedToOffer(status, installedVersion(), noBlockedStage).isEmpty());
    // Another version is the skipped one: this one is still to offer.
    status.state.skippedVersion = "0.2.5";
    EXPECT_EQ(updateStagedToOffer(status, installedVersion(), noBlockedStage),
              QStringLiteral("0.3.0"));
    status.state.skippedVersion.clear();

    // The stage a finished update left behind - what is installed, or older -
    // is nothing to install; nor is text that is no version.
    for (const char* stale : {"0.2.0", "0.2", "0.1.9", "none", "0.3.0-beta"}) {
        status.stagedVersion = stale;
        EXPECT_TRUE(updateStagedToOffer(status, installedVersion(), noBlockedStage).isEmpty())
            << stale;
    }
}

TEST(UpdateDecisions, ACheckThatOffersAReleaseIsShownAsTheDesignSaysIt)
{
    const UpdateView view =
        katana::qt::updateViewOfOffer(offerOf(CheckStatus::Available), factsOf());
    EXPECT_EQ(view.state, UpdateDialogState::Available);
    EXPECT_EQ(saidBy(view),
              QStringLiteral("Update Available Katana 0.3.0 (you have 0.2.0). 23 of 411 files "
                             "changed, 71 MB to download."));
    EXPECT_EQ(view.notesUrl,
              QStringLiteral("https://github.com/kjarada/Katana/releases/tag/v0.3.0"));
    EXPECT_FALSE(view.notifyOnly);
    EXPECT_FALSE(view.informationIsOld);
    EXPECT_EQ(katana::qt::updateStatusButtonText(view), QStringLiteral("Update 0.3.0"));
}

TEST(UpdateDecisions, TheReleaseThatIsAlreadyDownloadedIsReadyNotToBeDownloadedAgain)
{
    UpdateOfferFacts facts = factsOf();
    facts.stagedVersion = QStringLiteral("0.3"); // the same release as 0.3.0
    const UpdateView view = katana::qt::updateViewOfOffer(offerOf(CheckStatus::Available), facts);
    EXPECT_EQ(saidBy(view), QStringLiteral("Katana 0.3.0 is ready. Katana will close, update and "
                                           "reopen in a few seconds."));
    EXPECT_EQ(katana::qt::updateStatusButtonText(view), QStringLiteral("Update 0.3.0"));

    // What is downloaded is an older release than the one offered: that one
    // is offered for download.
    facts.stagedVersion = QStringLiteral("0.2.9");
    EXPECT_EQ(katana::qt::updateViewOfOffer(offerOf(CheckStatus::Available), facts).state,
              UpdateDialogState::Available);
}

TEST(UpdateDecisions, EveryOtherAnswerOfACheckIsItsOwnStateAndOffersNothing)
{
    using katana::qt::updateStatusButtonText;
    using katana::qt::updateViewOfOffer;
    // The feed has nothing above this copy.
    UpdateOffer current = offerOf(CheckStatus::Current);
    current.check.latestVersion = "0.2.0";
    UpdateView view = updateViewOfOffer(current, factsOf());
    EXPECT_EQ(saidBy(view), QStringLiteral("Katana 0.2.0 is up to date. Last checked 14:02."));
    EXPECT_TRUE(updateStatusButtonText(view).isEmpty());

    // A release the person skipped: `current`, reason "skipped" (check.hpp).
    UpdateOffer skipped = offerOf(CheckStatus::Current);
    skipped.check.reason = "skipped";
    view = updateViewOfOffer(skipped, factsOf());
    EXPECT_EQ(view.state, UpdateDialogState::NotOffered);
    EXPECT_EQ(saidBy(view), QStringLiteral("Katana 0.3.0 is available, and you chose to skip it. "
                                           "Settings > Updates offers it again."));
    EXPECT_TRUE(updateStatusButtonText(view).isEmpty()) << "a skipped release is not offered";

    // A release that was rolled back here: `blocked`. Its sentence names the
    // place that offers it again (ruling H5 of the hardening pass).
    UpdateOffer blocked = offerOf(CheckStatus::Blocked);
    blocked.check.reason = "blocked_before";
    view = updateViewOfOffer(blocked, factsOf());
    EXPECT_EQ(view.state, UpdateDialogState::NotOffered);
    EXPECT_EQ(saidBy(view), QStringLiteral("Katana 0.3.0 could not be installed here before, and "
                                           "is not offered again. A later release will be "
                                           "offered, and Settings > Updates offers this one "
                                           "again."));
    EXPECT_TRUE(updateStatusButtonText(view).isEmpty()) << "a blocked release is not offered";

    // The verbs give that answer a sentence of their own, for a line's reply
    // (update_verbs.hpp: CheckResult::detail of update=blocked). It changes
    // nothing here: the dialog says where its own button is.
    blocked.check.detail = "this release was installed here before, did not start and was put "
                           "back, so it is not offered; UPDATE BLOCKED CLEAR (Settings > Updates "
                           "in the window) offers it again";
    EXPECT_EQ(saidBy(updateViewOfOffer(blocked, factsOf())), saidBy(view));

    // A private feed's 404.
    UpdateOffer unreachable = offerOf(CheckStatus::Unreachable);
    unreachable.check.latestVersion.clear();
    unreachable.check.notesUrl.clear();
    unreachable.check.reason = "http_404";
    view = updateViewOfOffer(unreachable, factsOf());
    EXPECT_EQ(saidBy(view),
              QStringLiteral("No update information found. If the releases are private, add an "
                             "access token in Settings > Updates."));
    EXPECT_TRUE(updateStatusButtonText(view).isEmpty());

    // A copy that cannot update, whose answer names no version: the copy's own.
    UpdateOffer unsupported;
    unsupported.check.status = CheckStatus::Unsupported;
    unsupported.check.reason = "development_tree";
    view = updateViewOfOffer(unsupported, factsOf());
    EXPECT_EQ(saidBy(view),
              QStringLiteral("Updates are not available for this copy of Katana: it is a "
                             "development build, not an installed release."));
    EXPECT_EQ(view.installedVersion, QStringLiteral("0.2.0"));
    EXPECT_TRUE(updateStatusButtonText(view).isEmpty());
}

TEST(UpdateDecisions, ANotifyOnlyReleaseAndACopyThatCannotApplyAreToldOfAndNotInstalled)
{
    using katana::qt::updateViewOfOffer;
    UpdateOffer offer = offerOf(CheckStatus::Available);
    katana::update::Manifest manifest;
    manifest.expires = "2027-04-18T08:00:00Z";
    manifest.notifyOnly = true;
    offer.check.manifest = manifest;
    EXPECT_TRUE(updateViewOfOffer(offer, factsOf()).notifyOnly);

    offer.check.manifest->notifyOnly = false;
    EXPECT_FALSE(updateViewOfOffer(offer, factsOf()).notifyOnly);
    // The same release where the copy cannot replace its own files.
    UpdateOfferFacts elsewhere = factsOf();
    elsewhere.appliesHere = false;
    const UpdateView view = updateViewOfOffer(offer, elsewhere);
    EXPECT_TRUE(view.notifyOnly);
    EXPECT_EQ(katana::qt::updateDetailText(view),
              QStringLiteral("Katana 0.3.0 (you have 0.2.0). This copy cannot update itself: the "
                             "release notes say where to download it."));
}

TEST(UpdateDecisions, UpdateInformationPastItsExpiryIsSaidToBeOldAndTheReleaseStillOffered)
{
    using katana::qt::updateViewOfOffer;
    UpdateOffer offer = offerOf(CheckStatus::Available);
    katana::update::Manifest manifest;
    manifest.expires = "2027-04-18T08:00:00Z"; // 1 808 035 200
    offer.check.manifest = manifest;
    UpdateOfferFacts facts = factsOf();
    EXPECT_FALSE(updateViewOfOffer(offer, facts).informationIsOld) << "six months before";
    // At the second it expires it has not passed; one later it has.
    facts.nowEpoch = 1'808'035'200;
    EXPECT_FALSE(updateViewOfOffer(offer, facts).informationIsOld);
    facts.nowEpoch = 1'808'035'201;
    const UpdateView old = updateViewOfOffer(offer, facts);
    EXPECT_TRUE(old.informationIsOld);
    EXPECT_EQ(old.state, UpdateDialogState::Available) << "advisory: nothing is refused";
    // An answer that carries no manifest is never called old.
    offer.check.manifest.reset();
    EXPECT_FALSE(updateViewOfOffer(offer, facts).informationIsOld);
}

TEST(UpdateDecisions, TheAutomaticChecksAnswerIsAnnouncedShownWithoutAWordOrDropped)
{
    using katana::qt::updateQuietAnswer;
    struct Case {
        UpdateView shown;
        UpdateView answer;
        UpdateQuietAnswer expected;
        const char* why;
    };
    UpdateView upToDate = viewOf(UpdateDialogState::Current);
    upToDate.latestVersion = QStringLiteral("0.2.0");
    UpdateView blockedBefore = viewOf(UpdateDialogState::NotOffered);
    blockedBefore.reason = QStringLiteral("blocked_before");
    UpdateView skipped = viewOf(UpdateDialogState::NotOffered);
    skipped.reason = QStringLiteral("skipped");
    const std::vector<Case> cases{
        // News.
        {viewOf(UpdateDialogState::Idle), available(), UpdateQuietAnswer::Announce,
         "a release, and nothing was known"},
        {upToDate, available(), UpdateQuietAnswer::Announce, "a release since the last check"},
        {available(), viewOfVersion(UpdateDialogState::Available, "0.3.1"),
         UpdateQuietAnswer::Announce, "a later release than the one offered"},
        {viewOf(UpdateDialogState::CheckFailed), available(), UpdateQuietAnswer::Announce,
         "a release, after a check that failed"},
        // No news: the release is offered already, in whichever way.
        {available(), available(), UpdateQuietAnswer::Nothing, "the release already offered"},
        {viewOf(UpdateDialogState::Ready), viewOf(UpdateDialogState::Ready),
         UpdateQuietAnswer::Nothing, "the release already downloaded"},
        {viewOf(UpdateDialogState::Blocked), viewOf(UpdateDialogState::Ready),
         UpdateQuietAnswer::Nothing, "the programs to close stay named"},
        // Quiet when it learns nothing.
        {viewOf(UpdateDialogState::Idle), viewOf(UpdateDialogState::CheckFailed),
         UpdateQuietAnswer::Nothing, "a check that failed says nothing"},
        {viewOf(UpdateDialogState::Ready), viewOf(UpdateDialogState::CheckFailed),
         UpdateQuietAnswer::Nothing, "and takes nothing back"},
        {viewOf(UpdateDialogState::Idle), viewOf(UpdateDialogState::Unsupported),
         UpdateQuietAnswer::Nothing, "a copy that cannot update says nothing by itself"},
        // Nothing to tell, something to show.
        {viewOf(UpdateDialogState::Idle), upToDate, UpdateQuietAnswer::Show,
         "up to date, for when the dialog is opened"},
        {viewOf(UpdateDialogState::CheckFailed), upToDate, UpdateQuietAnswer::Show,
         "up to date, over an older failure"},
        // A release the window still offers is taken back, without a word.
        {viewOf(UpdateDialogState::Ready), blockedBefore, UpdateQuietAnswer::Show,
         "ready, and rolled back here: not offered"},
        {available(), skipped, UpdateQuietAnswer::Show, "offered, and skipped: not offered"},
        {viewOf(UpdateDialogState::Ready), upToDate, UpdateQuietAnswer::Show,
         "ready, and no longer released: not offered"},
        // What must stay until it is read, and what a job owns.
        {viewOf(UpdateDialogState::RolledBack), available(), UpdateQuietAnswer::Nothing,
         "how the last update ended stays until it is read"},
        {viewOf(UpdateDialogState::RolledBack), upToDate, UpdateQuietAnswer::Nothing,
         "how the last update ended stays until it is read"},
        {viewOf(UpdateDialogState::Checking), available(), UpdateQuietAnswer::Nothing,
         "a check a person asked for answers for itself"},
        {downloading(), upToDate, UpdateQuietAnswer::Nothing, "a download is not interrupted"},
    };
    for (const Case& each : cases) {
        EXPECT_EQ(updateQuietAnswer(each.shown, each.answer), each.expected) << each.why;
    }
}

TEST(UpdateDecisions, ADownloadedReleaseStaysReadyOnlyWhileACheckStillOffersIt)
{
    using katana::qt::updateStagedAfter;
    const QString staged = QStringLiteral("0.3.0");
    // The feed has nothing above this copy any more: the release was withdrawn.
    EXPECT_TRUE(updateStagedAfter(staged, viewOf(UpdateDialogState::Current)).isEmpty());
    // That very version was skipped, or rolled back here ("0.3" is 0.3.0).
    EXPECT_TRUE(updateStagedAfter(staged, viewOf(UpdateDialogState::NotOffered)).isEmpty());
    EXPECT_TRUE(updateStagedAfter(staged, viewOfVersion(UpdateDialogState::NotOffered, "0.3"))
                    .isEmpty());
    // Another version is the one not offered; a check that failed un-knows
    // nothing; and an answer that offers a release leaves what is downloaded.
    EXPECT_EQ(updateStagedAfter(staged, viewOfVersion(UpdateDialogState::NotOffered, "0.4.0")),
              staged);
    EXPECT_EQ(updateStagedAfter(staged, viewOf(UpdateDialogState::CheckFailed)), staged);
    EXPECT_EQ(updateStagedAfter(staged, viewOf(UpdateDialogState::Ready)), staged);
    EXPECT_EQ(updateStagedAfter(staged, viewOfVersion(UpdateDialogState::Available, "0.4.0")),
              staged);
}

TEST(UpdateDecisions, HowTheLastUpdateEndedIsItsOwnStateAndASuccessIsNone)
{
    using katana::qt::updateViewOfLastResult;
    const QString installed = QStringLiteral("0.2.0");
    const QString root = QStringLiteral("C:\\Programs\\Katana");
    katana::update::LastResult last;
    last.version = "0.3.0";
    last.transaction = "0a1b2c3d";

    // A success is one line in the log, not a dialog.
    last.status = "committed";
    EXPECT_FALSE(updateViewOfLastResult(last, installed, root).has_value());

    // The design's sentence, and where the release it tells of is offered
    // again (ruling H5 of the hardening pass: a release that did not start is
    // set aside, and the person is told where the way back is).
    last.status = "rolled_back";
    last.reason = "health_exit";
    std::optional<UpdateView> view = updateViewOfLastResult(last, installed, root);
    ASSERT_TRUE(view.has_value());
    EXPECT_EQ(view->state, UpdateDialogState::RolledBack);
    EXPECT_EQ(saidBy(*view),
              QStringLiteral("Katana 0.3.0 could not be installed, so 0.2.0 was restored. Reason: "
                             "the new version did not start. A later release will be offered, "
                             "and Settings > Updates offers this one again."));
    EXPECT_TRUE(katana::qt::updateStatusButtonText(*view).isEmpty())
        << "what was rolled back is not offered";

    // Nothing was touched: the programs to close, and the release still there.
    last.status = "blocked";
    last.reason = "blocked";
    last.holder = "katana_mcp.exe (4412)";
    view = updateViewOfLastResult(last, installed, root);
    ASSERT_TRUE(view.has_value());
    EXPECT_EQ(view->state, UpdateDialogState::Blocked);
    EXPECT_EQ(saidBy(*view),
              QStringLiteral("Close these first: katana_mcp.exe (4412). Then press Retry."));
    EXPECT_EQ(katana::qt::updateStatusButtonText(*view), QStringLiteral("Update 0.3.0"));

    last.holder.clear();
    last.status = "refused";
    last.reason = "space";
    view = updateViewOfLastResult(last, installed, root);
    ASSERT_TRUE(view.has_value());
    EXPECT_EQ(saidBy(*view), QStringLiteral("Katana 0.3.0 could not be installed; nothing was "
                                            "changed. Reason: there is not enough free disk "
                                            "space."));

    // The old version is NOT back: the command that puts it right, by hand.
    last.status = "rollback_incomplete";
    last.reason = "rollback_failed";
    view = updateViewOfLastResult(last, installed, root);
    ASSERT_TRUE(view.has_value());
    EXPECT_EQ(saidBy(*view),
              QStringLiteral("Katana 0.3.0 could not be installed, and 0.2.0 could not be put "
                             "back completely. Reason: the old version could not be put back "
                             "completely. Close Katana and run: katana_updater --recover "
                             "\"C:\\Programs\\Katana\""));
}

TEST(UpdateDecisions, AnUpdateRefusedOverAnEarlierOneThatIsNotSettledNamesTheCommandThatSettlesIt)
{
    // The helper refuses an apply as interrupted_update while an earlier
    // update of the install is neither confirmed nor put back, and names the
    // way out in its own `detail` alone (src/katana_update/apply.cpp,
    // Applier::begin: "... or run: katana_updater --recover \"<root>\" ...").
    // The result it leaves for the next start has no detail
    // (update::LastResult: status, reason, holder, version, transaction). So
    // in an install whose window could not confirm the earlier update, each
    // "Restart and update" closed Katana and reopened it on a sentence with
    // nothing to do, and the release was offered as Ready again. The view
    // carries the command for this ending now, built as it is for
    // rollback_incomplete: the helper's name, --recover, and the install's
    // folder in quotes, as the platform writes it.
    using katana::qt::updateViewOfLastResult;
    const QString installed = QStringLiteral("0.2.0");
    const QString root = QStringLiteral("C:\\Programs\\Katana");
    katana::update::LastResult last;
    last.version = "0.3.0";
    last.transaction = "0a1b2c3d";
    last.status = "refused";
    last.reason = "interrupted_update";
    const std::optional<UpdateView> view = updateViewOfLastResult(last, installed, root);
    ASSERT_TRUE(view.has_value());
    EXPECT_EQ(view->state, UpdateDialogState::RolledBack);
    EXPECT_EQ(view->recoverCommand,
              QStringLiteral("katana_updater --recover \"C:\\Programs\\Katana\""));
    EXPECT_EQ(saidBy(*view),
              QStringLiteral("Katana 0.3.0 could not be installed; nothing was changed. Reason: "
                             "an earlier update is not settled yet. Update again; if this is said "
                             "again, close Katana and run: katana_updater --recover "
                             "\"C:\\Programs\\Katana\""));
    EXPECT_TRUE(katana::qt::updateStatusButtonText(*view).isEmpty());

    // Every other refusal changed nothing and left nothing to settle: no
    // command is carried and none is said (tokens of apply.hpp's list of what
    // an apply is refused for).
    for (const char* reason : {"space", "downgrade", "staged_hash", "platform"}) {
        last.reason = reason;
        const std::optional<UpdateView> other = updateViewOfLastResult(last, installed, root);
        ASSERT_TRUE(other.has_value()) << reason;
        EXPECT_TRUE(other->recoverCommand.isEmpty()) << reason;
        EXPECT_FALSE(saidBy(*other).contains(QStringLiteral("katana_updater"))) << reason;
    }

    // And it is that refusal, not the token under any ending: an install that
    // was changed and put back is told what was restored, and no command.
    last.status = "rolled_back";
    last.reason = "interrupted_update";
    const std::optional<UpdateView> restored = updateViewOfLastResult(last, installed, root);
    ASSERT_TRUE(restored.has_value());
    EXPECT_TRUE(restored->recoverCommand.isEmpty());
    EXPECT_EQ(saidBy(*restored),
              QStringLiteral("Katana 0.3.0 could not be installed, so 0.2.0 was restored. Reason: "
                             "an earlier update is not settled yet."));
}

TEST(UpdateDecisions, TheWindowChecksByItselfOnlyWhenEveryConditionHolds)
{
    using katana::qt::updateAutomaticCheckIsDue;
    using katana::update::InstallKind;
    const std::chrono::seconds noJitter{0};
    const std::chrono::seconds twoHours{2 * 3600};
    // An installed copy, a window a person uses, nothing running, nothing
    // switched off - and never checked, with the automatic flag at its default.
    UpdateAutoCheckFacts go;
    go.kind = InstallKind::Installer;
    go.keepsState = true;
    const katana::update::UpdateState never;
    ASSERT_TRUE(never.automatic) << "the state's default is on";
    EXPECT_TRUE(updateAutomaticCheckIsDue(go, never, kNow, noJitter));
    UpdateAutoCheckFacts unpacked = go;
    unpacked.kind = InstallKind::Archive;
    EXPECT_TRUE(updateAutomaticCheckIsDue(unpacked, never, kNow, noJitter));

    // Each condition, alone, stops it.
    UpdateAutoCheckFacts headless = go;
    headless.headless = true;
    EXPECT_FALSE(updateAutomaticCheckIsDue(headless, never, kNow, noJitter)) << "headless";
    UpdateAutoCheckFacts disabled = go;
    disabled.disabled = true;
    EXPECT_FALSE(updateAutomaticCheckIsDue(disabled, never, kNow, noJitter))
        << "KATANA_UPDATE_DISABLE";
    UpdateAutoCheckFacts development = go;
    development.kind = InstallKind::Development;
    EXPECT_FALSE(updateAutomaticCheckIsDue(development, never, kNow, noJitter))
        << "a development tree stays offline";
    UpdateAutoCheckFacts busy = go;
    busy.jobRunning = true;
    EXPECT_FALSE(updateAutomaticCheckIsDue(busy, never, kNow, noJitter)) << "an update job runs";
    UpdateAutoCheckFacts stateless = go;
    stateless.keepsState = false;
    EXPECT_FALSE(updateAutomaticCheckIsDue(stateless, never, kNow, noJitter)) << "no state folder";
    katana::update::UpdateState switchedOff;
    switchedOff.automatic = false;
    EXPECT_FALSE(updateAutomaticCheckIsDue(go, switchedOff, kNow, noJitter))
        << "the person switched it off";

    // The clock: 24 hours and the jitter after the last check (state.hpp).
    katana::update::UpdateState checked;
    checked.lastCheckEpoch = kNow - 3600;
    EXPECT_FALSE(updateAutomaticCheckIsDue(go, checked, kNow, noJitter)) << "an hour ago";
    checked.lastCheckEpoch = kNow - 24 * 3600;
    EXPECT_TRUE(updateAutomaticCheckIsDue(go, checked, kNow, noJitter)) << "24 hours ago";
    EXPECT_FALSE(updateAutomaticCheckIsDue(go, checked, kNow, twoHours))
        << "24 hours ago, with two hours of jitter still to go";
    checked.lastCheckEpoch = kNow - 26 * 3600;
    EXPECT_TRUE(updateAutomaticCheckIsDue(go, checked, kNow, twoHours)) << "26 hours ago";
    // A last check in the future - the clock was set back - silences nothing.
    checked.lastCheckEpoch = kNow + 3600;
    EXPECT_TRUE(updateAutomaticCheckIsDue(go, checked, kNow, noJitter));
}

TEST(UpdateDecisions, AStartConfirmsAnUpdateOnlyInAWindowOfTheVersionThatWasInstalled)
{
    // The hardening pass's H1 (2026-10-14): an update whose helper died while
    // it waited for the new version to prove that it starts is settled by "an
    // interactive window (kAppliesHere, not headless, not development), first
    // idle turn, only when install.hasInstalledRecord and install.version is
    // the program's own". Each condition is taken away in turn, as the
    // automatic check's are above; the rule is that specification's, and the
    // versions are the design's 0.2.0 and 0.3.0.
    //
    // `installed` is install.version, which the same pass's H3, as it was
    // settled, makes the HIGHER of the build the helper recorded and the
    // build the package names of itself (docs/update.md, "The installed
    // version"). Of the two unequal cases below the first - a helper's record
    // above the program - is the one the rule exists for, and the second is
    // the function's own rule, asked directly.
    using katana::qt::UpdateConfirmFacts;
    using katana::qt::updateStartMayConfirm;
    using katana::update::InstallKind;
    const auto version = [](std::uint32_t minor, std::uint8_t written) {
        katana::update::Version made;
        made.components = {0, minor, 0, 0};
        made.written = written;
        return made;
    };
    // 0.3.0 was installed by an update, and this window is 0.3.0.
    UpdateConfirmFacts go;
    go.appliesHere = true;
    go.keepsState = true;
    go.kind = InstallKind::Installer;
    go.hasInstalledRecord = true;
    go.installed = version(3, 3);
    go.program = version(3, 3);
    EXPECT_TRUE(updateStartMayConfirm(go));
    UpdateConfirmFacts unpacked = go;
    unpacked.kind = InstallKind::Archive;
    EXPECT_TRUE(updateStartMayConfirm(unpacked)) << "an unpacked archive updates itself too";
    // "0.3" and "0.3.0" are one version (update/version.hpp).
    UpdateConfirmFacts shortened = go;
    shortened.program = version(3, 2);
    EXPECT_TRUE(updateStartMayConfirm(shortened));

    // THE CASE THE RULE EXISTS FOR: a window of the old files, started while
    // the update was swapping them. The install's record names 0.3.0 and the
    // program running is 0.2.0: its start says nothing about 0.3.0.
    UpdateConfirmFacts oldWindow = go;
    oldWindow.program = version(2, 3);
    EXPECT_FALSE(updateStartMayConfirm(oldWindow));
    // The rule is "the program's own", so a version below the program's is
    // refused too. A newer zip unpacked by hand over a copy that had updated
    // itself does not hand that over: the helper's record names 0.2.0, the
    // zip's own install.json names 0.3.0, and the higher is read (H3) - the
    // first case above, and that window confirms. What does hand it over is
    // a program of 0.3.0 put by hand into a copy whose package and record
    // both name 0.2.0.
    UpdateConfirmFacts below = go;
    below.installed = version(2, 3);
    EXPECT_FALSE(updateStartMayConfirm(below));

    // Each other condition, alone, stops it.
    UpdateConfirmFacts fresh = go;
    fresh.hasInstalledRecord = false;
    EXPECT_FALSE(updateStartMayConfirm(fresh)) << "no update ever committed here";
    UpdateConfirmFacts headless = go;
    headless.headless = true;
    EXPECT_FALSE(updateStartMayConfirm(headless)) << "a headless run writes no per-user state";
    UpdateConfirmFacts development = go;
    development.kind = InstallKind::Development;
    EXPECT_FALSE(updateStartMayConfirm(development)) << "a build tree is never updated";
    UpdateConfirmFacts stateless = go;
    stateless.keepsState = false;
    EXPECT_FALSE(updateStartMayConfirm(stateless)) << "no update folder to write the proof in";
    UpdateConfirmFacts elsewhere = go;
    elsewhere.appliesHere = false;
    EXPECT_FALSE(updateStartMayConfirm(elsewhere)) << "a copy that cannot replace its own files";
    // And with nothing known, nothing is confirmed.
    EXPECT_FALSE(updateStartMayConfirm(UpdateConfirmFacts{}));
}

TEST(UpdateDecisions, ALineThatWasCarriedOutTakesBackOnlyWhatItsOwnListSetAside)
{
    // What a line the window runs at once does to what the dialog shows. The
    // lines and their replies are the verbs' grammar (update_verbs.hpp:
    // "skipped=<v|none>", and with no update folder "the same with
    // reason=no_state, and nothing is kept") and the hardening pass's ruling
    // H5 (2026-10-14: "UPDATE BLOCKED CLEAR -> blocked=none", "With no update
    // folder: blocked=none reason=no_state, nothing kept"). The rule is the
    // window's: a dialog that says a release "is not offered again" must not
    // go on saying so once the list that set it aside is empty - and each of
    // the two lists is its own.
    using katana::qt::UpdateLineEffect;
    using katana::qt::updateEffectOfLine;
    const QString clear = QStringLiteral("UPDATE BLOCKED CLEAR");
    const QString cleared = QStringLiteral("blocked=none");
    UpdateView putBack = viewOf(UpdateDialogState::NotOffered);
    putBack.reason = QStringLiteral("blocked_before");
    UpdateView skipped = viewOf(UpdateDialogState::NotOffered);
    skipped.reason = QStringLiteral("skipped");

    // The release that was put back is set aside no longer.
    EXPECT_EQ(updateEffectOfLine(putBack, clear, cleared), UpdateLineEffect::Forgotten);
    // Typed as a person types it: in any case, with any blanks.
    EXPECT_EQ(updateEffectOfLine(putBack, QStringLiteral("  update   blocked  clear "), cleared),
              UpdateLineEffect::Forgotten);
    // No update folder: nothing was kept, so nothing was emptied.
    EXPECT_EQ(updateEffectOfLine(putBack, clear, QStringLiteral("blocked=none reason=no_state")),
              UpdateLineEffect::Nothing);
    // The skipped version is another list: this line does not offer it again.
    EXPECT_EQ(updateEffectOfLine(skipped, clear, cleared), UpdateLineEffect::Nothing);
    // And a view that says nothing is set aside has nothing to take back.
    for (const UpdateDialogState state :
         {UpdateDialogState::Idle, UpdateDialogState::Checking, UpdateDialogState::Current,
          UpdateDialogState::Available, UpdateDialogState::Downloading, UpdateDialogState::Ready,
          UpdateDialogState::Blocked, UpdateDialogState::RolledBack,
          UpdateDialogState::CheckFailed, UpdateDialogState::Unsupported}) {
        EXPECT_EQ(updateEffectOfLine(viewOf(state), clear, cleared), UpdateLineEffect::Nothing)
            << static_cast<int>(state);
    }
    // UPDATE BLOCKED has the one line (H5: "with another word or none is
    // InvalidArgument"); nothing else under those words empties anything.
    for (const char* line : {"UPDATE BLOCKED", "UPDATE BLOCKED LIST", "UPDATE BLOCKED CLEAR ALL",
                             "BLOCKED CLEAR UPDATE"}) {
        EXPECT_EQ(updateEffectOfLine(putBack, QString::fromLatin1(line), cleared),
                  UpdateLineEffect::Nothing)
            << line;
    }

    // UPDATE SKIP NONE does the same for its own list, and leaves the other.
    const QString unskip = QStringLiteral("UPDATE SKIP NONE");
    const QString unskipped = QStringLiteral("skipped=none");
    EXPECT_EQ(updateEffectOfLine(skipped, unskip, unskipped), UpdateLineEffect::Forgotten);
    EXPECT_EQ(updateEffectOfLine(skipped, QStringLiteral("update skip none"), unskipped),
              UpdateLineEffect::Forgotten);
    EXPECT_EQ(updateEffectOfLine(putBack, unskip, unskipped), UpdateLineEffect::Nothing);
    EXPECT_EQ(updateEffectOfLine(skipped, unskip, QStringLiteral("skipped=none reason=no_state")),
              UpdateLineEffect::Nothing);

    // UPDATE SKIP <version> of the release the dialog offers: offered no more.
    const UpdateView offered = viewOf(UpdateDialogState::Available);
    ASSERT_EQ(offered.latestVersion, QStringLiteral("0.3.0"));
    EXPECT_EQ(updateEffectOfLine(offered, QStringLiteral("UPDATE SKIP 0.3.0"),
                                 QStringLiteral("skipped=0.3.0")),
              UpdateLineEffect::Skipped);
    // "0.3" and "0.3.0" are one release (update/version.hpp).
    EXPECT_EQ(updateEffectOfLine(offered, QStringLiteral("UPDATE SKIP 0.3"),
                                 QStringLiteral("skipped=0.3")),
              UpdateLineEffect::Skipped);
    // Another version than the one offered leaves the offer as it is.
    EXPECT_EQ(updateEffectOfLine(offered, QStringLiteral("UPDATE SKIP 0.4.0"),
                                 QStringLiteral("skipped=0.4.0")),
              UpdateLineEffect::Nothing);
    // And with no update folder the skip was not kept.
    EXPECT_EQ(updateEffectOfLine(offered, QStringLiteral("UPDATE SKIP 0.3.0"),
                                 QStringLiteral("skipped=none reason=no_state")),
              UpdateLineEffect::Nothing);

    // Every other line changes nothing here, whatever the view.
    const std::pair<const char*, const char*> others[] = {
        {"UPDATE STATUS", "installed=0.2.0 staged=none skipped=none auto=on"},
        {"UPDATE SET AUTO OFF", "auto=off"},
        {"UPDATE TOKEN CLEAR", "token=absent"}};
    for (const auto& [line, reply] : others) {
        for (const UpdateView& view : {putBack, skipped, offered}) {
            EXPECT_EQ(updateEffectOfLine(view, QString::fromLatin1(line),
                                         QString::fromLatin1(reply)),
                      UpdateLineEffect::Nothing)
                << line;
        }
    }
}

TEST(UpdateDecisions, AStateFileThatDoesNotReadIsNobodyCanSayAndNeverNone)
{
    // What Settings > Updates is told of U/state.json. The file's rule
    // (include/katana/update/state.hpp): "absent is the defaults (not an
    // error); unreadable or from a newer schema is ParseFailure /
    // Unsupported". The verbs do not write over a file that does not read
    // (update_verbs.hpp: "refused (InvalidState), and the file is left as it
    // is"; H5 says the same of UPDATE BLOCKED CLEAR) and say why - which a
    // person reads only when the page's button is on. So such a file is
    // "nobody can say": told as "none", the page would say "No version was
    // put back." and switch the button off.
    using katana::core::ErrorCode;
    using katana::qt::updatePutBackOfState;
    using katana::qt::updateSkippedOfState;
    using State = katana::core::Result<katana::update::UpdateState>;
    // The install the page is asked in: a fresh one, with no record of its
    // own build, for which every build on the list counts. Which builds count
    // for an install that has a record is the next test's.
    const katana::qt::UpdatePutBackFacts fresh;

    for (const ErrorCode code : {ErrorCode::ParseFailure, ErrorCode::Unsupported}) {
        const State unreadable =
            katana::core::makeError(code, "the update settings could not be read");
        EXPECT_FALSE(updateSkippedOfState(unreadable).has_value());
        EXPECT_FALSE(updatePutBackOfState(unreadable, fresh).has_value());
    }

    // A file that is not there reads as the defaults, and those ARE known:
    // none skipped, none put back.
    const State defaults = katana::update::UpdateState{};
    const std::optional<QString> noneSkipped = updateSkippedOfState(defaults);
    ASSERT_TRUE(noneSkipped.has_value());
    EXPECT_TRUE(noneSkipped->isEmpty());
    const std::optional<bool> nonePutBack = updatePutBackOfState(defaults, fresh);
    ASSERT_TRUE(nonePutBack.has_value());
    EXPECT_FALSE(*nonePutBack);

    // What the file has: the skipped version as it is written, and whether
    // the list of blocked releases holds any - one is enough.
    katana::update::UpdateState kept;
    kept.skippedVersion = "0.3.0";
    kept.blockedSequences = {kSequence};
    const std::optional<QString> skipped = updateSkippedOfState(State(kept));
    ASSERT_TRUE(skipped.has_value());
    EXPECT_EQ(*skipped, QStringLiteral("0.3.0"));
    const std::optional<bool> putBack = updatePutBackOfState(State(kept), fresh);
    ASSERT_TRUE(putBack.has_value());
    EXPECT_TRUE(*putBack);

    // Each is its own list: a skip is no release that was put back, nor the
    // other way about.
    katana::update::UpdateState onlySkipped;
    onlySkipped.skippedVersion = "0.3.0";
    const std::optional<bool> notPutBack = updatePutBackOfState(State(onlySkipped), fresh);
    ASSERT_TRUE(notPutBack.has_value());
    EXPECT_FALSE(*notPutBack);
    katana::update::UpdateState onlyPutBack;
    onlyPutBack.blockedSequences = {kSequence, kSequence + 1};
    const std::optional<QString> notSkipped = updateSkippedOfState(State(onlyPutBack));
    ASSERT_TRUE(notSkipped.has_value());
    EXPECT_TRUE(notSkipped->isEmpty());
}

TEST(UpdateDecisions, AVersionThatWasPutBackIsToldOfOnlyWhileItCanStillBeOfferedHere)
{
    // THE REGRESSION. The state's list of blocked releases is only ever added
    // to (src/katana_update/apply.cpp, Applier::fail) until UPDATE BLOCKED
    // CLEAR empties it. So after 0.3.0 was put back, and 0.4.0 was installed
    // later and started, Settings > Updates went on saying "A version that
    // was installed here did not start and was put back: it is not offered."
    // with its button on - of a release no feed offers over 0.4.0, and with a
    // button that changed nothing a check could show.
    //
    // The list keeps a build by its sequence, "UTC epoch seconds at build"
    // (include/katana/update/manifest.hpp), and no version; the install's
    // record keeps the sequence of the build an update put there
    // (include/katana/update/install.hpp). The builds here, by hand: 0.3.0 is
    // the design's; 0.2.0 was made a week before it and 0.4.0 a week after
    // it, a week being 7 x 86 400 = 604 800 seconds.
    using katana::qt::UpdatePutBackFacts;
    using katana::qt::updatePutBackOfState;
    using State = katana::core::Result<katana::update::UpdateState>;
    constexpr std::uint64_t kWeek = 604'800;
    constexpr std::uint64_t kBuildOfTheOldVersion = kSequence - kWeek;  // 0.2.0
    constexpr std::uint64_t kBuildOfTheNextVersion = kSequence + kWeek; // 0.4.0
    // The answer for a file that read; one that did not fails here.
    const auto told = [](const katana::update::UpdateState& state,
                         const UpdatePutBackFacts& facts) {
        const std::optional<bool> answer = updatePutBackOfState(State(state), facts);
        EXPECT_TRUE(answer.has_value());
        return answer.value_or(false);
    };
    katana::update::UpdateState putBack;
    putBack.blockedSequences = {kSequence}; // 0.3.0 did not start here

    // 0.2.0 is installed, by an update that recorded its build. 0.3.0 was
    // made after it and is what a feed offers over it: it is told of.
    UpdatePutBackFacts onTheOldVersion;
    onTheOldVersion.hasInstalledRecord = true;
    onTheOldVersion.installedSequence = kBuildOfTheOldVersion;
    EXPECT_TRUE(told(putBack, onTheOldVersion));

    // 0.4.0 was installed since, and started: the build that was put back
    // was made before the one installed, and is not told of.
    UpdatePutBackFacts onTheNextVersion;
    onTheNextVersion.hasInstalledRecord = true;
    onTheNextVersion.installedSequence = kBuildOfTheNextVersion;
    EXPECT_FALSE(told(putBack, onTheNextVersion));

    // "After" is strictly after: a build is not offered over itself.
    UpdatePutBackFacts onThatVeryBuild;
    onThatVeryBuild.hasInstalledRecord = true;
    onThatVeryBuild.installedSequence = kSequence;
    EXPECT_FALSE(told(putBack, onThatVeryBuild));

    // One build made before the installed one and one made after it (0.5.0,
    // a week after 0.4.0): the later one is enough, wherever it stands.
    katana::update::UpdateState two;
    two.blockedSequences = {kSequence, kBuildOfTheNextVersion + kWeek};
    EXPECT_TRUE(told(two, onTheNextVersion));
    two.blockedSequences = {kBuildOfTheNextVersion + kWeek, kSequence};
    EXPECT_TRUE(told(two, onTheNextVersion));

    // A fresh install has no record of its build: its sequence is a
    // placeholder (install.hpp: "the sequence is 0 and `hasInstalledRecord`
    // is false") and is not read, whatever it holds. Nothing says the build
    // on the list is one this install has passed, so it is told of.
    UpdatePutBackFacts fresh;
    EXPECT_TRUE(told(putBack, fresh));
    fresh.installedSequence = kBuildOfTheNextVersion;
    EXPECT_TRUE(told(putBack, fresh));

    // A build made before the installed one can still be of a higher version
    // (a release line that went back and forward again), and the list has no
    // version to tell. A check that meets it does, and the dialog then says
    // "Settings > Updates offers this one again": while it says so the page
    // is told of the build, so that its button is on.
    UpdatePutBackFacts whileTheDialogTellsOfIt = onTheNextVersion;
    whileTheDialogTellsOfIt.dialogTellsOfOne = true;
    EXPECT_TRUE(told(putBack, whileTheDialogTellsOfIt));
    // The list is what the button empties: with nothing on it, none is told
    // of, whatever the dialog still shows.
    EXPECT_FALSE(told(katana::update::UpdateState{}, whileTheDialogTellsOfIt));
    EXPECT_FALSE(told(katana::update::UpdateState{}, onTheOldVersion));

    // And a file that does not read is nobody can say, under every fact.
    const State unreadable = katana::core::makeError(katana::core::ErrorCode::ParseFailure,
                                                     "the update settings could not be read");
    for (const UpdatePutBackFacts& facts :
         {onTheOldVersion, onTheNextVersion, fresh, whileTheDialogTellsOfIt}) {
        EXPECT_FALSE(updatePutBackOfState(unreadable, facts).has_value());
    }
}

TEST(UpdateDecisions, TheDialogTellsOfAReleaseThatIsSetAsideExactlyWhereItPointsAtSettings)
{
    // What keeps the page's button on while the dialog points at it. The two
    // sentences that tell of a release that is set aside end "Settings >
    // Updates offers this one again." (the hardening pass's ruling H5: "the
    // dialog's sentence for a blocked release says where that button is"): a
    // check's `update=blocked`, and the last apply's ending when the release
    // was put back for not starting - rolled_back for health_exit or
    // health_timeout, the endings after which the helper blocks it
    // (src/katana_update/apply.cpp, Applier::fail). updateTellsOfSetAside
    // holds for exactly the views that say it.
    using katana::qt::updateTellsOfSetAside;
    const QString again = QStringLiteral("Settings > Updates offers this one again.");
    const auto ended = [](const char* status, const char* reason) {
        UpdateView view = viewOf(UpdateDialogState::RolledBack);
        view.lastStatus = QString::fromLatin1(status);
        view.reason = QString::fromLatin1(reason);
        return view;
    };
    const auto withReason = [](UpdateDialogState state, const char* reason) {
        UpdateView view = viewOf(state);
        view.reason = QString::fromLatin1(reason);
        return view;
    };

    for (const UpdateView& view :
         {withReason(UpdateDialogState::NotOffered, "blocked_before"),
          ended("rolled_back", "health_exit"), ended("rolled_back", "health_timeout")}) {
        EXPECT_TRUE(updateTellsOfSetAside(view)) << saidBy(view).toStdString();
        EXPECT_TRUE(katana::qt::updateDetailText(view).endsWith(again))
            << saidBy(view).toStdString();
    }

    // A skipped release is another list's; an install that was put back for
    // another reason, or not put back, set nothing aside; and the token under
    // a state that does not read it tells of nothing.
    for (const UpdateView& view :
         {withReason(UpdateDialogState::NotOffered, "skipped"),
          ended("rolled_back", "rename_failed"), ended("refused", "health_exit"),
          ended("rollback_incomplete", "health_timeout"), ended("rolled_back", ""),
          withReason(UpdateDialogState::Available, "blocked_before"),
          withReason(UpdateDialogState::CheckFailed, "blocked_before"),
          viewOf(UpdateDialogState::Idle), viewOf(UpdateDialogState::Checking),
          viewOf(UpdateDialogState::Current), viewOf(UpdateDialogState::Downloading),
          viewOf(UpdateDialogState::Ready), viewOf(UpdateDialogState::Blocked),
          viewOf(UpdateDialogState::Unsupported)}) {
        EXPECT_FALSE(updateTellsOfSetAside(view)) << saidBy(view).toStdString();
        EXPECT_FALSE(katana::qt::updateDetailText(view).endsWith(again))
            << saidBy(view).toStdString();
    }
}

TEST(UpdateDecisions, AnUpdateThatCouldNotBeStartedIsBoxedOnlyInAWindowNobodyDrove)
{
    // The hardening pass's point for main() (2026-10-14): a helper that could
    // not be started is said "on stderr always, and show the box only when the
    // run was given no --command". The kinds are main()'s own: it keeps each
    // step under its switch's name, an --enter as a --command, and a
    // --run-line runs a line as a --command does (docs/headless.md).
    using katana::qt::updateFailureIsBoxed;
    // A window a person started: nobody reads a GUI program's stderr.
    EXPECT_TRUE(updateFailureIsBoxed({}));
    // A run that was given a line to run may have nobody at it to close a box.
    EXPECT_FALSE(updateFailureIsBoxed({QStringLiteral("--command")}));
    EXPECT_FALSE(updateFailureIsBoxed({QStringLiteral("--run-line")}));
    // Wherever among its steps the line stands, and however many there are.
    EXPECT_FALSE(updateFailureIsBoxed({QStringLiteral("--dialog"), QStringLiteral("--press"),
                                       QStringLiteral("--command")}));
    EXPECT_FALSE(updateFailureIsBoxed({QStringLiteral("--run-line"), QStringLiteral("--report")}));
    EXPECT_FALSE(updateFailureIsBoxed({QStringLiteral("--command"), QStringLiteral("--command"),
                                       QStringLiteral("--run-line")}));
    // Steps that run no line give the run no --command: it is still boxed.
    EXPECT_TRUE(updateFailureIsBoxed({QStringLiteral("--dialog"), QStringLiteral("--press")}));
}

TEST(UpdateDecisions, TheStatusBarButtonNamesTheReleaseAndIsHiddenUntilOneIsKnown)
{
    using katana::qt::updateStatusButtonText;
    // "Update <version>" while there is a release to act on.
    for (const UpdateDialogState state :
         {UpdateDialogState::Available, UpdateDialogState::Downloading, UpdateDialogState::Ready,
          UpdateDialogState::Blocked}) {
        EXPECT_EQ(updateStatusButtonText(viewOf(state)), QStringLiteral("Update 0.3.0"))
            << static_cast<int>(state);
    }
    // Hidden - no text - in every other state, though a version is named...
    for (const UpdateDialogState state :
         {UpdateDialogState::Idle, UpdateDialogState::Checking, UpdateDialogState::Current,
          UpdateDialogState::NotOffered, UpdateDialogState::RolledBack,
          UpdateDialogState::CheckFailed, UpdateDialogState::Unsupported}) {
        EXPECT_TRUE(updateStatusButtonText(viewOf(state)).isEmpty()) << static_cast<int>(state);
    }
    // ...as made, before anything is known, and for a download nobody named.
    EXPECT_TRUE(updateStatusButtonText(UpdateView{}).isEmpty());
    const UpdateView unnamed = viewOfVersion(UpdateDialogState::Downloading, "");
    EXPECT_TRUE(updateStatusButtonText(unnamed).isEmpty());
}

TEST(UpdateDecisions, ARecordOfAReplyIsReadByItsKeyAndTheLastOneCounts)
{
    using katana::qt::updateReplyValue;
    using katana::qt::updateStagedOfReply;
    // The grammar of update_verbs.hpp: records `key=value`, blanks between.
    const QString check = QStringLiteral("update=unreachable installed=0.2.0 reason=offline");
    EXPECT_EQ(updateReplyValue(check, "reason"), QStringLiteral("offline"));
    EXPECT_EQ(updateReplyValue(check, "update"), QStringLiteral("unreachable"));
    EXPECT_TRUE(updateReplyValue(check, "staged").isEmpty());
    // A key is a whole word's start, not the end of another key.
    EXPECT_TRUE(updateReplyValue(QStringLiteral("unstaged=0.3.0"), "staged").isEmpty());
    EXPECT_EQ(updateReplyValue(QStringLiteral("notes_url=\"https://example.org/v0.3.0\""),
                               "notes_url"),
              QStringLiteral("https://example.org/v0.3.0"));

    // DOWNLOAD: "progress lines, then staged=<v>" - the last record is the answer.
    const QString download = QStringLiteral("progress bytes=31000000/71000000 files=12/23\n"
                                            "progress bytes=71000000/71000000 files=23/23\r\n"
                                            "staged=0.3.0\n");
    EXPECT_EQ(updateStagedOfReply(download), QStringLiteral("0.3.0"));
    EXPECT_EQ(updateStagedOfReply(QStringLiteral("staged=none\nstaged=0.3.0")),
              QStringLiteral("0.3.0"));
    // Nothing staged: said as none, or not said at all.
    EXPECT_TRUE(updateStagedOfReply(QStringLiteral("staged=none reason=headless")).isEmpty());
    EXPECT_TRUE(updateStagedOfReply(QStringLiteral("reason=offline")).isEmpty());
    EXPECT_TRUE(updateStagedOfReply(QString()).isEmpty());
}

TEST(UpdateDecisions, WhatTheVerbsSayBackToALineWithATokenTypedOnItIsHiddenInAnyCase)
{
    // The echo of such a line is hidden by updateLoggedLine; a refusal may
    // quote the line all the same, and may have folded its case to find the
    // verb. Whatever is logged of it holds the word in no case.
    using katana::qt::updateLoggedReply;
    const QString token = QStringLiteral("ghp_NotARealTokenOnlyATestsWord0123456789");
    const QString line = QStringLiteral("update token ") + token;
    EXPECT_EQ(updateLoggedReply(QStringLiteral("InvalidArgument: UPDATE TOKEN takes STATUS or "
                                               "CLEAR [UPDATE TOKEN ") +
                                    token.toUpper() + QStringLiteral("]"),
                                line),
              QStringLiteral("InvalidArgument: UPDATE TOKEN takes STATUS or CLEAR [UPDATE TOKEN "
                             "***]"));
    EXPECT_EQ(updateLoggedReply(QStringLiteral("no such word: ") + token, line),
              QStringLiteral("no such word: ***"));
    EXPECT_EQ(updateLoggedReply(QStringLiteral("no such word: ") + token.toLower(), line),
              QStringLiteral("no such word: ***"));

    // The two lines the grammar has, and every other line, hide nothing.
    EXPECT_EQ(updateLoggedReply(QStringLiteral("token=present"),
                                QStringLiteral("UPDATE TOKEN STATUS")),
              QStringLiteral("token=present"));
    EXPECT_EQ(updateLoggedReply(QStringLiteral("skipped=0.3.0"),
                                QStringLiteral("UPDATE SKIP 0.3.0")),
              QStringLiteral("skipped=0.3.0"));
    EXPECT_EQ(updateLoggedReply(QStringLiteral("update=current"), QString()),
              QStringLiteral("update=current"));
}

namespace {

using Ids = std::vector<std::string>;

// Two transaction ids of the length update::newTransactionId gives them, 32
// characters of lower-case hex, written by hand.
const std::string kCrashed = "0123456789abcdef0123456789abcdef";
const std::string kPowerCut = "fedcba9876543210fedcba9876543210";

// The line a run of `katana_updater --recover` appends, by recover.hpp's form.
std::string recoveryLine(const std::string& found, const char* result, const std::string& left)
{
    return "2026-10-13T08:40:00Z recover found=" + found + " result=" + result + " left=" + left +
           "\n";
}

// An install as updateRecoveredTransactions asks about it: whether the folder
// R/.update/<id> is gone, for the ids in `gone`; and which ids it was asked
// about, in the order it was asked.
struct AskedInstall {
    Ids gone;
    Ids asked;

    [[nodiscard]] std::function<bool(const std::string&)> isOver()
    {
        return [this](const std::string& id) {
            asked.push_back(id);
            return std::find(gone.begin(), gone.end(), id) != gone.end();
        };
    }
};

} // namespace

TEST(UpdateDecisions, TheHelperCopyOfARecoveredUpdateIsListedOnlyOnceItsTransactionIsOver)
{
    using katana::qt::updateRecoveredTransactions;
    // An update that a crash interrupted, put right at the next start: the
    // recovery found its folder and left none.
    const std::string log = recoveryLine(kCrashed, "ok", "");

    // The folder R/.update/<tx> is gone: the transaction is over, and its
    // helper copy U/run-<tx> is one to remove.
    AskedInstall recovered;
    recovered.gone = {kCrashed};
    EXPECT_EQ(updateRecoveredTransactions(log, recovered.isOver()), Ids{kCrashed});
    EXPECT_EQ(recovered.asked, Ids{kCrashed});

    // The folder is still there - the rollback failed, and the next try runs
    // from that very copy: it is not one to remove.
    AskedInstall halfSwapped;
    EXPECT_TRUE(updateRecoveredTransactions(recoveryLine(kCrashed, "failed", kCrashed),
                                            halfSwapped.isOver())
                    .empty());
    EXPECT_EQ(halfSwapped.asked, Ids{kCrashed});
}

TEST(UpdateDecisions, ATransactionIsAskedAboutOnceHoweverManyRecoveriesNamedIt)
{
    using katana::qt::updateRecoveredTransactions;
    // A recovery that failed, the one that then put the install right and found
    // a second interrupted update beside it, and a third run that names the
    // second again: two transactions, the first one named first.
    const std::string log =
        "2026-10-13T08:40:00Z recover found=" + kCrashed + " result=failed left=" + kCrashed +
        " detail=InvalidState: an interrupted update could not be put back\n" +
        recoveryLine(kCrashed + "," + kPowerCut, "ok", "") + recoveryLine(kPowerCut, "ok", "");

    AskedInstall bothOver;
    bothOver.gone = {kPowerCut, kCrashed};
    EXPECT_EQ(updateRecoveredTransactions(log, bothOver.isOver()), (Ids{kCrashed, kPowerCut}));
    EXPECT_EQ(bothOver.asked, (Ids{kCrashed, kPowerCut}));

    // Only the second is over: only its copy is one to remove.
    AskedInstall oneOver;
    oneOver.gone = {kPowerCut};
    EXPECT_EQ(updateRecoveredTransactions(log, oneOver.isOver()), Ids{kPowerCut});
    EXPECT_EQ(oneOver.asked, (Ids{kCrashed, kPowerCut}));

    // A long log is no more questions: 2000 lines naming one transaction ask
    // about it once and name it once.
    std::string longLog;
    for (int line = 0; line < 2000; ++line) {
        longLog += recoveryLine(kCrashed, "ok", "");
    }
    AskedInstall often;
    often.gone = {kCrashed};
    EXPECT_EQ(updateRecoveredTransactions(longLog, often.isOver()), Ids{kCrashed});
    EXPECT_EQ(often.asked, Ids{kCrashed});
}

TEST(UpdateDecisions, OnlyAWholeLineThatARecoveryWroteNamesATransaction)
{
    using katana::qt::updateRecoveredTransactions;
    const std::string stamp = "2026-10-13T08:40:00Z ";
    const std::pair<const char*, std::string> lines[] = {
        // The log quotes a journal record that a rollback refused, and a
        // journal is anybody's text: its second word is `journal`, whatever
        // the record goes on to say.
        {"a refused journal record",
         kCrashed + " journal record refused: REPLACE x recover found=" + kPowerCut +
             " result=ok left=: outside the install\n"},
        // `recover` is the SECOND word and `found=` the third.
        {"no time", "recover found=" + kCrashed + " result=ok left=\n"},
        {"found= is not the third word",
         stamp + "recover result=ok found=" + kCrashed + " left=\n"},
        {"another mode's line", stamp + "apply found=" + kCrashed + " result=ok left=\n"},
        // `result=` is the fourth: it is what says the list before it is whole.
        {"the list ends the line", stamp + "recover found=" + kCrashed + "\n"},
        {"no result", stamp + "recover found=" + kCrashed + " left=\n"},
        // A write that was cut short has no line feed, and is not read though
        // every word of it is there...
        {"no line feed", stamp + "recover found=" + kCrashed + " result=ok left="},
        // ...and one cut inside its list ends in what is left of an id, which
        // is still hex and would name a folder of its own.
        {"cut inside the id", stamp + "recover found=" + kCrashed.substr(0, 12)},
    };
    for (const auto& [what, line] : lines) {
        AskedInstall install;
        install.gone = {kCrashed, kPowerCut, kCrashed.substr(0, 12)};
        EXPECT_TRUE(updateRecoveredTransactions(line, install.isOver()).empty()) << what;
        EXPECT_TRUE(install.asked.empty()) << what;
    }

    // A whole line before a torn one still counts, and the torn one does not.
    AskedInstall install;
    install.gone = {kCrashed, kPowerCut};
    EXPECT_EQ(updateRecoveredTransactions(recoveryLine(kCrashed, "ok", "") + stamp +
                                              "recover found=" + kPowerCut + " result=ok left=",
                                          install.isOver()),
              Ids{kCrashed});
    EXPECT_EQ(install.asked, Ids{kCrashed});
}

TEST(UpdateDecisions, WhatIsNoTransactionIdInARecoveryLineNamesNoFolder)
{
    using katana::qt::updateRecoveredTransactions;
    // Beside one real id: a way out of the folder, a drive, upper-case hex,
    // nothing at all between two commas, 65 characters, and a folder's own name.
    // None is 1 to 64 characters of 0-9 and a-f, so none is asked about - an
    // id is joined to U/run- and to R/.update/ as it stands.
    const std::string list = "../../outside,C:/Windows,0123456789ABCDEF,," +
                             std::string(65, 'f') + ",run-" + kCrashed + "," + kPowerCut;
    AskedInstall install;
    install.gone = {"../../outside",      "C:/Windows",      "0123456789ABCDEF", "",
                    std::string(65, 'f'), "run-" + kCrashed, kPowerCut};
    EXPECT_EQ(updateRecoveredTransactions(recoveryLine(list, "ok", ""), install.isOver()),
              Ids{kPowerCut});
    EXPECT_EQ(install.asked, Ids{kPowerCut});
}

TEST(UpdateDecisions, ARecoveryLogThatNamesNothingListsNothing)
{
    using katana::qt::updateRecoveredTransactions;
    AskedInstall install;
    install.gone = {kCrashed};
    // No log, empty lines, and the line of a run that failed before it found
    // anything (an empty list).
    EXPECT_TRUE(updateRecoveredTransactions("", install.isOver()).empty());
    EXPECT_TRUE(updateRecoveredTransactions("\n\n\n", install.isOver()).empty());
    EXPECT_TRUE(updateRecoveredTransactions(
                    "2026-10-13T08:40:00Z recover found= result=failed left= detail=Internal: "
                    "looking for an interrupted update stopped on an unexpected error\n",
                    install.isOver())
                    .empty());
    EXPECT_TRUE(install.asked.empty());

    // With nobody to say whether a transaction is over, none is known to be.
    EXPECT_TRUE(updateRecoveredTransactions(recoveryLine(kCrashed, "ok", ""), {}).empty());
}

// ---- Settings > Updates -------------------------------------------------------------------------

namespace {

// A credential store that records what was written to it and holds one token.
class RecordingStore final : public katana::update::CredentialStore {
  public:
    std::optional<std::string> token;
    std::vector<std::string> written;
    int cleared = 0;
    // What write answers in the place of storing. Null: it stores. (Pointed
    // to, not held in an optional: GCC 16's optimiser cannot follow an
    // optional of an Error that is assigned after the fixture is built, and
    // warns that the Error's strings may be read before they are written.)
    std::shared_ptr<const katana::core::Error> refusal;

    [[nodiscard]] katana::core::Result<std::string> read() const override
    {
        if (!token) {
            return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                           "no access token is stored");
        }
        return *token;
    }
    [[nodiscard]] katana::core::Status write(std::string_view value) override
    {
        if (refusal) {
            return *refusal;
        }
        written.emplace_back(value);
        token = std::string(value);
        return {};
    }
    [[nodiscard]] katana::core::Status clear() override
    {
        ++cleared;
        token.reset();
        return {};
    }
    [[nodiscard]] std::string_view where() const override { return "credential manager"; }
};

// A token of the shape a release host issues; no real one.
const QString kToken = QStringLiteral("ghp_NotARealTokenOnlyATestsWord0123456789");

// Settings with its Updates page filled and NO drawing: the page is of the
// program, so none of these tests gives it one.
struct UpdatesFixture {
    SettingsContext context;
    RecordingStore store;
    QStringList ran;
    std::vector<std::pair<QString, bool>> logged;
    bool automatic = true;
    QString skipped;
    bool putBack = false; // a release that did not start here is set aside
    int tokenChanges = 0;
    // What a line does and answers. Unset: nothing, and that it was carried out.
    std::function<VerbOutcome(const QString& line)> answer;

    UpdatesFixture()
    {
        context.run = [this](const QString& line) {
            ran << line;
            return answer ? answer(line) : carriedOut();
        };
        context.log = [this](const QString& message, bool isError) {
            logged.emplace_back(message, isError);
        };
        context.updates.automatic = [this] { return automatic; };
        context.updates.skippedVersion = [this] { return skipped; };
        context.updates.putBack = [this] { return putBack; };
        context.updates.credentials = &store;
        context.updates.tokenChanged = [this] { ++tokenChanges; };
        context.updates.userAgent = QStringLiteral("Katana/0.2.0 (win64)");
        context.updates.server = QStringLiteral("GitHub");
    }

    // Everything the dialog said anywhere a token could leak to: its status,
    // every label, and the log it was given.
    [[nodiscard]] QString everythingSaid(const QWidget& dialog) const
    {
        QStringList said;
        said << child<QPlainTextEdit>(dialog, "settingsStatus")->toPlainText();
        for (const QLabel* label : dialog.findChildren<QLabel*>()) {
            said << label->text();
        }
        for (const QAbstractButton* button : dialog.findChildren<QAbstractButton*>()) {
            said << button->text() << button->toolTip();
        }
        for (const auto& [message, isError] : logged) {
            said << message;
        }
        said << ran;
        return said.join(QLatin1Char('\n'));
    }
};

QString status(const QWidget& dialog)
{
    return child<QPlainTextEdit>(dialog, "settingsStatus")->toPlainText();
}

bool saysARefusal(const QWidget& dialog)
{
    return child<QPlainTextEdit>(dialog, "settingsStatus")
        ->styleSheet()
        .contains(katana::qt::theme::error().name());
}

} // namespace

TEST(SettingsUpdates, ThePageIsTheSecondOfSettingsWithANameForEveryControl)
{
    UpdatesFixture f;
    SettingsDialog dialog(f.context);
    const auto* pages = child<QListWidget>(dialog, "settingsPages");
    ASSERT_EQ(pages->count(), 2);
    EXPECT_EQ(pages->item(1)->text(), QStringLiteral("Updates"));
    auto* stack = child<QStackedWidget>(dialog, "settingsStack");
    ASSERT_EQ(stack->count(), 2);
    const QWidget* page = stack->widget(1);
    ASSERT_NE(page, nullptr);
    EXPECT_EQ(page->objectName(), QStringLiteral("settingsUpdates"));

    for (const char* name :
         {"settingsUpdateNote", "settingsUpdateAuto", "settingsUpdateSends", "settingsUpdateToken",
          "settingsUpdateTokenSave", "settingsUpdateTokenClear", "settingsUpdateTokenState",
          "settingsUpdateSkipped", "settingsUpdateUnskip", "settingsUpdateBlocked",
          "settingsUpdateUnblock"}) {
        EXPECT_NE(page->findChild<QWidget*>(QString::fromLatin1(name)), nullptr) << name;
    }
    // The words the design gives the controls.
    EXPECT_EQ(child<QCheckBox>(dialog, "settingsUpdateAuto")->text(),
              QStringLiteral("Check for updates automatically"));
    EXPECT_EQ(child<QPushButton>(dialog, "settingsUpdateUnskip")->text(),
              QStringLiteral("Offer the skipped version again"));
    EXPECT_EQ(child<QPushButton>(dialog, "settingsUpdateTokenSave")->text(),
              QStringLiteral("Save"));
    EXPECT_EQ(child<QPushButton>(dialog, "settingsUpdateTokenClear")->text(),
              QStringLiteral("Clear"));
    // A token is typed as dots.
    EXPECT_EQ(child<QLineEdit>(dialog, "settingsUpdateToken")->echoMode(), QLineEdit::Password);

    // Turned to as the headless driver's settingsPages=Updates turns to it.
    child<QListWidget>(dialog, "settingsPages")->setCurrentRow(1);
    EXPECT_EQ(stack->currentWidget(), page);
    EXPECT_TRUE(f.ran.isEmpty()) << "showing the page runs nothing";
    katana::qt::test::paint(dialog);
}

TEST(SettingsUpdates, ThePageSaysWhatACheckSendsAndThatTheServerSeesTheAddress)
{
    UpdatesFixture f;
    SettingsDialog dialog(f.context);
    EXPECT_EQ(shown(dialog, "settingsUpdateSends"),
              QStringLiteral("A check asks GitHub for the newest version. It sends only "
                             "\"Katana/0.2.0 (win64)\" as its user agent - nothing about you, "
                             "your files or your drawings - and, as with every website, GitHub "
                             "sees this computer's network address."));

    // With nobody to say which server or what user agent, the shape of both.
    UpdatesFixture bare;
    bare.context.updates.userAgent.clear();
    bare.context.updates.server.clear();
    SettingsDialog shape(bare.context);
    EXPECT_EQ(shown(shape, "settingsUpdateSends"),
              QStringLiteral("A check asks the release server for the newest version. It sends "
                             "only \"Katana/<version> (<target>)\" as its user agent - nothing "
                             "about you, your files or your drawings - and, as with every "
                             "website, the release server sees this computer's network "
                             "address."));
}

TEST(SettingsUpdates, TheBoxShowsWhatTheStateHasAndTogglingItRunsItsLineOnce)
{
    UpdatesFixture f;
    // The verb, as far as the page can tell: it writes the state.
    f.answer = [&f](const QString& line) {
        if (line == QStringLiteral("UPDATE SET AUTO OFF")) {
            f.automatic = false;
        } else if (line == QStringLiteral("UPDATE SET AUTO ON")) {
            f.automatic = true;
        }
        return carriedOut();
    };
    SettingsDialog dialog(f.context);
    auto* box = child<QCheckBox>(dialog, "settingsUpdateAuto");
    EXPECT_TRUE(box->isChecked()) << "on by default, as the state has it";
    EXPECT_TRUE(f.ran.isEmpty()) << "showing the state runs nothing";

    box->click();
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE SET AUTO OFF")}));
    EXPECT_FALSE(box->isChecked());
    EXPECT_EQ(status(dialog), QStringLiteral("> UPDATE SET AUTO OFF\nDone."));
    EXPECT_FALSE(saysARefusal(dialog));

    // Set as the headless driver's --fill sets a box: toggled, not clicked.
    box->setChecked(true);
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE SET AUTO OFF"),
                                  QStringLiteral("UPDATE SET AUTO ON")}));
    EXPECT_TRUE(box->isChecked());
}

TEST(SettingsUpdates, ABoxWhoseLineWasRefusedGoesBackToWhatTheStateHas)
{
    UpdatesFixture f;
    f.answer = [](const QString&) {
        return VerbOutcome{false, {},
                           QStringLiteral("Unsupported: a headless run keeps no update settings")};
    };
    SettingsDialog dialog(f.context);
    auto* box = child<QCheckBox>(dialog, "settingsUpdateAuto");
    ASSERT_TRUE(box->isChecked());
    box->click();
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE SET AUTO OFF")})) << "once, not again";
    EXPECT_TRUE(box->isChecked()) << "the state still has it on";
    EXPECT_EQ(status(dialog),
              QStringLiteral("> UPDATE SET AUTO OFF\nRefused: Unsupported: a headless run keeps "
                             "no update settings"));
    EXPECT_TRUE(saysARefusal(dialog));
}

TEST(SettingsUpdates, TurningToThePageReadsTheStateAgainAndRunsNothing)
{
    UpdatesFixture f;
    SettingsDialog dialog(f.context);
    ASSERT_TRUE(child<QCheckBox>(dialog, "settingsUpdateAuto")->isChecked());
    // Changed elsewhere since: a typed UPDATE SET AUTO OFF, the update dialog's Skip.
    f.automatic = false;
    f.skipped = QStringLiteral("0.3.0");
    child<QListWidget>(dialog, "settingsPages")->setCurrentRow(1);
    EXPECT_FALSE(child<QCheckBox>(dialog, "settingsUpdateAuto")->isChecked());
    EXPECT_EQ(shown(dialog, "settingsUpdateSkipped"),
              QStringLiteral("Katana 0.3.0 is skipped: it is not offered."));
    EXPECT_TRUE(f.ran.isEmpty()) << f.ran.join(" | ").toStdString();
}

TEST(SettingsUpdates, SaveWritesTheCredentialStoreAndNeverProducesALine)
{
    UpdatesFixture f;
    SettingsDialog dialog(f.context);
    EXPECT_EQ(shown(dialog, "settingsUpdateTokenState"),
              QStringLiteral("No access token is stored."));
    auto* field = child<QLineEdit>(dialog, "settingsUpdateToken");
    // Pasted with the blanks a copy brings: they are no part of the token.
    field->setText(QStringLiteral("  ") + kToken + QStringLiteral(" \n"));
    press(dialog, "settingsUpdateTokenSave");

    ASSERT_EQ(f.store.written.size(), 1u);
    EXPECT_EQ(f.store.written.front(), kToken.toStdString());
    // A token on a line would be echoed in the command log: there is no line.
    EXPECT_TRUE(f.ran.isEmpty()) << "Save ran a line";
    EXPECT_TRUE(f.logged.empty()) << "Save logged something";
    EXPECT_EQ(f.tokenChanges, 1);
    EXPECT_TRUE(field->text().isEmpty()) << "the field is emptied";
    EXPECT_EQ(status(dialog), QStringLiteral("The access token was saved to the credential "
                                             "manager. It is not shown again."));
    EXPECT_FALSE(saysARefusal(dialog));
    EXPECT_EQ(shown(dialog, "settingsUpdateTokenState"),
              QStringLiteral("An access token is stored in the credential manager."));
    // And nothing the dialog says anywhere holds it.
    EXPECT_FALSE(f.everythingSaid(dialog).contains(kToken));
}

TEST(SettingsUpdates, ATokenTheStoreRefusesIsSaidInTheStoresWordsAndNeverShown)
{
    UpdatesFixture f;
    // As update/credential.hpp says a store refuses: the message names the
    // rule and the place, never the value.
    f.store.refusal = std::make_shared<const katana::core::Error>(katana::core::makeError(
        katana::core::ErrorCode::InvalidArgument, "the access token has a space inside it"));
    SettingsDialog dialog(f.context);
    auto* field = child<QLineEdit>(dialog, "settingsUpdateToken");
    field->setText(kToken);
    press(dialog, "settingsUpdateTokenSave");

    EXPECT_TRUE(f.store.written.empty());
    EXPECT_TRUE(f.ran.isEmpty());
    EXPECT_EQ(f.tokenChanges, 0);
    EXPECT_EQ(status(dialog), QStringLiteral("Save: the access token has a space inside it"));
    EXPECT_TRUE(saysARefusal(dialog));
    // Logged, as every refusal the dialog words itself is - without the token.
    ASSERT_EQ(f.logged.size(), 1u);
    EXPECT_TRUE(f.logged.front().second);
    EXPECT_FALSE(f.everythingSaid(dialog).contains(kToken));
    // What was typed stays, to be corrected.
    EXPECT_EQ(field->text(), kToken);
}

TEST(SettingsUpdates, SaveWithNothingTypedWritesNothingAndSaysWhy)
{
    UpdatesFixture f;
    SettingsDialog dialog(f.context);
    for (const QString& typed : {QString(), QStringLiteral("   ")}) {
        child<QLineEdit>(dialog, "settingsUpdateToken")->setText(typed);
        press(dialog, "settingsUpdateTokenSave");
        EXPECT_TRUE(f.store.written.empty());
        EXPECT_EQ(status(dialog),
                  QStringLiteral("Save: no access token is typed. Paste one, or press Clear to "
                                 "remove the stored one."));
        EXPECT_TRUE(saysARefusal(dialog));
    }
    EXPECT_TRUE(f.ran.isEmpty());
    EXPECT_EQ(f.tokenChanges, 0);
}

TEST(SettingsUpdates, ClearRunsItsLineAndLeavesNoTokenTypedEither)
{
    UpdatesFixture f;
    f.store.token = kToken.toStdString();
    // The verb, as far as the page can tell: it empties the store.
    f.answer = [&f](const QString&) {
        f.store.token.reset();
        return VerbOutcome{true, QStringLiteral("token=absent"), {}};
    };
    SettingsDialog dialog(f.context);
    ASSERT_EQ(shown(dialog, "settingsUpdateTokenState"),
              QStringLiteral("An access token is stored in the credential manager."));
    auto* field = child<QLineEdit>(dialog, "settingsUpdateToken");
    field->setText(QStringLiteral("half a token"));
    press(dialog, "settingsUpdateTokenClear");

    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE TOKEN CLEAR")}));
    EXPECT_EQ(f.store.cleared, 0) << "the verb clears the store, not the dialog";
    EXPECT_TRUE(field->text().isEmpty());
    EXPECT_EQ(status(dialog), QStringLiteral("> UPDATE TOKEN CLEAR\ntoken=absent"));
    EXPECT_EQ(shown(dialog, "settingsUpdateTokenState"),
              QStringLiteral("No access token is stored."));
}

TEST(SettingsUpdates, OfferTheSkippedVersionAgainRunsItsLineAndIsOffWhenNoneIsSkipped)
{
    UpdatesFixture f;
    f.skipped = QStringLiteral("0.3.0");
    f.answer = [&f](const QString&) {
        f.skipped.clear();
        return carriedOut();
    };
    SettingsDialog dialog(f.context);
    EXPECT_EQ(shown(dialog, "settingsUpdateSkipped"),
              QStringLiteral("Katana 0.3.0 is skipped: it is not offered."));
    ASSERT_TRUE(enabled(dialog, "settingsUpdateUnskip"));
    press(dialog, "settingsUpdateUnskip");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE SKIP NONE")}));
    EXPECT_EQ(shown(dialog, "settingsUpdateSkipped"), QStringLiteral("No version is skipped."));
    EXPECT_FALSE(enabled(dialog, "settingsUpdateUnskip")) << "nothing left to offer again";
}

// ---- the version that was put back (the hardening pass's ruling H5, 2026-10-14) -----------------
//
// "Window: Settings > Updates gets a button under 'Offer the skipped version
// again': object name settingsUpdateUnblock, text 'Offer the version that was
// put back again', which runs that line" - UPDATE BLOCKED CLEAR, whose reply
// is blocked=none - "through the same path settingsUpdateUnskip uses". The
// name, the words, the line and the reply below are copied from that ruling.

TEST(SettingsUpdates, OfferTheVersionThatWasPutBackAgainIsAButtonUnderTheSkippedVersionsOne)
{
    UpdatesFixture f;
    SettingsDialog dialog(f.context);
    QWidget* page = child<QWidget>(dialog, "settingsUpdates");
    ASSERT_NE(page, nullptr);
    const auto* unblock = page->findChild<QPushButton*>(QStringLiteral("settingsUpdateUnblock"));
    ASSERT_NE(unblock, nullptr) << "on the Updates page, by its name";
    EXPECT_EQ(unblock->text(), QStringLiteral("Offer the version that was put back again"));
    // It says what it does, and by which line.
    EXPECT_TRUE(unblock->toolTip().contains(QStringLiteral("UPDATE BLOCKED CLEAR")))
        << unblock->toolTip().toStdString();
    // Like every button of Settings it is no default: Enter presses nothing.
    EXPECT_FALSE(unblock->isDefault());
    EXPECT_FALSE(unblock->autoDefault());
    EXPECT_NE(page->findChild<QLabel*>(QStringLiteral("settingsUpdateBlocked")), nullptr);

    // Under the skipped version's: each button is in a box of the page's
    // column, and a column lays its boxes out from the top in their order.
    const auto* unskip = child<QPushButton>(dialog, "settingsUpdateUnskip");
    const QLayout* column = page->layout();
    ASSERT_NE(column, nullptr);
    const int skippedAt = column->indexOf(unskip->parentWidget());
    const int putBackAt = column->indexOf(unblock->parentWidget());
    ASSERT_GE(skippedAt, 0) << "the skipped version's box is one of the column's";
    EXPECT_GT(putBackAt, skippedAt);
    // And as a person sees the page: turned to and shown, it is the lower one.
    child<QListWidget>(dialog, "settingsPages")->setCurrentRow(1);
    dialog.show();
    katana::qt::test::processEvents();
    EXPECT_TRUE(unblock->isVisibleTo(&dialog));
    EXPECT_GT(unblock->mapTo(&dialog, QPoint(0, 0)).y(), unskip->mapTo(&dialog, QPoint(0, 0)).y());

    EXPECT_TRUE(f.ran.isEmpty()) << "showing the page runs nothing";
    katana::qt::test::paint(dialog);
}

TEST(SettingsUpdates, OfferTheVersionThatWasPutBackAgainBuildsExactlyItsLineAndNothingElse)
{
    UpdatesFixture f;
    f.putBack = true;
    f.skipped = QStringLiteral("0.3.0");
    f.store.token = kToken.toStdString();
    // The verb, as far as the page can tell: it empties the list, and answers
    // as the ruling gives its reply.
    f.answer = [&f](const QString&) {
        f.putBack = false;
        return VerbOutcome{true, QStringLiteral("blocked=none"), {}};
    };
    SettingsDialog dialog(f.context);
    EXPECT_EQ(shown(dialog, "settingsUpdateBlocked"),
              QStringLiteral("A version that was installed here did not start and was put back: "
                             "it is not offered."));
    ASSERT_TRUE(enabled(dialog, "settingsUpdateUnblock"));
    EXPECT_TRUE(f.ran.isEmpty()) << "showing the state runs nothing";

    press(dialog, "settingsUpdateUnblock");
    // The one line, once, as a person would type it.
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE BLOCKED CLEAR")}));
    // And nothing else: the dialog writes no store, tells nobody the token
    // changed, and words nothing of its own into the log - the executor logs
    // the line it ran.
    EXPECT_TRUE(f.store.written.empty());
    EXPECT_EQ(f.store.cleared, 0);
    EXPECT_EQ(f.tokenChanges, 0);
    EXPECT_TRUE(f.logged.empty());
    EXPECT_EQ(status(dialog), QStringLiteral("> UPDATE BLOCKED CLEAR\nblocked=none"));
    EXPECT_FALSE(saysARefusal(dialog));
    // The page shows what the state has now.
    EXPECT_EQ(shown(dialog, "settingsUpdateBlocked"), QStringLiteral("No version was put back."));
    EXPECT_FALSE(enabled(dialog, "settingsUpdateUnblock")) << "nothing left to offer again";
    // The skipped version is another list, and this line is not its line.
    EXPECT_EQ(shown(dialog, "settingsUpdateSkipped"),
              QStringLiteral("Katana 0.3.0 is skipped: it is not offered."));
    EXPECT_TRUE(enabled(dialog, "settingsUpdateUnskip"));

    // Each of the two buttons runs its own line and not the other's.
    press(dialog, "settingsUpdateUnskip");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE BLOCKED CLEAR"),
                                  QStringLiteral("UPDATE SKIP NONE")}));
}

TEST(SettingsUpdates, ALineThatWasRefusedLeavesTheVersionThatWasPutBackAsTheStateHasIt)
{
    // The state file reads, says a release was put back, and cannot be
    // written (the update folder allows no writing): the verb refuses the
    // line, and the list is as it was. The refusal's words are this test's
    // own; the page shows whatever the executor answered.
    UpdatesFixture f;
    f.putBack = true;
    f.answer = [](const QString&) {
        return VerbOutcome{false, {},
                           QStringLiteral("InvalidState: the update settings could not be "
                                          "written")};
    };
    SettingsDialog dialog(f.context);
    press(dialog, "settingsUpdateUnblock");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE BLOCKED CLEAR")})) << "once, not again";
    EXPECT_EQ(status(dialog),
              QStringLiteral("> UPDATE BLOCKED CLEAR\nRefused: InvalidState: the update settings "
                             "could not be written"));
    EXPECT_TRUE(saysARefusal(dialog));
    EXPECT_EQ(shown(dialog, "settingsUpdateBlocked"),
              QStringLiteral("A version that was installed here did not start and was put back: "
                             "it is not offered."));
    EXPECT_TRUE(enabled(dialog, "settingsUpdateUnblock")) << "it can be tried again";
}

TEST(SettingsUpdates, TheButtonForAVersionThatWasPutBackIsOffWithNoneAndOnWhenNobodySays)
{
    // None was put back: nothing to do.
    UpdatesFixture none;
    SettingsDialog without(none.context);
    EXPECT_EQ(shown(without, "settingsUpdateBlocked"), QStringLiteral("No version was put back."));
    EXPECT_FALSE(enabled(without, "settingsUpdateUnblock"));
    press(without, "settingsUpdateUnblock");
    EXPECT_TRUE(none.ran.isEmpty()) << "a disabled button runs nothing";

    // Changed elsewhere since - an update was put back, a line was typed: the
    // page reads the state again when it is turned to, and runs nothing.
    none.putBack = true;
    child<QListWidget>(without, "settingsPages")->setCurrentRow(1);
    EXPECT_TRUE(enabled(without, "settingsUpdateUnblock"));
    EXPECT_TRUE(none.ran.isEmpty());

    // Nobody says whether one was (a headless window keeps no state): the
    // page says nothing of it, and the button stays on for the verb to answer.
    UpdatesFixture unsaid;
    unsaid.context.updates.putBack = {};
    SettingsDialog asking(unsaid.context);
    EXPECT_TRUE(shown(asking, "settingsUpdateBlocked").isEmpty());
    ASSERT_TRUE(enabled(asking, "settingsUpdateUnblock"));
    press(asking, "settingsUpdateUnblock");
    EXPECT_EQ(unsaid.ran, (QStringList{QStringLiteral("UPDATE BLOCKED CLEAR")}));

    // No executor: nothing can run the line, whatever the state has.
    UpdatesFixture mute;
    mute.putBack = true;
    mute.context.run = {};
    SettingsDialog silent(mute.context);
    EXPECT_FALSE(enabled(silent, "settingsUpdateUnblock"));
    // And with nothing given at all, as every control of the page.
    SettingsDialog bare(SettingsContext{});
    EXPECT_FALSE(enabled(bare, "settingsUpdateUnblock"));
    EXPECT_TRUE(shown(bare, "settingsUpdateBlocked").isEmpty());
}

TEST(SettingsUpdates, AStateThatDoesNotReadIsNotGuessedAtAndTheButtonsLetTheVerbAnswer)
{
    // U/state.json is there and does not read: nobody can say what is skipped
    // or put back, and the page is told so (nullopt; what the window gives is
    // UpdateDecisions.AStateFileThatDoesNotReadIsNobodyCanSayAndNeverNone).
    // It said "No version was put back." and switched the button off - a
    // guess, and one that kept a person from the verb's refusal, which names
    // the cause (the ruling H5: "With a state file that does not read:
    // refused (InvalidState) and the file left as it is, as SKIP does").
    UpdatesFixture f;
    f.context.updates.skippedVersion = [] { return std::optional<QString>(); };
    f.context.updates.putBack = [] { return std::optional<bool>(); };
    f.answer = [](const QString&) {
        return VerbOutcome{false, {},
                           QStringLiteral("InvalidState: the update settings could not be read")};
    };
    SettingsDialog dialog(f.context);
    EXPECT_TRUE(shown(dialog, "settingsUpdateBlocked").isEmpty());
    EXPECT_TRUE(shown(dialog, "settingsUpdateSkipped").isEmpty());
    ASSERT_TRUE(enabled(dialog, "settingsUpdateUnblock"));
    ASSERT_TRUE(enabled(dialog, "settingsUpdateUnskip"));
    EXPECT_TRUE(f.ran.isEmpty()) << "showing the page runs nothing";

    press(dialog, "settingsUpdateUnblock");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE BLOCKED CLEAR")}));
    EXPECT_EQ(status(dialog),
              QStringLiteral("> UPDATE BLOCKED CLEAR\nRefused: InvalidState: the update settings "
                             "could not be read"));
    EXPECT_TRUE(saysARefusal(dialog));
    // Still nothing known, and still there to press once the file is put right.
    EXPECT_TRUE(shown(dialog, "settingsUpdateBlocked").isEmpty());
    EXPECT_TRUE(enabled(dialog, "settingsUpdateUnblock"));

    // The skipped version's button likewise, by its own line.
    press(dialog, "settingsUpdateUnskip");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE BLOCKED CLEAR"),
                                  QStringLiteral("UPDATE SKIP NONE")}));
    EXPECT_EQ(status(dialog),
              QStringLiteral("> UPDATE SKIP NONE\nRefused: InvalidState: the update settings "
                             "could not be read"));
    EXPECT_TRUE(shown(dialog, "settingsUpdateSkipped").isEmpty());
    EXPECT_TRUE(enabled(dialog, "settingsUpdateUnskip"));
}

TEST(SettingsUpdates, AStoreThatCannotBeReadSaysSoInItsOwnWords)
{
    // Not NotFound: there is something there and it is no usable token.
    class Unreadable final : public katana::update::CredentialStore {
      public:
        [[nodiscard]] katana::core::Result<std::string> read() const override
        {
            return katana::core::makeError(
                katana::core::ErrorCode::InvalidState,
                "the access token in the credential manager is not a usable token");
        }
        [[nodiscard]] katana::core::Status write(std::string_view) override { return {}; }
        [[nodiscard]] katana::core::Status clear() override { return {}; }
        [[nodiscard]] std::string_view where() const override { return "credential manager"; }
    };
    Unreadable store;
    UpdatesFixture f;
    f.context.updates.credentials = &store;
    SettingsDialog dialog(f.context);
    EXPECT_EQ(shown(dialog, "settingsUpdateTokenState"),
              QStringLiteral("the access token in the credential manager is not a usable token"));
}

TEST(SettingsUpdates, WithNothingGivenThePagesControlsAreOffAndItSaysWhyNoTokenIsKept)
{
    // Every member of the context at its default: no executor, no state, no store.
    SettingsDialog dialog(SettingsContext{});
    for (const char* name : {"settingsUpdateAuto", "settingsUpdateToken",
                             "settingsUpdateTokenSave", "settingsUpdateTokenClear",
                             "settingsUpdateUnskip"}) {
        EXPECT_FALSE(enabled(dialog, name)) << name;
    }
    EXPECT_FALSE(child<QCheckBox>(dialog, "settingsUpdateAuto")->isChecked())
        << "nobody says it is on: it is not shown as on";
    EXPECT_EQ(shown(dialog, "settingsUpdateTokenState"),
              QStringLiteral("This session keeps no access token; the environment variable "
                             "KATANA_UPDATE_TOKEN gives a run one."));
    EXPECT_TRUE(shown(dialog, "settingsUpdateSkipped").isEmpty());
    // Its own flag, as every check of this page's note is: the page is not
    // the one the stack shows, so nothing on it is visible "to the dialog".
    EXPECT_TRUE(child<QLabel>(dialog, "settingsUpdateNote")->isHidden());
    katana::qt::test::paint(dialog);
}

TEST(SettingsUpdates, WhatTheWindowHandsOverLaterFillsThePageAndTheNoteIsShownWhenThereIsOne)
{
    // The Format workbench makes the dialog and knows nothing of updates: the
    // window gives the page its services once the dialog is there.
    UpdatesFixture f;
    SettingsContext withoutUpdates = f.context;
    withoutUpdates.updates = katana::qt::SettingsUpdates{};
    SettingsDialog dialog(withoutUpdates);
    ASSERT_FALSE(enabled(dialog, "settingsUpdateAuto"));
    ASSERT_FALSE(enabled(dialog, "settingsUpdateTokenSave"));

    katana::qt::SettingsUpdates updates = f.context.updates;
    updates.note = QStringLiteral("The environment variable KATANA_UPDATE_DISABLE is set: no "
                                  "automatic check runs in this session.");
    dialog.setUpdates(updates);
    EXPECT_TRUE(enabled(dialog, "settingsUpdateAuto"));
    EXPECT_TRUE(child<QCheckBox>(dialog, "settingsUpdateAuto")->isChecked());
    EXPECT_TRUE(enabled(dialog, "settingsUpdateTokenSave"));
    const auto* note = child<QLabel>(dialog, "settingsUpdateNote");
    EXPECT_FALSE(note->isHidden());
    EXPECT_EQ(note->text(), updates.note);
    EXPECT_TRUE(f.ran.isEmpty()) << "filling the page runs nothing";

    updates.note.clear();
    dialog.setUpdates(updates);
    EXPECT_TRUE(note->isHidden());

    // And on the page as a person sees it: turned to, the note is shown.
    updates.note = QStringLiteral("A note.");
    dialog.setUpdates(updates);
    child<QListWidget>(dialog, "settingsPages")->setCurrentRow(1);
    EXPECT_TRUE(note->isVisibleTo(&dialog));
}

TEST(SettingsUpdates, ThePageWorksWithNoDrawingAndAfterOneHasClosed)
{
    // The first page stops acting when its Document goes; this one is of the
    // program and goes on.
    UpdatesFixture f;
    auto document = std::make_unique<katana::cad::Document>();
    f.context.document = document.get();
    SettingsDialog dialog(f.context);
    document.reset();
    dialog.refresh();
    ASSERT_EQ(status(dialog), QStringLiteral("The drawing this dialog worked on has closed: "
                                             "nothing here acts on anything any more."));
    for (const char* name : {"settingsUpdateAuto", "settingsUpdateTokenSave",
                             "settingsUpdateTokenClear"}) {
        EXPECT_TRUE(enabled(dialog, name)) << name;
    }
    press(dialog, "settingsUpdateTokenClear");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("UPDATE TOKEN CLEAR")}));
    EXPECT_EQ(status(dialog), QStringLiteral("> UPDATE TOKEN CLEAR\nDone."));
}

TEST(SettingsUpdates, WithNoExecutorATokenIsStillSavedSinceSaveRunsNoLine)
{
    UpdatesFixture f;
    f.context.run = {};
    SettingsDialog dialog(f.context);
    EXPECT_FALSE(enabled(dialog, "settingsUpdateAuto"));
    EXPECT_FALSE(enabled(dialog, "settingsUpdateTokenClear"));
    ASSERT_TRUE(enabled(dialog, "settingsUpdateTokenSave"));
    child<QLineEdit>(dialog, "settingsUpdateToken")->setText(kToken);
    press(dialog, "settingsUpdateTokenSave");
    ASSERT_EQ(f.store.written.size(), 1u);
    EXPECT_EQ(f.store.written.front(), kToken.toStdString());
}
