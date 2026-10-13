// Help > Check for Updates (update_dialog.hpp).

#include "update_dialog.hpp"

#include <algorithm>

#include <QFont>
#include <QHBoxLayout>
#include <QLabel>
#include <QLatin1String>
#include <QProgressBar>
#include <QPushButton>
#include <QRegularExpression>
#include <QVBoxLayout>

#include "theme.hpp"

namespace katana::qt {

namespace {

// A million bytes, the megabyte of a download's size as a browser and a
// release page say it (SI; the binary unit is the mebibyte).
constexpr std::uint64_t kMegabyte = 1'000'000;
// The progress bar's range: thousandths, as the status bar's job progress is.
constexpr int kProgressSteps = 1000;

// A person's words for each token the engine gives as a reason: what a check
// could not do (include/katana/update/check.hpp) and how an apply ended
// (apply.hpp, ApplyResult::reason), with the verbs' own two (update_verbs.hpp:
// no_transport, no_store). Each stands after a colon and before a full stop.
struct ReasonWords {
    const char* token;
    const char* words;
};
constexpr ReasonWords kReasonWords[] = {
    // A check.
    {"offline", "this computer is offline, or the release server did not answer"},
    {"http_403", "the release server refused the request (error 403)"},
    {"http_404", "no update information was found (error 404)"},
    {"http_429", "the release server is limiting requests (error 429); try again later"},
    {"bad_signature", "the update information is not signed by Katana's release key"},
    {"wrong_product", "the update information is for another product"},
    {"wrong_target", "the update information is for another kind of computer"},
    {"malformed", "the update information could not be read"},
    {"untrusted_key", "the update information is signed by a key this copy does not trust"},
    {"placeholder_keys", "this build carries placeholder update keys and accepts no release"},
    {"partial_downloads", "this network does not allow partial downloads"},
    {"cancelled", "it was cancelled"},
    {"disabled_build", "this build has no update feed"},
    {"development_tree", "it is a development build, not an installed release"},
    {"root_not_writable", "its folder cannot be written by you"},
    {"headless", "a headless run does not download or install updates"},
    {"skipped", "you chose to skip that version"},
    {"blocked_before", "that version could not be installed here before"},
    {"no_transport", "this build has no network access"},
    {"no_store", "this session has no credential store"},
    // An apply.
    {"locked", "another update was already running"},
    {"interrupted_update", "an earlier update was interrupted and could not be put back"},
    {"invalid_request", "the update was started with a request it could not read"},
    {"install_record", "the install record could not be read"},
    {"staged_manifest", "the downloaded update is missing or not signed by Katana's release key"},
    {"notify_only", "this release can only be installed by hand"},
    {"applier_too_new", "the release needs a newer update helper than it brought"},
    {"downgrade", "the downloaded version is not newer than the installed one"},
    {"blocked", "other Katana programs were still open"},
    {"scan_failed", "the running programs could not be listed"},
    {"staged_missing", "a downloaded file was missing"},
    {"staged_hash", "a downloaded file did not match the release's signature"},
    {"reparse_point", "a folder of the install is a link to somewhere else"},
    {"path_conflict", "a file stands where a folder is needed, or a folder where a file is"},
    {"copy_failed", "a file could not be copied into the install"},
    {"space", "there is not enough free disk space"},
    {"journal", "the update's journal could not be written"},
    {"io_error", "a file could not be read or written"},
    {"transaction_exists", "an update with the same number was already there"},
    {"rename_failed", "a file was in use and could not be replaced"},
    {"health_timeout", "the new version did not finish starting in time"},
    {"health_exit", "the new version did not start"},
    {"rollback_failed", "the old version could not be put back completely"},
    {"internal_error", "the update stopped on an unexpected error"},
};

// "Katana 0.3.0", or "Katana" alone for a version nobody gave.
QString product(const QString& version)
{
    return version.isEmpty() ? QStringLiteral("Katana") : QStringLiteral("Katana ") + version;
}

std::uint64_t nearestMegabytes(std::uint64_t bytes)
{
    return bytes / kMegabyte + (bytes % kMegabyte >= kMegabyte / 2 ? 1 : 0);
}

// The megabytes that have arrived, never more than the whole is said to be.
QString downloadedMegabytes(const UpdateView& view)
{
    if (view.downloadBytes > 0 && view.downloadedBytes >= view.downloadBytes) {
        return updateMegabytes(view.downloadBytes);
    }
    return QString::number(nearestMegabytes(view.downloadedBytes));
}

// `sentence` after `text`, a blank between them when both are there.
QString with(const QString& text, const QString& sentence)
{
    if (text.isEmpty() || sentence.isEmpty()) {
        return text + sentence;
    }
    return text + QLatin1Char(' ') + sentence;
}

// The buttons a state shows; the others are hidden.
struct ShownButtons {
    bool checkNow = false;
    bool retry = false;
    bool install = false;
    bool downloadOnly = false;
    bool skip = false;
    bool notes = false;
    bool restart = false;
    bool cancel = false;
    bool later = false;
};

ShownButtons buttonsOf(const UpdateView& view)
{
    ShownButtons shown;
    switch (view.state) {
    case UpdateDialogState::Idle:
    case UpdateDialogState::Current:
    case UpdateDialogState::NotOffered:
        shown.checkNow = true;
        break;
    case UpdateDialogState::Checking:
    case UpdateDialogState::Downloading:
        shown.cancel = true;
        break;
    case UpdateDialogState::Available:
        // A copy that cannot replace its own files is told of the release and
        // sent to its notes; nothing here can install it.
        shown.install = !view.notifyOnly;
        shown.downloadOnly = !view.notifyOnly;
        shown.skip = true;
        shown.notes = true;
        shown.later = true;
        break;
    case UpdateDialogState::Ready:
        shown.restart = true;
        shown.later = true;
        break;
    case UpdateDialogState::Blocked:
    case UpdateDialogState::CheckFailed:
        shown.retry = true;
        break;
    case UpdateDialogState::RolledBack:
    case UpdateDialogState::Unsupported:
        break;
    }
    return shown;
}

QLabel* sentence(const char* objectName, QWidget* parent)
{
    auto* label = new QLabel(parent);
    label->setObjectName(QString::fromLatin1(objectName));
    // A version, a program's name and a reason are data, never markup.
    label->setTextFormat(Qt::PlainText);
    label->setWordWrap(true);
    label->setTextInteractionFlags(Qt::TextSelectableByMouse);
    return label;
}

QPushButton* button(const char* label, const char* objectName, const char* tip, QWidget* parent)
{
    auto* made = new QPushButton(QString::fromLatin1(label), parent);
    made->setObjectName(QString::fromLatin1(objectName));
    made->setToolTip(QString::fromLatin1(tip));
    // No button is a default: Enter in a dialog that can close the program
    // must press nothing.
    made->setAutoDefault(false);
    made->setDefault(false);
    return made;
}

} // namespace

QString updateSkipLine(const QString& version)
{
    return QStringLiteral("UPDATE SKIP ") + version;
}

QString updateReasonWords(const QString& token)
{
    for (const ReasonWords& entry : kReasonWords) {
        if (token == QLatin1String(entry.token)) {
            return QString::fromLatin1(entry.words);
        }
    }
    // Any other answer of the server's: the engine writes it http_<status>.
    static const QRegularExpression status(QStringLiteral("^http_([0-9]{3})$"));
    if (const QRegularExpressionMatch match = status.match(token); match.hasMatch()) {
        return QStringLiteral("the release server answered with error %1").arg(match.captured(1));
    }
    return token;
}

QString updateMegabytes(std::uint64_t bytes)
{
    if (bytes == 0) {
        return QStringLiteral("0");
    }
    return QString::number(std::max<std::uint64_t>(1, nearestMegabytes(bytes)));
}

std::optional<std::pair<std::uint64_t, std::uint64_t>>
updateFilesOfProgressLine(const QString& line)
{
    static const QRegularExpression files(
        QStringLiteral("(?:^|\\s)files=([0-9]{1,9})/([0-9]{1,9})(?:\\s|$)"));
    const QRegularExpressionMatch match = files.match(line);
    if (!match.hasMatch()) {
        return std::nullopt;
    }
    return std::pair<std::uint64_t, std::uint64_t>(match.captured(1).toULongLong(),
                                                   match.captured(2).toULongLong());
}

QString updateStateText(const UpdateView& view)
{
    switch (view.state) {
    case UpdateDialogState::Idle:
        return product(view.installedVersion);
    case UpdateDialogState::Checking:
        return QStringLiteral("Checking for updates...");
    case UpdateDialogState::Current:
        return QStringLiteral("%1 is up to date.").arg(product(view.installedVersion));
    case UpdateDialogState::NotOffered:
        if (view.reason == QLatin1String("blocked_before")) {
            return QStringLiteral("%1 could not be installed here before, and is not offered "
                                  "again.")
                .arg(product(view.latestVersion));
        }
        return QStringLiteral("%1 is available, and you chose to skip it.")
            .arg(product(view.latestVersion));
    case UpdateDialogState::Available:
        return QStringLiteral("Update Available");
    case UpdateDialogState::Downloading: {
        QString text = QStringLiteral("Downloading %1 of %2 MB")
                           .arg(downloadedMegabytes(view), updateMegabytes(view.downloadBytes));
        if (view.downloadedFiles && view.changedFiles > 0) {
            text += QStringLiteral(" (%1 of %2 files)")
                        .arg(std::min(*view.downloadedFiles, view.changedFiles))
                        .arg(view.changedFiles);
        }
        return text + QLatin1Char('.');
    }
    case UpdateDialogState::Ready:
        return QStringLiteral("%1 is ready.").arg(product(view.latestVersion));
    case UpdateDialogState::Blocked:
        return view.holders.isEmpty()
                   ? QStringLiteral("Close the other Katana programs that are open.")
                   : QStringLiteral("Close these first: %1.").arg(view.holders);
    case UpdateDialogState::RolledBack:
        if (view.lastStatus == QLatin1String("refused")) {
            return QStringLiteral("%1 could not be installed; nothing was changed.")
                .arg(product(view.latestVersion));
        }
        if (view.lastStatus == QLatin1String("rollback_incomplete")) {
            return QStringLiteral("%1 could not be installed, and %2 could not be put back "
                                  "completely.")
                .arg(product(view.latestVersion), view.installedVersion);
        }
        return QStringLiteral("%1 could not be installed, so %2 was restored.")
            .arg(product(view.latestVersion), view.installedVersion);
    case UpdateDialogState::CheckFailed:
        if (view.reason == QLatin1String("http_404")) {
            return QStringLiteral("No update information found.");
        }
        return view.reason.isEmpty()
                   ? QStringLiteral("Could not check for updates.")
                   : QStringLiteral("Could not check for updates: %1.")
                         .arg(updateReasonWords(view.reason));
    case UpdateDialogState::Unsupported:
        return view.reason.isEmpty()
                   ? QStringLiteral("Updates are not available for this copy of Katana.")
                   : QStringLiteral("Updates are not available for this copy of Katana: %1.")
                         .arg(updateReasonWords(view.reason));
    }
    return {};
}

QString updateDetailText(const UpdateView& view)
{
    switch (view.state) {
    case UpdateDialogState::Idle:
        return QStringLiteral("Not checked for updates yet.");
    case UpdateDialogState::Checking:
    case UpdateDialogState::Downloading:
        return QStringLiteral("You can keep working.");
    case UpdateDialogState::Current:
        return view.lastChecked.isEmpty() ? QString()
                                          : QStringLiteral("Last checked %1.").arg(view.lastChecked);
    case UpdateDialogState::NotOffered:
        return view.reason == QLatin1String("blocked_before")
                   ? QStringLiteral("A later release will be offered.")
                   : QStringLiteral("Settings > Updates offers it again.");
    case UpdateDialogState::Available: {
        QString text = QStringLiteral("%1 (you have %2).")
                           .arg(product(view.latestVersion), view.installedVersion);
        text = with(text, view.notifyOnly
                              ? QStringLiteral("This copy cannot update itself: the release "
                                               "notes say where to download it.")
                              : QStringLiteral("%1 of %2 files changed, %3 MB to download.")
                                    .arg(view.changedFiles)
                                    .arg(view.totalFiles)
                                    .arg(updateMegabytes(view.downloadBytes)));
        if (view.informationIsOld) {
            text = with(text, QStringLiteral("The update information is old: a newer release "
                                             "may exist."));
        }
        return text;
    }
    case UpdateDialogState::Ready:
        return view.waitingForJobs
                   ? QStringLiteral("Katana will close when the running job has finished, then "
                                    "update and reopen.")
                   : QStringLiteral("Katana will close, update and reopen in a few seconds.");
    case UpdateDialogState::Blocked:
        return QStringLiteral("Then press Retry.");
    case UpdateDialogState::RolledBack: {
        const QString reason = view.reason.isEmpty()
                                   ? QString()
                                   : QStringLiteral("Reason: %1.")
                                         .arg(updateReasonWords(view.reason));
        if (view.lastStatus == QLatin1String("rollback_incomplete") &&
            !view.recoverCommand.isEmpty()) {
            return with(reason,
                        QStringLiteral("Close Katana and run: %1").arg(view.recoverCommand));
        }
        return reason;
    }
    case UpdateDialogState::CheckFailed:
        return view.reason == QLatin1String("http_404")
                   ? QStringLiteral("If the releases are private, add an access token in "
                                    "Settings > Updates.")
                   : QString();
    case UpdateDialogState::Unsupported:
        return {};
    }
    return {};
}

UpdateDialog::UpdateDialog(UpdateDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName(QStringLiteral("updateDialog"));
    setWindowTitle(QStringLiteral("Check for Updates"));
    setModal(false);

    auto* outer = new QVBoxLayout(this);
    state_ = sentence("updateStateText", this);
    // The first sentence is the answer; it is read before anything else.
    QFont heading = state_->font();
    heading.setBold(true);
    state_->setFont(heading);
    detail_ = sentence("updateDetailText", this);
    problem_ = sentence("updateProblemText", this);
    problem_->setStyleSheet(QStringLiteral("color: %1;").arg(theme::error().name()));
    problem_->hide();
    progress_ = new QProgressBar(this);
    progress_->setObjectName(QStringLiteral("updateProgress"));
    progress_->setTextVisible(false);
    progress_->hide();
    outer->addWidget(state_);
    outer->addWidget(detail_);
    outer->addWidget(problem_);
    outer->addWidget(progress_);
    outer->addStretch(1);

    checkNow_ = button("Check now", "updateCheckNow",
                       "Ask the release server whether a newer Katana exists (UPDATE CHECK)", this);
    retry_ = button("Retry", "updateRetry", "Try again", this);
    install_ = button("Update", "updateInstall",
                      "Download what changed, then close Katana, update it and open it again",
                      this);
    downloadOnly_ = button("Download only", "updateDownloadOnly",
                           "Download what changed now and install it later (UPDATE DOWNLOAD "
                           "CONFIRM)",
                           this);
    skip_ = button("Skip this version", "updateSkip",
                   "Do not offer this version again; the next one is offered (UPDATE SKIP)", this);
    notes_ = button("Release notes", "updateNotes", "Open the release notes in your browser",
                    this);
    restart_ = button("Restart and update", "updateRestart",
                      "Close Katana, install the downloaded version and open Katana again (UPDATE "
                      "APPLY CONFIRM)",
                      this);
    cancel_ = button("Cancel", "updateCancel",
                     "Stop the check or the download; what has been downloaded is kept for the "
                     "next try",
                     this);
    later_ = button("Later", "updateLater", "Close this window and change nothing", this);
    // In the order the states list them: what acts first, Later last.
    auto* row = new QHBoxLayout;
    row->addStretch(1);
    for (QPushButton* each :
         {checkNow_, retry_, install_, downloadOnly_, skip_, notes_, restart_, cancel_, later_}) {
        row->addWidget(each);
    }
    outer->addLayout(row);
    // Wide enough for Available's five buttons in one row and its sentence in
    // two lines; the height follows the text.
    setMinimumWidth(560);

    // ---- wiring: lambdas, no moc.
    QObject::connect(checkNow_, &QPushButton::clicked, this,
                     [this] { runLine(QString::fromLatin1(kUpdateCheckLine)); });
    // What failed is what is tried again: the swap for a copy that was
    // blocked, the check otherwise.
    QObject::connect(retry_, &QPushButton::clicked, this, [this] {
        runLine(QString::fromLatin1(view_.state == UpdateDialogState::Blocked ? kUpdateApplyLine
                                                                              : kUpdateCheckLine));
    });
    QObject::connect(install_, &QPushButton::clicked, this, [this] {
        if (context_.update) {
            context_.update();
        }
    });
    QObject::connect(downloadOnly_, &QPushButton::clicked, this,
                     [this] { runLine(QString::fromLatin1(kUpdateDownloadLine)); });
    QObject::connect(skip_, &QPushButton::clicked, this, [this] {
        if (!view_.latestVersion.isEmpty()) {
            runLine(updateSkipLine(view_.latestVersion));
        }
    });
    QObject::connect(notes_, &QPushButton::clicked, this, [this] {
        if (context_.openNotes && !view_.notesUrl.isEmpty()) {
            context_.openNotes(view_.notesUrl);
        }
    });
    QObject::connect(restart_, &QPushButton::clicked, this,
                     [this] { runLine(QString::fromLatin1(kUpdateApplyLine)); });
    QObject::connect(cancel_, &QPushButton::clicked, this, [this] {
        if (context_.cancel) {
            context_.cancel();
        }
    });
    QObject::connect(later_, &QPushButton::clicked, this, [this] {
        if (context_.later) {
            context_.later();
        }
        close();
    });
    refresh();
}

UpdateDialog::~UpdateDialog()
{
    // The children outlive this body and one dying can still signal: cut
    // every connection into this dialog first, as the Settings dialog does.
    for (QObject* child : findChildren<QObject*>()) {
        QObject::disconnect(child, nullptr, this, nullptr);
    }
}

void UpdateDialog::setView(UpdateView view)
{
    view_ = std::move(view);
    refusal_.clear();
    refresh();
}

void UpdateDialog::detach()
{
    context_ = UpdateDialogContext{};
    refresh();
}

void UpdateDialog::runLine(const QString& line)
{
    if (!context_.run) {
        return;
    }
    refusal_.clear();
    // A copy: the owner may set a view, or detach, from inside its own line.
    const CommandRunner run = context_.run;
    const VerbOutcome outcome = run(line);
    if (!outcome.ok) {
        refusal_ = outcome.error.isEmpty() ? QStringLiteral("The command was refused.")
                                           : outcome.error;
    }
    refresh();
}

void UpdateDialog::refresh()
{
    state_->setText(updateStateText(view_));
    detail_->setText(updateDetailText(view_));
    const QString problem = refusal_.isEmpty() ? view_.problem : refusal_;
    problem_->setText(problem);
    problem_->setVisible(!problem.isEmpty());

    const bool downloading = view_.state == UpdateDialogState::Downloading;
    progress_->setVisible(downloading || view_.state == UpdateDialogState::Checking);
    if (downloading && view_.downloadBytes > 0) {
        const std::uint64_t done = std::min(view_.downloadedBytes, view_.downloadBytes);
        progress_->setRange(0, kProgressSteps);
        progress_->setValue(static_cast<int>(
            (done * kProgressSteps + view_.downloadBytes / 2) / view_.downloadBytes));
    } else {
        // Busy: a check cannot say how far it is, nor a download of no known size.
        progress_->setRange(0, 0);
    }

    const ShownButtons shown = buttonsOf(view_);
    checkNow_->setVisible(shown.checkNow);
    retry_->setVisible(shown.retry);
    install_->setVisible(shown.install);
    downloadOnly_->setVisible(shown.downloadOnly);
    skip_->setVisible(shown.skip);
    notes_->setVisible(shown.notes);
    restart_->setVisible(shown.restart);
    cancel_->setVisible(shown.cancel);
    later_->setVisible(shown.later);

    // A button with nothing to call is disabled: a dialog whose owner has
    // gone, or a test's that gave no executor.
    const bool lines = static_cast<bool>(context_.run);
    checkNow_->setEnabled(lines);
    retry_->setEnabled(lines);
    install_->setEnabled(static_cast<bool>(context_.update));
    downloadOnly_->setEnabled(lines);
    skip_->setEnabled(lines && !view_.latestVersion.isEmpty());
    notes_->setEnabled(static_cast<bool>(context_.openNotes) && !view_.notesUrl.isEmpty());
    // While the window waits for a job before it closes, the press is already made.
    restart_->setEnabled(lines && !view_.waitingForJobs);
    cancel_->setEnabled(static_cast<bool>(context_.cancel));
}

} // namespace katana::qt
