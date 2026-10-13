// The deterministic fault injection of the update (docs/update.md): FaultFs, and the three loops
// that the swap's safety rests on.
//
//   1. The apply of a 40-file release is crashed before EVERY one of its file system operations;
//      then recover() runs on what the crash left, and the whole tree must hash to the old
//      version's or the new one's - never a mix.
//   2. The same, with the crash INSIDE the recovery, at every one of its operations, from several
//      kinds of interrupted state (and again inside the second recovery).
//   3. An error 32 (a locked file) injected at every rename of the commit: the update must end
//      rolled back, and the tree must be the old one exactly.
//
// The expectation of each loop is not read from the swap. The two hashes are of trees built
// from the specification of the releases, in their own folders (apply_harness.hpp). A loop
// that crashed nowhere would pass trivially, so each shows that it reached what it claims:
// every k crashed, at the operation numbered k, and the crashes cover every state the journal
// can be in.

#include <gtest/gtest.h>

#include <algorithm>
#include <atomic>
#include <iostream>
#include <thread>
#include <type_traits>

#include "apply_harness.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#endif

using namespace katana;
using namespace katana::update;
using namespace katana::update::harness;

namespace {

} // namespace

// ---- FaultFs itself --------------------------------------------------------------------------------------

TEST(FaultFs, TheEightChangingOperationsAreCountedAndNothingElseIs)
{
    Scratch scratch("faultfs-count");
    StdFs disk;
    FaultFs faults(disk);
    const Path dir = scratch.path() / "d";
    writeText(scratch.path() / "existing", "x");

    // Looking is not counted.
    (void)faults.stat(scratch.path() / "existing");
    (void)faults.listDirectory(scratch.path());
    (void)faults.readFile(scratch.path() / "existing");
    (void)faults.resolve(scratch.path());
    (void)faults.freeSpace(scratch.path());
    (void)faults.syncDirectory(scratch.path());
    {
        const auto lock = faults.tryLock(scratch.path() / "lock");
        ASSERT_TRUE(lock.ok());
    }
    EXPECT_EQ(faults.mutatingOperations(), 0u);

    // The eight that change.
    ASSERT_TRUE(faults.makeDirectories(dir).ok());
    EXPECT_EQ(faults.mutatingOperations(), 1u);
    ASSERT_TRUE(faults.writeDurable(dir / "a", "A").ok());
    EXPECT_EQ(faults.mutatingOperations(), 2u);
    ASSERT_TRUE(faults.appendDurable(dir / "a", "B").ok());
    EXPECT_EQ(faults.mutatingOperations(), 3u);
    ASSERT_TRUE(faults.copyFile(dir / "a", dir / "b").ok());
    EXPECT_EQ(faults.mutatingOperations(), 4u);
    ASSERT_TRUE(faults.renameFile(dir / "b", dir / "c").ok());
    EXPECT_EQ(faults.mutatingOperations(), 5u);
    ASSERT_TRUE(faults.setExecutable(dir / "c").ok());
    EXPECT_EQ(faults.mutatingOperations(), 6u);
    ASSERT_TRUE(faults.removeFile(dir / "c").ok());
    EXPECT_EQ(faults.mutatingOperations(), 7u);
    ASSERT_TRUE(faults.removeEmptyDirectory(scratch.path() / "nothing-here").ok());
    EXPECT_EQ(faults.mutatingOperations(), 8u);
    EXPECT_EQ(readText(dir / "a"), "AB") << "and each did its work";
}

TEST(FaultFs, ACrashBeforeTheKthOperationPerformsNeitherItNorAnythingAfterAndKeepsWhatCameBefore)
{
    Scratch scratch("faultfs-crash");
    StdFs disk;
    FaultFs faults(disk);
    faults.crashBefore(3);
    ASSERT_TRUE(faults.makeDirectories(scratch.path() / "d").ok());          // 1
    ASSERT_TRUE(faults.writeDurable(scratch.path() / "d" / "a", "A").ok());  // 2
    try {
        (void)faults.writeDurable(scratch.path() / "d" / "b", "B");          // 3: dies here
        FAIL() << "no crash";
    } catch (const SimulatedCrash& crash) {
        EXPECT_EQ(crash.operation, 3u);
        EXPECT_NE(crash.description.find("writeDurable"), std::string::npos) << crash.description;
        EXPECT_NE(crash.description.find("b"), std::string::npos);
    }
    EXPECT_EQ(faults.mutatingOperations(), 2u) << "the one that crashed is not counted as performed";
    EXPECT_TRUE(isPresent(scratch.path() / "d" / "a")) << "earlier work stays on disk";
    EXPECT_FALSE(isPresent(scratch.path() / "d" / "b")) << "the crashed operation did not happen";
}

TEST(FaultFs, EveryKindOfChangeCanBeTheOneThatCrashes)
{
    Scratch scratch("faultfs-kinds");
    StdFs disk;
    writeText(scratch.path() / "from", "F");
    fs::create_directories(scratch.path() / "emptydir");
    const std::vector<std::pair<std::string, std::function<void(FaultFs&)>>> operations = {
        {"makeDirectories", [&](FaultFs& f) { (void)f.makeDirectories(scratch.path() / "newdir"); }},
        {"copyFile", [&](FaultFs& f) { (void)f.copyFile(scratch.path() / "from", scratch.path() / "copy"); }},
        {"renameFile", [&](FaultFs& f) { (void)f.renameFile(scratch.path() / "from", scratch.path() / "moved"); }},
        {"removeFile", [&](FaultFs& f) { (void)f.removeFile(scratch.path() / "from"); }},
        {"removeEmptyDirectory", [&](FaultFs& f) { (void)f.removeEmptyDirectory(scratch.path() / "emptydir"); }},
        {"appendDurable", [&](FaultFs& f) { (void)f.appendDurable(scratch.path() / "from", "more"); }},
        {"writeDurable", [&](FaultFs& f) { (void)f.writeDurable(scratch.path() / "from", "over"); }},
        {"setExecutable", [&](FaultFs& f) { (void)f.setExecutable(scratch.path() / "from"); }},
    };
    for (const auto& [name, operation] : operations) {
        FaultFs faults(disk);
        faults.crashBefore(1);
        try {
            operation(faults);
            ADD_FAILURE() << name << " did not crash";
        } catch (const SimulatedCrash& crash) {
            EXPECT_EQ(crash.description.rfind(name, 0), 0u) << crash.description;
        }
    }
    // Nothing happened to the disk in any of them.
    EXPECT_EQ(readText(scratch.path() / "from"), "F");
    EXPECT_FALSE(isPresent(scratch.path() / "newdir"));
    EXPECT_FALSE(isPresent(scratch.path() / "copy"));
    EXPECT_FALSE(isPresent(scratch.path() / "moved"));
    EXPECT_TRUE(fs::is_directory(scratch.path() / "emptydir"));
}

TEST(FaultFs, ACrashIsNotAnErrorACallerCouldCatchAndCarryOn)
{
    static_assert(!std::is_base_of_v<std::exception, SimulatedCrash>,
                  "a catch (const std::exception&) must not swallow a simulated death");
    static_assert(!std::is_convertible_v<SimulatedCrash, core::Error>);
    Scratch scratch("faultfs-uncatchable");
    StdFs disk;
    FaultFs faults(disk);
    faults.crashBefore(1);
    bool swallowed = false;
    bool reached = false;
    try {
        try {
            (void)faults.makeDirectories(scratch.path() / "d");
        } catch (const std::exception&) {
            swallowed = true;
        }
    } catch (const SimulatedCrash&) {
        reached = true;
    }
    EXPECT_FALSE(swallowed);
    EXPECT_TRUE(reached);
}

TEST(FaultFs, TheKthRenameFailsWithTheOsErrorAndIsNotPerformed)
{
    Scratch scratch("faultfs-rename");
    StdFs disk;
    FaultFs faults(disk);
    faults.failRenameWith(2, 32);
    writeText(scratch.path() / "a", "A");
    writeText(scratch.path() / "b", "B");
    writeText(scratch.path() / "c", "C");

    EXPECT_TRUE(faults.renameFile(scratch.path() / "a", scratch.path() / "a2").ok());      // the 1st
    // Other operations in between are not renames and do not count as one.
    ASSERT_TRUE(faults.writeDurable(scratch.path() / "w", "w").ok());
    const core::Status failed = faults.renameFile(scratch.path() / "b", scratch.path() / "b2"); // the 2nd
    ASSERT_FALSE(failed.ok());
    EXPECT_EQ(failed.error().context.rfind("os_error=32", 0), 0u) << failed.error().describe();
    EXPECT_TRUE(isPresent(scratch.path() / "b")) << "not performed";
    EXPECT_FALSE(isPresent(scratch.path() / "b2"));
    EXPECT_TRUE(faults.renameFile(scratch.path() / "c", scratch.path() / "c2").ok());      // the 3rd
    EXPECT_TRUE(isPresent(scratch.path() / "c2"));
    // The failed rename was an operation like the others for the count of operations.
    EXPECT_EQ(faults.mutatingOperations(), 4u);
}

TEST(FaultFs, AnotherOsErrorNumberIsCarriedThrough)
{
    Scratch scratch("faultfs-rename-5");
    StdFs disk;
    FaultFs faults(disk);
    faults.failRenameWith(1, 5);
    writeText(scratch.path() / "a", "A");
    const core::Status failed = faults.renameFile(scratch.path() / "a", scratch.path() / "z");
    ASSERT_FALSE(failed.ok());
    EXPECT_EQ(failed.error().context.rfind("os_error=5 ", 0), 0u);
}

TEST(FaultFs, ResetClearsBothSettingsAndTheCounters)
{
    Scratch scratch("faultfs-reset");
    StdFs disk;
    FaultFs faults(disk);
    faults.crashBefore(2);
    faults.failRenameWith(1, 32);
    ASSERT_TRUE(faults.makeDirectories(scratch.path() / "d").ok());
    EXPECT_EQ(faults.mutatingOperations(), 1u);
    faults.reset();
    EXPECT_EQ(faults.mutatingOperations(), 0u);
    writeText(scratch.path() / "a", "A");
    EXPECT_TRUE(faults.writeDurable(scratch.path() / "d" / "x", "x").ok()) << "no crash after the reset";
    EXPECT_TRUE(faults.renameFile(scratch.path() / "a", scratch.path() / "b").ok())
        << "no failure after the reset";
}

TEST(FaultFs, ALogAboveItNumbersTheOperationsTheWayItDoes)
{
    // The loops name "the k-th operation" from a LogFs over a FaultFs: the two must agree.
    Scratch scratch("faultfs-log");
    StdFs disk;
    FaultFs faults(disk);
    LogFs log(faults);
    writeText(scratch.path() / "a", "A");
    ASSERT_TRUE(log.makeDirectories(scratch.path() / "d").ok());
    ASSERT_TRUE(log.copyFile(scratch.path() / "a", scratch.path() / "d" / "b").ok());
    ASSERT_TRUE(log.renameFile(scratch.path() / "d" / "b", scratch.path() / "d" / "c").ok());
    ASSERT_TRUE(log.setExecutable(scratch.path() / "d" / "c").ok());
    ASSERT_TRUE(log.appendDurable(scratch.path() / "d" / "j", "r").ok());
    ASSERT_TRUE(log.writeDurable(scratch.path() / "d" / "w", "w").ok());
    ASSERT_TRUE(log.removeFile(scratch.path() / "d" / "c").ok());
    ASSERT_TRUE(log.removeEmptyDirectory(scratch.path() / "d2").ok());
    EXPECT_EQ(log.log.size(), faults.mutatingOperations());
    EXPECT_EQ(log.find("renameFile"), 3u);
    EXPECT_EQ(log.find("appendDurable", "r"), 5u);
}

// ---- running a loop on several threads ----------------------------------------------------------------------

namespace {

unsigned workerCount()
{
    const unsigned cores = std::thread::hardware_concurrency();
    return std::clamp(cores == 0 ? 2u : cores, 2u, 8u);
}

using Build = std::function<void(Scenario&)>;

// Runs body(loop, item, state) for item = first, first + stride, ... up to last, spread over
// several threads. Each thread has a scenario of its own - its own install, stage and state
// folders, built from the same specification, so every iteration starts from the same bytes -
// and a state object of the caller's; nothing is shared but the counter that hands out the
// next item (and the locks StdFs holds, which are per path). The iterations are independent
// by construction, which is what lets the several hundred of a loop take a minute and not
// five. `body` returns false after a failure and everyone stops.
template <typename State, typename Body>
void forEachItem(const std::string& label, const Build& build, std::uint64_t first,
                 std::uint64_t last, std::uint64_t stride, std::vector<State>& states, Body body)
{
    std::atomic<std::uint64_t> next{first};
    std::atomic<bool> failed{false};
    std::vector<std::thread> threads;
    for (std::size_t worker = 0; worker < states.size(); ++worker) {
        threads.emplace_back([&, worker] {
            Loop loop(label + "-" + std::to_string(worker), build);
            while (!failed.load()) {
                const std::uint64_t item = next.fetch_add(stride);
                if (item > last) {
                    return;
                }
                if (!body(loop, item, states[worker])) {
                    failed = true;
                }
            }
        });
    }
    for (std::thread& thread : threads) {
        thread.join();
    }
}

} // namespace

// ---- loop 1: crash before every operation ------------------------------------------------------------------

namespace {

struct CrashCounts {
    std::map<std::string, int> byState; // what the journal said at each crash
    std::uint64_t crashes = 0;
    std::uint64_t old = 0;
    std::uint64_t fresh = 0;
};

// Crashes the apply before operation k, for every k, and requires recover() to leave the old
// tree or the new one - and, the strong form, the one the clean run's log of operations says:
// the old one unless the crash came after the COMMITTED_PENDING_HEALTH record was written.
void crashLoop(const std::string& label, const Build& build, std::uint64_t stride,
               std::uint64_t minimumOperations)
{
    Loop sizing(label + "-sizing", build);
    Scenario& s = sizing.scenario();
    ASSERT_NE(s.v1Hash, s.v2Hash);

    // Size the loop with one apply that has nothing wrong with it. It also proves the
    // expectation: the update, run to its end, leaves the tree built from the specification.
    std::uint64_t total = 0;
    std::uint64_t pendingRecord = 0; // the operation that writes COMMITTED_PENDING_HEALTH
    {
        sizing.reset();
        StdFs disk;
        FaultFs counting(disk);
        LogFs log(counting);
        ApplyResult result;
        ASSERT_FALSE(applyUntilCrash(s, log, &result).has_value());
        ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
        total = counting.mutatingOperations();
        ASSERT_EQ(log.log.size(), total);
        pendingRecord = log.find("appendDurable", "COMMITTED_PENDING_HEALTH");
        ASSERT_GT(pendingRecord, 0u);
        ASSERT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
    }
    ASSERT_GE(total, minimumOperations) << "an apply of this size does more than that";

    std::vector<CrashCounts> counts(workerCount());
    forEachItem(label, build, 1, total, stride, counts,
                [&](Loop& loop, std::uint64_t k, CrashCounts& c) -> bool {
        Scenario& t = loop.scenario();
        loop.reset();
        StdFs disk;
        FaultFs faults(disk);
        faults.crashBefore(k);
        const std::optional<SimulatedCrash> crash = applyUntilCrash(t, faults);
        if (!crash) {
            ADD_FAILURE() << "no crash before operation " << k << " of " << total;
            return false;
        }
        ++c.crashes;
        if (crash->operation != k || faults.mutatingOperations() != k - 1) {
            ADD_FAILURE() << "the crash was at operation " << crash->operation << ", not " << k;
            return false;
        }
        const JournalState state = journalStateOf(t.root);
        ++c.byState[stateName(state)];

        const core::Status recovered = recoverClean(t);
        if (!recovered.ok()) {
            ADD_FAILURE() << "recovery failed after the crash before operation " << k << " ("
                          << crash->description << "): " << recovered.error().describe();
            return false;
        }
        const std::string hash = treeHash(t.root);
        // Operations before k were performed; the pending record is one of them or it is not.
        const bool committed = k > pendingRecord;
        if (hash != t.v1Hash && hash != t.v2Hash) {
            ADD_FAILURE() << "after the crash before operation " << k << " (" << crash->description
                          << ", journal " << stateName(state) << ") the tree is neither version:\n"
                          << "versus the old: " << difference(t.v1Snapshot, snapshot(t.root))
                          << "versus the new: " << difference(snapshot(t.expected), snapshot(t.root));
            return false;
        }
        if (hash != (committed ? t.v2Hash : t.v1Hash)) {
            ADD_FAILURE() << "after the crash before operation " << k << " (" << crash->description
                          << ", journal " << stateName(state) << ") the tree is the "
                          << (committed ? "old" : "new") << " version, not the "
                          << (committed ? "new" : "old") << " one";
            return false;
        }
        (hash == t.v1Hash ? c.old : c.fresh)++;

        // Recovery is idempotent: again changes nothing, and does not even write.
        StdFs clean;
        FaultFs counting(clean);
        core::Status again;
        const bool crashed = recoverUntilCrash(t, counting, &again).has_value();
        if (crashed || !again.ok() || counting.mutatingOperations() != 0 || treeHash(t.root) != hash) {
            ADD_FAILURE() << "a second recovery found work, or changed the tree, at k=" << k;
            return false;
        }
        if (state != JournalState::CommittedPendingHealth && !leftoversInUpdate(t.root).empty()) {
            ADD_FAILURE() << "only a transaction waiting for its health check keeps its folder (k="
                          << k << ")";
            return false;
        }
        return true;
    });

    // Past the last operation there is nothing to crash before: the apply finishes.
    {
        sizing.reset();
        StdFs disk;
        FaultFs faults(disk);
        faults.crashBefore(total + 1);
        ApplyResult result;
        EXPECT_FALSE(applyUntilCrash(s, faults, &result).has_value());
        EXPECT_EQ(result.outcome, ApplyOutcome::Committed);
        EXPECT_EQ(faults.mutatingOperations(), total);
    }

    CrashCounts all;
    for (const CrashCounts& c : counts) {
        all.crashes += c.crashes;
        all.old += c.old;
        all.fresh += c.fresh;
        for (const auto& [state, n] : c.byState) {
            all.byState[state] += n;
        }
    }
    // The loop reached what it claims.
    const std::uint64_t expectedCrashes = (total + stride - 1) / stride;
    EXPECT_EQ(all.crashes, expectedCrashes) << "a crash at every k";
    EXPECT_EQ(all.old + all.fresh, all.crashes);
    EXPECT_GT(all.old, 0u) << "some crashes ended at the old tree";
    EXPECT_GT(all.fresh, 0u) << "and some at the new one";
    for (const char* state : {"Empty", "Committing", "Healthy"}) {
        EXPECT_GT(all.byState[state], 0) << "no crash left the journal " << state;
    }
    if (stride == 1) {
        EXPECT_GT(all.byState["Planned"], 0);
        EXPECT_GT(all.byState["CommittedPendingHealth"], 0);
    }
    std::cout << "  " << label << ": " << total << " operations, " << all.crashes << " crashes, "
              << all.old << " ended at the old tree and " << all.fresh << " at the new; journal states:";
    for (const auto& [state, n] : all.byState) {
        std::cout << " " << state << "=" << n;
    }
    std::cout << "\n";
}

} // namespace

TEST(FaultLoops, CrashingBeforeEveryOperationOfAFortyFileUpdateLeavesTheOldTreeOrTheNewNeverAMix)
{
    REQUIRE_CAN_SIGN();
    crashLoop("loop-crash-forty", [](Scenario& s) { buildFortyFileScenario(s); }, 1, 200);
}

TEST(FaultLoops, TheSameLoopOverTheFixtureReleasesMadeByTheProducer)
{
    crashLoop("loop-crash-fixture", [](Scenario& s) { buildFixtureScenario(s); }, 1, 50);
}

// ---- loop 2: crash inside recovery -----------------------------------------------------------------------------

namespace {

struct FirstFault {
    std::string name;
    std::uint64_t crashBefore = 0; // the apply's own operation to die before (0: none)
    std::uint64_t failRename = 0;  // the commit's rename to fail with error 32 (0: none)
};

struct RecoveryItem {
    std::size_t point = 0;
    std::uint64_t operation = 0; // the recovery operation to die before
};

struct RecoveryState {
    std::size_t builtFor = static_cast<std::size_t>(-1); // the point whose crashed state is kept
    Path crashed;
    std::uint64_t crashesInRecovery = 0;
};

// Brings the loop's scenario to the state the first death of `point` leaves, and keeps a copy.
bool buildCrashedState(Loop& loop, const FirstFault& point, const Path& keep)
{
    Scenario& s = loop.scenario();
    loop.reset();
    StdFs bottom;
    FaultFs faults(bottom);
    faults.crashBefore(point.crashBefore);
    if (point.failRename != 0) {
        faults.failRenameWith(point.failRename, 32);
    }
    if (!applyUntilCrash(s, faults).has_value()) {
        ADD_FAILURE() << "the apply did not die at " << point.name;
        return false;
    }
    replaceTree(s.root, keep);
    return true;
}

// `only` names the interrupted states to run (indices into the list below); empty: all six.
void recoveryLoop(const std::string& label, const Build& build, std::uint64_t secondLevelStride,
                  const std::vector<std::size_t>& only = {})
{
    Loop sizing(label + "-sizing", build);
    Scenario& s = sizing.scenario();

    // Name the apply's operations from a log of one that goes right. The log is above the
    // FaultFs, the disk below it, so every operation the FaultFs counts has an entry.
    std::vector<FirstFault> points;
    {
        sizing.reset();
        StdFs bottom;
        FaultFs faults(bottom);
        LogFs log(faults);
        ApplyResult result;
        ASSERT_FALSE(applyUntilCrash(s, log, &result).has_value());
        ASSERT_EQ(result.outcome, ApplyOutcome::Committed);
        const std::uint64_t committing = log.find("appendDurable", "COMMITTING");
        const std::uint64_t pending = log.find("appendDurable", "COMMITTED_PENDING_HEALTH");
        const std::vector<std::uint64_t> renames = log.all("renameFile");
        ASSERT_GT(committing, 0u);
        ASSERT_GT(pending, committing);
        ASSERT_GE(renames.size(), 10u);
        points.push_back({"before COMMITTING is written (state Planned)", committing, 0});
        points.push_back({"before the first rename", renames.front(), 0});
        points.push_back({"half way through the renames", renames[renames.size() / 2], 0});
        points.push_back({"after the last rename", renames.back() + 1, 0});
        points.push_back({"before COMMITTED_PENDING_HEALTH is written", pending, 0});
    }
    {
        // A rename that fails makes the apply roll itself back; die a few operations into THAT.
        sizing.reset();
        StdFs bottom;
        FaultFs faults(bottom);
        faults.failRenameWith(7, 32);
        LogFs log(faults);
        ApplyResult result;
        ASSERT_FALSE(applyUntilCrash(s, log, &result).has_value());
        ASSERT_EQ(result.outcome, ApplyOutcome::RolledBack);
        const std::uint64_t rolling = log.find("appendDurable", "ROLLING_BACK");
        ASSERT_GT(rolling, 0u);
        ASSERT_GT(log.log.size(), rolling + 6) << "the rollback has operations to die in";
        points.push_back({"inside the apply's own rollback", rolling + 5, 7});
    }

    // How many operations a recovery from each of those states has.
    std::vector<RecoveryItem> items;
    for (std::size_t i = 0; i < points.size(); ++i) {
        if (!only.empty() && std::find(only.begin(), only.end(), i) == only.end()) {
            continue;
        }
        SCOPED_TRACE(points[i].name);
        const Path keep = s.scratch.path() / "crashed-state";
        ASSERT_TRUE(buildCrashedState(sizing, points[i], keep));
        // Whatever the files look like - after the last rename they are the new version's -
        // the journal has not said the update is complete, so recovery must put the old back.
        ASSERT_TRUE(needsRecovery(journalStateOf(s.root)) || journalStateOf(s.root) == JournalState::Empty)
            << "the crash left a journal that needs nothing";
        StdFs bottom;
        FaultFs counting(bottom);
        core::Status status;
        ASSERT_FALSE(recoverUntilCrash(s, counting, &status).has_value());
        ASSERT_TRUE(status.ok()) << status.error().describe();
        ASSERT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
        const std::uint64_t operations = counting.mutatingOperations();
        ASSERT_GT(operations, 10u);
        for (std::uint64_t j = 1; j <= operations; ++j) {
            items.push_back({i, j});
        }
    }

    std::vector<RecoveryState> states(workerCount());
    forEachItem(label, build, 1, items.size(), 1, states,
                [&](Loop& loop, std::uint64_t index, RecoveryState& state) -> bool {
        const RecoveryItem& item = items[index - 1];
        const FirstFault& point = points[item.point];
        Scenario& t = loop.scenario();
        if (state.builtFor != item.point) {
            state.crashed = t.scratch.path() / "crashed-state";
            if (!buildCrashedState(loop, point, state.crashed)) {
                return false;
            }
            state.builtFor = item.point;
        }
        const std::uint64_t j = item.operation;
        replaceTree(state.crashed, t.root);
        StdFs bottom;
        FaultFs faults(bottom);
        faults.crashBefore(j);
        const std::optional<SimulatedCrash> crash = recoverUntilCrash(t, faults);
        if (!crash || crash->operation != j) {
            ADD_FAILURE() << point.name << ": recovery did not crash before its operation " << j;
            return false;
        }
        ++state.crashesInRecovery;

        // A second death, inside the second recovery (at a few of its operations, from a
        // sample of the first): the third is the one that finishes.
        if (j % secondLevelStride == 0) {
            for (const std::uint64_t i : {1u, 4u, 17u}) {
                const Path between = t.scratch.path() / "between";
                replaceTree(t.root, between);
                StdFs again;
                FaultFs second(again);
                second.crashBefore(i);
                (void)recoverUntilCrash(t, second);
                const core::Status finished = recoverClean(t);
                if (!finished.ok() || treeHash(t.root) != t.v1Hash) {
                    ADD_FAILURE() << point.name << ": after deaths at recovery operations " << j
                                  << " and " << i << " the tree is not the old version: "
                                  << difference(t.v1Snapshot, snapshot(t.root));
                    return false;
                }
                replaceTree(between, t.root);
            }
        }

        const core::Status finished = recoverClean(t);
        if (!finished.ok()) {
            ADD_FAILURE() << point.name << ": recovery after dying at its operation " << j << " ("
                          << crash->description << ") failed: " << finished.error().describe();
            return false;
        }
        if (treeHash(t.root) != t.v1Hash) {
            ADD_FAILURE() << point.name << ": after dying at recovery operation " << j << " ("
                          << crash->description << ") the tree is not the old version:\n"
                          << difference(t.v1Snapshot, snapshot(t.root));
            return false;
        }
        if (!leftoversInUpdate(t.root).empty()) {
            ADD_FAILURE() << point.name << ": a folder is left after recovery operation " << j;
            return false;
        }
        return true;
    });

    std::uint64_t crashes = 0;
    for (const RecoveryState& state : states) {
        crashes += state.crashesInRecovery;
    }
    EXPECT_EQ(crashes, items.size()) << "a crash at every operation of every recovery";
    std::cout << "  " << label << ": " << (only.empty() ? points.size() : only.size())
              << " interrupted states, " << crashes << " crashes inside recovery\n";
}

} // namespace

TEST(FaultLoops, CrashingInsideRecoveryAtEveryOperationStillEndsAtTheOldTree)
{
    REQUIRE_CAN_SIGN();
    // The half-way state of the big update: the largest recovery there is. All six states are
    // run, with every operation, over the smaller update below; what a recovery does does not
    // depend on how many files there are.
    recoveryLoop("loop-recovery-forty", [](Scenario& s) { buildFortyFileScenario(s); }, 16, {2});
}

TEST(FaultLoops, TheRecoveryLoopOverTheFixtureReleases)
{
    recoveryLoop("loop-recovery-fixture", [](Scenario& s) { buildFixtureScenario(s); }, 8);
}

// ---- loop 3: a locked file at every rename ---------------------------------------------------------------------

namespace {

struct RenameCounts {
    std::uint64_t rolledBack = 0;
};

void renameFailureLoop(const std::string& label, const Build& build, std::uint64_t minimumRenames)
{
    Loop sizing(label + "-sizing", build);
    Scenario& s = sizing.scenario();

    // How many renames a good update makes.
    std::uint64_t renames = 0;
    {
        sizing.reset();
        StdFs disk;
        CountingFs counting(disk);
        Rig rig(s, counting);
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
        renames = counting.renames;
    }
    ASSERT_GE(renames, minimumRenames);

    const std::string stagePristine = treeHash(sizing.pristineRoot() / "stage");
    std::vector<RenameCounts> counts(workerCount());
    forEachItem(label, build, 1, renames, 1, counts,
                [&](Loop& loop, std::uint64_t k, RenameCounts& c) -> bool {
        Scenario& t = loop.scenario();
        loop.reset();
        StdFs disk;
        FaultFs faults(disk);
        faults.failRenameWith(k, 32);
        Rig rig(t, faults);
        const ApplyResult result = rig.run();
        if (result.outcome != ApplyOutcome::RolledBack || result.reason != "rename_failed") {
            ADD_FAILURE() << "error 32 at rename " << k << " of " << renames << " ended "
                          << toString(result.outcome) << " / " << result.reason << ": " << result.detail;
            return false;
        }
        ++c.rolledBack;
        if (treeHash(t.root) != t.v1Hash) {
            ADD_FAILURE() << "after error 32 at rename " << k << " the tree is not the old version:\n"
                          << difference(t.v1Snapshot, snapshot(t.root));
            return false;
        }
        EXPECT_TRUE(leftoversInUpdate(t.root).empty()) << "k=" << k;
        EXPECT_EQ(treeHash(t.stage), stagePristine)
            << "the stage is untouched, for another try (k=" << k << ")";
        EXPECT_EQ(rig.launcher.started.size(), 1u) << "the old version is started again (k=" << k << ")";
        EXPECT_TRUE(rig.launcher.ran.empty()) << "no health check of an update that did not happen";
        EXPECT_TRUE(rig.registry.runOnce.empty());
        const core::Result<std::optional<LastResult>> last = readLastResult(t.state);
        if (!last.ok() || !last->has_value()) {
            ADD_FAILURE() << "no last.json (k=" << k << ")";
            return false;
        }
        EXPECT_EQ((*last)->status, "rolled_back");
        EXPECT_EQ((*last)->reason, "rename_failed");
        return true;
    });
    std::uint64_t rolledBack = 0;
    for (const RenameCounts& c : counts) {
        rolledBack += c.rolledBack;
    }
    EXPECT_EQ(rolledBack, renames) << "every rename was failed once, and every update was rolled back";

    // One past the last rename there is none to fail: the update goes through.
    sizing.reset();
    StdFs disk;
    FaultFs faults(disk);
    faults.failRenameWith(renames + 1, 32);
    Rig rig(s, faults);
    EXPECT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
    std::cout << "  " << label << ": " << renames << " renames, each failed once with error 32\n";
}

} // namespace

TEST(FaultLoops, AnErrorThirtyTwoAtEveryRenameOfAFortyFileUpdateEndsRolledBackToExactlyTheOldTree)
{
    REQUIRE_CAN_SIGN();
    renameFailureLoop("loop-error-forty", [](Scenario& s) { buildFortyFileScenario(s); }, 36);
}

TEST(FaultLoops, TheSameLoopOverTheFixtureReleases)
{
    renameFailureLoop("loop-error-fixture", [](Scenario& s) { buildFixtureScenario(s); }, 12);
}

// ---- the same, through the real Windows file system, a sample of the operations -----------------------------------

#ifdef _WIN32

TEST(FaultLoopsOnWindows, TheRealFileSystemUnderTheFaultsEndsInTheSameTwoTrees)
{
    Loop loop("loop-real", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const auto real = makeSystemFsOps(RetryPolicy{std::chrono::milliseconds(1), std::chrono::milliseconds(10)});

    std::uint64_t total = 0;
    std::uint64_t pendingRecord = 0;
    {
        loop.reset();
        FaultFs counting(*real);
        LogFs log(counting);
        ApplyResult result;
        ASSERT_FALSE(applyUntilCrash(s, log, &result).has_value());
        ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << result.detail;
        total = counting.mutatingOperations();
        pendingRecord = log.find("appendDurable", "COMMITTED_PENDING_HEALTH");
        ASSERT_EQ(treeHash(s.root), s.v2Hash);
    }
    std::uint64_t checked = 0;
    for (std::uint64_t k = 1; k <= total; k += 3) {
        loop.reset();
        FaultFs faults(*real);
        faults.crashBefore(k);
        ASSERT_TRUE(applyUntilCrash(s, faults).has_value()) << k;
        Rig rig(s, *real, "eeeeeeee");
        const core::Status status = recoverInProcess(s.root, rig.services());
        ASSERT_TRUE(status.ok()) << "k=" << k << ": " << status.error().describe();
        EXPECT_EQ(treeHash(s.root), k > pendingRecord ? s.v2Hash : s.v1Hash) << "k=" << k;
        ++checked;
    }
    EXPECT_GE(checked, 15u);

    // And an error 32 at some renames, through the real file system.
    for (std::uint64_t k = 1; k <= 12; k += 2) {
        loop.reset();
        FaultFs faults(*real);
        faults.failRenameWith(k, 32);
        Rig rig(s, faults);
        const ApplyResult result = rig.run();
        ASSERT_EQ(result.outcome, ApplyOutcome::RolledBack) << k << result.detail;
        EXPECT_EQ(treeHash(s.root), s.v1Hash) << "k=" << k;
    }
}

#endif // _WIN32
