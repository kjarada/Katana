// Help > Check for Updates (update_dialog.hpp).

#include "update_dialog.hpp"

#include <algorithm>
#include <cstddef>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <QFont>
#include <QHBoxLayout>
#include <QLabel>
#include <QLatin1String>
#include <QProgressBar>
#include <QPushButton>
#include <QRegularExpression>
#include <QStringList>
#include <QVBoxLayout>

#include "katana/core/error.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/check.hpp"
#include "katana/update/manifest.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

// A million bytes, the megabyte of a download's size as a browser and a
// release page say it (SI; the binary unit is the mebibyte).
constexpr std::uint64_t kMegabyte = 1'000'000;
// The progress bar's range: thousandths, as the status bar's job progress is.
constexpr int kProgressSteps = 1000;

// A person's words for each token the engine gives as a reason: what a check
// could not do (include/katana/update/check.hpp), what the transport under it
// and under the download refused (feed.hpp, fetchFailureReason), why a
// download or the helper's preflight stopped (stage.hpp) and how an apply ended
// (apply.hpp, ApplyResult::reason), with the verbs' own (update_verbs.hpp:
// no_transport, no_store, and the two refusals of a copy that may not install
// here and now, platform and elevated). Each stands after a colon and before
// a full stop, and where there is something a person can do about it, says
// what.
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
    {"wrong_channel", "the update information is for another release channel"},
    {"malformed", "the update information could not be read"},
    {"too_large", "the update information is longer than update information can be"},
    {"html_page", "a web page answered in the place of the release server; this network may "
                  "want you to sign in first"},
    {"asset_missing", "the release is missing a file the update needs"},
    {"verify_unavailable", "the signature of the update information could not be checked on "
                           "this computer"},
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
    // A copy that may not install: where this version installs nothing (it is
    // written for Windows; the download, the apply and the helper refuse
    // alike), and a Katana started as administrator, whose helper would run
    // with those rights from a folder ordinary programs can write
    // (update/process.hpp, processIsElevated).
    {"platform", "this copy of Katana cannot install an update on this kind of computer; the "
                 "release notes say where to download it"},
    {"elevated", "Katana was started as administrator; start it the ordinary way, not as "
                 "administrator, and update again - the download is kept"},
    // The transport, under a check and under a download.
    {"bad_range", "the release server sent another part of the download than was asked for"},
    {"bad_redirect", "the release server sent the request on to an address that cannot be used"},
    {"too_many_redirects", "the release server sent the request on too many times"},
    {"error", "the release server's answer could not be used"},
    // A download.
    {"staged_corrupt", "a downloaded file could not be unpacked"},
    {"write_failed", "the folder the download is kept in could not be written"},
    {"stage_busy", "another Katana is downloading this update now"},
    {"low_disk_space", "there is not enough free disk space for the download"},
    {"short_range", "the release server sent less of the download than was asked for"},
    {"unverified_manifest", "the update information was not verified"},
    {"no_token", "the releases are private and no access token is stored"},
    {"payload_mismatch", "the release's download is not the one its update information names"},
    // The helper's preflight.
    {"helper_missing", "the update helper program is missing"},
    {"helper_not_started", "the update helper could not be started"},
    {"helper_timeout", "the update helper did not answer in time"},
    {"helper_failed", "the update helper did not pass its own test"},
    // An apply.
    {"locked", "another update was already running"},
    // An earlier update of this install is still open: one a crash
    // interrupted that could not be put back, or one whose helper died while
    // the new version was proving that it starts - installed, and neither
    // confirmed nor put back yet, which a start of the window or the next
    // logon settles (update/recover.hpp).
    {"interrupted_update", "an earlier update is not settled yet"},
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

// What is said of a release that is set aside because it did not start here:
// that it is not the end of updates, and where the way back is. One start too
// slow for the health check sets a sound release aside, so the person must be
// told how to have it offered again: Settings > Updates has the button
// (settingsUpdateUnblock, which runs UPDATE BLOCKED CLEAR).
QString setAsideWords()
{
    return QStringLiteral("A later release will be offered, and Settings > Updates offers this "
                          "one again.");
}

// Whether the apply this view tells of left its release set aside. The helper
// puts a release on the state's blocked list when it was installed, did not
// prove that it starts and was put back - a rollback for either of the health
// check's two reasons (src/katana_update/apply.cpp, Applier::fail) - and for
// no other ending: a swap that failed for want of disk space is offered again
// as it was.
bool wasSetAside(const UpdateView& view)
{
    return view.lastStatus == QLatin1String("rolled_back") &&
           (view.reason == QLatin1String("health_exit") ||
            view.reason == QLatin1String("health_timeout"));
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

namespace {

// The words of `line` when it is UPDATE TOKEN with something after it, and
// none otherwise: the lines whose later words may be a token somebody typed.
QStringList tokenLineWords(const QString& line)
{
    QStringList words = line.simplified().split(QChar::Space, Qt::SkipEmptyParts);
    if (words.size() < 3 || words[0].compare(QLatin1String("UPDATE"), Qt::CaseInsensitive) != 0 ||
        words[1].compare(QLatin1String("TOKEN"), Qt::CaseInsensitive) != 0) {
        words.clear();
    }
    return words;
}

// The two words the grammar has after UPDATE TOKEN; every other is hidden.
bool isTokenVerbWord(const QString& word)
{
    return word.compare(QLatin1String("STATUS"), Qt::CaseInsensitive) == 0 ||
           word.compare(QLatin1String("CLEAR"), Qt::CaseInsensitive) == 0;
}

} // namespace

QString updateLoggedLine(const QString& line)
{
    const QStringList words = tokenLineWords(line);
    if (words.isEmpty()) {
        return line;
    }
    QStringList shown{words[0], words[1]};
    bool hidden = false;
    for (qsizetype at = 2; at < words.size(); ++at) {
        const bool known = isTokenVerbWord(words[at]);
        shown << (known ? words[at] : QStringLiteral("***"));
        hidden = hidden || !known;
    }
    // A line with nothing to hide is echoed as it was typed, blanks and all.
    return hidden ? shown.join(QChar::Space) : line;
}

QString updateLoggedReply(const QString& reply, const QString& line)
{
    const QStringList words = tokenLineWords(line);
    QString shown = reply;
    for (qsizetype at = 2; at < words.size(); ++at) {
        if (!isTokenVerbWord(words[at])) {
            shown.replace(words[at], QStringLiteral("***"), Qt::CaseInsensitive);
        }
    }
    return shown;
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
        return view.lastChecked.isEmpty()
                   ? QString()
                   : QStringLiteral("Last checked %1.").arg(view.lastChecked);
    case UpdateDialogState::NotOffered:
        return view.reason == QLatin1String("blocked_before")
                   ? setAsideWords()
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
        if (view.waitingForJobs) {
            return QStringLiteral("Katana will close when the running job has finished, then "
                                  "update and reopen.");
        }
        if (view.waitingForDialog) {
            return QStringLiteral("Katana will close when the dialog that is open has been "
                                  "closed, then update and reopen.");
        }
        return QStringLiteral("Katana will close, update and reopen in a few seconds.");
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
        // The release that was put back is not offered again by itself.
        return wasSetAside(view) ? with(reason, setAsideWords()) : reason;
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

// ---- the decisions (update_dialog.hpp, "THE DECISIONS") ----------------------------------------

bool updateSameVersion(const QString& a, const QString& b)
{
    const katana::core::Result<katana::update::Version> first =
        katana::update::parseVersion(a.toStdString());
    const katana::core::Result<katana::update::Version> second =
        katana::update::parseVersion(b.toStdString());
    if (first && second) {
        return *first == *second;
    }
    return a == b;
}

QString updateReplyValue(const QString& reply, const char* key)
{
    // A record is a word: the two keys read here, reason and staged, have
    // values without a blank in them.
    static const QRegularExpression blanks(QStringLiteral("\\s+"));
    const QString prefix = QString::fromLatin1(key) + QLatin1Char('=');
    QString value;
    for (const QString& word : reply.split(blanks, Qt::SkipEmptyParts)) {
        if (word.startsWith(prefix)) {
            value = word.mid(prefix.size());
        }
    }
    if (value.size() >= 2 && value.startsWith(QLatin1Char('"')) &&
        value.endsWith(QLatin1Char('"'))) {
        value = value.mid(1, value.size() - 2);
    }
    return value;
}

QString updateStagedOfReply(const QString& reply)
{
    const QString staged = updateReplyValue(reply, "staged");
    return staged.compare(QLatin1String("none"), Qt::CaseInsensitive) == 0 ? QString() : staged;
}

QString updateStatusButtonText(const UpdateView& view)
{
    if (view.latestVersion.isEmpty()) {
        return {};
    }
    switch (view.state) {
    case UpdateDialogState::Available:
    case UpdateDialogState::Downloading:
    case UpdateDialogState::Ready:
    case UpdateDialogState::Blocked:
        return QStringLiteral("Update ") + view.latestVersion;
    case UpdateDialogState::Idle:
    case UpdateDialogState::Checking:
    case UpdateDialogState::Current:
    case UpdateDialogState::NotOffered:
    case UpdateDialogState::RolledBack:
    case UpdateDialogState::CheckFailed:
    case UpdateDialogState::Unsupported:
        break;
    }
    return {};
}

UpdateView updateViewOfOffer(const katana::app::UpdateOffer& offer, const UpdateOfferFacts& facts)
{
    using katana::update::CheckStatus;
    const katana::update::CheckResult& check = offer.check;
    UpdateView view;
    view.installedVersion = check.installedVersion.empty()
                                ? facts.installedVersion
                                : QString::fromStdString(check.installedVersion);
    view.latestVersion = QString::fromStdString(check.latestVersion);
    view.notesUrl = QString::fromStdString(check.notesUrl);
    view.reason = QString::fromStdString(check.reason);
    switch (check.status) {
    case CheckStatus::Available:
        // The release that is already downloaded is ready, not to be
        // downloaded again.
        if (!facts.stagedVersion.isEmpty() &&
            updateSameVersion(facts.stagedVersion, view.latestVersion)) {
            view.state = UpdateDialogState::Ready;
            break;
        }
        view.state = UpdateDialogState::Available;
        view.changedFiles = offer.changedFiles;
        view.totalFiles = offer.totalFiles;
        view.downloadBytes = offer.downloadBytes;
        // `expires` is advisory: said, and the update still offered.
        view.informationIsOld = check.manifest.has_value() &&
                                katana::update::isExpired(*check.manifest, facts.nowEpoch);
        view.notifyOnly =
            !facts.appliesHere || (check.manifest.has_value() && check.manifest->notifyOnly);
        break;
    case CheckStatus::Current:
        if (check.reason == "skipped") {
            view.state = UpdateDialogState::NotOffered;
        } else {
            view.state = UpdateDialogState::Current;
            view.lastChecked = facts.checkedAt;
        }
        break;
    case CheckStatus::Blocked:
        view.state = UpdateDialogState::NotOffered;
        view.reason = QStringLiteral("blocked_before");
        break;
    case CheckStatus::Unreachable:
        view.state = UpdateDialogState::CheckFailed;
        break;
    case CheckStatus::Unsupported:
        view.state = UpdateDialogState::Unsupported;
        break;
    }
    return view;
}

UpdateQuietAnswer updateQuietAnswer(const UpdateView& shown, const UpdateView& answer)
{
    // What must stay until it is read, and what a running job owns.
    if (shown.state == UpdateDialogState::RolledBack ||
        shown.state == UpdateDialogState::Checking ||
        shown.state == UpdateDialogState::Downloading) {
        return UpdateQuietAnswer::Nothing;
    }
    switch (answer.state) {
    case UpdateDialogState::Available:
    case UpdateDialogState::Ready:
        // A release the view already offers - to download, downloaded, or
        // waiting for other programs to close - is no news.
        if (!updateStatusButtonText(shown).isEmpty() &&
            updateSameVersion(shown.latestVersion, answer.latestVersion)) {
            return UpdateQuietAnswer::Nothing;
        }
        return UpdateQuietAnswer::Announce;
    case UpdateDialogState::Current:
    case UpdateDialogState::NotOffered:
        return UpdateQuietAnswer::Show;
    case UpdateDialogState::Idle:
    case UpdateDialogState::Checking:
    case UpdateDialogState::Downloading:
    case UpdateDialogState::Blocked:
    case UpdateDialogState::RolledBack:
    case UpdateDialogState::CheckFailed:
    case UpdateDialogState::Unsupported:
        break;
    }
    return UpdateQuietAnswer::Nothing;
}

namespace {

// Whether `word` is the grammar's `expected`, typed in any case.
bool isWord(const QString& word, const char* expected)
{
    return word.compare(QLatin1String(expected), Qt::CaseInsensitive) == 0;
}

} // namespace

UpdateLineEffect updateEffectOfLine(const UpdateView& shown, const QString& line,
                                    const QString& reply)
{
    // A reply that gives a reason is one that was not carried out.
    if (!updateReplyValue(reply, "reason").isEmpty()) {
        return UpdateLineEffect::Nothing;
    }
    // The lines read here have three words each: UPDATE SKIP NONE, UPDATE
    // SKIP <version>, UPDATE BLOCKED CLEAR.
    const QStringList words = line.simplified().split(QChar::Space, Qt::SkipEmptyParts);
    if (words.size() != 3 || !isWord(words[0], "UPDATE")) {
        return UpdateLineEffect::Nothing;
    }
    const bool notOffered = shown.state == UpdateDialogState::NotOffered;
    if (isWord(words[1], "SKIP")) {
        if (isWord(words[2], "NONE")) {
            return notOffered && shown.reason == QLatin1String("skipped")
                       ? UpdateLineEffect::Forgotten
                       : UpdateLineEffect::Nothing;
        }
        return shown.state == UpdateDialogState::Available &&
                       updateSameVersion(shown.latestVersion, words[2])
                   ? UpdateLineEffect::Skipped
                   : UpdateLineEffect::Nothing;
    }
    if (isWord(words[1], "BLOCKED") && isWord(words[2], "CLEAR")) {
        return notOffered && shown.reason == QLatin1String("blocked_before")
                   ? UpdateLineEffect::Forgotten
                   : UpdateLineEffect::Nothing;
    }
    return UpdateLineEffect::Nothing;
}

QString updateStagedAfter(const QString& staged, const UpdateView& answer)
{
    if (answer.state == UpdateDialogState::Current) {
        return {};
    }
    if (answer.state == UpdateDialogState::NotOffered &&
        updateSameVersion(staged, answer.latestVersion)) {
        return {};
    }
    return staged;
}

QString updateStagedToOffer(const katana::app::UpdateStatus& status,
                            const katana::update::Version& installed,
                            const std::function<bool(std::uint64_t sequence)>& stageIsThere)
{
    if (status.stagedVersion.empty()) {
        return {};
    }
    const katana::core::Result<katana::update::Version> staged =
        katana::update::parseVersion(status.stagedVersion);
    if (!staged || !(*staged > installed)) {
        return {};
    }
    const QString version = QString::fromStdString(status.stagedVersion);
    if (!status.state.skippedVersion.empty() &&
        updateSameVersion(version, QString::fromStdString(status.state.skippedVersion))) {
        return {};
    }
    if (stageIsThere) {
        for (const std::uint64_t sequence : status.state.blockedSequences) {
            if (stageIsThere(sequence)) {
                return {};
            }
        }
    }
    return version;
}

std::optional<UpdateView> updateViewOfLastResult(const katana::update::LastResult& last,
                                                 const QString& installedVersion,
                                                 const QString& installRoot)
{
    if (last.status == "committed") {
        return std::nullopt;
    }
    UpdateView view;
    view.installedVersion = installedVersion;
    view.latestVersion = QString::fromStdString(last.version);
    view.reason = QString::fromStdString(last.reason);
    if (last.status == "blocked") {
        // Nothing was touched and the release is still downloaded: Retry
        // applies it again once the programs named are closed.
        view.state = UpdateDialogState::Blocked;
        view.holders = QString::fromStdString(last.holder);
        return view;
    }
    view.state = UpdateDialogState::RolledBack;
    view.lastStatus = QString::fromStdString(last.status);
    if (last.status == "rollback_incomplete") {
        view.recoverCommand = QStringLiteral("katana_updater --recover \"%1\"").arg(installRoot);
    }
    return view;
}

bool updateAutomaticCheckIsDue(const UpdateAutoCheckFacts& facts,
                               const katana::update::UpdateState& state, std::int64_t nowEpoch,
                               std::chrono::seconds jitter)
{
    if (facts.headless || facts.disabled || facts.jobRunning || !facts.keepsState ||
        facts.kind == katana::update::InstallKind::Development) {
        return false;
    }
    return katana::update::checkIsDue(state, nowEpoch, jitter);
}

std::optional<QString>
updateSkippedOfState(const katana::core::Result<katana::update::UpdateState>& state)
{
    if (!state) {
        return std::nullopt; // nobody can say: never "none"
    }
    return QString::fromStdString(state->skippedVersion);
}

std::optional<bool>
updatePutBackOfState(const katana::core::Result<katana::update::UpdateState>& state)
{
    if (!state) {
        return std::nullopt; // nobody can say: never "none"
    }
    return !state->blockedSequences.empty();
}

bool updateStartMayConfirm(const UpdateConfirmFacts& facts)
{
    if (!facts.appliesHere || facts.headless || !facts.keepsState ||
        facts.kind == katana::update::InstallKind::Development) {
        return false;
    }
    // As versions: "0.3" and "0.3.0" are one release (update/version.hpp).
    // Equal, and not "not above": what `installed` can be is the reader's
    // rule (update_dialog.hpp), and this one stands without it.
    return facts.hasInstalledRecord && facts.installed == facts.program;
}

bool updateFailureIsBoxed(const QStringList& stepKinds)
{
    // The two kinds of step that run a line in a window that goes on to its
    // event loop (main(): an --enter is kept as a --command).
    return !stepKinds.contains(QStringLiteral("--command")) &&
           !stepKinds.contains(QStringLiteral("--run-line"));
}

std::vector<std::string>
updateRecoveredTransactions(std::string_view recoveryLog,
                            const std::function<bool(const std::string& transaction)>& isOver)
{
    // A line of `katana_updater --recover` after its first word, the time, and
    // the word that follows its list of ids (update/recover.hpp).
    constexpr std::string_view kFound = "recover found=";
    constexpr std::string_view kResult = "result=";
    std::vector<std::string> over;
    if (!isOver) {
        return over;
    }
    // Every id met so far: a recovery that failed and the one that then put the
    // install right both name the transaction, and it is asked about once.
    std::vector<std::string> met;
    // Line by line, each ended by its line feed: what is left after the last
    // one is a write that was cut short, and is not read.
    for (std::size_t lineEnd = recoveryLog.find('\n'); lineEnd != std::string_view::npos;
         lineEnd = recoveryLog.find('\n')) {
        std::string_view line = recoveryLog.substr(0, lineEnd);
        recoveryLog.remove_prefix(lineEnd + 1);
        const std::size_t timeEnd = line.find(' ');
        if (timeEnd == std::string_view::npos) {
            continue;
        }
        line.remove_prefix(timeEnd + 1);
        if (!line.starts_with(kFound)) {
            continue;
        }
        line.remove_prefix(kFound.size());
        const std::size_t listEnd = line.find(' ');
        if (listEnd == std::string_view::npos || !line.substr(listEnd + 1).starts_with(kResult)) {
            continue;
        }
        std::string_view list = line.substr(0, listEnd);
        while (!list.empty()) {
            const std::size_t comma = list.find(',');
            std::string id(list.substr(0, comma));
            list.remove_prefix(comma == std::string_view::npos ? list.size() : comma + 1);
            if (!katana::update::isValidTransactionId(id) ||
                std::find(met.begin(), met.end(), id) != met.end()) {
                continue;
            }
            if (isOver(id)) {
                over.push_back(id);
            }
            met.push_back(std::move(id));
        }
    }
    return over;
}

// ---- the dialog --------------------------------------------------------------------------------

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
    // While the window waits - for a job, for a dialog that is open - before
    // it closes, the press is already made.
    restart_->setEnabled(lines && !view_.waitingForJobs && !view_.waitingForDialog);
    cancel_->setEnabled(static_cast<bool>(context_.cancel));
}

} // namespace katana::qt
