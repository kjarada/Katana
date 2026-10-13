// The swap of include/katana/update/apply.hpp: lock and verify, wait, plan, journal, commit
// by renames, rollback decided from the disk, health. The algorithm is the header's; this file
// is the order it runs in and the reason behind each choice that the header does not state.
//
// Three rules shape every line:
//   1. EVERY change to the disk goes through FsOps, so the fault loops see it. Nothing here
//      uses std::filesystem to write, rename or delete. (Reads that only look - hashing a file
//      - may go to the disk directly: FaultFs counts changes, not reads.)
//   2. NOTHING catches a SimulatedCrash and nothing does disk work in a destructor, so a
//      crash injected between two operations leaves exactly what a dead process would.
//   3. ROLLBACK DOES NOT REPLAY THE JOURNAL. It looks at the disk: where old/p exists, p goes
//      back; where the transaction put a new file in place, it is moved back out. So it is
//      the same code after a crash at any point, including inside itself.

#include "katana/update/apply.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <charconv>
#include <cstdint>
#include <functional>
#include <map>
#include <memory>
#include <optional>
#include <random>
#include <set>
#include <span>
#include <string_view>
#include <thread>
#include <utility>

#include "long_path.hpp"
#include "recover_internal.hpp"
#include "remove_tree.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/sha256.hpp"
#include "katana/update/install.hpp"
#include "katana/update/journal.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/paths.hpp"
#include "katana/update/stage.hpp"
#include "katana/update/state.hpp"
#include "katana/update/version.hpp"

namespace katana::update {

namespace {

using Path = std::filesystem::path;
using Clock = std::chrono::steady_clock;

constexpr std::string_view kJournalName = "journal";
constexpr std::string_view kNewName = "new";
constexpr std::string_view kOldName = "old";
constexpr std::string_view kLockName = "lock";
constexpr std::string_view kRecoveryLogName = "recovery.log";
constexpr std::string_view kHelperStem = "katana_updater";
constexpr std::string_view kProgramStem = "katana";
// Room for the journal, the three records and the directory entries, on top of the bytes
// the changed files take under new/ (old/ is a rename and takes none).
constexpr std::uint64_t kSpaceMargin = 1024 * 1024;
// A lock that is held is looked at again for this long before it is believed. A virus scanner
// or an indexer that has the lock file open for a moment fails the share-0 open exactly as
// another helper holding it does, and a helper that gave up at once would close the window and
// start nothing: the person would see Katana vanish.
constexpr std::chrono::milliseconds kLockGrace{2000};

// ---- text and paths ---------------------------------------------------------------------

// "bin/<stem>" with ".exe" on Windows: the manifest path of an installed program.
std::string installedProgramRelative(std::string_view stem)
{
    std::string path = "bin/" + std::string(stem);
#ifdef _WIN32
    path += ".exe";
#endif
    return path;
}

// A manifest path (always '/') joined under a base.
Path under(const Path& base, std::string_view relative)
{
    return base / core::pathFromUtf8(relative);
}

// Windows and macOS file systems fold case, so "is it under the root" must too. Only ASCII
// is folded: every name the update system writes is ASCII (paths.hpp).
Path folded(const Path& path)
{
#ifdef _WIN32
    std::wstring native = path.native();
    for (wchar_t& c : native) {
        if (c >= L'A' && c <= L'Z') {
            c = static_cast<wchar_t>(c - L'A' + L'a');
        }
    }
    return Path(native);
#else
    return path;
#endif
}

bool isUnderOrEqual(const Path& base, const Path& path)
{
    const Path relative = folded(path).lexically_relative(folded(base));
    return !relative.empty() && *relative.begin() != "..";
}

// The SHA-256 of a file, named the way the file system names it (long paths: long_path.hpp).
// Hashing goes to the disk directly, not through FsOps: it only looks, and a file can be large.
core::Result<std::string> hashFile(const Path& path)
{
    return core::sha256FileHex(detail::openable(path));
}

std::string jsonString(std::string_view text)
{
    std::string out = "\"";
    for (const char c : text) {
        switch (c) {
        case '"':
            out += "\\\"";
            break;
        case '\\':
            out += "\\\\";
            break;
        case '\n':
            out += "\\n";
            break;
        case '\r':
            out += "\\r";
            break;
        case '\t':
            out += "\\t";
            break;
        default:
            if (static_cast<unsigned char>(c) < 0x20) {
                constexpr std::string_view hex = "0123456789abcdef";
                out += "\\u00";
                out += hex[(static_cast<unsigned char>(c) >> 4) & 0x0F];
                out += hex[static_cast<unsigned char>(c) & 0x0F];
            } else {
                out += c;
            }
        }
    }
    out += '"';
    return out;
}

// installed.json: what the helper records at commit (install.hpp). Keys sorted, one LF.
std::string installedRecord(const Manifest& manifest)
{
    std::string out = "{\"product\":" + jsonString(manifest.product);
    out += ",\"schema\":1,\"sequence\":" + std::to_string(manifest.sequence);
    out += ",\"tag\":" + jsonString(manifest.tag);
    out += ",\"target\":" + jsonString(manifest.target);
    out += ",\"version\":" + jsonString(toString(manifest.version)) + "}\n";
    return out;
}

std::string holderText(const std::vector<RunningProgram>& holders)
{
    std::string text;
    for (const RunningProgram& holder : holders) {
        if (!text.empty()) {
            text += ", ";
        }
        std::string name = core::pathToUtf8(holder.image.filename());
        if (name.empty()) {
            name = "Katana";
        }
        text += name + " (" + std::to_string(holder.pid) + ")";
    }
    return text;
}

// The shape every "this failed" has before it becomes an ApplyResult.
struct Failure {
    ApplyOutcome outcome = ApplyOutcome::Refused;
    std::string reason;
    std::string detail;
    std::vector<RunningProgram> holders;
    // A refusal the other helper owns (the lock is held): no last.json, no relaunch.
    bool silent = false;
};

Failure refuse(std::string reason, std::string detail)
{
    Failure failure;
    failure.reason = std::move(reason);
    failure.detail = std::move(detail);
    return failure;
}

Failure refuse(std::string reason, const std::string& what, const core::Error& error)
{
    return refuse(std::move(reason), what + ": " + error.describe());
}

// Programs of the install are running and the update gives way to them: the Blocked outcome,
// the same whichever of step 2's two looks found them.
Failure blockedBy(std::vector<RunningProgram> holders)
{
    Failure blocked;
    blocked.outcome = ApplyOutcome::Blocked;
    blocked.reason = "blocked";
    blocked.detail = "Close these first: " + holderText(holders) + ". Then press Retry.";
    blocked.holders = std::move(holders);
    return blocked;
}

// ---- keeping every write inside the install ---------------------------------------------

// "Every parent must not be a reparse point and must resolve under R": a folder that is a
// junction or a symbolic link could send a write outside the install. The answer for a folder
// is remembered, so the hundreds of files of an install cost its few dozen folders.
class PathGuard {
  public:
    PathGuard(const FsOps& fs, Path root, Path resolvedRoot)
        : fs_(fs), root_(std::move(root)), resolvedRoot_(std::move(resolvedRoot))
    {
    }

    // The folders from the root down to `directory` that exist are real folders under the
    // root. Folders that do not exist yet are fine (they will be made by us, below one that
    // was checked).
    [[nodiscard]] std::optional<Failure> check(const Path& directory)
    {
        std::vector<Path> chain;
        for (Path walk = directory; !walk.empty() && walk != root_; walk = walk.parent_path()) {
            if (verified_.contains(walk.native())) {
                break;
            }
            if (!isUnderOrEqual(root_, walk)) {
                return refuse("reparse_point", "a path leaves the install folder: " +
                                                   core::pathToUtf8(walk));
            }
            chain.push_back(walk);
            if (walk == walk.parent_path()) {
                break;
            }
        }
        for (auto folder = chain.rbegin(); folder != chain.rend(); ++folder) {
            const core::Result<EntryInfo> info = fs_.stat(*folder);
            if (!info.ok()) {
                return refuse("io_error", "a folder of the install cannot be examined",
                              info.error());
            }
            if (info->kind == EntryKind::Missing) {
                return std::nullopt; // below this nothing exists yet
            }
            if (info->reparsePoint) {
                return refuse("reparse_point", "a folder of the install is a link: " +
                                                   core::pathToUtf8(*folder));
            }
            if (info->kind != EntryKind::Directory) {
                return refuse("path_conflict", "a file is where a folder is needed: " +
                                                   core::pathToUtf8(*folder));
            }
            const core::Result<Path> resolved = fs_.resolve(*folder);
            if (!resolved.ok()) {
                return refuse("io_error", "a folder of the install cannot be resolved",
                              resolved.error());
            }
            if (!isUnderOrEqual(resolvedRoot_, resolved.value())) {
                return refuse("reparse_point", "a folder of the install resolves outside it: " +
                                                   core::pathToUtf8(*folder));
            }
            verified_.insert(folder->native());
        }
        return std::nullopt;
    }

  private:
    const FsOps& fs_;
    Path root_;
    Path resolvedRoot_;
    std::set<Path::string_type> verified_;
};

// ---- the plan a journal carries -------------------------------------------------------------

enum class Action { Add, Replace, Remove };

struct PlanEntry {
    Action action = Action::Add;
    std::string path;
};

struct JournalPlan {
    std::vector<PlanEntry> entries;       // in the order the journal lists them (commit order)
    std::vector<std::string> directories; // created, shallowest first
    std::vector<std::string> refused;     // "<op> <path>: why", for the recovery log
};

// True when a '/'-separated path has "." or ".." as a whole component - the only forms of dot
// a path may not contain. "a..b.dat" is a file name like any other, and a rule that refused it
// would leave the old file of a release that ships one unrestored.
bool hasDotComponent(std::string_view path)
{
    std::size_t start = 0;
    while (start <= path.size()) {
        std::size_t end = path.find('/', start);
        if (end == std::string_view::npos) {
            end = path.size();
        }
        const std::string_view part = path.substr(start, end - start);
        if (part == "." || part == "..") {
            return true;
        }
        start = end + 1;
    }
    return false;
}

// What a path read back from a journal, or found under old/, must pass before anything is done
// with it: checkJournalPath (checkJournalDirectory for the folder an MKDIR record names: a
// release that first brings etc or lib makes that folder bare) AND a second look that stays
// inside the root when joined, which does not depend on the first being right.
core::Status acceptablePath(const std::string& path, bool directory)
{
    const core::Status allowed = directory ? checkJournalDirectory(path) : checkJournalPath(path);
    if (!allowed.ok()) {
        return allowed;
    }
    const Path parsed = core::pathFromUtf8(path);
    const bool contained = !path.empty() && !hasDotComponent(path) &&
                           path.find('\\') == std::string::npos &&
                           path.find(':') == std::string::npos && !parsed.is_absolute() &&
                           !parsed.has_root_name() &&
                           isUnderOrEqual(Path("root"), under(Path("root"), path));
    if (!contained) {
        return core::makeError(core::ErrorCode::InvalidArgument, "leaves the install", path);
    }
    return {};
}

// Reads the PLAN group of a journal. The journal is not authenticated (journal.hpp), so a
// path in it is acted on only if acceptablePath says so.
JournalPlan readPlan(const ParsedJournal& journal)
{
    JournalPlan plan;
    for (const JournalRecord& record : journal.records) {
        std::optional<Action> action;
        bool directory = false;
        switch (record.op) {
        case JournalOp::Add:
            action = Action::Add;
            break;
        case JournalOp::Replace:
            action = Action::Replace;
            break;
        case JournalOp::Remove:
            action = Action::Remove;
            break;
        case JournalOp::MakeDirectory:
            directory = true;
            break;
        default:
            continue;
        }
        if (record.args.size() != 1) {
            plan.refused.push_back(std::string(toString(record.op)) + ": not one path");
            continue;
        }
        const std::string& path = record.args.front();
        const core::Status allowed = acceptablePath(path, directory);
        if (!allowed.ok()) {
            plan.refused.push_back(std::string(toString(record.op)) + " " + path + ": " +
                                   allowed.error().describe());
            continue;
        }
        if (directory) {
            plan.directories.push_back(path);
        } else {
            plan.entries.push_back({*action, path});
        }
    }
    return plan;
}

// Every file under `folder`, as the '/'-joined path below it (`prefix` is what comes before).
// Nothing is followed or entered that is a link; what is not an ordinary file or folder, and
// what is too deep to be an install's, is named in `skipped`. A folder that is not there holds
// nothing; one that cannot be read is `unreadable`, never "empty".
void collectFiles(const FsOps& fs, const Path& folder, const std::string& prefix, int depth,
                  std::vector<std::string>& files, std::vector<std::string>& skipped,
                  std::optional<core::Error>& unreadable)
{
    constexpr int kMaxDepth = 32;
    const core::Result<EntryInfo> self = fs.stat(folder);
    if (!self.ok()) {
        unreadable = self.error();
        return;
    }
    if (self->kind == EntryKind::Missing) {
        return;
    }
    if (self->kind != EntryKind::Directory || self->reparsePoint) {
        skipped.push_back(prefix.empty() ? std::string("(the folder itself)") : prefix);
        return;
    }
    const core::Result<std::vector<Path>> children = fs.listDirectory(folder);
    if (!children.ok()) {
        unreadable = children.error();
        return;
    }
    for (const Path& child : children.value()) {
        const std::optional<std::string> text = detail::nameAsText(child);
        if (!text) {
            // A name that is not valid text (an unpaired surrogate) is no name an update writes.
            skipped.push_back((prefix.empty() ? std::string() : prefix + "/") +
                              "(a name that is not text)");
            continue;
        }
        const std::string& name = *text;
        const std::string relative = prefix.empty() ? name : prefix + "/" + name;
        const core::Result<EntryInfo> info = fs.stat(child);
        if (!info.ok()) {
            unreadable = info.error();
            continue;
        }
        const bool ordinary = info->kind == EntryKind::File || info->kind == EntryKind::Directory;
        if (info->reparsePoint || !ordinary) {
            skipped.push_back(relative);
        } else if (info->kind == EntryKind::File) {
            files.push_back(relative);
        } else if (depth + 1 >= kMaxDepth) {
            skipped.push_back(relative);
        } else {
            collectFiles(fs, child, relative, depth + 1, files, skipped, unreadable);
        }
    }
}

// Step 6, the rollback, decided from the disk. `work` is R/.update/<tx>.
//
// For every file the plan names:
//   REPLACE, REMOVE  old/p exists  -> it goes back over R/p. If R/p holds the NEW file it is
//                                     moved out first (to new/p - a rename, because deleting a
//                                     program that is running is refused and moving it is not).
//   ADD              R/p exists, new/p does not -> the transaction moved it in: move it back
//                                     to new/p. If new/p still exists R/p is not ours.
// then the directories the transaction created go, deepest first, when they are empty.
// Nothing here depends on what the journal says happened - only on what is there. That
// includes WHICH files there are: every file under old/ whose name an update could have written
// goes back, whether or not the journal lists it, because the caller deletes the folder
// afterwards and a file left in old/ is a file lost (a journal damaged in the middle, or one
// with a record the path rules refused, must not turn into a v1/v2 mix). A name under old/ that
// no update writes is not ours: it is added to plan.refused and goes with the folder.
//
// Every file in old/ is either put back, reported as a failure, or - for a REMOVE whose file
// has been made again since - left to the person's newer file; so an OK means old/ holds nothing
// that was meant to be kept.
core::Status restoreFromDisk(FsOps& fs, const Path& root, const Path& work, JournalPlan& plan)
{
    const core::Result<Path> resolvedRoot = fs.resolve(root);
    if (!resolvedRoot.ok()) {
        return resolvedRoot.error();
    }
    PathGuard guard(fs, root, resolvedRoot.value());
    if (const std::optional<Failure> bad = guard.check(work)) {
        return core::makeError(core::ErrorCode::InvalidState, bad->detail, bad->reason);
    }
    const Path oldRoot = work / core::pathFromUtf8(kOldName);
    const Path newRoot = work / core::pathFromUtf8(kNewName);

    std::optional<core::Error> failure;
    const auto fail = [&failure](core::Error error) {
        if (!failure) {
            failure = std::move(error);
        }
    };

    // The journal's entries, then the files old/ holds that it does not list.
    std::vector<PlanEntry> entries = plan.entries;
    {
        std::set<std::string> listed;
        for (const PlanEntry& entry : entries) {
            listed.insert(entry.path);
        }
        std::vector<std::string> files;
        std::vector<std::string> skipped;
        std::optional<core::Error> unreadable;
        collectFiles(fs, oldRoot, std::string(), 0, files, skipped, unreadable);
        if (unreadable) {
            fail(*unreadable);
        }
        for (const std::string& relative : files) {
            if (listed.contains(relative)) {
                continue;
            }
            if (const core::Status allowed = acceptablePath(relative, false); !allowed.ok()) {
                skipped.push_back(relative);
                continue;
            }
            entries.push_back({Action::Replace, relative});
        }
        for (const std::string& name : skipped) {
            plan.refused.push_back("old/" + name + ": not a file an update puts there");
        }
    }

    for (auto entry = entries.rbegin(); entry != entries.rend(); ++entry) {
        const Path target = under(root, entry->path);
        const Path oldPath = under(oldRoot, entry->path);
        const Path newPath = under(newRoot, entry->path);
        // All three sides of a move are held to the rule, not only the install's: old/<dir> and
        // new/<dir> can be links as well, and a file renamed through one comes from, or goes to,
        // a place outside the install.
        std::optional<Failure> unsafe = guard.check(target.parent_path());
        if (!unsafe) {
            unsafe = guard.check(oldPath.parent_path());
        }
        if (!unsafe) {
            unsafe = guard.check(newPath.parent_path());
        }
        if (unsafe) {
            fail(core::makeError(core::ErrorCode::InvalidState, unsafe->detail,
                                 unsafe->reason + " " + entry->path));
            continue;
        }
        const core::Result<EntryInfo> inTree = fs.stat(target);
        if (!inTree.ok()) {
            fail(inTree.error());
            continue;
        }

        if (entry->action == Action::Add) {
            if (inTree->kind != EntryKind::File) {
                continue; // never moved in (or already moved back)
            }
            const core::Result<EntryInfo> waiting = fs.stat(newPath);
            if (!waiting.ok()) {
                fail(waiting.error());
            } else if (waiting->kind == EntryKind::Missing) {
                if (const core::Status made = fs.makeDirectories(newPath.parent_path()); !made.ok()) {
                    fail(made.error());
                } else if (const core::Status moved = fs.renameFile(target, newPath); !moved.ok()) {
                    fail(moved.error());
                }
            }
            continue;
        }

        const core::Result<EntryInfo> saved = fs.stat(oldPath);
        if (!saved.ok()) {
            fail(saved.error());
            continue;
        }
        if (saved->kind != EntryKind::File) {
            continue; // the old file was never moved aside, or is back already
        }
        if (inTree->kind == EntryKind::Directory || inTree->kind == EntryKind::Other) {
            fail(core::makeError(core::ErrorCode::InvalidState,
                                 "something other than a file is where a file must go back",
                                 entry->path));
            continue;
        }
        if (inTree->kind == EntryKind::File && entry->action == Action::Remove) {
            continue; // a removed file has been made again since; that one is the person's
        }
        if (inTree->kind == EntryKind::File) {
            // The new file (or one that appeared since). Out of the way, not deleted.
            const core::Result<EntryInfo> waiting = fs.stat(newPath);
            if (!waiting.ok()) {
                fail(waiting.error());
                continue;
            }
            core::Status cleared;
            if (waiting->kind == EntryKind::Missing && entry->action == Action::Replace) {
                cleared = fs.makeDirectories(newPath.parent_path());
                if (cleared.ok()) {
                    cleared = fs.renameFile(target, newPath);
                }
            } else {
                cleared = fs.removeFile(target);
            }
            if (!cleared.ok()) {
                fail(cleared.error());
                continue;
            }
        }
        if (const core::Status made = fs.makeDirectories(target.parent_path()); !made.ok()) {
            fail(made.error());
            continue;
        }
        if (const core::Status back = fs.renameFile(oldPath, target); !back.ok()) {
            fail(back.error());
        }
    }

    if (failure) {
        return *failure;
    }
    // Only now, with every file where it was, are the folders the transaction made safe to
    // remove. A folder that is not empty holds something that is not ours; it stays, and so
    // does one that cannot be removed: an empty folder is not a half-swapped tree.
    for (auto directory = plan.directories.rbegin(); directory != plan.directories.rend();
         ++directory) {
        (void)fs.removeEmptyDirectory(under(root, *directory));
    }
    return {};
}

// What a skipped journal record is reported in. A line per record, appended; nothing is
// created when nothing was skipped. The window reads this file for the transactions a recovery
// found (recover.hpp, "THE FORM OF THOSE LINES IS READ"), and a record's text is anybody's: it
// names nothing there because the line's second word is `journal`, and because it stays ONE
// line (oneLine), so that a forged record cannot begin a line of its own.
void reportRefusals(FsOps& fs, const Path& root, const std::string& transaction,
                    const JournalPlan& plan)
{
    if (plan.refused.empty()) {
        return;
    }
    std::string text;
    for (const std::string& line : plan.refused) {
        text += transaction + " journal record refused: " + detail::oneLine(line) + "\n";
    }
    (void)fs.appendDurable(updateWorkDirectory(root) / core::pathFromUtf8(kRecoveryLogName), text);
}

// ---- the apply ----------------------------------------------------------------------------

// The order the classes are committed in (apply.hpp, step 5), the helper's own file after
// the other programs, the removals after every new file is in place, and the records last:
// installed.json first - the claim "this version is installed" - then the signed manifest and
// its signature (they sort in that order, and rollback does not depend on it).
enum class Phase { Data, Lib, Program, Helper, Remove, Record };

struct Entry {
    Action action = Action::Add;
    Phase phase = Phase::Data;
    std::string path;
    bool executable = false;
};

bool entryBefore(const Entry& a, const Entry& b)
{
    if (a.phase != b.phase) {
        return a.phase < b.phase;
    }
    return a.path < b.path;
}

class Applier {
  public:
    Applier(const ApplyRequest& request, const ApplyServices& services, std::stop_token stop)
        : request_(request), services_(services), fs_(services.fs), stop_(std::move(stop))
    {
    }

    ApplyResult run();
    // How a run ends when an exception cut it short (apply() below catches it).
    ApplyResult unexpected(const std::string& what);

  private:
    // ---- the steps
    std::optional<Failure> begin();
    std::optional<Failure> readInstallRecord();
    std::optional<Failure> verify();
    std::optional<Failure> waitForQuiet();
    std::optional<Failure> plan();
    std::optional<Failure> writeJournalPlan();
    std::optional<Failure> lookAgainForHolders();
    std::optional<Failure> commit();
    std::optional<Failure> watchHealth();

    // ---- the outcomes
    ApplyResult fail(const Failure& failure, bool rollbackFirst);
    ApplyResult rolledBack(const std::string& reason, const std::string& detail);
    ApplyResult finish(ApplyResult result, bool silent = false);
    void discardWork();
    void finishHealthy();

    // Every process but this one that was started from a file under R (step 2 asks, twice).
    [[nodiscard]] core::Result<std::vector<RunningProgram>> othersUnderRoot() const;

    [[nodiscard]] bool cancelled() const { return stop_.stop_requested(); }
    void pause() const { std::this_thread::sleep_for(services_.timings.pollInterval); }
    [[nodiscard]] Path newRoot() const { return work_ / core::pathFromUtf8(kNewName); }
    [[nodiscard]] Path oldRoot() const { return work_ / core::pathFromUtf8(kOldName); }

    const ApplyRequest& request_;
    const ApplyServices& services_;
    FsOps& fs_;
    std::stop_token stop_;

    Path root_;
    Path updateDir_;
    Path work_;
    Install install_;
    std::unique_ptr<FileLock> lock_;
    std::string stagedJson_;
    std::string stagedSignature_;
    std::optional<Manifest> manifest_;
    std::optional<Manifest> installedManifest_;
    std::vector<Entry> entries_;
    std::vector<std::string> directories_; // created by the transaction, shallowest first
    std::set<std::string> madeBeforehand_; // the same, as a set, for the commit
    bool runOnceSet_ = false;
    bool journalWritten_ = false;
    // This run made R/.update/<tx>, so it is this run's to delete if the plan does not finish.
    bool workMade_ = false;
    // The window that asked for the update did not exit: it is still there, and it must not be
    // started a second time beside itself.
    bool windowStillRunning_ = false;
    // The new version proved it starts. What is left is the clean-up, and a failure there does
    // not undo an update that is good.
    bool provedHealthy_ = false;
    std::optional<Journal> journal_;
};

ApplyResult Applier::run()
{
    root_ = request_.root.empty() ? Path() : detail::normalizedRoot(request_.root);
    // The state folder is where the health marker is looked for: without it a start that hung,
    // or exited without saying it had started, could not be told from one that was fine.
    if (root_.empty() || request_.stateDirectory.empty() ||
        !isValidTransactionId(request_.transaction)) {
        return finish(fail(refuse("invalid_request", "the helper was started with an empty "
                                                     "install or state folder or a transaction "
                                                     "id that is not lower-case hexadecimal"),
                           false));
    }
    updateDir_ = updateWorkDirectory(root_);
    work_ = updateDir_ / core::pathFromUtf8(request_.transaction);

    if (const std::optional<Failure> failure = begin()) {
        return finish(fail(*failure, false), failure->silent);
    }
    if (const std::optional<Failure> failure = verify()) {
        return finish(fail(*failure, false));
    }
    if (const std::optional<Failure> failure = waitForQuiet()) {
        return finish(fail(*failure, false));
    }
    if (const std::optional<Failure> failure = plan()) {
        // A folder that was there before this run is not this run's to delete.
        if (failure->reason != "transaction_exists") {
            discardWork();
        }
        return finish(fail(*failure, false));
    }
    // From here the journal exists, and everything that goes wrong is a rollback.
    if (const std::optional<Failure> failure = writeJournalPlan()) {
        return finish(fail(*failure, journalWritten_));
    }
    if (const std::optional<Failure> failure = commit()) {
        return finish(fail(*failure, true));
    }
    const JournalRecord pending{JournalOp::CommittedPendingHealth, {}};
    if (const core::Status written = journal_->append(std::span(&pending, 1)); !written.ok()) {
        return finish(fail(refuse("journal", "the journal cannot be written", written.error()),
                           true));
    }
    if (const std::optional<Failure> failure = watchHealth()) {
        return finish(fail(*failure, true));
    }
    provedHealthy_ = true;
    // Settings > Apps names the new version only once it has proved it starts: written before,
    // a release that is rolled back would still be the one the person is told they have.
    if (install_.kind == InstallKind::Installer) {
        const std::string version = toString(manifest_->version);
        (void)services_.registry.setInstalledVersion(version, "Katana " + version);
    }
    finishHealthy();

    ApplyResult result;
    result.outcome = ApplyOutcome::Committed;
    result.detail = "Katana " + toString(manifest_->version) + " is installed.";
    return finish(std::move(result));
}

// Step 1 up to the lock: what the install is, that its folder can be written, and that no
// other helper is running.
std::optional<Failure> Applier::begin()
{
    if (std::optional<Failure> bad = readInstallRecord()) {
        return bad;
    }

    if (const core::Status made = fs_.makeDirectories(updateDir_); !made.ok()) {
        return refuse("root_not_writable", "the install folder cannot be written", made.error());
    }
    const core::Result<EntryInfo> folder = fs_.stat(updateDir_);
    if (!folder.ok() || folder->kind != EntryKind::Directory || folder->reparsePoint) {
        return refuse("reparse_point", ".update is not an ordinary folder: " +
                                           core::pathToUtf8(updateDir_));
    }
    const Path lockPath = updateDir_ / core::pathFromUtf8(kLockName);
    const auto isHeld = [](const core::Error& error) {
        return error.code == core::ErrorCode::InvalidState && error.message == "held";
    };
    core::Result<std::unique_ptr<FileLock>> lock = fs_.tryLock(lockPath);
    // The grace is cut to the holders' wait, so the tests that shorten that shorten this too.
    const std::chrono::milliseconds grace =
        std::min<std::chrono::milliseconds>(kLockGrace, services_.timings.waitForHolders);
    const Clock::time_point lockStart = Clock::now();
    while (!lock.ok() && isHeld(lock.error()) && Clock::now() - lockStart < grace && !cancelled()) {
        pause();
        lock = fs_.tryLock(lockPath);
    }
    if (!lock.ok()) {
        if (isHeld(lock.error())) {
            Failure held = refuse("locked", "another update is running");
            held.silent = true;
            return held;
        }
        return refuse("root_not_writable", "the update lock cannot be taken", lock.error());
    }
    lock_ = std::move(lock).value();

    // An earlier transaction that a crash left unfinished, or whose rollback failed, is put
    // back BEFORE this one reads the tree: planned over a half-swapped tree, this update would
    // succeed, and at the next start recovery would roll the old files of that transaction over
    // the new version - a mix of both with the wrong installed.json. If it cannot be put back,
    // nothing new is started on top of it.
    if (const core::Status settled = detail::recoverLocked(root_, services_, request_.transaction,
                                                           request_.stateDirectory);
        !settled.ok()) {
        return refuse("interrupted_update",
                      "an earlier update was interrupted and could not be put back, so no new "
                      "one is started. Run: katana_updater --recover \"" + core::pathToUtf8(root_) +
                          "\"",
                      settled.error());
    }

    // An earlier update that is in place and was never settled - its helper died while the new
    // version was proving that it starts - is settled BEFORE this one too, by the one rule
    // (recover_internal.hpp, settlePending), and never by putting it back: a swap is not the
    // place to judge a release that somebody may be running from. With its marker there it is
    // healthy, and what its dead helper left in U goes. Without, nothing is touched and no new
    // update is started: over a release that was never shown to start, a failure of this one
    // would put back a tree nobody proved, and its folder - the old files of the release before
    // - would have no owner. So one such folder exists at most.
    const detail::PendingOutcome earlier = detail::settlePending(
        root_, services_, request_.transaction, request_.stateDirectory, false);
    detail::forgetSettled(fs_, request_.stateDirectory, earlier);
    if (!earlier.left.empty()) {
        return refuse("interrupted_update",
                      "an earlier update is in place and has not been confirmed to start, so no "
                      "new one is started over it. Katana's window confirms it when it starts - "
                      "or run: katana_updater --recover \"" + core::pathToUtf8(root_) +
                          "\", which confirms it or puts it back - then update again");
    }
    // What is installed may be another version now (the rollback put it back).
    return readInstallRecord();
}

// What the install is (install.json, installed.json, the registry), and that it is an install.
std::optional<Failure> Applier::readInstallRecord()
{
    const core::Result<Install> install = readInstall(root_, services_.registry);
    if (!install.ok()) {
        return refuse("install_record", "the install's records cannot be read", install.error());
    }
    install_ = install.value();
    if (install_.kind == InstallKind::Development || install_.target.empty()) {
        return refuse("development_tree", "this is a development tree, not an install: it has "
                                          "no share/katana/install.json, and is never updated");
    }
    return std::nullopt;
}

// Step 1, verify: with the helper's own keys, never the engine's word.
std::optional<Failure> Applier::verify()
{
    const core::Result<std::string> json = fs_.readFile(stagedManifestPath(request_.stageDirectory));
    const core::Result<std::string> signature =
        fs_.readFile(stagedSignaturePath(request_.stageDirectory));
    if (!json.ok() || !signature.ok()) {
        return refuse("staged_manifest", "the staged manifest or its signature is missing",
                      json.ok() ? signature.error() : json.error());
    }
    const core::Result<Manifest> verified =
        verifyManifest(json.value(), signature.value(), services_.trust, install_);
    if (!verified.ok()) {
        return refuse("staged_manifest", "the staged manifest is not trusted", verified.error());
    }
    stagedJson_ = json.value();
    stagedSignature_ = signature.value();
    manifest_ = verified.value();
    const Manifest& manifest = *manifest_;

    // verifyManifest has already refused, with the helper's own code, a manifest of another
    // product, target or channel (trust.cpp) and one that breaks the path rules (the parse:
    // checkManifestPaths), so they are not asked a second time here.
    if (manifest.notifyOnly) {
        return refuse("notify_only", "this release is announced only; it cannot be installed here");
    }
    if (manifest.applier > kApplierProtocol) {
        return refuse("applier_too_new", "this update needs a newer update helper than this one");
    }
    if (!isNewerThan(manifest, install_)) {
        return refuse("downgrade", "the staged release (" + toString(manifest.version) +
                                       ") is not newer than the installed one (" +
                                       toString(install_.version) + ")");
    }
    // The installed manifest decides what may be removed. One that does not verify (no record
    // on a fresh install; a key since dropped; a tampered file) decides nothing: nothing is
    // removed, which is the safe way to be wrong.
    if (install_.hasInstalledManifest) {
        const Path directory = installRecordDirectory(root_);
        const core::Result<std::string> oldJson =
            fs_.readFile(directory / core::pathFromUtf8("installed.manifest.json"));
        const core::Result<std::string> oldSignature =
            fs_.readFile(directory / core::pathFromUtf8("installed.manifest.json.sig"));
        if (oldJson.ok() && oldSignature.ok()) {
            core::Result<Manifest> old =
                verifyManifest(oldJson.value(), oldSignature.value(), services_.trust, install_);
            if (old.ok()) {
                installedManifest_ = std::move(old).value();
            }
        }
    }
    return std::nullopt;
}

// Step 2: the window, then everyone with an image under R. Never kills.
std::optional<Failure> Applier::waitForQuiet()
{
    const ApplyTimings& timings = services_.timings;
    if (request_.waitPid != 0) {
        const Clock::time_point start = Clock::now();
        while (services_.processes.isAlive(request_.waitPid)) {
            if (cancelled()) {
                return refuse("cancelled", "the update was cancelled");
            }
            if (Clock::now() - start >= timings.waitForParent) {
                windowStillRunning_ = true;
                Failure blocked;
                blocked.outcome = ApplyOutcome::Blocked;
                blocked.reason = "blocked";
                blocked.holders.push_back(RunningProgram{request_.waitPid, {}});
                blocked.detail = "Katana is still running (" + std::to_string(request_.waitPid) +
                                 "). Close it, then press Retry.";
                return blocked;
            }
            pause();
        }
    }
    const Clock::time_point start = Clock::now();
    while (true) {
        core::Result<std::vector<RunningProgram>> holders = othersUnderRoot();
        if (!holders.ok()) {
            return refuse("scan_failed", "the running programs cannot be listed", holders.error());
        }
        if (holders->empty()) {
            return std::nullopt;
        }
        if (cancelled()) {
            return refuse("cancelled", "the update was cancelled");
        }
        if (Clock::now() - start >= timings.waitForHolders) {
            return blockedBy(std::move(holders).value());
        }
        pause();
    }
}

core::Result<std::vector<RunningProgram>> Applier::othersUnderRoot() const
{
    core::Result<std::vector<RunningProgram>> scanned = services_.processes.holdersUnder(root_);
    if (!scanned.ok()) {
        return scanned.error();
    }
    std::vector<RunningProgram> others;
    for (RunningProgram& program : scanned.value()) {
        if (program.pid != services_.processes.currentPid()) {
            others.push_back(std::move(program));
        }
    }
    return others;
}

// Step 2's second look, at the last moment an update can still end with nothing touched. The
// first look is over before the plan, and the plan hashes and copies every changed file of a
// release: seconds, in which a person starts Katana again or a script runs katana_cli. A program
// started then is seen by nobody and goes on running old code over the new tree. So the question
// is asked once more, with the PLAN on disk and COMMITTING not yet: a holder now is the same
// Blocked as a holder then. It is ONE question and not a second wait: the first look already
// waited for whoever was closing, and a wait here would let the files the plan hashed grow old
// under a program that is running.
std::optional<Failure> Applier::lookAgainForHolders()
{
    core::Result<std::vector<RunningProgram>> holders = othersUnderRoot();
    if (!holders.ok()) {
        return refuse("scan_failed", "the running programs cannot be listed", holders.error());
    }
    if (holders->empty()) {
        return std::nullopt;
    }
    return blockedBy(std::move(holders).value());
}

// Step 3: hash, decide, copy. Nothing under the install changes; what is written goes to
// R/.update/<tx>/new.
std::optional<Failure> Applier::plan()
{
    const Manifest& manifest = *manifest_;
    const core::Result<Path> resolvedRoot = fs_.resolve(root_);
    if (!resolvedRoot.ok()) {
        return refuse("io_error", "the install folder cannot be resolved", resolvedRoot.error());
    }
    PathGuard guard(fs_, root_, resolvedRoot.value());

    // Which parents of the new files do not exist yet (shallowest first, no repeats).
    std::map<std::string, bool> folderExists; // relative folder -> exists
    std::set<std::string> toCreate;
    const auto needFolder = [&](const std::string& relativeFile) -> std::optional<Failure> {
        std::vector<std::string> prefixes;
        for (std::size_t slash = relativeFile.find('/'); slash != std::string::npos;
             slash = relativeFile.find('/', slash + 1)) {
            prefixes.push_back(relativeFile.substr(0, slash));
        }
        for (const std::string& prefix : prefixes) {
            auto known = folderExists.find(prefix);
            if (known == folderExists.end()) {
                const core::Result<EntryInfo> info = fs_.stat(under(root_, prefix));
                if (!info.ok()) {
                    return refuse("io_error", "a folder of the install cannot be examined",
                                  info.error());
                }
                if (info->kind != EntryKind::Missing && info->kind != EntryKind::Directory) {
                    return refuse("path_conflict",
                                  "a file is where a folder is needed: " + prefix);
                }
                known = folderExists.emplace(prefix, info->kind == EntryKind::Directory).first;
            }
            if (!known->second) {
                toCreate.insert(prefix);
            }
        }
        return std::nullopt;
    };

    // ---- the new manifest's files: keep, add or replace
    std::vector<const ManifestFile*> toCopy;
    std::set<std::string> inNewManifest;
    for (const ManifestFile& file : manifest.files) {
        if (cancelled()) {
            return refuse("cancelled", "the update was cancelled");
        }
        inNewManifest.insert(lowerAscii(file.path));
        const Path target = under(root_, file.path);
        if (const std::optional<Failure> unsafe = guard.check(target.parent_path())) {
            return unsafe;
        }
        const core::Result<EntryInfo> info = fs_.stat(target);
        if (!info.ok()) {
            return refuse("io_error", "a file of the install cannot be examined", info.error());
        }
        Entry entry;
        entry.path = file.path;
        entry.executable = file.executable;
        switch (file.fileClass) {
        case FileClass::Data:
        case FileClass::Seed:
            entry.phase = Phase::Data;
            break;
        case FileClass::Lib:
            entry.phase = Phase::Lib;
            break;
        case FileClass::Program:
            entry.phase = file.path == installedProgramRelative(kHelperStem) ? Phase::Helper
                                                                            : Phase::Program;
            break;
        }
        if (info->kind == EntryKind::Missing) {
            entry.action = Action::Add;
        } else if (info->kind != EntryKind::File) {
            return refuse("path_conflict", "a folder is where a file must go: " + file.path);
        } else if (info->reparsePoint) {
            return refuse("reparse_point", "a file of the install is a link: " + file.path);
        } else if (file.fileClass == FileClass::Seed) {
            continue; // written only when absent, so an edit by the person survives
        } else {
            const core::Result<std::string> hash = hashFile(target);
            if (hash.ok() && hash.value() == file.sha256) {
                continue; // KEEP
            }
            entry.action = Action::Replace; // an unreadable file counts as changed
        }
        if (const std::optional<Failure> folders = needFolder(file.path)) {
            return folders;
        }
        entries_.push_back(entry);
        toCopy.push_back(&file);
    }

    // ---- removals: only a file the installed manifest lists, the new one does not, and
    // whose bytes are still the ones that manifest signed. Anything else is not ours.
    if (installedManifest_) {
        for (const ManifestFile& old : installedManifest_->files) {
            if (inNewManifest.contains(lowerAscii(old.path)) || isReservedName(old.path) ||
                !checkManifestPath(old.path).ok()) {
                continue;
            }
            const Path target = under(root_, old.path);
            if (guard.check(target.parent_path())) {
                continue;
            }
            const core::Result<EntryInfo> info = fs_.stat(target);
            if (!info.ok() || info->kind != EntryKind::File || info->reparsePoint) {
                continue;
            }
            const core::Result<std::string> hash = hashFile(target);
            if (!hash.ok() || hash.value() != old.sha256) {
                continue; // changed by the person, or unreadable: left alone
            }
            Entry entry;
            entry.action = Action::Remove;
            entry.phase = Phase::Remove;
            entry.path = old.path;
            entries_.push_back(entry);
        }
    }

    // ---- the three records (installed.json, the signed manifest and its signature)
    struct Record {
        std::string path;
        std::string bytes;
    };
    const std::vector<Record> records = {
        {std::string(kRecordPaths[0]), installedRecord(manifest)},
        {std::string(kRecordPaths[1]), stagedJson_},
        {std::string(kRecordPaths[2]), stagedSignature_},
    };
    for (const Record& record : records) {
        // The records are written like any other file, so their folder is held to the same
        // rule: share/katana may be a link even when no file of the release lives in it.
        const Path recordPath = under(root_, record.path);
        if (const std::optional<Failure> unsafe = guard.check(recordPath.parent_path())) {
            return unsafe;
        }
        const core::Result<EntryInfo> info = fs_.stat(recordPath);
        if (!info.ok()) {
            return refuse("io_error", "a record of the install cannot be examined", info.error());
        }
        if (info->kind != EntryKind::Missing && info->kind != EntryKind::File) {
            return refuse("path_conflict", "a folder is where a record must go: " + record.path);
        }
        if (info->reparsePoint) {
            return refuse("reparse_point", "a record of the install is a link: " + record.path);
        }
        if (const std::optional<Failure> folders = needFolder(record.path)) {
            return folders;
        }
        Entry entry;
        entry.action = info->kind == EntryKind::File ? Action::Replace : Action::Add;
        entry.phase = Phase::Record;
        entry.path = record.path;
        entries_.push_back(entry);
    }
    std::stable_sort(entries_.begin(), entries_.end(), entryBefore);

    // Shallowest first: a folder is made before the one inside it, and rolled back after it.
    directories_.assign(toCreate.begin(), toCreate.end());
    std::sort(directories_.begin(), directories_.end(),
              [](const std::string& a, const std::string& b) {
                  const auto depthA = std::count(a.begin(), a.end(), '/');
                  const auto depthB = std::count(b.begin(), b.end(), '/');
                  return depthA != depthB ? depthA < depthB : a < b;
              });
    madeBeforehand_.insert(directories_.begin(), directories_.end());

    // ---- space for the copies
    std::uint64_t needed = kSpaceMargin;
    for (const ManifestFile* file : toCopy) {
        needed += file->size;
    }
    for (const Record& record : records) {
        needed += record.bytes.size();
    }
    const core::Result<std::uint64_t> space = fs_.freeSpace(root_);
    if (!space.ok()) {
        return refuse("io_error", "the free space cannot be read", space.error());
    }
    if (space.value() < needed) {
        return refuse("space", "the install folder's disk has " + std::to_string(space.value()) +
                                   " bytes free and the update needs " + std::to_string(needed));
    }

    // ---- the copies. The stage folder is user-writable, so a file in R is a COPY of it, made
    // here and hashed after it was made (what is hashed is what will be installed), never a
    // rename out of it.
    const core::Result<EntryInfo> existing = fs_.stat(work_);
    if (!existing.ok()) {
        return refuse("io_error", "the transaction folder cannot be examined", existing.error());
    }
    if (existing->kind != EntryKind::Missing) {
        return refuse("transaction_exists", "the transaction folder exists already: " +
                                                core::pathToUtf8(work_));
    }
    workMade_ = true; // from here a failure leaves a folder of ours, even half made
    if (const core::Status made = fs_.makeDirectories(newRoot()); !made.ok()) {
        return refuse("root_not_writable", "the transaction folder cannot be made", made.error());
    }
    if (const std::optional<Failure> unsafe = guard.check(work_)) {
        return unsafe;
    }
    std::set<std::string> madeNew;
    for (const ManifestFile* file : toCopy) {
        if (cancelled()) {
            return refuse("cancelled", "the update was cancelled");
        }
        const Path source = under(request_.stageDirectory, file->path);
        const core::Result<EntryInfo> staged = fs_.stat(source);
        if (!staged.ok() || staged->kind != EntryKind::File) {
            return refuse("staged_missing", "the staged file is missing: " + file->path);
        }
        if (staged->size != file->size) {
            return refuse("staged_hash", "the staged file has the wrong size: " + file->path);
        }
        const Path copy = under(newRoot(), file->path);
        if (madeNew.insert(core::pathToUtf8(copy.parent_path())).second) {
            if (const core::Status made = fs_.makeDirectories(copy.parent_path()); !made.ok()) {
                return refuse("root_not_writable", "a staging folder cannot be made", made.error());
            }
        }
        if (const core::Status copied = fs_.copyFile(source, copy); !copied.ok()) {
            return refuse("copy_failed", "a staged file cannot be copied: " + file->path,
                          copied.error());
        }
        const core::Result<std::string> hash = hashFile(copy);
        if (!hash.ok() || hash.value() != file->sha256) {
            return refuse("staged_hash", "the staged file does not hash to the manifest's value: " +
                                             file->path);
        }
        if (file->executable) {
            if (const core::Status marked = fs_.setExecutable(copy); !marked.ok()) {
                return refuse("copy_failed", "a file cannot be made executable: " + file->path,
                              marked.error());
            }
        }
    }
    for (const Record& record : records) {
        const Path copy = under(newRoot(), record.path);
        if (madeNew.insert(core::pathToUtf8(copy.parent_path())).second) {
            if (const core::Status made = fs_.makeDirectories(copy.parent_path()); !made.ok()) {
                return refuse("root_not_writable", "a staging folder cannot be made", made.error());
            }
        }
        if (const core::Status written = fs_.writeDurable(copy, record.bytes); !written.ok()) {
            return refuse("copy_failed", "a record cannot be written: " + record.path,
                          written.error());
        }
    }
    return std::nullopt;
}

// Step 4: the PLAN group, step 2's second look, then COMMITTING, and the RunOnce entry that
// re-runs recovery at the next logon should the machine die from here.
std::optional<Failure> Applier::writeJournalPlan()
{
    journal_.emplace(fs_, work_ / core::pathFromUtf8(kJournalName));

    std::vector<JournalRecord> group;
    group.push_back({JournalOp::Plan,
                     {request_.transaction, std::to_string(install_.sequence),
                      std::to_string(manifest_->sequence)}});
    for (const std::string& directory : directories_) {
        group.push_back({JournalOp::MakeDirectory, {directory}});
    }
    for (const Entry& entry : entries_) {
        const JournalOp op = entry.action == Action::Add       ? JournalOp::Add
                             : entry.action == Action::Replace ? JournalOp::Replace
                                                               : JournalOp::Remove;
        group.push_back({op, {entry.path}});
    }
    if (const core::Status written = journal_->append(group); !written.ok()) {
        // Nothing has moved: the folder is junk, not a half swap.
        discardWork();
        return refuse("journal", "the journal cannot be written", written.error());
    }
    journalWritten_ = true;
    if (cancelled()) {
        discardWork();
        journalWritten_ = false;
        return refuse("cancelled", "the update was cancelled");
    }
    // Nothing has moved yet, so whoever is found now is given way to as at the first look: the
    // folder of copies goes, the install is as it was, and the stage - the download - stays for
    // the Retry.
    if (std::optional<Failure> late = lookAgainForHolders()) {
        discardWork();
        journalWritten_ = false;
        return late;
    }
    const JournalRecord committing{JournalOp::Committing, {}};
    if (const core::Status written = journal_->append(std::span(&committing, 1)); !written.ok()) {
        return refuse("journal", "the journal cannot be written", written.error());
    }
    if (!request_.helper.empty()) {
        // Best effort: it is a second safety net behind the next start of any program.
        runOnceSet_ = services_.registry
                          .setRunOnce(kRunOnceName, detail::runOnceCommand(request_.helper, root_))
                          .ok();
    }
    return std::nullopt;
}

// Step 5: renames only. Per file R/p -> old/p, then new/p -> R/p; a failure stops the commit
// and the caller rolls back.
std::optional<Failure> Applier::commit()
{
    std::set<Path::string_type> madeOld;
    std::set<std::string> madeRoot;
    std::size_t index = 0;
    while (index < entries_.size()) {
        const Phase phase = entries_[index].phase;
        std::vector<JournalRecord> progress;
        std::set<Path> touched;
        std::vector<const Entry*> removed;
        for (; index < entries_.size() && entries_[index].phase == phase; ++index) {
            const Entry& entry = entries_[index];
            const Path target = under(root_, entry.path);
            const Path oldPath = under(oldRoot(), entry.path);
            const Path newPath = under(newRoot(), entry.path);
            if (entry.action != Action::Add) {
                if (madeOld.insert(oldPath.parent_path().native()).second) {
                    if (const core::Status made = fs_.makeDirectories(oldPath.parent_path());
                        !made.ok()) {
                        return refuse("rename_failed", "a folder for the old file cannot be made",
                                      made.error());
                    }
                }
                if (const core::Status moved = fs_.renameFile(target, oldPath); !moved.ok()) {
                    return refuse("rename_failed", "the old file cannot be moved aside: " + entry.path,
                                  moved.error());
                }
                progress.push_back({JournalOp::MovedOld, {entry.path}});
                touched.insert(oldPath.parent_path());
            }
            if (entry.action != Action::Remove) {
                const std::string parent = entry.path.substr(0, entry.path.rfind('/'));
                if (madeBeforehand_.contains(parent) && madeRoot.insert(parent).second) {
                    if (const core::Status made = fs_.makeDirectories(target.parent_path());
                        !made.ok()) {
                        return refuse("rename_failed", "a folder of the install cannot be made",
                                      made.error());
                    }
                }
                if (const core::Status moved = fs_.renameFile(newPath, target); !moved.ok()) {
                    return refuse("rename_failed", "the new file cannot be moved into place: " + entry.path,
                                  moved.error());
                }
                progress.push_back({JournalOp::MovedNew, {entry.path}});
            } else {
                removed.push_back(&entry);
            }
            touched.insert(target.parent_path());
        }
        // A folder the removals emptied goes too, so the tree is the new version's and not
        // the new version's plus empty folders. Best effort, and never one of the four top
        // folders; rollback makes the folder again before it puts a file back.
        for (const Entry* entry : removed) {
            std::string folder = entry->path;
            while (true) {
                const std::size_t slash = folder.rfind('/');
                if (slash == std::string::npos) {
                    break;
                }
                folder.resize(slash);
                if (folder.find('/') == std::string::npos) {
                    break; // bin, share, etc and lib stay
                }
                if (!fs_.removeEmptyDirectory(under(root_, folder)).ok()) {
                    break;
                }
            }
        }
        if (!progress.empty()) {
            if (const core::Status written = journal_->append(progress); !written.ok()) {
                return refuse("journal", "the journal cannot be written", written.error());
            }
        }
        for (const Path& directory : touched) {
            (void)fs_.syncDirectory(directory);
        }
    }
    return std::nullopt;
}

// Step 7, the health check. The new katana is started with --update-health <tx>; it writes
// the marker after its first idle turn and exits. A start that fails, exits non-zero, never
// writes the marker or does not finish in time is not a working release.
std::optional<Failure> Applier::watchHealth()
{
    SpawnRequest start;
    start.executable = installedProgram(root_, kProgramStem);
    start.arguments = {std::string(kHealthSwitch), request_.transaction};
    start.workingDirectory = start.executable.parent_path();
    start.timeout = services_.timings.healthWait;

    const core::Result<SpawnResult> ran = services_.launcher.run(start);
    if (!ran.ok()) {
        return refuse("health_exit", "the new version could not be started", ran.error());
    }
    const core::Result<EntryInfo> info =
        fs_.stat(healthMarkerPath(request_.stateDirectory, request_.transaction));
    const bool marker = info.ok() && info->kind == EntryKind::File;
    if (marker && (ran->timedOut || ran->exitCode == 0)) {
        return std::nullopt;
    }
    if (marker) {
        return refuse("health_exit", "the new version exited with code " +
                                         std::to_string(ran->exitCode) + " after starting");
    }
    if (ran->timedOut) {
        return refuse("health_timeout", "the new version did not finish starting in time");
    }
    return refuse("health_exit", ran->exitCode == 0
                                     ? std::string("the new version exited without saying it had started")
                                     : "the new version exited with code " +
                                           std::to_string(ran->exitCode));
}

// What a healthy transaction leaves behind goes: its folder, the staged files (only those
// the manifest names - the stage path came from a command line), the marker, the RunOnce
// entry. What a lock keeps from being deleted goes to cleanup.list for the next start.
void Applier::finishHealthy()
{
    const JournalRecord healthy{JournalOp::Healthy, {}};
    (void)journal_->append(std::span(&healthy, 1));

    std::vector<Path> leftovers;
    detail::removeTree(fs_, work_, leftovers, kJournalName);

    const Path stage = request_.stageDirectory;
    for (const ManifestFile& file : manifest_->files) {
        const Path staged = under(stage, file.path);
        if (!fs_.removeFile(staged).ok()) {
            leftovers.push_back(staged);
        }
        for (Path folder = staged.parent_path(); folder != stage && !folder.empty() &&
                                                 isUnderOrEqual(stage, folder);
             folder = folder.parent_path()) {
            if (!fs_.removeEmptyDirectory(folder).ok()) {
                break;
            }
        }
    }
    (void)fs_.removeFile(stagedManifestPath(stage));
    (void)fs_.removeFile(stagedSignaturePath(stage));
    (void)fs_.removeEmptyDirectory(stage);
    if (!request_.stateDirectory.empty()) {
        (void)fs_.removeFile(healthMarkerPath(request_.stateDirectory, request_.transaction));
        for (const Path& leftover : leftovers) {
            (void)appendCleanup(request_.stateDirectory, leftover, fs_);
        }
    }
}

// The work folder of a transaction that never reached a journal: nothing under R changed, so
// it is only junk.
void Applier::discardWork()
{
    std::vector<Path> leftovers;
    detail::removeTree(fs_, work_, leftovers);
    if (!request_.stateDirectory.empty()) {
        for (const Path& leftover : leftovers) {
            (void)appendCleanup(request_.stateDirectory, leftover, fs_);
        }
    }
}

ApplyResult Applier::rolledBack(const std::string& reason, const std::string& detail)
{
    ApplyResult result;
    const core::Status undone = rollback(root_, request_.transaction, services_);
    if (!undone.ok()) {
        result.outcome = ApplyOutcome::RollbackIncomplete;
        result.reason = "rollback_failed";
        result.detail = "The update failed (" + reason + ": " + detail +
                        ") and putting the old version back failed too (" +
                        undone.error().describe() + "). The install may be half updated: run "
                        "katana_updater --recover \"" + core::pathToUtf8(root_) + "\".";
        return result;
    }
    result.outcome = ApplyOutcome::RolledBack;
    result.reason = reason;
    result.detail = detail;
    return result;
}

ApplyResult Applier::fail(const Failure& failure, bool rollbackFirst)
{
    if (rollbackFirst) {
        ApplyResult result = rolledBack(failure.reason, failure.detail);
        // A release that does not start must not be offered again as it was (state.hpp).
        if (result.outcome == ApplyOutcome::RolledBack &&
            (failure.reason == "health_exit" || failure.reason == "health_timeout") &&
            manifest_ && !request_.stateDirectory.empty()) {
            // The marker a start that then failed wrote is no evidence for the next one.
            (void)fs_.removeFile(healthMarkerPath(request_.stateDirectory, request_.transaction));
            core::Result<UpdateState> state = readState(request_.stateDirectory);
            // A file that is not ours to read is replaced; one that failed to read for another
            // reason is left alone (recover_internal.hpp).
            const bool replaceable = state.ok() || detail::mayReplaceUnreadableState(state.error());
            UpdateState updated = state.ok() ? state.value() : UpdateState{};
            if (replaceable &&
                std::find(updated.blockedSequences.begin(), updated.blockedSequences.end(),
                          manifest_->sequence) == updated.blockedSequences.end()) {
                updated.blockedSequences.push_back(manifest_->sequence);
                (void)writeState(request_.stateDirectory, updated);
            }
        }
        return result;
    }
    ApplyResult result;
    result.outcome = failure.outcome;
    result.reason = failure.reason;
    result.detail = failure.detail;
    result.holders = failure.holders;
    return result;
}

// The end of every path that is not a crash: say how it ended, let the lock go, and start
// the program again. The lock goes BEFORE the relaunch, because the program's first act is
// recover(), which waits for a live holder.
ApplyResult Applier::finish(ApplyResult result, bool silent)
{
    lock_.reset();
    // The helper runs from a copy in U/run-<tx> and cannot delete the program it is running:
    // the folder goes on the list the next start works through. Not after a rollback that
    // failed - the RunOnce entry runs `--recover` from that very copy.
    if (result.outcome != ApplyOutcome::RollbackIncomplete && !request_.helper.empty() &&
        !request_.stateDirectory.empty()) {
        (void)appendCleanup(request_.stateDirectory, request_.helper.parent_path(), fs_);
    }
    if (silent) {
        return result;
    }
    if (result.outcome != ApplyOutcome::RollbackIncomplete && runOnceSet_) {
        (void)services_.registry.clearRunOnce(kRunOnceName);
    }
    if (!request_.stateDirectory.empty()) {
        LastResult last;
        last.status = std::string(toString(result.outcome));
        last.reason = result.reason;
        last.holder = holderText(result.holders);
        last.version = manifest_ ? toString(manifest_->version) : std::string();
        last.transaction = request_.transaction;
        (void)writeLastResult(request_.stateDirectory, last);
    }
    // A half-swapped tree is not started: its programs may not load, and a person who sees
    // nothing happen reads last.json at the next start instead.
    // Nor a window that is still running: a second one would open beside the one that hung.
    if (!request_.relaunch.empty() && result.outcome != ApplyOutcome::RollbackIncomplete &&
        !windowStillRunning_) {
        SpawnRequest again;
        again.executable = request_.relaunch;
        again.workingDirectory = request_.relaunch.parent_path();
        (void)services_.launcher.startDetached(again);
    }
    return result;
}

// The end of a run that an exception cut short (a std::exception from a file system or a
// conversion: nothing here throws on purpose). It is ended the way a failure at that point
// would have been: before the journal there is only a folder of copies to delete; after it the
// tree is put back from the disk, which is what every other failure does; once the new version
// has proved itself only the clean-up was lost, and the update stands. If even that throws, the
// outcome says what is known and names the command that puts a half-swapped tree right.
ApplyResult Applier::unexpected(const std::string& what)
{
    const std::string reason = "internal_error";
    const std::string message = "the update stopped on an unexpected error: " + what;
    try {
        if (provedHealthy_) {
            ApplyResult result;
            result.outcome = ApplyOutcome::Committed;
            result.detail = "Katana " + toString(manifest_->version) +
                            " is installed; clearing up after the update was cut short.";
            return finish(std::move(result));
        }
        if (workMade_ && !journalWritten_) {
            discardWork();
        }
        return finish(fail(refuse(reason, message), journalWritten_));
    } catch (const std::exception& again) {
        lock_.reset();
        ApplyResult result;
        if (provedHealthy_) {
            // Only the clean-up and the report failed; the new version is in and works.
            result.outcome = ApplyOutcome::Committed;
            result.detail = "the update is installed; clearing up after it stopped on an error: " +
                            std::string(again.what());
        } else if (journalWritten_) {
            result.outcome = ApplyOutcome::RollbackIncomplete;
            result.reason = "rollback_failed";
            result.detail = message + ", and then putting the old version back stopped on "
                            "another (" + std::string(again.what()) + "). The install may be "
                            "half updated: run katana_updater --recover <the install folder>.";
        } else {
            result.outcome = ApplyOutcome::Refused;
            result.reason = reason;
            result.detail = message;
        }
        return result;
    }
}

} // namespace

// ---- the command line -----------------------------------------------------------------------

std::vector<std::string> helperArguments(const ApplyRequest& request)
{
    return {"--root",     core::pathToUtf8(request.root),
            "--stage",    core::pathToUtf8(request.stageDirectory),
            "--state",    core::pathToUtf8(request.stateDirectory),
            "--tx",       request.transaction,
            "--wait-pid", std::to_string(request.waitPid),
            "--relaunch", request.relaunch.empty() ? std::string("none")
                                                   : core::pathToUtf8(request.relaunch)};
}

core::Result<ApplyRequest> parseHelperArguments(std::span<const std::string> arguments)
{
    static constexpr std::array<std::string_view, 6> kOptions = {
        "--root", "--stage", "--state", "--tx", "--wait-pid", "--relaunch"};
    std::map<std::string, std::string, std::less<>> values;
    for (std::size_t i = 0; i < arguments.size(); i += 2) {
        const std::string& option = arguments[i];
        if (std::find(kOptions.begin(), kOptions.end(), option) == kOptions.end()) {
            return core::makeError(core::ErrorCode::InvalidArgument, "unknown option", option);
        }
        if (i + 1 >= arguments.size()) {
            return core::makeError(core::ErrorCode::InvalidArgument, "option without a value",
                                   option);
        }
        if (!values.emplace(option, arguments[i + 1]).second) {
            return core::makeError(core::ErrorCode::InvalidArgument, "option given twice", option);
        }
    }
    for (const std::string_view option : kOptions) {
        if (!values.contains(std::string(option))) {
            return core::makeError(core::ErrorCode::InvalidArgument, "missing option",
                                   std::string(option));
        }
    }
    ApplyRequest request;
    const std::string& root = values["--root"];
    const std::string& stage = values["--stage"];
    const std::string& state = values["--state"];
    if (root.empty() || stage.empty() || state.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "--root, --stage and --state need a folder", "");
    }
    request.root = core::pathFromUtf8(root);
    request.stageDirectory = core::pathFromUtf8(stage);
    request.stateDirectory = core::pathFromUtf8(state);
    request.transaction = values["--tx"];
    if (!isValidTransactionId(request.transaction)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "a transaction id is 1 to 64 characters of 0-9 and a-f",
                               request.transaction);
    }
    const std::string& pid = values["--wait-pid"];
    std::uint64_t parsed = 0;
    const auto [end, error] = std::from_chars(pid.data(), pid.data() + pid.size(), parsed);
    if (pid.empty() || error != std::errc() || end != pid.data() + pid.size()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "--wait-pid is a process id (a number; 0 for none)", pid);
    }
    request.waitPid = parsed;
    const std::string& relaunch = values["--relaunch"];
    if (relaunch != "none") {
        request.relaunch = core::pathFromUtf8(relaunch);
    }
    return request;
}

Path installedProgram(const Path& root, std::string_view stem)
{
#ifdef _WIN32
    return root / "bin" / core::pathFromUtf8(std::string(stem) + ".exe");
#else
    return root / "bin" / core::pathFromUtf8(stem);
#endif
}

std::string newTransactionId()
{
    // Unique across runs without relying on random_device, which some MinGW runtimes seed
    // with a constant: the clocks, a counter, the address of a local and the thread, hashed.
    static std::atomic<std::uint64_t> counter{0};
    int marker = 0;
    std::string seed = std::to_string(Clock::now().time_since_epoch().count());
    seed += ':' + std::to_string(std::chrono::system_clock::now().time_since_epoch().count());
    seed += ':' + std::to_string(++counter);
    seed += ':' + std::to_string(reinterpret_cast<std::uintptr_t>(&marker));
    seed += ':' + std::to_string(std::hash<std::thread::id>{}(std::this_thread::get_id()));
    // An extra, not the only source: a system with no random device throws, and an id is
    // asked for at the moment an update is wanted.
    try {
        std::random_device device;
        seed += ':' + std::to_string(device());
    } catch (const std::exception&) {
    }
    return core::sha256Hex(seed).substr(0, 32);
}

// ---- the entry points --------------------------------------------------------------------------

ApplyResult apply(const ApplyRequest& request, const ApplyServices& services, std::stop_token stop)
{
    Applier applier(request, services, std::move(stop));
    // The one place an exception is caught: SimulatedCrash is not a std::exception, so a crash
    // injected by a FaultFs still goes through to the test that injected it.
    try {
        return applier.run();
    } catch (const std::exception& error) {
        return applier.unexpected(error.what());
    }
}

namespace {

core::Status rollbackTransaction(const Path& rootGiven, const std::string& transaction,
                                 const ApplyServices& services)
{
    if (rootGiven.empty()) {
        // updateWorkDirectory("") is ".update" under the current folder: not an install.
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "no install folder to roll back in", transaction);
    }
    if (!isValidTransactionId(transaction)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "a transaction id is 1 to 64 characters of 0-9 and a-f", transaction);
    }
    FsOps& fs = services.fs;
    const Path root = detail::normalizedRoot(rootGiven);
    const Path work = updateWorkDirectory(root) / core::pathFromUtf8(transaction);
    const core::Result<EntryInfo> folder = fs.stat(work);
    if (!folder.ok()) {
        return folder.error();
    }
    if (folder->kind == EntryKind::Missing) {
        return {}; // nothing to undo
    }
    if (folder->kind != EntryKind::Directory || folder->reparsePoint) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "the transaction folder is not an ordinary folder",
                               core::pathToUtf8(work));
    }

    Journal journal(fs, work / core::pathFromUtf8(kJournalName));
    const core::Result<ParsedJournal> parsed = journal.load();
    if (!parsed.ok()) {
        return parsed.error();
    }
    if (parsed->state == JournalState::Healthy) {
        return {}; // the new version proved it starts; it is not ours to undo
    }

    std::vector<Path> leftovers;
    if (parsed->state != JournalState::RolledBack) {
        JournalPlan plan = readPlan(parsed.value());
        if (parsed->state != JournalState::Empty) {
            const JournalRecord rolling{JournalOp::RollingBack, {}};
            (void)journal.append(std::span(&rolling, 1));
        }
        const core::Status restored = restoreFromDisk(fs, root, work, plan);
        // After the restore, which adds what it found under old/ and would not touch.
        reportRefusals(fs, root, transaction, plan);
        if (!restored.ok()) {
            return restored; // the folder stays: it holds the old files
        }
        const JournalRecord done{JournalOp::RolledBack, {}};
        if (parsed->state != JournalState::Empty) {
            (void)journal.append(std::span(&done, 1));
        }
    }
    detail::removeTree(fs, work, leftovers, kJournalName);
    return {};
}

} // namespace

core::Status rollback(const Path& rootGiven, const std::string& transaction,
                      const ApplyServices& services)
{
    // An exception becomes an error: recovery runs first in every program, and a name or a
    // folder somebody planted must not be able to stop one from starting.
    try {
        return rollbackTransaction(rootGiven, transaction, services);
    } catch (const std::exception& error) {
        return core::makeError(core::ErrorCode::Internal,
                               "the rollback stopped on an unexpected error", error.what());
    }
}

namespace detail {

// recover_internal.hpp says what this answers and who asks: the rule that settles an update
// whose helper died before the new version was proved, before it puts that update back. A
// rollback moves the files the journal names whatever they hold now, so a tree that somebody
// has since changed - another zip unpacked over it, a file edited - is not put back: only the
// very release the transaction installed is.
bool treeIsRelease(const Path& rootGiven, std::uint64_t sequence, const ApplyServices& services)
{
    try {
        FsOps& fs = services.fs;
        const Path root = normalizedRoot(rootGiven);
        if (root.empty()) {
            return false;
        }
        // What the install is decides which manifest it may carry (product and target), as
        // for the manifest a swap is asked to install.
        const core::Result<Install> install = readInstall(root, services.registry);
        if (!install.ok()) {
            return false;
        }
        const Path directory = installRecordDirectory(root);
        const core::Result<std::string> json =
            fs.readFile(directory / core::pathFromUtf8("installed.manifest.json"));
        const core::Result<std::string> signature =
            fs.readFile(directory / core::pathFromUtf8("installed.manifest.json.sig"));
        if (!json.ok() || !signature.ok()) {
            return false;
        }
        const core::Result<Manifest> manifest =
            verifyManifest(json.value(), signature.value(), services.trust, install.value());
        if (!manifest.ok() || manifest->sequence != sequence) {
            return false;
        }
        for (const ManifestFile& file : manifest->files) {
            if (file.fileClass == FileClass::Seed) {
                continue; // written only when absent: what is there is the person's
            }
            const Path target = under(root, file.path);
            const core::Result<EntryInfo> info = fs.stat(target);
            if (!info.ok() || info->kind != EntryKind::File || info->reparsePoint) {
                return false;
            }
            const core::Result<std::string> hash = hashFile(target);
            if (!hash.ok() || hash.value() != file.sha256) {
                return false;
            }
        }
        return true;
    } catch (const std::exception&) {
        return false; // what cannot be read is not known to be the release
    }
}

} // namespace detail

core::Status launchApply(const ApplyRequest& request)
{
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    return launchApply(request, *launcher);
}

core::Status launchApply(const ApplyRequest& request, Launcher& launcher)
{
    if (request.helper.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "no helper program to start (preflight has not run)");
    }
    SpawnRequest start;
    start.executable = request.helper;
    start.arguments = helperArguments(request);
    // Not the install folder: a program whose working directory is inside it keeps that
    // folder from being renamed or removed.
    start.workingDirectory = request.helper.parent_path();
    return launcher.startDetached(start);
}

} // namespace katana::update
