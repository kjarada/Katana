// The UPDATE verbs (update_verbs.hpp): one line in, one reply out, over the
// update engine (include/katana/update). Nothing here is the engine's work -
// the feed, the signature, the plan, the staging and the helper are its - and
// nothing here is a front end's: what a line needs comes in the context, so
// that katana_cli, the window, its headless form and katana_mcp's tool run
// these very functions, and a test runs them on a fake feed and a scratch
// folder.

#include "update_verbs.hpp"

#include <algorithm>
#include <atomic>
#include <charconv>
#include <chrono>
#include <cstddef>
#include <format>
#include <memory>
#include <system_error>
#include <utility>
#include <vector>

#include "import_records.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/plan.hpp"
#include "katana/update/registry.hpp"
#include "katana/update/stage.hpp"
#include "katana/update/version.hpp"
#include "update_transport.hpp"

namespace katana::app {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
namespace fs = std::filesystem;

constexpr const char* kUsage =
    "UPDATE CHECK | UPDATE STATUS | UPDATE DOWNLOAD CONFIRM | UPDATE APPLY CONFIRM | "
    "UPDATE SKIP <version>|NONE | UPDATE SET AUTO ON|OFF | UPDATE TOKEN STATUS|CLEAR";

// What a line without its CONFIRM is told: that nothing was done, and what
// the line with the word does - so that the word is typed knowing it.
constexpr const char* kDownloadNeedsConfirm =
    "UPDATE DOWNLOAD needs the word CONFIRM, and nothing was downloaded: UPDATE DOWNLOAD "
    "CONFIRM asks the release server for the newest release and downloads the files of it "
    "that differ from the installed ones into the per-user update folder; the install itself "
    "is not changed";
constexpr const char* kApplyNeedsConfirm =
    "UPDATE APPLY needs the word CONFIRM, and nothing was changed: UPDATE APPLY CONFIRM ends "
    "this session and has the update helper replace the installed files with the downloaded "
    "release, putting every one back if the new version does not start";

// No word after TOKEN is repeated in what is said back: a person who types a
// token there has typed it into a log, and this must not write it twice.
constexpr const char* kTokenUsage =
    "UPDATE TOKEN takes STATUS or CLEAR and no other word. A token is never typed on a line, "
    "because lines are logged: the window's Settings > Updates stores one, and the environment "
    "variable KATANA_UPDATE_TOKEN gives a run one";

// What the transport of a build without an HTTP stack answers every request
// with (update_transport.cpp). No server can cure it, so it is this copy that
// cannot update, not a feed that cannot be reached.
constexpr std::string_view kNoNetworkReason = "no_network";

// A copy of Katana replaces its own files on Windows only in this version: the
// swap's file system and the helper's launcher are not written for Linux
// (include/katana/update/fsops.hpp), and a macOS bundle is sealed. Elsewhere a
// release is told of and no more. The window's own switch for its buttons is
// kAppliesHere (src/katana_qt/update_workbench.cpp).
#ifdef _WIN32
constexpr bool kInstallsHere = true;
#else
constexpr bool kInstallsHere = false;
#endif

// The most progress lines one download says: a line each time another tenth
// has arrived, so that a log shows it moving and a reply stays a screenful
// however the transport chunks its transfer.
constexpr std::uint64_t kProgressSteps = 10;

// The surfaces that run every line, for the words of a refusal.
constexpr const char* kWhoRunsEveryLine = "the Katana window or katana_cli";

bool sameWord(std::string_view a, std::string_view b)
{
    if (a.size() != b.size()) {
        return false;
    }
    for (std::size_t i = 0; i < a.size(); ++i) {
        if (katana::core::asciiLower(a[i]) != katana::core::asciiLower(b[i])) {
            return false;
        }
    }
    return true;
}

// The line's words. No quotes are read: no word of this grammar holds a blank.
std::vector<std::string_view> wordsOf(std::string_view line)
{
    constexpr std::string_view kBlanks = " \t\r\n";
    std::vector<std::string_view> words;
    for (std::size_t at = line.find_first_not_of(kBlanks); at != std::string_view::npos;) {
        const std::size_t end = line.find_first_of(kBlanks, at);
        words.push_back(line.substr(at, end == std::string_view::npos ? end : end - at));
        at = end == std::string_view::npos ? end : line.find_first_not_of(kBlanks, end);
    }
    return words;
}

enum class Action { Check, Status, Download, Apply, Skip, SetAuto, TokenStatus, TokenClear };

// The line an action is, for the words of a refusal.
const char* lineOf(Action action)
{
    switch (action) {
    case Action::Check:
        return "UPDATE CHECK";
    case Action::Status:
        return "UPDATE STATUS";
    case Action::Download:
        return "UPDATE DOWNLOAD";
    case Action::Apply:
        return "UPDATE APPLY";
    case Action::Skip:
        return "UPDATE SKIP";
    case Action::SetAuto:
        return "UPDATE SET AUTO";
    case Action::TokenStatus:
        return "UPDATE TOKEN STATUS";
    case Action::TokenClear:
        return "UPDATE TOKEN CLEAR";
    }
    return "UPDATE";
}

struct UpdateLine {
    Action action = Action::Status;
    std::string version; // SKIP: the version as update::toString writes it; empty for NONE
    bool on = false;     // SET AUTO
};

Result<UpdateLine> parse(std::string_view line)
{
    const std::vector<std::string_view> words = wordsOf(line);
    const auto refused = [](const std::string& why) {
        return makeError(ErrorCode::InvalidArgument, why + "; usage: " + kUsage);
    };
    if (words.empty() || !sameWord(words[0], "UPDATE")) {
        return makeError(ErrorCode::InvalidArgument, "not an UPDATE line");
    }
    if (words.size() < 2) {
        return makeError(ErrorCode::InvalidArgument, std::string("usage: ") + kUsage);
    }
    const std::string_view action = words[1];
    UpdateLine parsed;
    if (sameWord(action, "CHECK") || sameWord(action, "STATUS")) {
        parsed.action = sameWord(action, "CHECK") ? Action::Check : Action::Status;
        if (words.size() != 2) {
            return refused(std::string(lineOf(parsed.action)) + " takes no other word");
        }
        return parsed;
    }
    if (sameWord(action, "DOWNLOAD") || sameWord(action, "APPLY")) {
        parsed.action = sameWord(action, "DOWNLOAD") ? Action::Download : Action::Apply;
        if (words.size() == 3 && sameWord(words[2], "CONFIRM")) {
            return parsed;
        }
        if (words.size() == 2) {
            return makeError(ErrorCode::InvalidArgument, parsed.action == Action::Download
                                                             ? kDownloadNeedsConfirm
                                                             : kApplyNeedsConfirm);
        }
        return refused(std::string(lineOf(parsed.action)) + " takes CONFIRM and no other word");
    }
    if (sameWord(action, "SKIP")) {
        parsed.action = Action::Skip;
        if (words.size() != 3) {
            return refused("UPDATE SKIP takes one version, or NONE");
        }
        if (sameWord(words[2], "NONE")) {
            return parsed;
        }
        const Result<update::Version> version = update::parseVersion(words[2]);
        if (!version) {
            return refused("'" + std::string(words[2]) +
                           "' is not a version (two to four numbers with dots between, as 0.3.0)");
        }
        parsed.version = update::toString(*version);
        return parsed;
    }
    if (sameWord(action, "SET")) {
        parsed.action = Action::SetAuto;
        if (words.size() != 4 || !sameWord(words[2], "AUTO") ||
            !(sameWord(words[3], "ON") || sameWord(words[3], "OFF"))) {
            return refused("UPDATE SET takes AUTO ON or AUTO OFF");
        }
        parsed.on = sameWord(words[3], "ON");
        return parsed;
    }
    if (sameWord(action, "TOKEN")) {
        if (words.size() == 3 && sameWord(words[2], "STATUS")) {
            parsed.action = Action::TokenStatus;
            return parsed;
        }
        if (words.size() == 3 && sameWord(words[2], "CLEAR")) {
            parsed.action = Action::TokenClear;
            return parsed;
        }
        return makeError(ErrorCode::InvalidArgument, kTokenUsage);
    }
    return refused("'" + std::string(action) + "' is not an UPDATE action");
}

// ---- what a reply is made of ----------------------------------------------------------------

// " key=value", the value quoted when it holds a blank; nothing for no value,
// so that a key that is there always says something.
void add(std::string& record, std::string_view key, std::string_view value)
{
    if (value.empty()) {
        return;
    }
    record += ' ';
    record += key;
    record += '=';
    record += recordText(value);
}

std::string joined(const std::vector<std::string>& lines)
{
    std::string text;
    for (const std::string& line : lines) {
        if (!text.empty()) {
            text += '\n';
        }
        text += line;
    }
    return text;
}

// UTC seconds as the manifest writes a time: 2026-10-20T08:00:00Z.
std::string utcText(std::int64_t epoch)
{
    namespace chrono = std::chrono;
    const chrono::sys_seconds moment{chrono::seconds{epoch}};
    const chrono::sys_days day = chrono::floor<chrono::days>(moment);
    const chrono::year_month_day date{day};
    const chrono::hh_mm_ss<chrono::seconds> time{moment - day};
    return std::format("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", static_cast<int>(date.year()),
                       static_cast<unsigned>(date.month()), static_cast<unsigned>(date.day()),
                       static_cast<long long>(time.hours().count()),
                       static_cast<long long>(time.minutes().count()),
                       static_cast<long long>(time.seconds().count()));
}

std::int64_t nowOf(const UpdateContext& context)
{
    if (context.nowEpoch != 0) {
        return context.nowEpoch;
    }
    return std::chrono::duration_cast<std::chrono::seconds>(
               std::chrono::system_clock::now().time_since_epoch())
        .count();
}

const update::Trust& trustOf(const UpdateContext& context)
{
    return context.trust != nullptr ? *context.trust : update::productionTrust();
}

// A copy nothing is ever downloaded to or applied to: one packaging did not
// write, or one whose own record of what it is does not read.
bool isInstalledCopy(const UpdateContext& context)
{
    return context.install.kind != update::InstallKind::Development &&
           context.installProblem.empty();
}

// ---- the token is in nothing that is said ---------------------------------------------------

// Every token this line could meet: the one the feed would send, and the one
// the store holds now where that is another.
std::vector<std::string> secretsOf(const UpdateContext& context)
{
    std::vector<std::string> secrets;
    if (!context.feed.token.empty()) {
        secrets.push_back(context.feed.token);
    }
    if (context.credentials != nullptr) {
        Result<std::string> stored = context.credentials->read();
        if (stored && !stored->empty() &&
            std::find(secrets.begin(), secrets.end(), *stored) == secrets.end()) {
            secrets.push_back(std::move(*stored));
        }
    }
    return secrets;
}

std::string withoutSecrets(std::string text, const std::vector<std::string>& secrets)
{
    for (const std::string& secret : secrets) {
        text = update::redactToken(text, secret);
    }
    return text;
}

// ---- the per-user state ---------------------------------------------------------------------

struct KeptState {
    update::UpdateState state; // the defaults when none is kept, or it did not read
    bool kept = false;         // the context has a state folder
    bool readable = true;      // false: state.json is there and does not read
    std::string problem;       // why not
};

KeptState readKeptState(const UpdateContext& context)
{
    KeptState kept;
    if (context.stateDirectory.empty()) {
        return kept;
    }
    kept.kept = true;
    Result<update::UpdateState> read = update::readState(context.stateDirectory);
    if (!read) {
        kept.readable = false;
        kept.problem = read.error().describe();
        return kept;
    }
    kept.state = std::move(*read);
    return kept;
}

// What is said of a state file that does not read. It is not written over: a
// file from a newer Katana holds what this one cannot keep, and one that is
// damaged holds the releases that were rolled back here.
std::string unreadableState(const KeptState& kept)
{
    return warningText("the update settings could not be read, so the defaults were used and "
                       "nothing was written over them: " +
                       kept.problem);
}

// A setting changed in state.json: read at the moment it is written, so that
// what another program wrote meanwhile is kept but for this one member.
template <typename Change> core::Status changeState(const UpdateContext& context, Change change)
{
    KeptState kept = readKeptState(context);
    if (!kept.readable) {
        return makeError(ErrorCode::InvalidState,
                         "the update settings could not be read, and are not written over",
                         kept.problem);
    }
    change(kept.state);
    return update::writeState(context.stateDirectory, kept.state);
}

// ---- what is staged -------------------------------------------------------------------------

struct Staged {
    update::Manifest manifest;
    fs::path directory; // U/stage-<sequence>
};

// A file of at most `limit` bytes, whole; nothing for one that is not there,
// is not a file, or is longer (the stage folder is the user's to write: a
// "manifest" of a gigabyte is not read to learn that it is not one).
std::optional<std::string> smallFile(const fs::path& path, std::uint64_t limit)
{
    std::error_code error;
    if (!fs::is_regular_file(fs::symlink_status(path, error)) || error) {
        return std::nullopt;
    }
    const std::uintmax_t size = fs::file_size(path, error);
    if (error || size > limit) {
        return std::nullopt;
    }
    Result<std::string> bytes = katana::core::readFileBytes(path);
    if (!bytes) {
        return std::nullopt;
    }
    return std::move(*bytes);
}

// The sequence a folder name stage-<n> carries. The name is read as the file
// system holds it and is never converted: on Windows a name need not be
// Unicode text (an unpaired surrogate is a name there), converting such a one
// throws (core/path_text.hpp, pathToUtf8), and the update folder is the
// user's to write - one entry of that kind in it would end every STATUS and
// APPLY with an exception that nothing up the line catches.
std::optional<std::uint64_t> stageSequence(const fs::path::string_type& name)
{
    constexpr std::string_view kPrefix = "stage-";
    if (name.size() <= kPrefix.size() || name.size() > 40) {
        return std::nullopt;
    }
    std::string ascii;
    ascii.reserve(name.size());
    for (const fs::path::value_type unit : name) {
        if (unit < 0x20 || unit > 0x7E) {
            return std::nullopt; // no letter of stage-<digits> lies outside ASCII
        }
        ascii.push_back(static_cast<char>(unit));
    }
    if (!std::string_view(ascii).starts_with(kPrefix)) {
        return std::nullopt;
    }
    const std::string_view digits = std::string_view(ascii).substr(kPrefix.size());
    std::uint64_t sequence = 0;
    const auto parsed = std::from_chars(digits.data(), digits.data() + digits.size(), sequence);
    if (parsed.ec != std::errc{} || parsed.ptr != digits.data() + digits.size()) {
        return std::nullopt;
    }
    return sequence;
}

// The release that is downloaded and ready: a folder U/stage-<sequence> that
// holds the two records stage() writes last (stage.hpp), which verify with
// this program's keys, name that very sequence and are above what is
// installed. The newest when there are several. A folder without them is a
// download that did not finish; one that is not above the install is what a
// finished update left behind.
//
// A LINK to a folder is not told from a folder here, and need not be. (On
// Windows this toolchain's symlink_status answers for what a junction or a
// symbolic link points at: src/katana_update/plan.cpp, look.) Nothing is
// believed of a stage but what the signature covers, and the helper verifies
// the release again with its own keys and COPIES each file out of the stage,
// hashing the copy (update/apply.hpp, steps 1 and 3): a stage that lies
// somewhere else is still only the user's own folder.
std::optional<Staged> findStaged(const UpdateContext& context)
{
    std::optional<Staged> newest;
    if (context.stateDirectory.empty() || !isInstalledCopy(context)) {
        return newest;
    }
    std::error_code error;
    for (fs::directory_iterator at(context.stateDirectory, error), end; !error && at != end;
         at.increment(error)) {
        const fs::path folder = at->path();
        const fs::path name = folder.filename();
        const std::optional<std::uint64_t> named = stageSequence(name.native());
        if (!named.has_value()) {
            continue;
        }
        const std::uint64_t sequence = named.value_or(0);
        std::error_code ignored;
        if (!fs::is_directory(fs::symlink_status(folder, ignored)) || ignored) {
            continue; // a file of that name is not a stage
        }
        const std::optional<std::string> json =
            smallFile(update::stagedManifestPath(folder), update::kMaxManifestBytes);
        const std::optional<std::string> signature =
            smallFile(update::stagedSignaturePath(folder), update::kMaxSignatureBytes);
        if (!json || !signature) {
            continue;
        }
        Result<update::Manifest> manifest =
            update::verifyManifest(*json, *signature, trustOf(context), context.install);
        if (!manifest || manifest->sequence != sequence ||
            !update::isNewerThan(*manifest, context.install)) {
            continue;
        }
        if (!newest || manifest->sequence > newest->manifest.sequence) {
            newest = Staged{std::move(*manifest), folder};
        }
    }
    return newest;
}

// ---- UPDATE CHECK ---------------------------------------------------------------------------

struct Checked {
    UpdateOffer offer;
    std::optional<update::Plan> plan;      // when the installed files were hashed
    std::optional<core::Error> planError;  // why they could not be
    bool informationIsOld = false;         // the release's `expires` has passed
    std::vector<std::string> warnings;     // records that follow the reply's own
};

Checked unsupported(Checked checked, std::string reason, std::string detail)
{
    checked.offer.check.status = update::CheckStatus::Unsupported;
    checked.offer.check.reason = std::move(reason);
    checked.offer.check.detail = std::move(detail);
    return checked;
}

// Asks the feed, applies what the person has decided about the release it
// offers, works out what the update would fetch, and records when the feed
// answered. An error only where the question could not be put (a stop already
// asked for): a feed that cannot be read is an answer.
Result<Checked> runCheck(const UpdateContext& context)
{
    Checked checked;
    update::CheckResult& result = checked.offer.check;
    result.installedVersion = update::toString(context.install.version);

    if (!context.installProblem.empty()) {
        return unsupported(std::move(checked), "install_record", context.installProblem);
    }
    // Before anything is asked of anyone: a development copy has no target to
    // ask a feed for, and a build tree must stay offline.
    if (context.install.kind == update::InstallKind::Development) {
        return unsupported(std::move(checked), "development_tree",
                           "this copy of Katana is a development build, which is not updated "
                           "in place");
    }
    if (!context.fetch) {
        result.status = update::CheckStatus::Unreachable;
        result.reason = "no_transport";
        result.detail = "this program was given no way to reach the release server";
        return checked;
    }

    Result<update::CheckResult> answered = update::check(context.fetch, context.feed,
                                                         context.install, trustOf(context),
                                                         context.stop);
    if (!answered) {
        return answered.error();
    }
    result = std::move(*answered);
    if (result.status == update::CheckStatus::Unreachable && result.reason == kNoNetworkReason) {
        result.status = update::CheckStatus::Unsupported;
    }
    // The feed answered when a manifest verified: available or current.
    const bool answeredByFeed = result.manifest.has_value();

    // Read now, not before the request: what another program wrote while this
    // one waited for the network is in it.
    const KeptState kept = readKeptState(context);
    if (kept.kept && !kept.readable) {
        checked.warnings.push_back(unreadableState(kept));
    }
    if (kept.kept && kept.readable) {
        update::applyUpdateState(result, kept.state);
    }

    if (result.status == update::CheckStatus::Available && result.manifest.has_value()) {
        const update::Manifest& manifest = *result.manifest;
        checked.informationIsOld = update::isExpired(manifest, nowOf(context));
        if (manifest.notifyOnly) {
            // An announcement: there is nothing to hash against and nothing to fetch.
            result.reason = "notify_only";
        } else {
            Result<update::Plan> plan = update::plan(manifest, context.install, context.stop);
            if (plan) {
                checked.offer.changedFiles = plan->changedFiles;
                checked.offer.totalFiles = plan->totalFiles;
                checked.offer.downloadBytes = plan->downloadBytes;
                checked.plan = std::move(*plan);
            } else if (context.stop.stop_requested()) {
                return plan.error();
            } else {
                // The release is known all the same; what it would fetch is not.
                checked.planError = plan.error();
                checked.warnings.push_back(
                    warningText("what the update would change could not be worked out: " +
                                plan.error().describe()));
            }
        }
    }

    // Recorded through changeState, which reads the file again at the moment
    // it writes. `kept` is from before the installed files were hashed, which
    // takes seconds: written back as it was read, it would undo whatever
    // another program of this install kept meanwhile - a skip, the switch, or
    // the release the helper has just put back and blocked.
    if (answeredByFeed && kept.kept && kept.readable) {
        const std::int64_t now = nowOf(context);
        if (const core::Status written = changeState(
                context, [now](update::UpdateState& state) { state.lastCheckEpoch = now; });
            !written) {
            checked.warnings.push_back(warningText("the time of this check could not be "
                                                   "recorded: " +
                                                   written.error().describe()));
        }
    }
    return checked;
}

std::string checkRecord(const Checked& checked)
{
    const update::CheckResult& check = checked.offer.check;
    std::string record = "update=" + std::string(update::toString(check.status));
    add(record, "installed", check.installedVersion);
    add(record, "latest", check.latestVersion);
    if (checked.plan.has_value()) {
        record += " files=" + std::to_string(checked.offer.changedFiles) + "/" +
                  std::to_string(checked.offer.totalFiles) +
                  " download_bytes=" + std::to_string(checked.offer.downloadBytes);
    }
    add(record, "notes_url", check.notesUrl);
    if (checked.informationIsOld) {
        record += " information=old";
    }
    add(record, "reason", check.reason);
    add(record, "detail", check.detail);
    return record;
}

// The check's record and the warnings after it.
std::vector<std::string> checkLines(const Checked& checked)
{
    std::vector<std::string> lines{checkRecord(checked)};
    lines.insert(lines.end(), checked.warnings.begin(), checked.warnings.end());
    return lines;
}

Result<UpdateReply> check(const UpdateContext& context)
{
    Result<Checked> checked = runCheck(context);
    if (!checked) {
        return checked.error();
    }
    UpdateReply reply;
    reply.text = joined(checkLines(*checked));
    reply.offer = std::move(checked->offer);
    return reply;
}

// ---- UPDATE STATUS --------------------------------------------------------------------------

bool tokenIsPresent(const UpdateContext& context)
{
    if (!context.feed.token.empty()) {
        return true;
    }
    return context.credentials != nullptr && context.credentials->read().ok();
}

Result<UpdateReply> status(const UpdateContext& context)
{
    std::vector<std::string> warnings;
    UpdateStatus said;
    said.installedVersion = update::toString(context.install.version);
    said.target = context.install.target;
    said.kind = context.install.kind;
    if (!context.installProblem.empty()) {
        // kind= has three words and none of them is "an install whose record
        // does not read" (update_verbs.hpp): the record says what the context
        // holds, and this says that it is not what the copy is.
        warnings.push_back(warningText("this copy's install record could not be read, so what "
                                       "kind of copy it is is not known and nothing is "
                                       "downloaded or applied to it: " +
                                       context.installProblem));
    }
    const KeptState kept = readKeptState(context);
    if (kept.kept && !kept.readable) {
        warnings.push_back(unreadableState(kept));
    }
    said.state = kept.state;
    said.tokenPresent = tokenIsPresent(context);
    // What is downloaded and how the last apply ended are an install's: a
    // development copy shares the update folder with an installed one on the
    // same machine, and neither is its own.
    if (kept.kept && isInstalledCopy(context)) {
        if (const std::optional<Staged> staged = findStaged(context)) {
            said.stagedVersion = update::toString(staged->manifest.version);
        }
        Result<std::optional<update::LastResult>> last =
            update::readLastResult(context.stateDirectory);
        if (last) {
            said.last = std::move(*last);
        } else {
            warnings.push_back(warningText("how the last update ended could not be read: " +
                                           last.error().describe()));
        }
    }

    std::string record = "installed=" + recordText(said.installedVersion);
    record += " target=" + (said.target.empty() ? std::string("none") : recordText(said.target));
    record += " kind=" + std::string(update::toString(said.kind));
    record += " staged=" +
              (said.stagedVersion.empty() ? std::string("none") : recordText(said.stagedVersion));
    record += " skipped=" + (said.state.skippedVersion.empty()
                                 ? std::string("none")
                                 : recordText(said.state.skippedVersion));
    record += said.state.automatic ? " auto=on" : " auto=off";
    record += " last_check=" + (said.state.lastCheckEpoch > 0 ? utcText(said.state.lastCheckEpoch)
                                                             : std::string("never"));
    record += said.tokenPresent ? " token=present" : " token=absent";
    std::string last = "none";
    if (said.last.has_value() && !said.last->status.empty()) {
        last = said.last->status;
        if (!said.last->reason.empty()) {
            last += ":" + said.last->reason;
        }
    }
    record += " last=" + recordText(last);

    UpdateReply reply;
    std::vector<std::string> lines{std::move(record)};
    lines.insert(lines.end(), warnings.begin(), warnings.end());
    reply.text = joined(lines);
    reply.status = std::move(said);
    return reply;
}

// ---- UPDATE DOWNLOAD CONFIRM ----------------------------------------------------------------

// A failure of the engine's staging or preflight as this line's: its own
// sentence, and its reason token first in the context, where a caller reads it
// (fetchFailureReason: the transport's failures carry a status there, the
// engine's own a reason= already).
core::Error engineError(const core::Error& error)
{
    core::Error said = error;
    if (said.context.starts_with("reason=")) {
        return said;
    }
    const std::string reason = "reason=" + update::fetchFailureReason(error);
    said.context = said.context.empty() ? reason : reason + "; " + said.context;
    return said;
}

core::Error cannotInstallHere(Action action)
{
    return makeError(ErrorCode::Unsupported,
                     std::string(lineOf(action)) +
                         " is not available on this system: this version of Katana installs "
                         "an update on Windows only. UPDATE CHECK tells of a newer release, "
                         "and its notes say where to get it",
                     "reason=platform");
}

// DOWNLOAD's progress, said as lines: the first report, one each time another
// tenth of the bytes has arrived, and - the bytes all there - one more when
// the files have followed them into the stage folder, so that the last line
// says every changed file is staged. (The bytes of a request arrive before
// its files are unpacked and verified: "all the bytes, none of the files" is
// a state a line can truthfully show.)
class ProgressLines {
  public:
    ProgressLines(const UpdateContext& context, const update::Manifest& manifest,
                  const update::Plan& plan, fs::path stageDirectory,
                  const std::vector<std::string>& secrets)
        : context_(context), manifest_(manifest), plan_(plan),
          stageDirectory_(std::move(stageDirectory)), secrets_(secrets)
    {
    }

    void report(std::uint64_t done, std::uint64_t total)
    {
        if (context_.progress) {
            context_.progress(done, total);
        }
        // Nothing to fetch is all of it fetched.
        const std::uint64_t step =
            total == 0 ? kProgressSteps : std::min(done, total) * kProgressSteps / total;
        const bool complete = done >= total;
        // The transport reports many times a second, and the files are
        // counted on the disk: they are counted only for a line that is about
        // to be said, or once the bytes are all there.
        if (said_ && step <= step_ && !complete) {
            return;
        }
        const std::uint64_t files = stagedFiles();
        if (said_ && step <= step_ && files == files_) {
            return;
        }
        said_ = true;
        step_ = std::max(step_, step);
        files_ = files;
        std::string line = "progress bytes=" + std::to_string(std::min(done, total)) + "/" +
                           std::to_string(total) + " files=" + std::to_string(files) + "/" +
                           std::to_string(plan_.changedFiles);
        line = withoutSecrets(std::move(line), secrets_);
        if (context_.progressLine) {
            context_.progressLine(line);
        }
        lines_.push_back(std::move(line));
    }

    [[nodiscard]] std::vector<std::string> take() { return std::move(lines_); }

  private:
    // The changed files that are in the stage folder: a file is given its name
    // there only once it has hashed to the signed value (stage.hpp), so being
    // there is being done.
    [[nodiscard]] std::uint64_t stagedFiles() const
    {
        std::uint64_t count = 0;
        for (const update::PlannedFile& planned : plan_.files) {
            if (!planned.changed || planned.index >= manifest_.files.size()) {
                continue;
            }
            std::error_code error;
            const fs::path file =
                stageDirectory_ / katana::core::pathFromUtf8(manifest_.files[planned.index].path);
            if (fs::is_regular_file(fs::symlink_status(file, error)) && !error) {
                ++count;
            }
        }
        return count;
    }

    const UpdateContext& context_;
    const update::Manifest& manifest_;
    const update::Plan& plan_;
    fs::path stageDirectory_;
    const std::vector<std::string>& secrets_;
    std::vector<std::string> lines_;
    bool said_ = false;
    std::uint64_t step_ = 0;  // the tenth the last line was said at
    std::uint64_t files_ = 0; // the files it counted
};

Result<UpdateReply> download(const UpdateContext& context, const std::vector<std::string>& secrets)
{
    if (!kInstallsHere) {
        return cannotInstallHere(Action::Download);
    }
    UpdateReply reply;
    if (context.stateDirectory.empty()) {
        // Nowhere to put a download, and nothing was asked of anyone.
        reply.text = "staged=none reason=no_state";
        return reply;
    }
    Result<Checked> checked = runCheck(context);
    if (!checked) {
        return checked.error();
    }
    reply.offer = checked->offer;
    const update::CheckResult& answer = checked->offer.check;
    if (answer.status != update::CheckStatus::Available || !answer.manifest.has_value() ||
        answer.manifest->notifyOnly) {
        // Nothing to download, and the check's own record says why: current,
        // skipped, blocked, unreachable, unsupported, or a release that is
        // only an announcement.
        std::vector<std::string> lines = checkLines(*checked);
        lines.emplace_back("staged=none");
        reply.text = joined(lines);
        return reply;
    }
    if (!checked->plan.has_value()) {
        core::Error error = checked->planError.value_or(
            makeError(ErrorCode::Internal, "the installed files were not compared"));
        error.message = "what the update would change could not be worked out, so nothing was "
                        "downloaded: " +
                        error.message;
        error.context = error.context.empty() ? std::string("reason=plan_failed")
                                              : "reason=plan_failed; " + error.context;
        return error;
    }

    const update::Manifest& manifest = *answer.manifest;
    const update::Plan& plan = *checked->plan;
    // The payload is fetched the way the manifest was (feed.hpp).
    update::FeedConfig feed = context.feed;
    feed.route = answer.route;
    const fs::path stageDirectory = update::stagePath(context.stateDirectory, manifest.sequence);
    ProgressLines progress(context, manifest, plan, stageDirectory, secrets);
    const core::Status staged = update::stage(
        context.fetch, feed, manifest, plan, stageDirectory, context.stop,
        [&progress](std::uint64_t done, std::uint64_t total) { progress.report(done, total); });
    if (!staged) {
        return engineError(staged.error());
    }
    std::vector<std::string> lines = progress.take();
    lines.push_back("staged=" + recordText(update::toString(manifest.version)));
    lines.insert(lines.end(), checked->warnings.begin(), checked->warnings.end());
    reply.text = joined(lines);
    return reply;
}

// ---- UPDATE APPLY CONFIRM -------------------------------------------------------------------

Result<UpdateReply> apply(const UpdateContext& context)
{
    if (!kInstallsHere) {
        return cannotInstallHere(Action::Apply);
    }
    if (!context.installProblem.empty()) {
        return makeError(ErrorCode::Unsupported,
                         "this copy's install record could not be read, so nothing is applied "
                         "to it: " +
                             context.installProblem,
                         "reason=install_record");
    }
    if (context.install.kind == update::InstallKind::Development) {
        return makeError(ErrorCode::Unsupported,
                         "this copy of Katana is a development build, which is never updated "
                         "in place",
                         "reason=development_tree");
    }
    if (context.stateDirectory.empty()) {
        return makeError(ErrorCode::InvalidState,
                         "no update is downloaded: this session keeps no update folder to "
                         "download one into",
                         "reason=no_state");
    }
    const std::optional<Staged> staged = findStaged(context);
    if (!staged) {
        return makeError(ErrorCode::InvalidState,
                         "no update is downloaded and ready: UPDATE DOWNLOAD CONFIRM downloads "
                         "the newest release",
                         "reason=not_staged");
    }
    // A release whose apply was rolled back here is not tried again: the
    // helper found that it does not start on this machine (apply.hpp, step 7).
    // Which releases those are is in the state file, so one that does not
    // read stops the line: its defaults say that none was ever put back,
    // and the file that is damaged, or was written by a newer Katana, is the
    // very one that holds them. The way out is said too, since nothing here
    // takes it for the person: the file is theirs, and is not written over.
    const KeptState kept = readKeptState(context);
    if (!kept.readable) {
        return makeError(ErrorCode::InvalidState,
                         "the update settings could not be read, so whether Katana " +
                             update::toString(staged->manifest.version) +
                             " was put back here before is not known, and it is not applied (" +
                             kept.problem +
                             "); with that file removed the settings are the defaults again and "
                             "the update can be applied",
                         "reason=state_unreadable");
    }
    if (std::find(kept.state.blockedSequences.begin(), kept.state.blockedSequences.end(),
                  staged->manifest.sequence) != kept.state.blockedSequences.end()) {
        return makeError(ErrorCode::InvalidState,
                         "Katana " + update::toString(staged->manifest.version) +
                             " could not be installed here before and was put back; it is not "
                             "applied again, and a later release will be offered",
                         "reason=blocked_before");
    }
    // Started "as administrator": the helper would run with those rights from
    // its copy in the update folder, which the same person's ordinary programs
    // can write (update/process.hpp, processIsElevated). The last refusal, and
    // before the preflight: no copy is made for a helper that will not be
    // started, and the release stays where it was downloaded to.
    if (context.elevated) {
        return makeError(ErrorCode::Unsupported,
                         "this Katana was started as administrator, and an update is not "
                         "installed from there: the update helper would run with administrator "
                         "rights from a folder that ordinary programs can write. Start Katana "
                         "the ordinary way and update again; Katana " +
                             update::toString(staged->manifest.version) +
                             " stays downloaded and is not fetched a second time",
                         "reason=elevated");
    }

    // The helper is copied out of the install and proves it starts now, while
    // this session is still here to say that it does not.
    std::unique_ptr<update::Launcher> system;
    update::Launcher* launcher = context.launcher;
    if (launcher == nullptr) {
        system = update::makeSystemLauncher();
        launcher = system.get();
    }
    Result<update::PreparedApply> prepared = update::preflight(
        staged->manifest, context.install, staged->directory, context.stateDirectory, *launcher);
    if (!prepared) {
        return engineError(prepared.error());
    }

    update::ApplyRequest request;
    request.root = context.install.root;
    request.stageDirectory = staged->directory;
    request.stateDirectory = context.stateDirectory;
    request.helper = prepared->helper;
    request.transaction = prepared->transaction;
    request.waitPid = context.pid;
    request.relaunch = context.relaunch;

    UpdateReply reply;
    reply.text = "ready=" + recordText(update::toString(staged->manifest.version)) +
                 " transaction=" + recordText(request.transaction);
    reply.apply = std::move(request);
    return reply;
}

// ---- UPDATE SKIP, UPDATE SET AUTO -----------------------------------------------------------

Result<UpdateReply> skip(const UpdateContext& context, const UpdateLine& line)
{
    UpdateReply reply;
    if (context.stateDirectory.empty()) {
        reply.text = "skipped=none reason=no_state";
        return reply;
    }
    if (const core::Status changed = changeState(
            context, [&line](update::UpdateState& state) { state.skippedVersion = line.version; });
        !changed) {
        return changed.error();
    }
    reply.text = "skipped=" + (line.version.empty() ? std::string("none") : line.version);
    return reply;
}

Result<UpdateReply> setAutomatic(const UpdateContext& context, const UpdateLine& line)
{
    UpdateReply reply;
    if (context.stateDirectory.empty()) {
        // What an unkept setting is: the default, which is on (state.hpp).
        reply.text = std::string(update::UpdateState{}.automatic ? "auto=on" : "auto=off") +
                     " reason=no_state";
        return reply;
    }
    if (const core::Status changed = changeState(
            context, [&line](update::UpdateState& state) { state.automatic = line.on; });
        !changed) {
        return changed.error();
    }
    reply.text = line.on ? "auto=on" : "auto=off";
    return reply;
}

// ---- UPDATE TOKEN STATUS | CLEAR ------------------------------------------------------------

// Whether a token is stored: present or absent, and never the token. When the
// store cannot say - what it holds is no usable token, or it cannot be asked -
// its own sentence follows, which names the place and not the value
// (credential.hpp).
std::string tokenRecord(const update::CredentialStore& store)
{
    const Result<std::string> stored = store.read();
    if (stored) {
        return "token=present";
    }
    if (stored.error().code == ErrorCode::NotFound) {
        return "token=absent";
    }
    std::string record = "token=absent";
    add(record, "reason",
        stored.error().code == ErrorCode::InvalidState ? "token_unusable" : "store_unavailable");
    add(record, "detail", stored.error().message);
    return record;
}

std::string noStoreRecord(const UpdateContext& context)
{
    return std::string(context.feed.token.empty() ? "token=absent" : "token=present") +
           " reason=no_store";
}

Result<UpdateReply> tokenStatus(const UpdateContext& context)
{
    UpdateReply reply;
    reply.text = context.credentials == nullptr ? noStoreRecord(context)
                                                : tokenRecord(*context.credentials);
    return reply;
}

Result<UpdateReply> tokenClear(const UpdateContext& context)
{
    UpdateReply reply;
    if (context.credentials == nullptr) {
        reply.text = noStoreRecord(context);
        return reply;
    }
    update::CredentialStore& store = *context.credentials;
    const core::Status cleared = store.clear();
    // A store that cannot be written (the environment's) has nothing of this
    // program's to remove: what it still gives is said below. Any other
    // failure is the store's, and nothing can be said of what it holds.
    if (!cleared && cleared.error().code != ErrorCode::Unsupported) {
        return cleared.error();
    }
    reply.text = tokenRecord(store);
    if (reply.text == "token=present") {
        const bool fromEnvironment = store.where() == "environment";
        add(reply.text, "reason", fromEnvironment ? "environment" : "store_unavailable");
        add(reply.text, "detail",
            fromEnvironment ? "the environment variable " +
                                  std::string(update::kTokenEnvironmentVariable) +
                                  " still gives one, which Katana does not change: remove it "
                                  "from the environment that starts Katana"
                            : (cleared ? std::string("the store still holds one")
                                       : cleared.error().message));
    }
    return reply;
}

// ---- what a surface may not run -------------------------------------------------------------

// A reply for the two lines a headless window answers rather than refuses
// (update_verbs.hpp): the record's own key saying nothing was done, the
// reason, and who does run the line.
UpdateReply headlessReply(Action action)
{
    UpdateReply reply;
    reply.text = action == Action::Download ? "staged=none" : "ready=none";
    add(reply.text, "reason", "headless");
    add(reply.text, "text",
        std::string("a headless run ") +
            (action == Action::Download ? "downloads no update" : "installs no update") + ": " +
            kWhoRunsEveryLine + " does");
    return reply;
}

// Nothing when `surface` may run the line; a reply or a refusal when it may not.
std::optional<Result<UpdateReply>> refusedBySurface(UpdateSurface surface, Action action)
{
    if (action == Action::Check || action == Action::Status) {
        return std::nullopt;
    }
    switch (surface) {
    case UpdateSurface::Cli:
    case UpdateSurface::Window:
        return std::nullopt;
    case UpdateSurface::HeadlessWindow:
        if (action == Action::Download || action == Action::Apply) {
            return Result<UpdateReply>(headlessReply(action));
        }
        return Result<UpdateReply>(makeError(
            ErrorCode::Unsupported,
            std::string(lineOf(action)) +
                " reads or changes what Katana keeps for one user, and a headless run keeps "
                "nothing per user: " +
                kWhoRunsEveryLine + " runs it",
            "reason=headless"));
    case UpdateSurface::Mcp:
        return Result<UpdateReply>(makeError(
            ErrorCode::Unsupported,
            std::string(lineOf(action)) +
                " is not katana_mcp's to run: an agent checks for an update and reads this "
                "copy's status (katana_check_update) and no more, so that it never replaces "
                "the program its client holds open; " +
                kWhoRunsEveryLine + " runs it",
            "reason=mcp"));
    }
    return std::nullopt;
}

Result<UpdateReply> run(const UpdateContext& context, std::string_view line,
                        const std::vector<std::string>& secrets)
{
    const Result<UpdateLine> parsed = parse(line);
    if (!parsed) {
        return parsed.error();
    }
    if (std::optional<Result<UpdateReply>> refused =
            refusedBySurface(context.surface, parsed->action)) {
        return std::move(*refused);
    }
    switch (parsed->action) {
    case Action::Check:
        return check(context);
    case Action::Status:
        return status(context);
    case Action::Download:
        return download(context, secrets);
    case Action::Apply:
        return apply(context);
    case Action::Skip:
        return skip(context, *parsed);
    case Action::SetAuto:
        return setAutomatic(context, *parsed);
    case Action::TokenStatus:
        return tokenStatus(context);
    case Action::TokenClear:
        return tokenClear(context);
    }
    return makeError(ErrorCode::Internal, "an UPDATE action nobody runs");
}

// ---- the running program's context ----------------------------------------------------------

// The token of a program a person runs: the environment variable first - it
// is an override everywhere (update/credential.hpp) - and then the system's
// store, where the window's Settings page keeps one. Written and cleared in
// the system's store alone: a variable is not this program's to change.
class ProgramCredentials final : public update::CredentialStore {
  public:
    ProgramCredentials()
        : environment_(update::makeEnvironmentCredentialStore()),
          system_(update::makeSystemCredentialStore())
    {
    }

    [[nodiscard]] Result<std::string> read() const override
    {
        Result<std::string> fromEnvironment = environment_->read();
        if (fromEnvironment) {
            fromEnvironment_.store(true);
            return fromEnvironment;
        }
        // Whatever the variable's fault, the store is asked: a variable that
        // holds something unusable must not hide a good token kept there.
        Result<std::string> fromSystem = system_->read();
        if (fromSystem) {
            fromEnvironment_.store(false);
            return fromSystem;
        }
        // Neither gives one. "There is none" only when the variable says so
        // too: one that is set and unusable is said, so that it is corrected.
        if (fromEnvironment.error().code != ErrorCode::NotFound) {
            return fromEnvironment.error();
        }
        return fromSystem.error();
    }

    [[nodiscard]] core::Status write(std::string_view token) override
    {
        core::Status written = system_->write(token);
        if (written) {
            fromEnvironment_.store(false);
        }
        return written;
    }

    [[nodiscard]] core::Status clear() override { return system_->clear(); }

    // The place the last token read or written is in.
    [[nodiscard]] std::string_view where() const override
    {
        return fromEnvironment_.load() ? environment_->where() : system_->where();
    }

  private:
    std::unique_ptr<update::CredentialStore> environment_;
    std::unique_ptr<update::CredentialStore> system_;
    mutable std::atomic<bool> fromEnvironment_{false};
};

// What a program is, `root` being the install it was found in: that install's
// own records, or - with no install to speak of - the program as it was
// built, a development copy.
void readInstallOf(UpdateContext& context, const Result<fs::path>& root,
                   const update::RegistryOps& registry)
{
    if (const Result<update::Version> built = update::programVersion()) {
        context.install.version = *built;
    }
    if (!root) {
        return; // not in a bin/ beside a share/: a test's program, a tool's
    }
    Result<update::Install> install = update::readInstall(*root, registry);
    if (!install) {
        context.install.root = *root;
        context.installProblem = install.error().describe();
        return;
    }
    context.install = std::move(*install);
}

// The context of the program found in `root` (update_verbs.hpp,
// makeUpdateContext): the running one's, or the one a caller named.
UpdateContext contextOf(UpdateSurface surface, const Result<fs::path>& root)
{
    // As long as the program: a context holds pointers to them, and a window
    // hands a copy of its context to a job's thread.
    static const std::unique_ptr<update::RegistryOps> registry = update::makeSystemRegistryOps();
    static const std::unique_ptr<update::CredentialStore> environment =
        update::makeEnvironmentCredentialStore();
    static ProgramCredentials program;

    // A headless window and katana_mcp keep and read nothing a PERSON chose:
    // no state folder, and no store but the variable their own run was given.
    // What the copy IS is read for every surface, and on Windows one part of
    // that is per user as well: the folder the per-user installer recorded
    // (HKCU, update/registry.hpp), which is what tells kind=installer from
    // kind=archive. It is the install's own record, read and never written.
    const bool perUser = surface == UpdateSurface::Cli || surface == UpdateSurface::Window;

    UpdateContext context;
    context.surface = surface;
    readInstallOf(context, root, *registry);
    context.feed = update::defaultFeedConfig(context.install.target);
    context.credentials = perUser ? static_cast<update::CredentialStore*>(&program)
                                  : environment.get();
    if (Result<std::string> token = context.credentials->read()) {
        context.feed.token = std::move(*token);
    }
    // In a build without an HTTP stack this is a transport that answers every
    // request with no_network (update_transport.hpp), which CHECK reports as
    // update=unsupported: there is no second way to say so here.
    context.fetch = makeUpdateFetch();
    if (perUser) {
        context.stateDirectory = update::stateDirectory();
    }
    // How the process was started is nothing a person keeps: asked for every
    // surface, of the system, and never of a file.
    context.elevated = update::processIsElevated();
    return context;
}

} // namespace

bool isUpdateLine(std::string_view line)
{
    const std::vector<std::string_view> words = wordsOf(line);
    return !words.empty() && sameWord(words.front(), "UPDATE");
}

bool isUpdateApplyLine(std::string_view line)
{
    // The words as parse() takes them, so that the line a front end holds to
    // be the last one is exactly the line that would prepare an apply.
    const std::vector<std::string_view> words = wordsOf(line);
    return words.size() >= 2 && sameWord(words[0], "UPDATE") && sameWord(words[1], "APPLY");
}

Result<UpdateReply> runUpdateLine(const UpdateContext& context, std::string_view line)
{
    // Read before the line runs: the token a TOKEN CLEAR removes is still one
    // that nothing may show.
    const std::vector<std::string> secrets = secretsOf(context);
    Result<UpdateReply> reply = run(context, line, secrets);
    if (secrets.empty()) {
        return reply;
    }
    if (!reply) {
        core::Error error = reply.error();
        error.message = withoutSecrets(std::move(error.message), secrets);
        error.context = withoutSecrets(std::move(error.context), secrets);
        return error;
    }
    reply->text = withoutSecrets(std::move(reply->text), secrets);
    if (reply->offer.has_value()) {
        // What a window shows of a check is these two, not the text.
        update::CheckResult& shown = reply->offer->check;
        shown.reason = withoutSecrets(std::move(shown.reason), secrets);
        shown.detail = withoutSecrets(std::move(shown.detail), secrets);
    }
    return reply;
}

UpdateContext makeUpdateContext(UpdateSurface surface)
{
    return contextOf(surface, update::installRootOfRunningProgram());
}

UpdateContext makeUpdateContext(UpdateSurface surface, const std::filesystem::path& executable)
{
    return contextOf(surface, update::installRootOfExecutable(executable));
}

core::Status abandonApply(const update::ApplyRequest& request)
{
    if (request.stateDirectory.empty() || !update::isValidTransactionId(request.transaction)) {
        return {}; // no request the preflight made: there is no folder of its to list
    }
    const std::unique_ptr<update::FsOps> files = update::makeSystemFsOps();
    return update::appendCleanup(request.stateDirectory,
                                 update::runPath(request.stateDirectory, request.transaction),
                                 *files);
}

const char* updateHelpText()
{
    return "Update    UPDATE CHECK  whether a newer Katana has been released:\n"
           "          update=available|current|unreachable|unsupported|blocked, with\n"
           "          installed=, latest=, files=<changed>/<total>, download_bytes=,\n"
           "          notes_url= and reason=\n"
           "          UPDATE STATUS  this copy: installed=, target=,\n"
           "          kind=installer|archive|development, staged=, skipped=, auto=,\n"
           "          last_check=, token=present|absent and last= (how the last update\n"
           "          ended)\n"
           "          UPDATE DOWNLOAD CONFIRM  downloads the files of the newest release that\n"
           "          differ from the installed ones into the per-user update folder:\n"
           "          progress lines, then staged=<version>. The install is not changed\n"
           "          UPDATE APPLY CONFIRM  installs the downloaded release: ready=<version>\n"
           "          transaction=<id>, the session ends, and the update helper replaces\n"
           "          the files, putting every one back if the new version does not\n"
           "          start. It must be the last line of a katana_cli run, which starts\n"
           "          nothing again; the window closes and reopens. A Katana that was\n"
           "          started as administrator installs no update (reason=elevated):\n"
           "          started the ordinary way it does, and the download is kept\n"
           "          UPDATE SKIP <version> | UPDATE SKIP NONE  do not offer that version;\n"
           "          offer it again\n"
           "          UPDATE SET AUTO ON|OFF  whether the window checks by itself, once a day\n"
           "          UPDATE TOKEN STATUS | UPDATE TOKEN CLEAR  whether an access token for a\n"
           "          private release repository is stored (present or absent, never the\n"
           "          token); remove the stored one. No line takes a token, because lines\n"
           "          are logged: the window's Settings > Updates stores one, and the\n"
           "          environment variable KATANA_UPDATE_TOKEN gives a run one\n"
           "          A line without its CONFIRM is refused, saying what the line with it\n"
           "          does. A copy that was not installed from a release (a build tree) is\n"
           "          kind=development and is never updated. A headless window checks and\n"
           "          reports only when asked, and katana_mcp runs no UPDATE line: its tool\n"
           "          katana_check_update checks and reports, and nothing more\n";
}

} // namespace katana::app
