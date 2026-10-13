// Finishing what a crash interrupted (include/katana/update/recover.hpp).
//
// recover() is the first thing every program does, so its cost when there is nothing to do is
// the point: one stat of R/.update, and a listing and one small file per transaction folder in
// it - nothing when the folder is absent, which is every install that has never updated.
//
// What it does with a transaction folder depends on where its journal stands:
//   Empty, Planned, Committing, RollingBack   the crash left it half done: rolled back.
//   CommittedPendingHealth                    the new version is in place and not yet proved:
//                                             left as it is - the tree is a whole version, and
//                                             the folder is what a health rollback needs. (When
//                                             its helper died, a LATER update that proves
//                                             healthy deletes it: apply.cpp, sweepSuperseded.)
//   RolledBack, Healthy                       over. The tree is whole; what is left of the
//                                             folder (a crash part way through deleting it, a
//                                             file a scanner held at the time) is swept, so a
//                                             crash in the clean-up does not leak the copies of
//                                             a release for ever. recover() sweeps these itself,
//                                             without starting the helper: there is no tree to
//                                             put back.
//
// recover() is called by katana_cli and katana_mcp too, and a headless program writes no
// per-user state (recover.hpp, "WHAT RECOVERY WRITES, AND WHERE"). So the state folder U is
// READ here, to find the helper copy U/run-<tx>/, and nothing in it is made, changed or deleted:
// the copy that was run stays where it is, and what a sweep cannot delete is not queued in
// U/cleanup.list. A folder under R/.update that would not go needs no list: it keeps its
// journal (remove_tree.hpp) and is found and swept again at the next start. The copy is left
// for a program that may write per-user state, and NOTHING REMOVES IT YET: one helper's size
// stays in U for each update that a crash interrupted.

#include "katana/update/recover.hpp"

#include <algorithm>
#include <exception>
#include <thread>

#include "katana/core/path_text.hpp"
#include "recover_internal.hpp"
#include "remove_tree.hpp"
#include "katana/update/journal.hpp"
#include "katana/update/install.hpp"
#include "katana/update/state.hpp"

namespace katana::update {

namespace {

using Path = std::filesystem::path;
using Clock = std::chrono::steady_clock;

constexpr std::chrono::milliseconds kPoll{250};
constexpr std::chrono::seconds kHelperRunLimit{120};
constexpr std::string_view kHelperStem = "katana_updater";

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
    JournalState state = JournalState::Empty;
};

// A folder with no complete record at all is what a crash during planning leaves: copies and
// nothing moved. It is junk to clear, and counts as work.
bool needsWork(JournalState state)
{
    return state == JournalState::Empty || needsRecovery(state);
}

bool isFinished(JournalState state)
{
    return state == JournalState::RolledBack || state == JournalState::Healthy;
}

// The transaction folders under R/.update, with where each journal stands. Only the ones
// that need something done are returned (see the top of the file).
core::Result<std::vector<Found>> actionable(const FsOps& fs, const Path& root)
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
        if (!folder.ok() || folder->kind != EntryKind::Directory || folder->reparsePoint) {
            continue;
        }
        JournalState state = JournalState::Empty;
        const core::Result<std::string> text = fs.readFile(entry / "journal");
        if (text.ok()) {
            state = parseJournal(text.value()).state;
        } else if (text.error().code != core::ErrorCode::NotFound) {
            continue; // unreadable: neither this nor the helper can do more
        }
        if (needsWork(state) || isFinished(state)) {
            found.push_back({name, state});
        }
    }
    std::sort(found.begin(), found.end(),
              [](const Found& a, const Found& b) { return a.name < b.name; });
    return found;
}

bool anyNeedsWork(const std::vector<Found>& found)
{
    return std::any_of(found.begin(), found.end(),
                       [](const Found& one) { return needsWork(one.state); });
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
        if (isFinished(one.state)) {
            sweep(fs, updateWorkDirectory(root) / core::pathFromUtf8(one.name), stateDirectory,
                  "journal");
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

namespace {

core::Status recoverWithUnguarded(const Path& rootGiven, FsOps& fs, Launcher& launcher,
                                  const Path& stateDirectory, std::chrono::milliseconds lockWait)
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
    const Path lockPath = updateWorkDirectory(root) / "lock";
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
                if (isFinished(one.state)) {
                    // No state folder to queue in: this may be a headless program, which
                    // begins no cleanup.list. What is still held stays, under its journal, and
                    // the next start comes here again.
                    sweep(fs, updateWorkDirectory(root) / core::pathFromUtf8(one.name), Path(),
                          "journal");
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
    const Path stem = installedProgram(Path(), kHelperStem).filename();
    Path helper;
    for (const Found& one : pending) {
        if (stateDirectory.empty()) {
            break; // no state folder, so no copies in it
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
                               "an interrupted update was found and there is no update helper to "
                               "put it back with; " + byHand(root));
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
    // program changing per-user state (recover.hpp; the top of this file says what that costs).
    return {};
}

} // namespace

// The entry of recover(): an exception from a name or a folder somebody else can write becomes
// an error to report, never a program that dies in its first lines at every start.
core::Status recoverWith(const Path& rootGiven, FsOps& fs, Launcher& launcher,
                         const Path& stateDirectory, std::chrono::milliseconds lockWait)
{
    try {
        return recoverWithUnguarded(rootGiven, fs, launcher, stateDirectory, lockWait);
    } catch (const std::exception& error) {
        return core::makeError(core::ErrorCode::Internal,
                               "looking for an interrupted update stopped on an unexpected error",
                               error.what());
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
    const Path lockPath = updateWorkDirectory(root) / "lock";
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

core::Status recover(const Path& root)
{
    try {
        const std::unique_ptr<FsOps> fs = makeSystemFsOps();
        const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
        return detail::recoverWith(root, *fs, *launcher, stateDirectory(), kRecoverWait);
    } catch (const std::exception& error) {
        return core::makeError(core::ErrorCode::Internal,
                               "looking for an interrupted update stopped on an unexpected error",
                               error.what());
    }
}

} // namespace katana::update
