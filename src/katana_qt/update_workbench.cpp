// The update system in the window (update_workbench.hpp).

#include "update_workbench.hpp"

#include <QAction>
#include <QApplication>
#include <QCoreApplication>
#include <QDesktopServices>
#include <QDir>
#include <QKeySequence>
#include <QObject>
#include <QStringList>
#include <QTime>
#include <QTimer>
#include <QToolButton>
#include <QUrl>

#include <atomic>
#include <cstdint>
#include <random>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/update/check.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/install.hpp"
#include "katana/update/stage.hpp"
#include "katana/update/state.hpp"
#include "katana/update/version.hpp"
#include "main_window.hpp"

namespace katana::qt {

// What a job's worker and the workbench share. The atomics are the download's
// progress, written on the worker and read by a timer here; `reply` is written
// by the worker as its last act and read on the interface thread once the job
// runner has delivered its completion, whose post orders the two (as the
// runner's own result is ordered, jobs.cpp).
struct UpdateWorkbench::Shared {
    std::atomic<std::uint64_t> done{0};
    std::atomic<std::uint64_t> total{0};
    std::atomic<std::uint64_t> filesDone{0};
    std::atomic<bool> filesKnown{false};
    std::optional<katana::core::Result<katana::app::UpdateReply>> reply;
};

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

// A copy of Katana can replace its own files on Windows only: the apply is
// not written for Linux, and a macOS bundle is sealed (docs: the update
// document's platform table). Elsewhere a release is told of and nothing more.
#ifdef _WIN32
constexpr bool kAppliesHere = true;
#else
constexpr bool kAppliesHere = false;
#endif

// Set to anything, it switches the automatic check off for the session.
constexpr const char* kDisableVariable = "KATANA_UPDATE_DISABLE";
// How often an open window asks whether an automatic check has come due. The
// check itself is every 24 hours and up to 2 more (update::kCheckInterval,
// kCheckJitterMax); an hour is how late past due a window left open may be,
// and costs one small file read.
constexpr std::chrono::minutes kDueProbe{60};
// The dialog's progress: four times a second reads as moving and costs the
// event loop nothing (the status bar's own job progress ticks faster).
constexpr std::chrono::milliseconds kProgressTick{250};
// How often a close that waits for a job looks again.
constexpr std::chrono::milliseconds kJobPoll{250};
// The install's own record of its recoveries, R/.update/recovery.log
// (update/recover.hpp, "WHAT RECOVERY WRITES, AND WHERE").
constexpr std::string_view kRecoveryLogName = "recovery.log";
// The most of that record a start reads. A recovery's line is its time, a few
// words and the ids it found and left, 32 characters each (update/apply.hpp,
// newTransactionId): about a hundred bytes, written once for an update that a
// crash interrupted. A megabyte is some ten thousand of them, and what
// cleanup.list is held to (src/katana_update/state.cpp); a file beyond it is
// not one the helper wrote, and is not read.
constexpr std::uint64_t kRecoveryLogLimit = 1024 * 1024;

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

QString fromView(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

std::int64_t nowEpoch()
{
    return std::chrono::duration_cast<std::chrono::seconds>(
               std::chrono::system_clock::now().time_since_epoch())
        .count();
}

// A reply as the log shows it: without the line breaks it may end with.
QString replyText(const std::string& text)
{
    QString shown = qs(text);
    while (shown.endsWith(QLatin1Char('\n')) || shown.endsWith(QLatin1Char('\r'))) {
        shown.chop(1);
    }
    return shown;
}

// What could not be done, for the sentence under the dialog's two: the
// refusal's own words, or the words for the reason its reply gives.
QString whyNot(const Result<katana::app::UpdateReply>& reply)
{
    if (!reply) {
        return qs(reply.error().message);
    }
    return updateReasonWords(updateReplyValue(qs(reply->text), "reason"));
}

QString problemSentence(const char* what, const QString& why)
{
    return why.isEmpty() ? QStringLiteral("%1; the command log says why.").arg(QLatin1String(what))
                         : QStringLiteral("%1: %2.").arg(QLatin1String(what), why);
}

bool automaticCheckDisabled()
{
    return !katana::core::environmentVariable(kDisableVariable).empty();
}

// Whether the complete stage of the release with this sequence is in the
// update folder: its manifest is the last thing a download writes there
// (update/stage.hpp), so the file's presence is the stage's.
bool stageIsThere(const std::filesystem::path& stateDirectory, std::uint64_t sequence)
{
    std::error_code failed;
    return std::filesystem::exists(
        katana::update::stagedManifestPath(katana::update::stagePath(stateDirectory, sequence)),
        failed);
}

// Lists for the window's sweep (update/state.hpp, cleanup.list) the helper's
// copy of each update of THIS install that needed a recovery and is over.
// `katana_updater --recover` writes
// nothing per user, whoever started it (update/recover.hpp), so nothing in the
// update folder names that copy: unlisted, one helper's size would stay there
// for every update a crash interrupted. Which transactions those are is
// updateRecoveredTransactions' (update_dialog.hpp), which also says why no
// other `run-<tx>` is touched: the update folder is every install's.
//
// The record is read as recovery reads the folder it lies in - never through a
// link - and a copy that is gone already, which is every start but the first
// after a recovery, is not listed again. Empty when there was nothing to list
// or it is listed; else why it could not be.
QString listRecoveredCopies(const std::filesystem::path& stateDirectory,
                            const std::filesystem::path& root, katana::update::FsOps& files)
{
    using katana::update::EntryInfo;
    using katana::update::EntryKind;
    if (root.empty()) {
        return {};
    }
    const std::filesystem::path work = katana::update::updateWorkDirectory(root);
    const Result<EntryInfo> folder = files.stat(work);
    if (!folder) {
        return qs(folder.error().message);
    }
    if (folder->kind != EntryKind::Directory || folder->reparsePoint) {
        // A copy that was never updated, or a folder that is not the install's own.
        return {};
    }
    const std::filesystem::path record = work / katana::core::pathFromUtf8(kRecoveryLogName);
    const Result<EntryInfo> written = files.stat(record);
    if (!written) {
        return qs(written.error().message);
    }
    if (written->kind != EntryKind::File || written->reparsePoint ||
        written->size > kRecoveryLogLimit) {
        // No record: no recovery has run here, which is every install but the
        // few a crash caught. Or one the helper did not write - a link, a
        // folder, a file too long: not read.
        return {};
    }
    const Result<std::string> text = files.readFile(record);
    if (!text) {
        return qs(text.error().message);
    }
    const std::vector<std::string> over =
        updateRecoveredTransactions(*text, [&files, &work](const std::string& transaction) {
            // A folder that cannot be examined is not a folder that is gone.
            const Result<EntryInfo> left =
                files.stat(work / katana::core::pathFromUtf8(transaction));
            return left && left->kind == EntryKind::Missing;
        });
    QString failed;
    for (const std::string& transaction : over) {
        const std::filesystem::path copy = katana::update::runPath(stateDirectory, transaction);
        if (const Result<EntryInfo> there = files.stat(copy);
            there && there->kind == EntryKind::Missing) {
            continue;
        }
        // Everything that can be listed is; the first refusal is the one said.
        if (const katana::core::Status listed =
                katana::update::appendCleanup(stateDirectory, copy, files);
            !listed && failed.isEmpty()) {
            failed = qs(listed.error().message);
        }
    }
    return failed;
}

} // namespace

UpdateWorkbench::UpdateWorkbench(MainWindow& window, UpdateServices services)
    : window_(window), services_(std::move(services)), scope_(std::make_unique<QObject>())
{
    action_ = services_.makeAction(
        Icon::Download, QStringLiteral("Check for &Updates..."),
        QStringLiteral("Ask whether a newer Katana has been released; download and install it "
                       "from the dialog that opens"),
        QKeySequence(), QStringLiteral("helpCheckForUpdates"));
    // The dialog it opens, by object name: how --dialog finds it.
    action_->setData(QStringLiteral("updateDialog"));
    QObject::connect(action_, &QAction::triggered, scope_.get(), [this] { open(); });

    // Hidden until a release is known; the window puts it in its status bar.
    statusButton_ = new QToolButton(&window_);
    statusButton_->setObjectName(QStringLiteral("statusUpdateAvailable"));
    statusButton_->setAutoRaise(true);
    statusButton_->setIcon(katana::qt::icon(Icon::Download));
    statusButton_->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    statusButton_->setToolTip(QStringLiteral("A newer Katana is available: click to see it"));
    statusButton_->hide();
    QObject::connect(statusButton_, &QToolButton::clicked, scope_.get(), [this] { open(); });

    progressTimer_ = new QTimer(scope_.get());
    progressTimer_->setInterval(kProgressTick);
    QObject::connect(progressTimer_, &QTimer::timeout, scope_.get(), [this] { showProgress(); });
    dueTimer_ = new QTimer(scope_.get());
    dueTimer_->setInterval(kDueProbe);
    QObject::connect(dueTimer_, &QTimer::timeout, scope_.get(), [this] { checkIfDue(); });
    waitTimer_ = new QTimer(scope_.get());
    waitTimer_->setInterval(kJobPoll);
    QObject::connect(waitTimer_, &QTimer::timeout, scope_.get(), [this] { closeWhenIdle(); });

    // Once for the process, so that a release does not meet every copy at the
    // same minute (update/state.hpp, checkIsDue).
    std::random_device seed;
    std::mt19937_64 engine(seed());
    jitter_ = std::chrono::seconds(std::uniform_int_distribution<std::int64_t>(
        0, katana::update::kCheckJitterMax.count())(engine));
}

UpdateWorkbench::~UpdateWorkbench()
{
    alive_.reset();
    // A job still running is told to stop, so that the window's runner does
    // not wait out a download as it joins it; its completion is dropped with
    // the runner and never reaches this object.
    cancelJob();
    // A request that was prepared and is still here was never started: the
    // person closed the window themselves while the close for it waited, and
    // main() was handed nothing. The helper's copy goes to the next start's
    // sweep, as after Later (dropPrepared) - without a word, since the log is
    // the window's and the window is going.
    if (pendingApply_.has_value()) {
        (void)katana::app::abandonApply(*pendingApply_);
    }
    // The dialog is the window's child only so that it floats over it; it
    // calls back into this object, so it goes with it.
    delete dialog_.data();
}

bool UpdateWorkbench::headless() const
{
    return services_.headless && services_.headless();
}

void UpdateWorkbench::log(const QString& text, bool isError) const
{
    if (services_.log) {
        services_.log(text, isError);
    }
}

const katana::app::UpdateContext& UpdateWorkbench::context()
{
    if (!context_) {
        // By now the window knows whether it is headless: every first use is
        // after main() has said so. A headless run gets the surface that
        // keeps no per-user state and refuses to download or apply.
        katana::app::UpdateContext made = katana::app::makeUpdateContext(
            headless() ? katana::app::UpdateSurface::HeadlessWindow
                       : katana::app::UpdateSurface::Window);
        // What the helper waits for before it swaps anything, and what it
        // starts again afterwards: this process and this program.
        made.pid = static_cast<std::uint64_t>(QCoreApplication::applicationPid());
        made.relaunch =
            std::filesystem::path(QCoreApplication::applicationFilePath().toStdWString());
        if (route_) {
            made.feed.route = *route_;
        }
        context_ = std::move(made);
    }
    return *context_;
}

QString UpdateWorkbench::installedVersion()
{
    const katana::update::Install& install = context().install;
    // With no install folder found there is no record to read a version
    // from: the copy is the program as it was built.
    if (install.root.empty()) {
        return fromView(katana::update::programVersionText());
    }
    return qs(katana::update::toString(install.version));
}

UpdateView UpdateWorkbench::viewOf(UpdateDialogState state)
{
    UpdateView view;
    view.state = state;
    view.installedVersion = installedVersion();
    return view;
}

UpdateView UpdateWorkbench::readyView()
{
    UpdateView view = viewOf(UpdateDialogState::Ready);
    view.latestVersion = stagedVersion_;
    return view;
}

UpdateView UpdateWorkbench::viewOfOffer(const katana::app::UpdateOffer& offer)
{
    // The facts only this object has; what they make of the answer is
    // updateViewOfOffer's (update_dialog.hpp, "THE DECISIONS").
    UpdateOfferFacts facts;
    facts.installedVersion = installedVersion();
    facts.stagedVersion = stagedVersion_;
    facts.checkedAt = QTime::currentTime().toString(QStringLiteral("HH:mm"));
    facts.nowEpoch = nowEpoch();
    facts.appliesHere = kAppliesHere;
    return updateViewOfOffer(offer, facts);
}

void UpdateWorkbench::show(UpdateView view)
{
    view_ = std::move(view);
    if (!dialog_.isNull()) {
        dialog_->setView(view_);
    }
    // The button is there while there is a release to act on, and says which
    // (updateStatusButtonText); it keeps its words while it is hidden.
    const QString offered = updateStatusButtonText(view_);
    if (!offered.isEmpty()) {
        statusButton_->setText(offered);
    }
    statusButton_->setVisible(!offered.isEmpty());
}

// ---- the lines ---------------------------------------------------------------------------------

bool UpdateWorkbench::runLine(const QString& line)
{
    if (!katana::app::isUpdateLine(line.trimmed().toStdString())) {
        return false;
    }
    // The three lines that take time are known by their whole text, which is
    // the whole of their grammar; anything else under those words is a line
    // the verbs refuse at once (a DOWNLOAD without its CONFIRM), and is run
    // at once like the rest.
    const QString words = line.simplified().toUpper();
    const bool check = words == QLatin1String(kUpdateCheckLine);
    const bool download = words == QLatin1String(kUpdateDownloadLine);
    const bool apply = words == QLatin1String(kUpdateApplyLine);
    if (check) {
        startCheck(false);
    } else if ((download || apply) && !headless()) {
        if (download) {
            startDownload(applyAfterDownload_);
        } else {
            startApply();
        }
    } else {
        // In a headless run DOWNLOAD and APPLY come here too: the verbs refuse
        // them for that surface, in their own words (reason=headless).
        runNow(line);
    }
    return true;
}

void UpdateWorkbench::logReply(const Result<katana::app::UpdateReply>& reply,
                               const QString& line) const
{
    const std::string said = reply ? reply->text : reply.error().describe();
    // Whatever the verbs say back to a line that was typed where a token was
    // hoped to go is logged without the words that may be one, in any case
    // (updateLoggedReply): a refusal may quote the line it refuses.
    log(updateLoggedReply(replyText(said), line), !reply);
}

bool UpdateWorkbench::mayStart(const QString& line)
{
    if (running_ == JobKind::None) {
        return true;
    }
    // The line as the log may show it: never what was typed after UPDATE TOKEN.
    log(qs(makeError(ErrorCode::InvalidState,
                     "an update job is still running; wait for it to end, or cancel it",
                     (updateLoggedLine(line.simplified()) + QStringLiteral(" while ") + jobTitle_)
                         .toStdString())
               .describe()),
        true);
    return false;
}

void UpdateWorkbench::runNow(const QString& line)
{
    const QStringList words = line.simplified().split(QChar::Space, Qt::SkipEmptyParts);
    const QString action = words.size() > 1 ? words[1].toUpper() : QString();
    // STATUS only reads. Every other line may write the state file a running
    // job writes too (a check records when it ran), and two writers of one
    // small file lose one of the two changes.
    if (action != QLatin1String("STATUS") && !mayStart(line)) {
        return;
    }
    // No UPDATE line takes a token, and a person may type one that does all
    // the same. The verbs refuse it; whatever they say back is logged without
    // the words that may be one (logReply; updateLoggedLine hides them in the
    // echo).
    const Result<katana::app::UpdateReply> reply =
        katana::app::runUpdateLine(context(), line.trimmed().toStdString());
    logReply(reply, line);
    if (!reply) {
        return;
    }
    if (action == QLatin1String("TOKEN")) {
        // The context carries the token it was made with: made again at the
        // next use, it reads the store as the line left it.
        context_.reset();
    }
    // A reply that gives a reason is one that was not carried out.
    if (action == QLatin1String("SKIP") && words.size() == 3 &&
        updateReplyValue(qs(reply->text), "reason").isEmpty()) {
        if (words[2].compare(QLatin1String("NONE"), Qt::CaseInsensitive) == 0) {
            if (view_.state == UpdateDialogState::NotOffered &&
                view_.reason == QLatin1String("skipped")) {
                show(viewOf(UpdateDialogState::Idle));
            }
        } else if (view_.state == UpdateDialogState::Available &&
                   updateSameVersion(view_.latestVersion, words[2])) {
            // What was offered is not any more: neither the dialog nor a
            // download typed next goes on about it.
            offer_.reset();
            UpdateView view = viewOf(UpdateDialogState::NotOffered);
            view.latestVersion = view_.latestVersion;
            view.reason = QStringLiteral("skipped");
            show(std::move(view));
        }
    }
}

// ---- the jobs ----------------------------------------------------------------------------------

void UpdateWorkbench::startCheck(bool automatic)
{
    if (automatic) {
        // Quiet in every way: it does not even say it could not start.
        if (running_ != JobKind::None) {
            return;
        }
    } else if (running_ == JobKind::Check && automaticCheck_) {
        // The check the window started by itself is on its way, and a person
        // asks for one: that one becomes theirs - answered aloud, in the log
        // and in the dialog - and is not started twice, nor are they told
        // that a job they never started is in their way. Cancelled, it goes
        // back to what was shown before it began (viewBeforeJob_ is still
        // that).
        automaticCheck_ = false;
        show(viewOf(UpdateDialogState::Checking));
        return;
    } else if (!mayStart(QString::fromLatin1(kUpdateCheckLine))) {
        return;
    }
    viewBeforeJob_ = view_;
    automaticCheck_ = automatic;
    if (!automatic) {
        show(viewOf(UpdateDialogState::Checking));
    }
    beginJob(JobKind::Check, QStringLiteral("Checking for updates"), kUpdateCheckLine);
}

void UpdateWorkbench::startDownload(bool thenApply)
{
    if (!mayStart(QString::fromLatin1(kUpdateDownloadLine))) {
        return;
    }
    viewBeforeJob_ = view_;
    thenApply_ = thenApply;
    // What the last check said the download is; a download typed before any
    // check has only what its progress will say.
    UpdateView view = viewOf(UpdateDialogState::Downloading);
    view.latestVersion = view_.latestVersion;
    if (offer_) {
        view.latestVersion = qs(offer_->check.latestVersion);
        view.changedFiles = offer_->changedFiles;
        view.totalFiles = offer_->totalFiles;
        view.downloadBytes = offer_->downloadBytes;
    }
    const QString title = view.latestVersion.isEmpty()
                              ? QStringLiteral("Downloading the update")
                              : QStringLiteral("Downloading Katana ") + view.latestVersion;
    show(std::move(view));
    beginJob(JobKind::Download, title, kUpdateDownloadLine);
}

void UpdateWorkbench::startApply()
{
    if (!mayStart(QString::fromLatin1(kUpdateApplyLine))) {
        return;
    }
    viewBeforeJob_ = view_;
    // The press is made: until the helper has been copied and has proved it
    // starts, and the window has closed, only Later - which takes the press
    // back - is left.
    UpdateView view = readyView();
    if (view.latestVersion.isEmpty()) {
        view.latestVersion = view_.latestVersion;
    }
    view.waitingForJobs = true;
    show(std::move(view));
    beginJob(JobKind::Apply, QStringLiteral("Preparing the update"), kUpdateApplyLine);
}

void UpdateWorkbench::beginJob(JobKind kind, const QString& title, const char* line)
{
    // The job owns what its worker reads: a copy of the context, taken here on
    // the interface thread. Nothing of this object goes to the worker.
    katana::app::UpdateContext copy = context();
    auto shared = std::make_shared<Shared>();
    running_ = kind;
    jobTitle_ = title;
    shared_ = shared;
    JobRunner& runner = JobRunner::of(window_);
    job_ = runner.start(
        title,
        [copy = std::move(copy), text = std::string(line),
         shared](JobControl& control) mutable -> Result<JobRunner::Apply> {
            copy.stop = control.stopToken();
            copy.progress = [&control, shared](std::uint64_t done, std::uint64_t total) {
                shared->done.store(done, std::memory_order_relaxed);
                shared->total.store(total, std::memory_order_relaxed);
                if (total > 0) {
                    control.setProgress(static_cast<double>(done) / static_cast<double>(total));
                }
            };
            copy.progressLine = [shared](std::string_view said) {
                if (const auto files = updateFilesOfProgressLine(fromView(said))) {
                    shared->filesDone.store(files->first, std::memory_order_relaxed);
                    shared->filesKnown.store(true, std::memory_order_relaxed);
                }
            };
            shared->reply.emplace(katana::app::runUpdateLine(copy, text));
            // Nothing to apply: the reply is read where the job ends.
            return JobRunner::Apply{};
        },
        [this, kind, shared](const JobReport& report) { finished(kind, *shared, report); });
    if (kind == JobKind::Download) {
        progressTimer_->start();
    }
    // A headless run has nobody to wait for the reply, and its next step may
    // read it: the line is over when its job is.
    if (headless()) {
        runner.waitFor(job_);
    }
}

void UpdateWorkbench::finished(JobKind kind, Shared& shared, const JobReport& report)
{
    running_ = JobKind::None;
    job_ = kNoJob;
    jobTitle_.clear();
    progressTimer_->stop();
    // `shared` stays alive in the caller's own pointer while this runs.
    shared_.reset();
    const bool automatic = std::exchange(automaticCheck_, false);
    const bool thenApply = std::exchange(thenApply_, false);

    if (report.outcome == JobOutcome::Cancelled) {
        // Whatever the worker had got to, a cancelled job changes nothing
        // here: the dialog goes back to what it showed.
        if (kind == JobKind::Check && !automatic) {
            log(QStringLiteral("The check for updates was cancelled."));
        } else if (kind == JobKind::Download) {
            log(QStringLiteral("The download was cancelled. What had arrived is kept, and the "
                               "next download starts from it."));
        } else if (kind == JobKind::Apply && shared.reply.has_value()) {
            // Later was pressed while the helper was being prepared, and the
            // preparation had got as far as its copy: nobody will start it.
            if (const Result<katana::app::UpdateReply>& prepared = *shared.reply;
                prepared && prepared->apply.has_value()) {
                dropPrepared(*prepared->apply);
            }
        }
        if (!(kind == JobKind::Check && automatic)) {
            show(viewBeforeJob_);
        }
        return;
    }
    const auto deliver = [&](const Result<katana::app::UpdateReply>& reply) {
        switch (kind) {
        case JobKind::Check:
            checkFinished(reply, automatic);
            break;
        case JobKind::Download:
            downloadFinished(reply, thenApply);
            break;
        case JobKind::Apply:
            applyFinished(reply);
            break;
        case JobKind::None:
            break;
        }
    };
    if (report.outcome == JobOutcome::Failed || !shared.reply.has_value()) {
        // The worker threw, or ended with nothing: said as a refusal of the line.
        deliver(makeError(ErrorCode::Internal, "the update job failed", report.error));
    } else {
        deliver(*shared.reply);
    }
}

void UpdateWorkbench::checkFinished(const Result<katana::app::UpdateReply>& reply, bool automatic)
{
    using katana::update::CheckStatus;
    if (!reply || !reply->offer.has_value()) {
        // The line could not be run, or answered nothing a check answers.
        if (automatic) {
            return;
        }
        logReply(reply);
        UpdateView view = viewOf(UpdateDialogState::CheckFailed);
        if (reply) {
            // A reply with no record of the offer is read by its words: the
            // reason, and whether it is the copy that cannot update rather
            // than the check that failed.
            const QString said = qs(reply->text);
            view.reason = updateReplyValue(said, "reason");
            if (updateReplyValue(said, "update") == QLatin1String("unsupported")) {
                view.state = UpdateDialogState::Unsupported;
            }
        } else {
            view.problem = qs(reply.error().message);
        }
        show(std::move(view));
        return;
    }
    katana::app::UpdateOffer offer = *reply->offer;
    if (automatic) {
        // What the person already decided about this release is applied before
        // anything is offered: one they skipped, or one that was rolled back
        // here, is not news.
        if (const Result<katana::update::UpdateState> state =
                katana::update::readState(context().stateDirectory)) {
            katana::update::applyUpdateState(offer.check, *state);
        }
    }
    const CheckStatus status = offer.check.status;
    const UpdateView answer = viewOfOffer(offer);
    // Nobody asked for the automatic check's answer: whether it is said,
    // shown without a word, or dropped is updateQuietAnswer's.
    const UpdateQuietAnswer quiet =
        automatic ? updateQuietAnswer(view_, answer) : UpdateQuietAnswer::Announce;
    if (quiet == UpdateQuietAnswer::Nothing) {
        return;
    }
    if (status != CheckStatus::Unreachable && status != CheckStatus::Unsupported) {
        // The feed answered. The download asks the way the check was answered
        // (update/feed.hpp); the release offered is the one a download typed
        // next is of, and an answer that offers none leaves none - a check
        // that failed un-knows nothing; and a downloaded release the answer
        // no longer offers is not ready any more (updateStagedAfter).
        route_ = offer.check.route;
        if (context_) {
            context_->feed.route = offer.check.route;
        }
        if (status == CheckStatus::Available) {
            offer_ = offer;
        } else {
            offer_.reset();
        }
        stagedVersion_ = updateStagedAfter(stagedVersion_, answer);
    }
    if (quiet == UpdateQuietAnswer::Announce) {
        if (automatic) {
            // News: logged as the typed line would be, from its echo on.
            log(QStringLiteral("> ") + QLatin1String(kUpdateCheckLine));
        }
        logReply(reply);
    }
    show(answer);
}

void UpdateWorkbench::downloadFinished(const Result<katana::app::UpdateReply>& reply,
                                       bool thenApply)
{
    logReply(reply);
    // The line's last record says what is staged (update_verbs.hpp).
    const QString staged = reply ? updateStagedOfReply(qs(reply->text)) : QString();
    if (staged.isEmpty()) {
        UpdateView view = viewBeforeJob_;
        view.problem = problemSentence("The download did not finish", whyNot(reply));
        show(std::move(view));
        return;
    }
    stagedVersion_ = staged;
    show(readyView());
    if (thenApply) {
        // The second line of the one click, run as the first was - echoed and
        // logged as a typed line - once the job runner has finished with this
        // job.
        QTimer::singleShot(0, scope_.get(), [this] {
            (void)window_.runVerbLine(QString::fromLatin1(kUpdateApplyLine));
        });
    }
}

void UpdateWorkbench::applyFinished(const Result<katana::app::UpdateReply>& reply)
{
    logReply(reply);
    if (!reply || !reply->apply.has_value()) {
        UpdateView view = viewBeforeJob_;
        view.problem = problemSentence("The update could not be started", whyNot(reply));
        show(std::move(view));
        return;
    }
    // Nothing in the install has changed yet. The window closes first - on
    // the next turn, out of the job runner's own completion.
    pendingApply_ = *reply->apply;
    QTimer::singleShot(0, scope_.get(), [this] { closeWhenIdle(); });
}

void UpdateWorkbench::showProgress()
{
    if (running_ != JobKind::Download || !shared_ ||
        view_.state != UpdateDialogState::Downloading) {
        return;
    }
    UpdateView view = view_;
    // The bytes the download itself says it will fetch, once it says.
    if (const std::uint64_t total = shared_->total.load(std::memory_order_relaxed); total > 0) {
        view.downloadBytes = total;
    }
    view.downloadedBytes = shared_->done.load(std::memory_order_relaxed);
    if (shared_->filesKnown.load(std::memory_order_relaxed)) {
        view.downloadedFiles = shared_->filesDone.load(std::memory_order_relaxed);
    }
    if (view.downloadBytes != view_.downloadBytes ||
        view.downloadedBytes != view_.downloadedBytes ||
        view.downloadedFiles != view_.downloadedFiles) {
        show(std::move(view));
    }
}

// ---- the dialog --------------------------------------------------------------------------------

UpdateDialog& UpdateWorkbench::dialog()
{
    if (dialog_.isNull()) {
        UpdateDialogContext dialogContext;
        dialogContext.run = [this](const QString& line) { return window_.runVerbLine(line); };
        dialogContext.update = [this] { installUpdate(); };
        dialogContext.cancel = [this] { cancelJob(); };
        dialogContext.openNotes = [this](const QString& url) { openNotes(url); };
        dialogContext.later = [this] { later(); };
        dialog_ = new UpdateDialog(std::move(dialogContext), &window_);
        dialog_->setView(view_);
    }
    return *dialog_;
}

void UpdateWorkbench::open()
{
    UpdateDialog& shown = dialog();
    // How the last update ended is shown once (showLastResult, which shows the
    // dialog itself): asked for again, the dialog starts over, or it would say
    // "could not be installed" for the rest of the session with nothing to
    // press.
    if (running_ == JobKind::None && view_.state == UpdateDialogState::RolledBack) {
        show(viewOf(UpdateDialogState::Idle));
    }
    // With nothing known yet, and nothing on its way - or only the check the
    // window started by itself, which the typed line below then takes over
    // (startCheck) rather than say "not checked yet" while one is running.
    const bool quietCheck = running_ == JobKind::Check && automaticCheck_;
    if ((running_ == JobKind::None || quietCheck) && view_.state == UpdateDialogState::Idle) {
        if (context().install.kind == katana::update::InstallKind::Development) {
            // Known without asking anyone: a copy packaging did not write is
            // never updated (update_verbs.hpp), and the check would say this.
            UpdateView view = viewOf(UpdateDialogState::Unsupported);
            view.reason = QStringLiteral("development_tree");
            show(std::move(view));
        } else if (headless()) {
            // A headless run checks only when it is told to: the dialog says
            // what copy this is and waits for Check now.
            show(viewOf(UpdateDialogState::Idle));
        } else {
            // What the item's name says it does, as the typed line would.
            (void)window_.runVerbLine(QString::fromLatin1(kUpdateCheckLine));
        }
    }
    shown.show();
    shown.raise();
    shown.activateWindow();
}

void UpdateWorkbench::installUpdate()
{
    // One click, two lines and a close: the download's line now, marked so
    // that the apply's follows when it has staged the release (downloadFinished).
    applyAfterDownload_ = true;
    const VerbOutcome outcome = window_.runVerbLine(QString::fromLatin1(kUpdateDownloadLine));
    applyAfterDownload_ = false;
    // A line that was refused before it began - another update job is running
    // - is in the log; the press was made in the dialog, so it is said there
    // too, unless the line's own end has said why already.
    if (!outcome.ok && view_.problem.isEmpty()) {
        UpdateView view = view_;
        view.problem = outcome.error.isEmpty()
                           ? QStringLiteral("The update could not be started; the command log "
                                            "says why.")
                           : outcome.error;
        show(std::move(view));
    }
}

void UpdateWorkbench::cancelJob()
{
    if (job_ != kNoJob) {
        (void)JobRunner::of(window_).cancel(job_);
    }
}

void UpdateWorkbench::openNotes(const QString& url)
{
    // A signed manifest gives only an https address (update/manifest.hpp);
    // nothing else is handed to the system to open.
    const QUrl target(url, QUrl::StrictMode);
    if (!target.isValid() || target.scheme() != QLatin1String("https")) {
        log(qs(makeError(ErrorCode::InvalidArgument,
                         "the release notes' address is not an https address",
                         url.toStdString())
                   .describe()),
            true);
        return;
    }
    if (headless()) {
        // Nobody to show a browser to: the address, for whoever reads the log.
        log(QStringLiteral("notes_url=") + url);
        return;
    }
    if (!QDesktopServices::openUrl(target)) {
        log(qs(makeError(ErrorCode::InvalidState, "no browser could be started for the release "
                                                  "notes",
                         url.toStdString())
                   .describe()),
            true);
    }
}

void UpdateWorkbench::later()
{
    // A press of Restart and update that has not closed the window yet is
    // taken back: the helper's preparation is cancelled, or the close that
    // was waiting for a job is given up. The release stays downloaded.
    if (running_ == JobKind::Apply) {
        cancelJob();
        return;
    }
    if (pendingApply_.has_value() && !closedForApply_) {
        dropPrepared(*std::exchange(pendingApply_, std::nullopt));
        waitTimer_->stop();
        show(readyView());
    }
}

void UpdateWorkbench::dropPrepared(const katana::update::ApplyRequest& request)
{
    // The one way a prepared apply is given up, which the session and
    // katana_cli take too (app::abandonApply, update_verbs.hpp): the helper's
    // copy U/run-<tx> goes on the list the next start sweeps. It is a
    // program's size, and one would otherwise stay behind for every press
    // that was taken back.
    if (const katana::core::Status listed = katana::app::abandonApply(request); !listed) {
        // A leftover only: said, not an error of the session.
        log(QStringLiteral("Update: the helper's copy made for the update that was not started "
                           "could not be listed for removal (%1).")
                .arg(qs(listed.error().message)));
    }
}

// ---- the close ---------------------------------------------------------------------------------

void UpdateWorkbench::closeWhenIdle()
{
    if (!pendingApply_.has_value() || closedForApply_) {
        waitTimer_->stop();
        return;
    }
    // A window that closes stops every job it runs (the runner joins them as
    // it goes): a geoprocessing run is not cut short for an update. And this
    // close comes from a timer, not from a press: under a modal dialog the
    // person opened meanwhile - a file dialog, a question - it would close
    // the program around that dialog. It waits for both.
    const bool jobs = JobRunner::of(window_).activeCount() > 0;
    const bool dialogOpen = !jobs && QApplication::activeModalWidget() != nullptr;
    if (jobs || dialogOpen) {
        if (view_.state != UpdateDialogState::Ready || view_.waitingForJobs != jobs ||
            view_.waitingForDialog != dialogOpen) {
            UpdateView view = readyView();
            view.waitingForJobs = jobs;
            view.waitingForDialog = dialogOpen;
            show(std::move(view));
        }
        waitTimer_->start();
        return;
    }
    waitTimer_->stop();
    // The window's ordinary close: the Format managers' unapplied edits and
    // the unsaved-changes question are asked as at any other close.
    if (window_.close()) {
        closedForApply_ = true;
        return;
    }
    // Cancel there: the window stays, and so does the release, downloaded.
    dropPrepared(*std::exchange(pendingApply_, std::nullopt));
    show(readyView());
    log(QStringLiteral("The window stayed open, so Katana %1 was not installed. It is still "
                       "downloaded: Help > Check for Updates installs it.")
            .arg(stagedVersion_));
}

std::optional<katana::update::ApplyRequest> UpdateWorkbench::takePendingApply()
{
    if (!closedForApply_) {
        return std::nullopt;
    }
    closedForApply_ = false;
    return std::exchange(pendingApply_, std::nullopt);
}

// ---- the start ---------------------------------------------------------------------------------

void UpdateWorkbench::start()
{
    // A headless run reads and writes no per-user place and checks nothing by
    // itself (docs/headless.md).
    if (started_ || headless()) {
        return;
    }
    started_ = true;
    const katana::app::UpdateContext& made = context();
    // A development copy is never updated, and it shares the per-user update
    // folder with an installed copy on the same machine: it must not show -
    // and clear - how THAT copy's last update ended, nor sweep what its helper
    // left to be swept. It does nothing here, and asks nobody anything.
    if (made.install.kind == katana::update::InstallKind::Development ||
        made.stateDirectory.empty()) {
        return;
    }
    if (kAppliesHere) {
        // What a lock kept an earlier update from deleting (update/state.hpp);
        // only where an update can have run at all. A leftover that stays is
        // tried again at the next start: said, not an error of this session.
        const std::unique_ptr<katana::update::FsOps> files = katana::update::makeSystemFsOps();
        // First what no list names yet - the helper's copy of an update that a
        // recovery put right (listRecoveredCopies) - so that the sweep below
        // takes it at this start with the rest.
        if (const QString failed =
                listRecoveredCopies(made.stateDirectory, made.install.root, *files);
            !failed.isEmpty()) {
            log(QStringLiteral("Update: the helper's copy that a recovered update left behind "
                               "could not be listed for removal (%1); the next start tries "
                               "again.")
                    .arg(failed));
        }
        if (const katana::core::Status swept = katana::update::processCleanupList(
                made.stateDirectory, made.install.root, *files);
            !swept) {
            log(QStringLiteral("Update: what an earlier update left behind could not all be "
                               "removed (%1); the next start tries again.")
                    .arg(qs(swept.error().message)));
        }
    }
    restoreStaged();
    showLastResult();
    dueTimer_->start();
    // On the first idle turn, so that the window is up before anything is asked.
    QTimer::singleShot(0, scope_.get(), [this] { checkIfDue(); });
}

void UpdateWorkbench::checkIfDue()
{
    // A headless run is turned away before anything is made or read for it.
    if (headless()) {
        return;
    }
    const katana::app::UpdateContext& made = context();
    // Every condition but the clock's (updateAutomaticCheckIsDue names them):
    // development and test trees stay offline, and with no state folder there
    // is no saying when the last check was.
    UpdateAutoCheckFacts facts;
    facts.headless = false;
    facts.disabled = automaticCheckDisabled();
    facts.jobRunning = running_ != JobKind::None;
    facts.kind = made.install.kind;
    facts.keepsState = !made.stateDirectory.empty();
    // A state file that does not read is carried on from as the defaults are
    // (update/state.hpp): the check the person never switched off still runs.
    katana::update::UpdateState state;
    if (facts.keepsState) {
        if (Result<katana::update::UpdateState> read =
                katana::update::readState(made.stateDirectory)) {
            state = std::move(*read);
        }
    }
    if (!updateAutomaticCheckIsDue(facts, state, nowEpoch(), jitter_)) {
        return;
    }
    startCheck(true);
}

void UpdateWorkbench::restoreStaged()
{
    // UPDATE STATUS, asked without a line in the log: nobody typed it. A
    // release downloaded in an earlier session ("Download only") is offered
    // again as ready - but not one the person skipped since, and never one
    // that was rolled back here: the apply keeps the stage of a release that
    // did not start, and "ready" would install it again
    // (updateStagedToOffer).
    const Result<katana::app::UpdateReply> reply =
        katana::app::runUpdateLine(context(), "UPDATE STATUS");
    if (!reply || !reply->status.has_value()) {
        return;
    }
    const std::filesystem::path stateDirectory = context().stateDirectory;
    katana::app::UpdateStatus status = *reply->status;
    // The skipped version and the blocked list as the engine's own file has
    // them, read here rather than trusted to have been copied whole into the
    // reply: they are what this decision turns on.
    if (Result<katana::update::UpdateState> read = katana::update::readState(stateDirectory)) {
        status.state = std::move(*read);
    }
    const QString staged = updateStagedToOffer(
        status, context().install.version, [&stateDirectory](std::uint64_t sequence) {
            return stageIsThere(stateDirectory, sequence);
        });
    if (staged.isEmpty()) {
        return;
    }
    stagedVersion_ = staged;
    show(readyView());
}

void UpdateWorkbench::showLastResult()
{
    const std::filesystem::path stateDirectory = context().stateDirectory;
    const Result<std::optional<katana::update::LastResult>> read =
        katana::update::readLastResult(stateDirectory);
    if (!read) {
        log(QStringLiteral("Update: how the last update ended could not be read (%1).")
                .arg(qs(read.error().message)));
        return;
    }
    if (!read->has_value()) {
        return;
    }
    const katana::update::LastResult last = **read;
    // Shown once: cleared whatever it says, so the next start is not told again.
    (void)katana::update::clearLastResult(stateDirectory);

    // What it is shown as is updateViewOfLastResult's; a success is no dialog.
    const std::optional<UpdateView> ended = updateViewOfLastResult(
        last, installedVersion(),
        QDir::toNativeSeparators(qs(katana::core::pathToUtf8(context().install.root))));
    if (!ended.has_value()) {
        log(QStringLiteral("Katana was updated to %1.").arg(qs(last.version)));
        return;
    }
    UpdateView view = *ended;
    if (view.state == UpdateDialogState::Blocked) {
        // Nothing was touched and the release is still downloaded: Retry
        // applies it again once the programs named are closed.
        stagedVersion_ = view.latestVersion;
    }
    const QString said = updateDetailText(view).isEmpty()
                             ? updateStateText(view)
                             : updateStateText(view) + QLatin1Char(' ') + updateDetailText(view);
    // In the log as well as in the dialog: an update that did not happen is
    // not told by a window alone. An error only when the install changed and
    // was put back, or could not be.
    log(said, view.state == UpdateDialogState::RolledBack);
    show(std::move(view));
    UpdateDialog& shown = dialog();
    shown.show();
    shown.raise();
}

// ---- Settings > Updates ------------------------------------------------------------------------

SettingsUpdates UpdateWorkbench::settingsUpdates()
{
    SettingsUpdates updates;
    if (headless()) {
        // No per-user state and no credential store: the page says so, and
        // its lines are answered by the verbs for this surface.
        updates.note = QStringLiteral("A headless run keeps no update settings and checks only "
                                      "when it is told to (UPDATE CHECK).");
        return updates;
    }
    const katana::app::UpdateContext& made = context();
    if (!made.stateDirectory.empty()) {
        const std::filesystem::path stateDirectory = made.stateDirectory;
        updates.automatic = [stateDirectory] {
            const Result<katana::update::UpdateState> state =
                katana::update::readState(stateDirectory);
            return state ? state->automatic : katana::update::UpdateState{}.automatic;
        };
        updates.skippedVersion = [stateDirectory] {
            const Result<katana::update::UpdateState> state =
                katana::update::readState(stateDirectory);
            return state ? qs(state->skippedVersion) : QString();
        };
    }
    updates.credentials = made.credentials;
    // The context carries the token it was made with: dropped, the next check
    // makes one that reads the store as Save left it.
    updates.tokenChanged = [this, alive = std::weak_ptr<int>(alive_)] {
        if (!alive.expired()) {
            context_.reset();
        }
    };
    updates.userAgent = qs(made.feed.userAgent);
    updates.server = made.feed.feedBase.empty() ? QStringLiteral("GitHub")
                                                : QUrl(qs(made.feed.feedBase)).host();
    if (made.install.kind == katana::update::InstallKind::Development) {
        updates.note = QStringLiteral("This copy of Katana is a development build: it is never "
                                      "updated, and it runs no automatic check.");
    } else if (automaticCheckDisabled()) {
        updates.note = QStringLiteral("The environment variable %1 is set: no automatic check "
                                      "runs in this session.")
                           .arg(QLatin1String(kDisableVariable));
    }
    return updates;
}

} // namespace katana::qt
