// Finishing what a crash interrupted (include/katana/update/recover.hpp).
//
// recover() is the first thing every program does, so its cost when there is nothing to do is
// the point: one stat of R/.update, and a listing and one small file per transaction folder in
// it - nothing when the folder is absent, which is every install that has never updated. (And
// one question of the process's own token, which touches no disk: whether this program was
// started as administrator, for then the helper copy in the per-user folder is not the one it
// runs. recoverWithUnguarded says why.)
//
// What it does with a transaction folder depends on where its journal stands:
//   Empty, Planned, Committing, RollingBack   the crash left it half done: rolled back.
//   CommittedPendingHealth                    the new version is in place and not yet proved:
//                                             left as it is by recover() and by
//                                             recoverInProcess - the tree is a whole version,
//                                             and the helper that owns the folder may be alive
//                                             and waiting for the very program that is asking.
//                                             When that helper died, the folder is settled by
//                                             the second half of this file (settlePending and
//                                             its callers), never by recover().
//   RolledBack, Healthy                       over. The tree is whole; what is left of the
//                                             folder (a crash part way through deleting it, a
//                                             file a scanner held at the time) is swept, so a
//                                             crash in the clean-up does not leak the copies of
//                                             a release for ever. recover() sweeps these itself,
//                                             without starting the helper: there is no tree to
//                                             put back.
//   a journal that is there and cannot be     not known to be any of these. recover() and
//   read (a scanner, a backup, the disk)      recoverInProcess pass over it - they can do no
//                                             more, and a start is not held up - but it is NOT
//                                             taken for "no such folder": to the settlers of the
//                                             second half it is an update that may be there.
//
// recover() is called by katana_cli and katana_mcp too, and a headless program writes no
// per-user state (recover.hpp, "WHAT RECOVERY WRITES, AND WHERE"). So the state folder U is
// READ here, to find the helper copy U/run-<tx>/, and nothing in it is made, changed or deleted:
// the copy that was run stays where it is, and what a sweep cannot delete is not queued in
// U/cleanup.list. A folder under R/.update that would not go needs no list: it keeps its
// journal (remove_tree.hpp) and is found and swept again at the next start. The copy is left
// for the program that sweeps U: the window a person uses, whose start lists it for removal
// from what the install's own R/.update/recovery.log names (UpdateWorkbench::start,
// src/katana_qt/update_workbench.cpp; recover.hpp says which transactions, and that the form of
// the log's lines is therefore read). Until that start one helper's size stays in U for each
// update that needed a recovery - one that a crash interrupted, and one whose rollback failed
// and which `--recover` put right later (Applier::finish, apply.cpp, keeps that copy off
// cleanup.list because the RunOnce entry runs `--recover` from it).

#include "katana/update/recover.hpp"

#include <algorithm>
#include <charconv>
#include <cstdint>
#include <exception>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <system_error>
#include <thread>
#include <utility>
#include <vector>

#include "katana/core/path_text.hpp"
#include "recover_internal.hpp"
#include "remove_tree.hpp"
#include "katana/update/journal.hpp"
#include "katana/update/install.hpp"
#include "katana/update/paths.hpp"
#include "katana/update/registry.hpp"
#include "katana/update/state.hpp"
#include "katana/update/trust.hpp"
#include "katana/update/version.hpp"

namespace katana::update {

namespace {

using Path = std::filesystem::path;
using Clock = std::chrono::steady_clock;

constexpr std::chrono::milliseconds kPoll{250};
constexpr std::chrono::seconds kHelperRunLimit{120};
constexpr std::string_view kHelperStem = "katana_updater";
constexpr std::string_view kJournalName = "journal";
constexpr std::string_view kOldName = "old";
constexpr std::string_view kLockName = "lock";

// What to run by hand, in every message that says recovery did not finish.
std::string byHand(const Path& root)
{
    return "run: katana_updater --recover \"" + core::pathToUtf8(root) + "\"";
}

bool isHeld(const core::Error& error)
{
    return error.code == core::ErrorCode::InvalidState && error.message == "held";
}

struct Found {
    std::string name;
    // Where the journal stands - or nothing, when that could not be found out: the folder could
    // not be examined, or its journal is there and would not be read (a scanner or a backup has
    // it open at a logon, the disk would not answer). That is NOT a journal that is absent, which
    // is Empty. Of such a folder nothing is known, so nothing here acts on it and nothing here
    // takes it for a folder that is not there: it may be an update that is not settled.
    std::optional<JournalState> state;
};

// A folder with no complete record at all is what a crash during planning leaves: copies and
// nothing moved. It is junk to clear, and counts as work.
bool needsWork(const Found& one)
{
    return one.state && (*one.state == JournalState::Empty || needsRecovery(*one.state));
}

bool isFinished(const Found& one)
{
    return one.state == JournalState::RolledBack || one.state == JournalState::Healthy;
}

// Installed, and never said to start (the second half of this file).
bool isUnproved(const Found& one)
{
    return one.state == JournalState::CommittedPendingHealth;
}

// Could not be looked at (see Found): it may be any of the three above.
bool isUnread(const Found& one)
{
    return !one.state.has_value();
}

// The transaction folders under R/.update, with where each journal stands, in the order of
// their names. A folder that could not be read is among them, and says so (Found::state).
core::Result<std::vector<Found>> transactions(const FsOps& fs, const Path& root)
{
    const Path directory = updateWorkDirectory(root);
    const core::Result<EntryInfo> info = fs.stat(directory);
    if (!info.ok()) {
        return info.error();
    }
    if (info->kind != EntryKind::Directory || info->reparsePoint) {
        return std::vector<Found>{};
    }
    const core::Result<std::vector<Path>> entries = fs.listDirectory(directory);
    if (!entries.ok()) {
        return entries.error();
    }
    std::vector<Found> found;
    for (const Path& entry : entries.value()) {
        const std::string name = detail::nameAsText(entry).value_or(std::string());
        if (!isValidTransactionId(name)) {
            // The lock, the recovery log, anything that is not a transaction's folder - and a
            // name that is not even text.
            continue;
        }
        const core::Result<EntryInfo> folder = fs.stat(entry);
        if (!folder.ok()) {
            found.push_back({name, std::nullopt}); // it cannot be examined: see Found
            continue;
        }
        if (folder->kind != EntryKind::Directory || folder->reparsePoint) {
            continue;
        }
        std::optional<JournalState> state = JournalState::Empty;
        const core::Result<std::string> text = fs.readFile(entry / core::pathFromUtf8(kJournalName));
        if (text.ok()) {
            state = parseJournal(text.value()).state;
        } else if (text.error().code != core::ErrorCode::NotFound) {
            // There, and not to be read just now. Reported, not dropped: dropped, an update
            // that was never said to start read as "this install holds none" - to the logon's
            // `--recover`, which then left its RunOnce entry away for good, and to the next
            // swap, which then planned a new update over it.
            state.reset();
        }
        found.push_back({name, state});
    }
    std::sort(found.begin(), found.end(),
              [](const Found& a, const Found& b) { return a.name < b.name; });
    return found;
}

// Only the ones recovery has something to do with (see the top of the file): the ones to put
// back and the ones to sweep. An update that is installed and not yet proved is not among
// them. Nor is a folder that could not be read: neither recover() nor the helper it would start
// can do more with one, and a start is not held up by it. (It is the settlers of the second
// half of this file that must not take it for nothing: settlePending and its callers.)
core::Result<std::vector<Found>> actionable(const FsOps& fs, const Path& root)
{
    core::Result<std::vector<Found>> all = transactions(fs, root);
    if (!all.ok()) {
        return all;
    }
    std::erase_if(all.value(),
                  [](const Found& one) { return !needsWork(one) && !isFinished(one); });
    return all;
}

bool anyNeedsWork(const std::vector<Found>& found)
{
    return std::any_of(found.begin(), found.end(),
                       [](const Found& one) { return needsWork(one); });
}

bool anyUnproved(const std::vector<Found>& found)
{
    return std::any_of(found.begin(), found.end(),
                       [](const Found& one) { return isUnproved(one); });
}

bool anyUnread(const std::vector<Found>& found)
{
    return std::any_of(found.begin(), found.end(),
                       [](const Found& one) { return isUnread(one); });
}

// Whether a look found nothing for a settler to do: it could be made, and it shows no update
// that was never said to start and no folder that could not be read. A look that FAILED is not
// that, and neither is a folder whose journal would not be read: "could not look" is "it may
// hold one", for whoever must not leave such an update without somebody to come back to it.
bool holdsNoneToSettle(const core::Result<std::vector<Found>>& looked)
{
    return looked.ok() && !anyUnproved(looked.value()) && !anyUnread(looked.value());
}

// Deletes a folder through FsOps; what cannot be deleted now (a file something holds open) is
// queued for the next start when there is a state folder to queue it in. Only the helper run
// for the window gives one (apply.cpp, with its `--state U`): recover() and `--recover` give an
// empty path, and write nothing per-user.
void sweep(FsOps& fs, const Path& folder, const Path& stateDirectory, std::string_view last)
{
    std::vector<Path> leftovers;
    detail::removeTree(fs, folder, leftovers, last);
    if (!stateDirectory.empty()) {
        for (const Path& leftover : leftovers) {
            (void)appendCleanup(stateDirectory, leftover, fs);
        }
    }
}

// ---- what the rule asks -------------------------------------------------------------------------

// Whether the new version of a transaction said that it starts: the marker U/healthy-<tx>
// (state.hpp), judged as the swap judges it (Applier::watchHealth, apply.cpp: it is a file).
// Unknown is neither answer - no state folder to look in, a folder that cannot be examined,
// something that is not a file under the marker's name - and decides nothing.
enum class Proof { There, Absent, Unknown };

Proof proofOf(const FsOps& fs, const Path& stateDirectory, const std::string& transaction)
{
    if (stateDirectory.empty()) {
        return Proof::Unknown;
    }
    const core::Result<EntryInfo> info = fs.stat(healthMarkerPath(stateDirectory, transaction));
    if (!info.ok()) {
        return Proof::Unknown;
    }
    if (info->kind == EntryKind::File) {
        return Proof::There;
    }
    return info->kind == EntryKind::Missing ? Proof::Absent : Proof::Unknown;
}

// What the rule reads in a transaction's journal before it puts anything back.
struct Planned {
    // The sequence of the release the transaction installed: the third argument of the PLAN
    // record (journal.hpp), read as a whole number and as nothing else.
    std::uint64_t sequence = 0;
    // The files the commit moved aside, to old/<path>: the paths of the REPLACE and REMOVE
    // records. They are what a rollback puts back.
    std::vector<std::string> setAside;
};

// The journal is not authenticated, and nothing here acts on it. All the number can do is fail
// to match the signed manifest in the tree; all a path is used for is to look whether
// old/<path> is a file; and a record the path rules refuse (paths.hpp, checkJournalPath - the
// rule a rollback holds the same records to) makes the whole reading nothing. With nothing
// read, nothing is rolled back.
std::optional<Planned> plannedBy(const FsOps& fs, const Path& work)
{
    const core::Result<std::string> text = fs.readFile(work / core::pathFromUtf8(kJournalName));
    if (!text.ok()) {
        return std::nullopt;
    }
    const ParsedJournal journal = parseJournal(text.value());
    if (journal.records.empty() || journal.records.front().op != JournalOp::Plan ||
        journal.records.front().args.size() != 3) {
        return std::nullopt;
    }
    Planned planned;
    const std::string& digits = journal.records.front().args[2];
    const auto [end, error] =
        std::from_chars(digits.data(), digits.data() + digits.size(), planned.sequence);
    if (digits.empty() || error != std::errc() || end != digits.data() + digits.size()) {
        return std::nullopt;
    }
    for (const JournalRecord& record : journal.records) {
        if (record.op != JournalOp::Replace && record.op != JournalOp::Remove) {
            continue;
        }
        if (record.args.size() != 1 || !checkJournalPath(record.args.front()).ok()) {
            return std::nullopt;
        }
        planned.setAside.push_back(record.args.front());
    }
    return planned;
}

// Whether every file the commit moved aside is still in the transaction's folder, an ordinary
// file under old/. A rollback puts back what is there (apply.cpp, restoreFromDisk), so with one
// of them gone - a clean-up that was cut off, a folder somebody emptied - it would leave the
// new version of that file among the old ones and call the mix "put back".
bool oldFilesAreKept(const FsOps& fs, const Path& work, const Planned& planned)
{
    const Path old = work / core::pathFromUtf8(kOldName);
    for (const std::string& path : planned.setAside) {
        const core::Result<EntryInfo> info = fs.stat(old / core::pathFromUtf8(path));
        if (!info.ok() || info->kind != EntryKind::File || info->reparsePoint) {
            return false;
        }
    }
    return true;
}

// Whether `stateDirectory` is the per-user folder the update was applied from: the helper that
// made the swap ran from a copy in it, U/run-<tx> (state.hpp), and that copy stays there until
// the transaction is settled. Where it is not, this run is looking in another folder than the
// one the new version writes its marker to - KATANA_UPDATE_STATE_DIR set for Katana and not
// for this run, another account - and a marker that is absent THERE says nothing about
// whether the release started.
bool isWhereTheUpdateRanFrom(const FsOps& fs, const Path& stateDirectory,
                             const std::string& transaction)
{
    const core::Result<EntryInfo> info = fs.stat(runPath(stateDirectory, transaction));
    return info.ok() && info->kind == EntryKind::Directory;
}

// True when the scan ANSWERS and names no process but this one. A scan that fails is not
// "nobody": nothing is ever put back under a program that might be running.
bool nobodyElseRuns(const ProcessOps& processes, const Path& root)
{
    const core::Result<std::vector<RunningProgram>> scanned = processes.holdersUnder(root);
    if (!scanned.ok()) {
        return false;
    }
    const std::uint64_t self = processes.currentPid();
    return std::all_of(scanned->begin(), scanned->end(),
                       [self](const RunningProgram& program) { return program.pid == self; });
}

// The rule for one transaction (recover_internal.hpp, settlePending).
void settleOne(const Path& root, const ApplyServices& services, const std::string& name,
               const Path& stateDirectory, bool mayRollBack, detail::PendingOutcome& outcome)
{
    FsOps& fs = services.fs;
    const Path work = updateWorkDirectory(root) / core::pathFromUtf8(name);
    const Proof proof = proofOf(fs, stateDirectory, name);

    if (proof == Proof::There) {
        // Settings > Apps names the new version only once it has proved it starts
        // (Applier::run): the helper that died may not have got that far, and writing the same
        // two values again changes nothing.
        const core::Result<Install> install = readInstall(root, services.registry);
        if (install.ok() && install->kind == InstallKind::Installer) {
            const std::string version = toString(install->version);
            (void)services.registry.setInstalledVersion(version, "Katana " + version);
        }
        Journal journal(fs, work / core::pathFromUtf8(kJournalName));
        const JournalRecord healthy{JournalOp::Healthy, {}};
        if (const core::Status written = journal.append(std::span(&healthy, 1)); !written.ok()) {
            // Not on disk, so not settled: the next start decides again, from the same marker.
            outcome.left.push_back(name);
            return;
        }
        // From here any start sweeps what a crash leaves of the folder (the top of the file).
        detail::removeTree(fs, work, outcome.leftovers, kJournalName);
        outcome.healthy.push_back(name);
        return;
    }

    if (!mayRollBack || proof != Proof::Absent) {
        outcome.left.push_back(name);
        return;
    }
    // "No marker" is evidence only in the folder the marker would have been written to, and a
    // rollback is whole only while every file it puts back is still kept: without either the
    // update is left as it is. Both are looked at, not assumed.
    const std::optional<Planned> planned = plannedBy(fs, work);
    if (!planned || !isWhereTheUpdateRanFrom(fs, stateDirectory, name) ||
        !oldFilesAreKept(fs, work, *planned)) {
        outcome.left.push_back(name);
        return;
    }
    // The scan is asked twice: hashing a whole install takes seconds, and a program started in
    // them must not have the files moved from under it.
    if (!nobodyElseRuns(services.processes, root) ||
        !detail::treeIsRelease(root, planned->sequence, services) ||
        !nobodyElseRuns(services.processes, root)) {
        outcome.left.push_back(name);
        return;
    }
    if (const core::Status undone = rollback(root, name, services); !undone.ok()) {
        // Either way somebody comes back to it. A rollback moves nothing from this state until
        // its ROLLING_BACK record is on disk (apply.cpp, rollbackTransaction): when it could
        // not begin - the record or the folder would not be written or read - the tree is still
        // the whole new version and the next `--recover` decides again; once the record is
        // there every start finishes it, as it finishes any rollback that was cut off
        // (journal.hpp, needsRecovery).
        outcome.incomplete.push_back(name);
        return;
    }
    outcome.rolledBack.push_back(name);
}

// Whether any folder under R/.update still has a transaction's name, whatever is in it. A
// listing that fails is "yes": the RunOnce entry is not removed on a guess.
bool anyTransactionFolder(const FsOps& fs, const Path& root)
{
    const Path directory = updateWorkDirectory(root);
    const core::Result<EntryInfo> info = fs.stat(directory);
    if (!info.ok()) {
        return true;
    }
    if (info->kind != EntryKind::Directory || info->reparsePoint) {
        return false;
    }
    const core::Result<std::vector<Path>> entries = fs.listDirectory(directory);
    if (!entries.ok()) {
        return true;
    }
    for (const Path& entry : entries.value()) {
        const std::string name = detail::nameAsText(entry).value_or(std::string());
        if (!isValidTransactionId(name)) {
            continue;
        }
        const core::Result<EntryInfo> folder = fs.stat(entry);
        if (!folder.ok() || folder->kind != EntryKind::Missing) {
            return true;
        }
    }
    return false;
}

} // namespace

namespace detail {

core::Status recoverLocked(const Path& rootGiven, const ApplyServices& services,
                           std::string_view except, const Path& stateDirectory)
{
    FsOps& fs = services.fs;
    const Path root = normalizedRoot(rootGiven);
    const core::Result<std::vector<Found>> pending = actionable(fs, root);
    if (!pending.ok()) {
        return pending.error();
    }
    std::optional<core::Error> failure;
    for (const Found& one : pending.value()) {
        if (one.name == except) {
            continue;
        }
        if (isFinished(one)) {
            sweep(fs, updateWorkDirectory(root) / core::pathFromUtf8(one.name), stateDirectory,
                  kJournalName);
            continue;
        }
        if (const core::Status undone = rollback(root, one.name, services); !undone.ok()) {
            if (!failure) {
                failure = undone.error();
            }
        }
    }
    if (failure) {
        return *failure;
    }
    return {};
}

PendingOutcome settlePending(const Path& rootGiven, const ApplyServices& services,
                             std::string_view except, const Path& stateDirectory, bool mayRollBack)
{
    PendingOutcome outcome;
    const Path root = normalizedRoot(rootGiven);
    if (root.empty()) {
        return outcome; // ".update" of the current folder is not an install's
    }
    // An exception - a name that is not text, a conversion - ends the pass with what was
    // settled until then: whatever it did not reach is as it was, and a later start comes
    // back to it. (A FaultFs's SimulatedCrash is no std::exception and goes through.) A pass
    // that was cut short, like one whose listing failed, says so: `unread` is not "none left".
    try {
        const core::Result<std::vector<Found>> all = transactions(services.fs, root);
        if (!all.ok()) {
            outcome.unread = true;
            return outcome;
        }
        for (const Found& one : all.value()) {
            if (one.name == except) {
                continue;
            }
            if (isUnread(one)) {
                // It may be an update that is not settled. Nothing is done to it, and the
                // caller hears what it hears when the listing fails: not everything was seen.
                outcome.unread = true;
            } else if (isUnproved(one)) {
                settleOne(root, services, one.name, stateDirectory, mayRollBack, outcome);
            }
        }
    } catch (const std::exception&) {
        outcome.unread = true;
    }
    return outcome;
}

void forgetSettled(FsOps& fs, const Path& stateDirectory, const PendingOutcome& outcome)
{
    if (stateDirectory.empty()) {
        return;
    }
    std::vector<Path> leftovers = outcome.leftovers;
    for (const std::string& transaction : outcome.healthy) {
        const Path marker = healthMarkerPath(stateDirectory, transaction);
        if (!fs.removeFile(marker).ok()) {
            leftovers.push_back(marker);
        }
        removeTree(fs, runPath(stateDirectory, transaction), leftovers);
    }
    for (const Path& leftover : leftovers) {
        (void)appendCleanup(stateDirectory, leftover, fs);
    }
}

namespace {

core::Status recoverWithUnguarded(const Path& rootGiven, FsOps& fs, Launcher& launcher,
                                  const Path& stateDirectory, std::chrono::milliseconds lockWait,
                                  bool elevated)
{
    const Path root = normalizedRoot(rootGiven);
    if (root.empty()) {
        // No install folder: nothing to recover, and ".update" of the current folder is not
        // an install's.
        return {};
    }
    const core::Result<std::vector<Found>> first = actionable(fs, root);
    if (!first.ok()) {
        // No way to tell: a start must not be held up by a question it cannot answer.
        return {};
    }
    const Path lockPath = updateWorkDirectory(root) / core::pathFromUtf8(kLockName);
    if (!anyNeedsWork(first.value())) {
        if (first->empty()) {
            return {};
        }
        // Only folders of transactions that are over: no tree to put back, so no helper to
        // run. Swept here, if no update is running, so that a clean-up cut short (a crash, a
        // file a scanner held) does not leave the copies of a release behind for ever.
        core::Result<std::unique_ptr<FileLock>> taken = fs.tryLock(lockPath);
        if (!taken.ok()) {
            return {}; // an update is running; it is cleaning up after itself
        }
        const core::Result<std::vector<Found>> again = actionable(fs, root);
        if (again.ok()) {
            for (const Found& one : again.value()) {
                if (isFinished(one)) {
                    // No state folder to queue in: this may be a headless program, which
                    // begins no cleanup.list. What is still held stays, under its journal, and
                    // the next start comes here again.
                    sweep(fs, updateWorkDirectory(root) / core::pathFromUtf8(one.name), Path(),
                          kJournalName);
                }
            }
        }
        return {};
    }

    // A live update holds the lock: wait for it, and look again when it lets go.
    std::vector<Found> pending = first.value();
    const Clock::time_point start = Clock::now();
    while (true) {
        core::Result<std::unique_ptr<FileLock>> taken = fs.tryLock(lockPath);
        if (taken.ok()) {
            break; // released at the end of this statement's scope: the helper takes it itself
        }
        if (!isHeld(taken.error())) {
            return core::makeError(core::ErrorCode::InvalidState,
                                   "an interrupted update was found and the update lock cannot be "
                                   "taken; " + byHand(root),
                                   taken.error().describe());
        }
        if (Clock::now() - start >= lockWait) {
            return {};
        }
        std::this_thread::sleep_for(std::min(kPoll, lockWait));
        const core::Result<std::vector<Found>> again = actionable(fs, root);
        if (!again.ok() || !anyNeedsWork(again.value())) {
            return {};
        }
        pending = again.value();
    }

    // The helper copy the interrupted update ran from, if it is still there: it is outside
    // R, so it works when R's own bin/ is half swapped. Else the install's own. Looking for
    // the copy is all that is done in the state folder.
    //
    // NOT THE COPY, when this program was started as administrator (`elevated`: process.hpp,
    // processIsElevated). The copy is in the per-user folder, which the same person's programs
    // WITHOUT those rights can write - and so is all that sends this function there: a
    // journal with one PLAN record under R/.update/<tx> (its check is no key's) and a file
    // named katana_updater in U/run-<tx>. Run from here, that file would have administrator
    // rights before this program has done anything else. `UPDATE APPLY CONFIRM` is refused in
    // such a program for the same reason (src/katana_app/update_verbs.hpp, reason=elevated);
    // this is the other way to a helper in U, and it is closed the same way. The install's own
    // helper is no new risk: it is beside the very program that is running with those rights.
    // Where that one is not there (a commit cut off between the two renames of that very
    // file), the answer is the error that names the command to run by hand.
    const Path stem = installedProgram(Path(), kHelperStem).filename();
    Path helper;
    for (const Found& one : pending) {
        if (stateDirectory.empty() || elevated) {
            break; // no state folder, so no copies in it - or none this program may run
        }
        const Path copy = runPath(stateDirectory, one.name) / stem;
        const core::Result<EntryInfo> info = fs.stat(copy);
        if (info.ok() && info->kind == EntryKind::File) {
            helper = copy;
            break;
        }
    }
    if (helper.empty()) {
        const Path own = installedProgram(root, kHelperStem);
        const core::Result<EntryInfo> info = fs.stat(own);
        if (info.ok() && info->kind == EntryKind::File) {
            helper = own;
        }
    }
    if (helper.empty()) {
        return core::makeError(core::ErrorCode::NotFound,
                               elevated
                                   ? "an interrupted update was found, and Katana was started as "
                                     "administrator, so only the install's own update helper may "
                                     "put it back, and that one is not there; start Katana the "
                                     "ordinary way, or " + byHand(root)
                                   : "an interrupted update was found and there is no update "
                                     "helper to put it back with; " + byHand(root));
    }

    SpawnRequest request;
    request.executable = helper;
    request.arguments = {"--recover", core::pathToUtf8(root)};
    request.workingDirectory = helper.parent_path();
    request.timeout = std::chrono::duration_cast<std::chrono::milliseconds>(kHelperRunLimit);
    const core::Result<SpawnResult> ran = launcher.run(request);
    if (!ran.ok()) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "an interrupted update was found and the update helper could not be "
                               "started; " + byHand(root),
                               ran.error().describe());
    }
    if (ran->timedOut || ran->exitCode != 0) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "an interrupted update could not be put back; " + byHand(root),
                               ran->timedOut ? "the helper did not finish in time"
                                             : "the helper exited with code " +
                                                   std::to_string(ran->exitCode));
    }

    // The helper has exited and put everything back. Its copy U/run-<tx>/ is of no use any
    // more, and it STAYS: deleting it, or listing it in U/cleanup.list, would be a headless
    // program changing per-user state (recover.hpp). The window's start lists it (the top of
    // this file says from what).
    return {};
}

// The window that confirms has no process scan, no launcher and no keys: it never puts
// anything back (settlePending is told so), and these stand in the services it hands the rule
// so that, were the rule ever to ask, the answer is a refusal and not a guess.
class NoProcesses final : public ProcessOps {
  public:
    [[nodiscard]] std::uint64_t currentPid() const override { return 0; }
    [[nodiscard]] bool isAlive(std::uint64_t) const override { return true; }
    [[nodiscard]] core::Result<std::vector<RunningProgram>> holdersUnder(const Path&) const override
    {
        return core::makeError(core::ErrorCode::Unsupported,
                               "a confirmation asks nobody who is running");
    }
};

class NoLauncher final : public Launcher {
  public:
    [[nodiscard]] core::Result<SpawnResult> run(const SpawnRequest&) override
    {
        return core::makeError(core::ErrorCode::Unsupported, "a confirmation starts no program");
    }
    [[nodiscard]] core::Status startDetached(const SpawnRequest&) override
    {
        return core::makeError(core::ErrorCode::Unsupported, "a confirmation starts no program");
    }
};

Confirmed confirmStartedUnguarded(const Path& rootGiven, const Path& stateDirectory, FsOps& fs,
                                  RegistryOps& registry)
{
    const Path root = normalizedRoot(rootGiven);
    if (root.empty() || stateDirectory.empty()) {
        return Confirmed::Nothing;
    }
    const core::Result<std::vector<Found>> first = transactions(fs, root);
    if (!first.ok()) {
        // Could not look, which is not "there is none": a folder that will not be listed just
        // now may hold the update this window is the proof of. It asks again.
        return Confirmed::Busy;
    }
    if (holdsNoneToSettle(first)) {
        return Confirmed::Nothing; // every start but the one after a helper died
    }
    // One attempt. A lock that is held is a helper that is alive and doing its own health run
    // (or a scanner, for a moment): the caller asks again, it does not wait.
    core::Result<std::unique_ptr<FileLock>> taken =
        fs.tryLock(updateWorkDirectory(root) / core::pathFromUtf8(kLockName));
    if (!taken.ok()) {
        return isHeld(taken.error()) ? Confirmed::Busy : Confirmed::Nothing;
    }
    const std::unique_ptr<FileLock> lock = std::move(taken).value();

    // Under the lock, look again: the helper that held it may have settled its own.
    const core::Result<std::vector<Found>> pending = transactions(fs, root);
    if (!pending.ok()) {
        return Confirmed::Busy; // as above: not seen is not settled
    }
    if (holdsNoneToSettle(pending)) {
        return Confirmed::Nothing;
    }
    // The proof first, and on disk before anything is settled by it: this window is at its
    // first idle turn, which is what the health run is started to reach. Cut off after this
    // line, the update is settled healthy by whoever comes next - the logon's `--recover`
    // too, which would otherwise put a working release back. Only for a journal that was READ
    // and says the update is pending: a folder whose journal would not be read may be one a
    // crash cut off in its commit, and this window is no proof of that. The rule below says
    // of such a folder that it was not seen, and the window asks again.
    for (const Found& one : pending.value()) {
        if (!isUnproved(one) || proofOf(fs, stateDirectory, one.name) != Proof::Absent) {
            continue; // written already, by the health run of the helper that died
        }
        if (fs.makeDirectories(stateDirectory).ok()) {
            (void)fs.writeDurable(healthMarkerPath(stateDirectory, one.name), "ok\n");
        }
    }

    NoProcesses processes;
    NoLauncher launcher;
    const Trust nobody{};
    const ApplyServices services{fs, processes, launcher, registry, nobody, ApplyTimings{}};
    const PendingOutcome outcome = settlePending(root, services, {}, stateDirectory, false);
    forgetSettled(fs, stateDirectory, outcome);
    if (!outcome.left.empty() || outcome.unread || outcome.healthy.empty()) {
        // The marker or the record could not be written just now, or the folder could not be
        // read again: something is not settled, or may not be, so the logon entry stays, and
        // the window asks again.
        return Confirmed::Busy;
    }
    // The command it names runs the helper copy that has just been deleted.
    (void)registry.clearRunOnce(kRunOnceName);
    return Confirmed::Done;
}

} // namespace

// The entry of recover(): an exception from a name or a folder somebody else can write becomes
// an error to report, never a program that dies in its first lines at every start.
core::Status recoverWith(const Path& rootGiven, FsOps& fs, Launcher& launcher,
                         const Path& stateDirectory, std::chrono::milliseconds lockWait,
                         bool elevated)
{
    try {
        return recoverWithUnguarded(rootGiven, fs, launcher, stateDirectory, lockWait, elevated);
    } catch (const std::exception& error) {
        return core::makeError(core::ErrorCode::Internal,
                               "looking for an interrupted update stopped on an unexpected error",
                               error.what());
    }
}

// A window that cannot confirm goes on being a window: an exception is "nothing was done".
Confirmed confirmStartedWith(const Path& rootGiven, const Path& stateDirectory, FsOps& fs,
                             RegistryOps& registry)
{
    try {
        return confirmStartedUnguarded(rootGiven, stateDirectory, fs, registry);
    } catch (const std::exception&) {
        return Confirmed::Nothing;
    }
}

} // namespace detail

namespace {

core::Status recoverInProcessUnguarded(const Path& rootGiven, const ApplyServices& services)
{
    if (rootGiven.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "no install folder to recover: --recover needs one", "");
    }
    FsOps& fs = services.fs;
    const Path root = detail::normalizedRoot(rootGiven);
    const core::Result<std::vector<Found>> first = actionable(fs, root);
    if (!first.ok()) {
        return first.error();
    }
    if (first->empty()) {
        return {}; // the cheap answer, without touching the lock
    }

    // A live holder is an update running now: wait for it, never touch its files. The wait
    // is kRecoverWait, shortened by a test through the timings.
    const std::chrono::milliseconds wait =
        std::min<std::chrono::milliseconds>(kRecoverWait, services.timings.waitForHolders);
    const Path lockPath = updateWorkDirectory(root) / core::pathFromUtf8(kLockName);
    std::unique_ptr<FileLock> lock;
    const Clock::time_point start = Clock::now();
    while (true) {
        core::Result<std::unique_ptr<FileLock>> taken = fs.tryLock(lockPath);
        if (taken.ok()) {
            lock = std::move(taken).value();
            break;
        }
        if (!isHeld(taken.error())) {
            return core::makeError(core::ErrorCode::InvalidState,
                                   "an interrupted update was found and the update lock cannot be "
                                   "taken; " + byHand(root),
                                   taken.error().describe());
        }
        if (Clock::now() - start >= wait) {
            return {}; // not ours to touch
        }
        std::this_thread::sleep_for(services.timings.pollInterval);
    }

    // Under the lock, look again: the holder we waited for may have finished the job.
    const core::Status undone = detail::recoverLocked(root, services, {}, Path());
    if (!undone.ok()) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "an interrupted update could not be put back; " + byHand(root),
                               undone.error().describe());
    }
    (void)services.registry.clearRunOnce(kRunOnceName);
    return {};
}

Settled settleUnprovedUnguarded(const Path& rootGiven, const Path& helper,
                                const Path& evidenceDirectory, const ApplyServices& services)
{
    FsOps& fs = services.fs;
    const Path root = detail::normalizedRoot(rootGiven);
    if (root.empty()) {
        return Settled::Nothing;
    }
    const core::Result<std::vector<Found>> first = transactions(fs, root);
    if (holdsNoneToSettle(first)) {
        return Settled::Nothing; // the cheap answer, without touching the lock
    }
    // From here the install holds such an update - or MAY: R/.update would not be listed, or a
    // folder in it, or its journal, would not be read (a scanner or a backup at the logon).
    // That is not "none". Started by the RunOnce entry, this run is the last one Windows starts
    // unless the entry is put back; taken for "none", an update whose release does not start
    // would have nobody come back to it. So it goes the way of one that is there: the lock,
    // the entry, the rule - which says that it could not look, and the answer is Left.

    // The logon net: `<helper> --recover <R>`, the very text the swap registered. Best effort,
    // like the swap's own registration. (And one value for every copy of Katana a person runs:
    // the name is kRunOnceName whatever R is, so a swap or a confirmation in ANOTHER copy
    // replaces or removes this entry, and this copy's net is then gone until its next
    // `--recover`. Known, and not changed here: the uninstaller removes the value by that name.)
    const auto rearm = [&] {
        if (!helper.empty()) {
            (void)services.registry.setRunOnce(kRunOnceName,
                                               detail::runOnceCommand(helper, root));
        }
    };

    // The lock is how a dead helper is told from a live one: a helper holds it from its first
    // step to its last, and it goes with the process. Polled as recoverInProcess polls it - one
    // attempt would lose to a scanner that has the lock file open for a moment - and a lock
    // that stays held is a health run in progress, which is not this run's to judge.
    const std::chrono::milliseconds wait =
        std::min<std::chrono::milliseconds>(kRecoverWait, services.timings.waitForHolders);
    const Path lockPath = updateWorkDirectory(root) / core::pathFromUtf8(kLockName);
    std::unique_ptr<FileLock> lock;
    const Clock::time_point start = Clock::now();
    while (true) {
        core::Result<std::unique_ptr<FileLock>> taken = fs.tryLock(lockPath);
        if (taken.ok()) {
            lock = std::move(taken).value();
            break;
        }
        if (!isHeld(taken.error())) {
            // No lock, so no way to know that nobody is working on it: it is left. But not
            // without the logon net - a lock that is HELD is a helper that is alive and whose
            // registry this is not, and this is not that: the lock file cannot be opened at all
            // (a scanner that answers "access denied", a folder that will not be written just
            // now). Started by the RunOnce entry, this run is the last one Windows starts unless
            // the entry is put back, and then nothing would ever come to settle the update.
            rearm();
            return Settled::Left;
        }
        if (Clock::now() - start >= wait) {
            return Settled::Busy;
        }
        std::this_thread::sleep_for(services.timings.pollInterval);
    }

    // Under the lock, look again: the helper that was waited for may have settled its own. A
    // look that fails now, or a folder that will not be read, is not "it has": the rule below
    // looks once more and says so.
    const core::Result<std::vector<Found>> pending = transactions(fs, root);
    if (holdsNoneToSettle(pending)) {
        return Settled::Nothing;
    }

    // FIRST the logon net. When this run was started by the RunOnce entry, Windows has taken
    // the entry away already; were the machine to lose power in the lines below, no logon
    // would come back here.
    rearm();

    const detail::PendingOutcome outcome =
        detail::settlePending(root, services, {}, evidenceDirectory, true);

    // The entry goes when there is nothing left for a logon to do; a folder that is still
    // there - left undecided, half put back, or held by a scanner while it was deleted - is
    // what the next logon comes for.
    if (!anyTransactionFolder(fs, root)) {
        (void)services.registry.clearRunOnce(kRunOnceName);
    }

    if (!outcome.incomplete.empty()) {
        return Settled::RollbackIncomplete;
    }
    if (!outcome.left.empty() || outcome.unread) {
        return Settled::Left;
    }
    if (!outcome.rolledBack.empty()) {
        return Settled::RolledBack;
    }
    return outcome.healthy.empty() ? Settled::Nothing : Settled::Healthy;
}

} // namespace

core::Status recoverInProcess(const Path& root, const ApplyServices& services)
{
    try {
        return recoverInProcessUnguarded(root, services);
    } catch (const std::exception& error) {
        return core::makeError(core::ErrorCode::Internal,
                               "looking for an interrupted update stopped on an unexpected error",
                               error.what());
    }
}

Settled settleUnproved(const Path& root, const Path& helper, const Path& evidenceDirectory,
                       const ApplyServices& services)
{
    // An exception before anything was decided leaves the update as it was. (One inside a
    // rollback never arrives here: rollback() turns it into its own error.)
    try {
        return settleUnprovedUnguarded(root, helper, evidenceDirectory, services);
    } catch (const std::exception&) {
        return Settled::Left;
    }
}

Confirmed confirmStarted(const Path& root, const Path& stateDirectory)
{
    try {
        const std::unique_ptr<FsOps> fs = makeSystemFsOps();
        const std::unique_ptr<RegistryOps> registry = makeSystemRegistryOps();
        return detail::confirmStartedWith(root, stateDirectory, *fs, *registry);
    } catch (const std::exception&) {
        return Confirmed::Nothing;
    }
}

core::Status recover(const Path& root)
{
    try {
        const std::unique_ptr<FsOps> fs = makeSystemFsOps();
        const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
        return detail::recoverWith(root, *fs, *launcher, stateDirectory(), kRecoverWait,
                                   processIsElevated());
    } catch (const std::exception& error) {
        return core::makeError(core::ErrorCode::Internal,
                               "looking for an interrupted update stopped on an unexpected error",
                               error.what());
    }
}

} // namespace katana::update
