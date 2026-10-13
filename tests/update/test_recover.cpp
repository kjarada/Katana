// Finishing what a crash interrupted (include/katana/update/recover.hpp), and the rollback
// under it (rollback() in apply.hpp).
//
// The interrupted states are not written by hand where a crash can make them: the apply is
// run on a FaultFs that dies before a chosen operation, and the operation is chosen from a log
// of an apply that went right (LogFs), so "the COMMITTING record" or "the third rename" is
// found, not guessed. The forged journals ARE written by hand - nothing but a person with a
// text editor makes them - and every expectation is that the root stays as it was, and that
// nothing outside it is touched, which is a fact about the disk, not about the code.

#include <gtest/gtest.h>

#include <ostream>
#include <stdexcept>
#include <thread>

#include "apply_harness.hpp"
#include "recover_internal.hpp"
#include "seam_doubles.hpp"

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

namespace katana::update {

// So that a failed comparison says "Left" and not a number (GoogleTest finds these by the
// namespace of the type).
inline void PrintTo(Settled settled, std::ostream* out)
{
    switch (settled) {
    case Settled::Nothing:
        *out << "Nothing";
        return;
    case Settled::Healthy:
        *out << "Healthy";
        return;
    case Settled::RolledBack:
        *out << "RolledBack";
        return;
    case Settled::Left:
        *out << "Left";
        return;
    case Settled::Busy:
        *out << "Busy";
        return;
    case Settled::RollbackIncomplete:
        *out << "RollbackIncomplete";
        return;
    }
    *out << "?";
}

inline void PrintTo(Confirmed confirmed, std::ostream* out)
{
    switch (confirmed) {
    case Confirmed::Nothing:
        *out << "Nothing";
        return;
    case Confirmed::Done:
        *out << "Done";
        return;
    case Confirmed::Busy:
        *out << "Busy";
        return;
    }
    *out << "?";
}

} // namespace katana::update

namespace {

// The operations of a clean apply of the fixture release, by number.
struct Points {
    std::uint64_t plan = 0;       // the PLAN group is appended
    std::uint64_t committing = 0; // the COMMITTING record
    std::vector<std::uint64_t> renames;
    std::uint64_t pending = 0;    // COMMITTED_PENDING_HEALTH
    std::uint64_t healthy = 0;    // HEALTHY
    std::uint64_t total = 0;
};

Points measureCleanApply(Loop& loop)
{
    Points points;
    loop.reset();
    Scenario& s = loop.scenario();
    StdFs bottom;
    FaultFs faults(bottom);
    LogFs log(faults);
    ApplyResult result;
    const std::optional<SimulatedCrash> crash = applyUntilCrash(s, log, &result);
    EXPECT_FALSE(crash.has_value());
    EXPECT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << result.detail;
    points.plan = log.find("appendDurable", "PLAN");
    points.committing = log.find("appendDurable", "COMMITTING");
    points.renames = log.all("renameFile");
    points.pending = log.find("appendDurable", "COMMITTED_PENDING_HEALTH");
    points.healthy = log.find("appendDurable", " HEALTHY ");
    points.total = log.log.size();
    EXPECT_GT(points.plan, 0u);
    EXPECT_GT(points.committing, points.plan);
    EXPECT_GT(points.pending, points.committing);
    EXPECT_GT(points.healthy, points.pending);
    EXPECT_FALSE(points.renames.empty());
    return points;
}

// Dies before operation `k`, with a rename failed on the way if `failRename` is not 0.
void crashAt(Loop& loop, std::uint64_t k, std::uint64_t failRename = 0)
{
    loop.reset();
    StdFs bottom;
    FaultFs faults(bottom);
    faults.crashBefore(k);
    if (failRename != 0) {
        faults.failRenameWith(failRename, 32);
    }
    EXPECT_TRUE(applyUntilCrash(loop.scenario(), faults).has_value()) << "no crash before " << k;
}

ReleaseSpec tiny(const std::string& version, std::uint64_t sequence)
{
    ReleaseSpec release;
    release.version = version;
    release.tag = "v" + version;
    release.sequence = sequence;
    release.files = {
        {"bin/katana.exe", "katana " + version + "\n", FileClass::Program, true},
        {"bin/lib.dll", "lib " + version + "\n", FileClass::Lib, false},
        {"share/katana/data.txt", "data " + version + "\n", FileClass::Data, false},
    };
    return release;
}

// A counting FsOps for "nothing to do costs next to nothing".
class WatchedFs final : public ForwardingFs {
  public:
    using ForwardingFs::ForwardingFs;
    core::Result<std::unique_ptr<FileLock>> tryLock(const Path& p) override
    {
        ++locks;
        return inner_.tryLock(p);
    }
    core::Result<std::vector<Path>> listDirectory(const Path& p) const override
    {
        ++listings;
        return inner_.listDirectory(p);
    }
    mutable int locks = 0;
    mutable int listings = 0;
};

} // namespace

// ---- every state a journal can be left in ------------------------------------------------------------------

TEST(Recover, EveryInterruptedStateIsPutBackAndEveryFinishedOneIsLeftAlone)
{
    Loop loop("recover-states", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);

    // The log of an update whose seventh rename fails, to find the rollback's own records.
    std::uint64_t rolling = 0;
    std::uint64_t rolled = 0;
    {
        loop.reset();
        StdFs bottom;
        FaultFs faults(bottom);
        faults.failRenameWith(7, 32);
        LogFs log(faults);
        ApplyResult result;
        ASSERT_FALSE(applyUntilCrash(s, log, &result).has_value());
        ASSERT_EQ(result.outcome, ApplyOutcome::RolledBack);
        rolling = log.find("appendDurable", "ROLLING_BACK");
        rolled = log.find("appendDurable", "ROLLED_BACK");
        ASSERT_GT(rolling, 0u);
        ASSERT_GT(rolled, rolling);
        ASSERT_GT(log.log.size(), rolled) << "the rollback has cleaning to do after its last record";
    }

    struct Row {
        const char* name;
        std::uint64_t crashBefore;
        std::uint64_t failRename;
        JournalState state;     // what the journal must say after the crash
        bool recoveryRollsBack; // else: the tree is a whole version and recovery leaves it
        bool newTree;           // the tree after recovery is the new version
        bool folderStays;       // the transaction folder is still there after recovery
    };
    const std::vector<Row> rows = {
        {"planning, before the PLAN group", p.plan, 0, JournalState::Empty, true, false, false},
        {"PLAN written, COMMITTING not", p.committing, 0, JournalState::Planned, true, false, false},
        {"COMMITTING written, nothing moved", p.renames.front(), 0, JournalState::Committing, true, false, false},
        {"half the renames done", p.renames[p.renames.size() / 2], 0, JournalState::Committing, true, false, false},
        {"every rename done, the pending record not", p.pending, 0, JournalState::Committing, true, false, false},
        {"the apply's own rollback, part way", rolling + 4, 7, JournalState::RollingBack, true, false, false},
        // The new version is in and not yet proved: whole, and the folder is what a health
        // rollback needs, so it is left exactly as it is.
        {"pending health, nothing more", p.healthy, 0, JournalState::CommittedPendingHealth, false, true, true},
        // Over, with the clean-up cut short: the tree is whole and what is left is swept.
        {"healthy, the clean-up part way", p.healthy + 2, 0, JournalState::Healthy, false, true, false},
        {"rolled back, the clean-up part way", rolled + 2, 7, JournalState::RolledBack, false, false, false},
    };
    for (const Row& row : rows) {
        SCOPED_TRACE(row.name);
        crashAt(loop, row.crashBefore, row.failRename);
        ASSERT_EQ(journalStateOf(s.root), row.state) << "the crash left another state than the row names";
        const std::string crashedHash = treeHash(s.root);

        StdFs bottom;
        FaultFs counting(bottom);
        core::Status status;
        ASSERT_FALSE(recoverUntilCrash(s, counting, &status).has_value());
        ASSERT_TRUE(status.ok()) << status.error().describe();
        if (row.recoveryRollsBack) {
            EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
            EXPECT_TRUE(leftoversInUpdate(s.root).empty()) << "its folder is gone";
        } else {
            EXPECT_EQ(treeHash(s.root), row.newTree ? s.v2Hash : s.v1Hash);
            EXPECT_EQ(treeHash(s.root), crashedHash) << "the tree is left as it was";
            EXPECT_EQ(leftoversInUpdate(s.root).empty(), !row.folderStays);
            if (row.folderStays) {
                EXPECT_EQ(counting.mutatingOperations(), 0u) << "and not so much as written to";
                EXPECT_EQ(journalStateOf(s.root), row.state) << "nor its journal";
            }
        }
    }
}

TEST(Recover, AHealthyTransactionWhoseCleanUpWasCutShortIsSweptAndItsNewVersionIsNotTouched)
{
    Loop loop("recover-sweep", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.healthy + 1);
    ASSERT_EQ(journalStateOf(s.root), JournalState::Healthy);
    ASSERT_FALSE(leftoversInUpdate(s.root).empty());
    ASSERT_TRUE(recoverClean(s).ok());
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
}

TEST(Recover, TheRecoveryIsIdempotentTwiceChangesNothingTheSecondTime)
{
    Loop loop("recover-idempotent", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.renames[p.renames.size() / 2]);
    ASSERT_TRUE(recoverClean(s).ok());
    const std::map<std::string, std::string> first = snapshot(s.root);

    StdFs bottom;
    FaultFs counting(bottom);
    core::Status status;
    ASSERT_FALSE(recoverUntilCrash(s, counting, &status).has_value());
    EXPECT_TRUE(status.ok());
    EXPECT_EQ(counting.mutatingOperations(), 0u);
    EXPECT_EQ(snapshot(s.root), first);
}

// ---- the lock --------------------------------------------------------------------------------------------------------

TEST(RecoverLock, ALiveHolderIsWaitedForAndItsFilesAreNotTouched)
{
    Loop loop("recover-lock-held", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.renames[p.renames.size() / 2]);
    const std::string crashedHash = treeHash(s.root);

    StdFs disk;
    core::Result<std::unique_ptr<FileLock>> holder = disk.tryLock(s.root / ".update" / "lock");
    ASSERT_TRUE(holder.ok());
    Rig rig(s, disk, "eeeeeeee");
    rig.timings.waitForHolders = std::chrono::milliseconds(100);
    const auto start = std::chrono::steady_clock::now();
    const core::Status status = recoverInProcess(s.root, rig.services());
    const auto took = std::chrono::steady_clock::now() - start;
    EXPECT_TRUE(status.ok()) << "a live update is not a failure of recovery";
    EXPECT_GE(took, std::chrono::milliseconds(90)) << "it waited";
    EXPECT_EQ(treeHash(s.root), crashedHash) << "and touched nothing";
    EXPECT_EQ(journalStateOf(s.root), JournalState::Committing);
}

TEST(RecoverLock, ARecoveryThatWaitedForTheHolderDoesItsJobWhenTheHolderLetsGo)
{
    Loop loop("recover-lock-released", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.renames[p.renames.size() / 2]);

    StdFs disk;
    auto holder = std::make_unique<core::Result<std::unique_ptr<FileLock>>>(disk.tryLock(s.root / ".update" / "lock"));
    ASSERT_TRUE(holder->ok());
    std::thread releasing([&] {
        std::this_thread::sleep_for(std::chrono::milliseconds(60));
        holder.reset();
    });
    Rig rig(s, disk, "eeeeeeee");
    rig.timings.waitForHolders = std::chrono::milliseconds(5000);
    const core::Status status = recoverInProcess(s.root, rig.services());
    releasing.join();
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
}

TEST(RecoverLock, TheLockIsFreeAgainWhenRecoveryEnds)
{
    Loop loop("recover-lock-free", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.renames[p.renames.size() / 2]);
    ASSERT_TRUE(recoverClean(s).ok());
    StdFs disk;
    EXPECT_TRUE(disk.tryLock(s.root / ".update" / "lock").ok());
}

TEST(RecoverLock, NothingToDoTakesNoLockAndLooksAtOneFolder)
{
    Scenario s("recover-nothing");
    s.v1 = tiny("0.1.0", 1);
    writeInstall(s.root, s.v1);
    StdFs disk;
    {
        // An install that never updated has no .update at all.
        WatchedFs watched(disk);
        FaultFs counting(watched);
        Rig rig(s, counting, "eeeeeeee");
        EXPECT_TRUE(recoverInProcess(s.root, rig.services()).ok());
        EXPECT_EQ(counting.mutatingOperations(), 0u);
        EXPECT_EQ(watched.locks, 0);
        EXPECT_FALSE(isPresent(s.root / ".update")) << "recovery does not make the folder";
    }
    {
        // One that did has the lock file and nothing else.
        writeText(s.root / ".update" / "lock", "");
        WatchedFs watched(disk);
        FaultFs counting(watched);
        Rig rig(s, counting, "eeeeeeee");
        EXPECT_TRUE(recoverInProcess(s.root, rig.services()).ok());
        EXPECT_EQ(counting.mutatingOperations(), 0u);
        EXPECT_EQ(watched.locks, 0) << "nothing interrupted, so the lock is not even asked for";
        EXPECT_EQ(watched.listings, 1) << "one directory listing";
    }
}

// ---- a recovery that cannot finish ------------------------------------------------------------------------------------

TEST(RecoverFailure, ARecoveryThatCannotFinishSaysTheCommandToRunByHand)
{
    Loop loop("recover-cannot", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.renames[p.renames.size() / 2]);
    const std::string crashedHash = treeHash(s.root);

    StdFs disk;
    HookFs hooked(disk);
    hooked.beforeRename = [](const Path& from, const Path&) -> std::optional<core::Status> {
        return injected(32, from); // a file that stays locked
    };
    Rig rig(s, hooked, "eeeeeeee");
    const core::Status status = recoverInProcess(s.root, rig.services());
    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().message.find("katana_updater --recover"), std::string::npos)
        << status.error().describe();
    EXPECT_NE(status.error().message.find(core::pathToUtf8(s.root)), std::string::npos);
    // The folder with the old files is still there for the next try.
    EXPECT_EQ(journalStateOf(s.root), JournalState::RollingBack);
    EXPECT_FALSE(leftoversInUpdate(s.root).empty());
    EXPECT_NE(treeHash(s.root), s.v1Hash);

    // And the next try, with the file free, finishes it.
    ASSERT_TRUE(recoverClean(s).ok());
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

TEST(RecoverFailure, TheRunOnceEntryIsClearedWhenARecoverySucceedsAndKeptWhenItFails)
{
    Loop loop("recover-runonce", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.renames[p.renames.size() / 2]);

    StdFs disk;
    HookFs hooked(disk);
    hooked.beforeRename = [](const Path& from, const Path&) -> std::optional<core::Status> {
        return injected(5, from);
    };
    {
        Rig rig(s, hooked, "eeeeeeee");
        rig.registry.runOnce[std::string(kRunOnceName)] = "x";
        ASSERT_FALSE(recoverInProcess(s.root, rig.services()).ok());
        EXPECT_EQ(rig.registry.runOnce.count(std::string(kRunOnceName)), 1u);
    }
    {
        Rig rig(s, disk, "eeeeeeee");
        rig.registry.runOnce[std::string(kRunOnceName)] = "x";
        ASSERT_TRUE(recoverInProcess(s.root, rig.services()).ok());
        EXPECT_TRUE(rig.registry.runOnce.empty());
    }
}

// ---- journals nobody trusts ----------------------------------------------------------------------------------------------------

namespace {

// A root with v1 installed, and a transaction folder `deadbeef` that holds whatever the test
// puts in it, for a hand-made journal.
struct Forgery {
    explicit Forgery(const std::string& label) : s(label)
    {
        s.v1 = tiny("0.1.0", 100);
        writeInstall(s.root, s.v1);
        work = s.root / ".update" / "deadbeef";
        // The legitimate part: bin/lib.dll was moved aside and the new one is in place.
        writeText(s.root / "bin/lib.dll", "the new lib\n");
        writeText(work / "old/bin/lib.dll", "lib 0.1.0\n");
    }

    void journal(const std::vector<JournalRecord>& records)
    {
        std::string text;
        std::uint64_t n = 1;
        for (const JournalRecord& record : records) {
            text += encodeRecord(n++, record);
        }
        writeText(work / "journal", text);
    }

    // Everything under the scratch folder but the transaction's own working folder.
    std::map<std::string, std::string> outside() const
    {
        std::map<std::string, std::string> entries = snapshot(s.scratch.path());
        for (auto it = entries.begin(); it != entries.end();) {
            it = it->first.rfind("install/.update", 0) == 0 ? entries.erase(it) : std::next(it);
        }
        return entries;
    }

    Scenario s;
    Path work;
};

std::string fileEntry(std::string_view bytes)
{
    return "file " + std::to_string(bytes.size()) + " " + core::sha256Hex(bytes);
}

} // namespace

TEST(RecoverForgery, APathThatLeavesTheInstallIsSkippedAndNothingOutsideIsTouched)
{
    Forgery f("recover-forged-paths");
    const Path outside = f.s.scratch.path();
    writeText(outside / "victim.txt", "victim bytes");
    fs::create_directories(outside / "emptydir");
    // Planted for a naive restore to carry out: old/../planted.txt is work/planted.txt.
    writeText(f.work / "planted.txt", "attacker bytes");
    writeText(f.work / "old/share/katana/INSTALL.JSON", "evil install record");
    writeText(f.work / "old/.update/lock", "evil");
    writeText(f.work / "planted2.txt", "attacker bytes");
    const std::string absolute = core::pathToUtf8(outside / "abs_target.txt");
    writeText(f.work / "old" / core::pathFromUtf8(absolute).relative_path(), "attacker bytes");

    f.journal({
        {JournalOp::Plan, {"deadbeef", "100", "200"}},
        {JournalOp::MakeDirectory, {"../emptydir"}},
        {JournalOp::Replace, {"bin/lib.dll"}}, // the one honest record
        {JournalOp::Replace, {"../planted.txt"}},
        {JournalOp::Add, {"../victim.txt"}},
        {JournalOp::Replace, {"share/katana/INSTALL.JSON"}},
        {JournalOp::Replace, {".update/lock"}},
        {JournalOp::Replace, {absolute}},
        {JournalOp::Replace, {"bin\\..\\..\\planted2.txt"}},
        {JournalOp::Committing, {}},
    });
    const std::map<std::string, std::string> before = f.outside();

    StdFs disk;
    Rig rig(f.s, disk, "eeeeeeee");
    const core::Status status = recoverInProcess(f.s.root, rig.services());
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());

    // The honest record was carried out.
    EXPECT_EQ(readText(f.s.root / "bin/lib.dll"), "lib 0.1.0\n");
    // Nothing outside bin, share, etc and lib of the root changed, and nothing was made.
    std::map<std::string, std::string> expected = before;
    expected["install/bin/lib.dll"] = fileEntry("lib 0.1.0\n");
    EXPECT_EQ(f.outside(), expected) << difference(expected, f.outside());
    EXPECT_FALSE(isPresent(outside / "planted.txt"));
    EXPECT_FALSE(isPresent(outside / "planted2.txt"));
    EXPECT_FALSE(isPresent(outside / "abs_target.txt"));
    EXPECT_EQ(readText(outside / "victim.txt"), "victim bytes");
    EXPECT_TRUE(fs::is_directory(outside / "emptydir"));
    EXPECT_EQ(readText(f.s.root / "share/katana/install.json"), std::string(kInstallJson));
    EXPECT_FALSE(isPresent(f.s.root / "share/katana/INSTALL.JSON") &&
                 readText(f.s.root / "share/katana/INSTALL.JSON") == "evil install record");

    // Each skipped record is reported, in a file beside the lock.
    const std::string log = readText(f.s.root / ".update" / "recovery.log");
    for (const char* refused : {"../planted.txt", "../victim.txt", "INSTALL.JSON", ".update/lock", "../emptydir"}) {
        EXPECT_NE(log.find(refused), std::string::npos) << "not reported: " << refused << "\n" << log;
    }
    EXPECT_NE(log.find("deadbeef"), std::string::npos) << "and by transaction";
}

TEST(RecoverForgery, APathWithADotDotInsideAGoodFirstComponentIsNotFollowedEither)
{
    Forgery f("recover-forged-dotdot");
    writeText(f.s.scratch.path() / "victim2.txt", "victim bytes");
    writeText(f.work / "old/bin/x/../../../planted3.txt", "attacker bytes"); // = work/planted3.txt
    f.journal({
        {JournalOp::Plan, {"deadbeef", "1", "2"}},
        {JournalOp::Replace, {"bin/../../planted3.txt"}},
        {JournalOp::Add, {"share/../../victim2.txt"}},
        {JournalOp::Committing, {}},
    });
    StdFs disk;
    Rig rig(f.s, disk, "eeeeeeee");
    EXPECT_TRUE(recoverInProcess(f.s.root, rig.services()).ok());
    EXPECT_FALSE(isPresent(f.s.scratch.path() / "planted3.txt"));
    EXPECT_EQ(readText(f.s.scratch.path() / "victim2.txt"), "victim bytes");
}

TEST(RecoverForgery, AFolderWhoseNameIsNotATransactionIdIsNeverEnteredWhateverItsJournalSays)
{
    Forgery f("recover-forged-folder");
    std::filesystem::rename(f.work, f.s.root / ".update" / "EVIL"); // the honest content, evil name
    f.work = f.s.root / ".update" / "EVIL";
    f.journal({
        {JournalOp::Plan, {"EVIL", "1", "2"}},
        {JournalOp::Replace, {"bin/lib.dll"}},
        {JournalOp::Committing, {}},
    });
    for (const char* name : {"..", "a b", "g1", "not-a-transaction", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0"}) {
        const Path other = f.s.root / ".update" / name;
        if (std::string(name) == "..") {
            continue; // not a name a directory listing yields
        }
        writeText(other / "old/bin/lib.dll", "lib 0.1.0\n");
        writeText(other / "journal", readText(f.work / "journal"));
    }
    StdFs disk;
    Rig rig(f.s, disk, "eeeeeeee");
    EXPECT_TRUE(recoverInProcess(f.s.root, rig.services()).ok());
    EXPECT_EQ(readText(f.s.root / "bin/lib.dll"), "the new lib\n") << "none of them was acted on";
    EXPECT_TRUE(isPresent(f.work / "old/bin/lib.dll"));
}

TEST(RecoverForgery, ATransactionIdPassedToRollbackDirectlyIsCheckedToo)
{
    Forgery f("recover-forged-direct");
    StdFs disk;
    Rig rig(f.s, disk, "eeeeeeee");
    for (const std::string& id : {std::string("../x"), std::string("A"), std::string(""), std::string("a/b")}) {
        const core::Status status = rollback(f.s.root, id, rig.services());
        ASSERT_FALSE(status.ok()) << id;
        EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
    }
}

TEST(RecoverForgery, AFolderTheJournalNamesThatIsALinkIsNotRestoredThroughAndTheFailureIsReported)
{
    Forgery f("recover-forged-link");
    f.journal({
        {JournalOp::Plan, {"deadbeef", "1", "2"}},
        {JournalOp::Replace, {"bin/lib.dll"}},
        {JournalOp::Committing, {}},
    });
    StdFs disk;
    HookFs hooked(disk);
    hooked.adjustStat = [&](const Path& p, EntryInfo& info) {
        if (p == f.s.root / "bin") {
            info.reparsePoint = true; // a junction to somewhere else
        }
    };
    Rig rig(f.s, hooked, "eeeeeeee");
    const core::Status status = recoverInProcess(f.s.root, rig.services());
    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().message.find("katana_updater --recover"), std::string::npos);
    EXPECT_EQ(readText(f.s.root / "bin/lib.dll"), "the new lib\n") << "nothing was moved through the link";
}

TEST(RecoverForgery, ALinkInsideOldOrNewIsNotMovedThroughEitherAndTheFailureIsReported)
{
    // old/<dir> and new/<dir> are in the transaction's folder, which is in the install, but a
    // junction there leads anywhere: a file renamed from, or into, one comes from or goes to a
    // place outside the install. The check on the install's side alone does not see it.
    for (const char* side : {"old", "new"}) {
        SCOPED_TRACE(side);
        Forgery f(std::string("recover-forged-link-") + side);
        fs::create_directories(f.work / "new/bin"); // as the plan made it
        f.journal({
            {JournalOp::Plan, {"deadbeef", "1", "2"}},
            {JournalOp::Replace, {"bin/lib.dll"}},
            {JournalOp::Committing, {}},
        });
        StdFs disk;
        HookFs hooked(disk);
        const Path linked = f.work / side / "bin";
        hooked.adjustStat = [&](const Path& p, EntryInfo& info) {
            if (p == linked) {
                info.reparsePoint = true; // a junction to somewhere else
            }
        };
        Rig rig(f.s, hooked, "eeeeeeee");
        const core::Status status = recoverInProcess(f.s.root, rig.services());
        ASSERT_FALSE(status.ok());
        EXPECT_NE(status.error().message.find("katana_updater --recover"), std::string::npos);
        EXPECT_EQ(readText(f.s.root / "bin/lib.dll"), "the new lib\n") << "nothing was moved through the link";
        EXPECT_TRUE(isPresent(f.work / "old/bin/lib.dll")) << "and the old file is where it was";
        EXPECT_FALSE(isPresent(f.work / "new/bin/lib.dll"));
    }
}

TEST(RecoverForgery, AControlCharacterInARefusedPathCannotMakeTheLogSaySomethingElse)
{
    Forgery f("recover-forged-logline");
    // A line feed and a carriage return, percent-encoded as the journal writes them.
    f.journal({
        {JournalOp::Plan, {"deadbeef", "1", "2"}},
        {JournalOp::Replace, {"bin/lib.dll"}},
        {JournalOp::Replace, {"../a\ndeadbeef journal record refused: FORGED\r\nmore"}},
        {JournalOp::Committing, {}},
    });
    StdFs disk;
    Rig rig(f.s, disk, "eeeeeeee");
    ASSERT_TRUE(recoverInProcess(f.s.root, rig.services()).ok());
    EXPECT_EQ(readText(f.s.root / "bin/lib.dll"), "lib 0.1.0\n");

    const std::string log = readText(f.s.root / ".update" / "recovery.log");
    // One refused record is one line: the path's own line feed and carriage return are shown as
    // %0A and %0D, from the byte values.
    EXPECT_EQ(std::count(log.begin(), log.end(), '\n'), 1) << log;
    EXPECT_EQ(log.find('\r'), std::string::npos) << log;
    EXPECT_EQ(log.rfind("deadbeef journal record refused: REPLACE ../a%0Adeadbeef journal record refused: "
                        "FORGED%0D%0Amore: ", 0),
              0u)
        << log;
}

TEST(RecoverBoundary, AnExceptionFromTheFileSystemIsAnErrorToReportAndNotACrash)
{
    Forgery f("recover-throws");
    f.journal({
        {JournalOp::Plan, {"deadbeef", "1", "2"}},
        {JournalOp::Replace, {"bin/lib.dll"}},
        {JournalOp::Committing, {}},
    });
    StdFs disk;
    class Throwing final : public ForwardingFs {
      public:
        using ForwardingFs::ForwardingFs;
        core::Result<EntryInfo> stat(const Path&) const override
        {
            throw std::runtime_error("injected: a conversion or an allocation failed");
        }
    } throwing(disk);

    core::Status inProcess;
    {
        Rig rig(f.s, throwing, "eeeeeeee");
        EXPECT_NO_THROW(inProcess = recoverInProcess(f.s.root, rig.services()));
        EXPECT_FALSE(inProcess.ok());
        core::Status rolled;
        EXPECT_NO_THROW(rolled = rollback(f.s.root, "deadbeef", rig.services()));
        EXPECT_FALSE(rolled.ok());
    }
    FakeLauncher launcher;
    core::Status withHelper;
    EXPECT_NO_THROW(withHelper = detail::recoverWith(f.s.root, throwing, launcher, f.s.state,
                                                      std::chrono::milliseconds(50)));
    EXPECT_FALSE(withHelper.ok()) << "recover() says so rather than dying: the caller reports it";
    EXPECT_TRUE(launcher.ran.empty());
    EXPECT_EQ(readText(f.s.root / "bin/lib.dll"), "the new lib\n") << "and nothing was touched";
}

#ifdef _WIN32

TEST(RecoverBoundary, ANameThatIsNotTextInTheUpdateFolderOrInTheOldFilesCannotStopAProgramStarting)
{
    // NTFS allows a name with an unpaired surrogate; converting it to UTF-8 throws.
    Forgery f("recover-odd-name");
    f.journal({
        {JournalOp::Plan, {"deadbeef", "1", "2"}},
        {JournalOp::Replace, {"bin/lib.dll"}},
        {JournalOp::Committing, {}},
    });
    const Path oddName = Path(std::wstring(L"a\xD800" L"b"));
    const Path oddFolder = f.s.root / ".update" / oddName;
    const Path oddFile = f.work / "old/bin" / oddName;
    std::error_code ec;
    fs::create_directory(oddFolder, ec);
    ASSERT_FALSE(ec) << ec.message();
    writeText(oddFile, "a file nobody can name in text");
    EXPECT_ANY_THROW((void)core::pathToUtf8(oddName)) << "the premise of the test";

    StdFs disk;
    Rig rig(f.s, disk, "eeeeeeee");
    core::Status status;
    EXPECT_NO_THROW(status = recoverInProcess(f.s.root, rig.services()));
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    EXPECT_EQ(readText(f.s.root / "bin/lib.dll"), "lib 0.1.0\n") << "the interrupted update is still put back";

    // The system seams too: recover() is the first thing a program does.
    FakeLauncher launcher;
    core::Status first;
    EXPECT_NO_THROW(first = detail::recoverWith(f.s.root, disk, launcher, f.s.state, std::chrono::milliseconds(50)));
    EXPECT_TRUE(first.ok()) << (first.ok() ? "" : first.error().describe());
    fs::remove(oddFolder, ec);
}

#endif // _WIN32

TEST(RecoverBoundary, AFolderPlantedFarDeeperThanAnInstallIsLeftAndRecoveryStillReturns)
{
    // A finished transaction's folder is swept by every program's first lines. A nest planted to
    // make that recursion run out of stack must not be entered without a bound.
    Forgery f("recover-deep");
    f.journal({
        {JournalOp::Plan, {"deadbeef", "1", "2"}},
        {JournalOp::Replace, {"bin/lib.dll"}},
        {JournalOp::Committing, {}},
        {JournalOp::CommittedPendingHealth, {}},
        {JournalOp::Healthy, {}},
    });
    Path deep = f.work / "new";
    for (int i = 0; i < 66; ++i) { // more than the 64 the clean-up goes down
        deep /= "d";
    }
    std::error_code ec;
    fs::create_directories(deep, ec);
    ASSERT_FALSE(ec) << ec.message();

    StdFs disk;
    Rig rig(f.s, disk, "eeeeeeee");
    core::Status status;
    EXPECT_NO_THROW(status = recoverInProcess(f.s.root, rig.services()));
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    EXPECT_TRUE(fs::is_directory(deep)) << "past the bound it is left, not walked";
    EXPECT_FALSE(isPresent(f.work / "old")) << "and what is within the bound still goes";
    EXPECT_EQ(readText(f.s.root / "bin/lib.dll"), "the new lib\n") << "a finished transaction is not rolled back";
    // Nothing of an ordinary size is affected: the fixture-sized folders in the other tests are
    // swept whole (EveryInterruptedStateIs...).
}

TEST(RecoverForgery, ATornJournalAndAnEmptyOneAreJunkNotAReasonToTouchTheInstall)
{
    {
        // A crash during the PLAN group's write: the journal has a good prefix and a torn line.
        Forgery f("recover-torn");
        const std::string whole = encodeRecord(1, {JournalOp::Plan, {"deadbeef", "1", "2"}}) +
                                  encodeRecord(2, {JournalOp::Replace, {"bin/lib.dll"}});
        writeText(f.work / "journal", whole.substr(0, whole.size() - 7));
        // The honest tree: nothing had moved, so the new lib is NOT in place; the old one is.
        writeText(f.s.root / "bin/lib.dll", "lib 0.1.0\n");
        fs::remove(f.work / "old/bin/lib.dll");
        const std::string before = treeHash(f.s.root);
        ASSERT_TRUE(recoverClean(f.s).ok());
        EXPECT_EQ(treeHash(f.s.root), before);
        EXPECT_TRUE(leftoversInUpdate(f.s.root).empty()) << "its folder is junk and goes";
    }
    {
        Forgery f("recover-empty-journal");
        writeText(f.work / "journal", "");
        writeText(f.s.root / "bin/lib.dll", "lib 0.1.0\n");
        const std::string before = treeHash(f.s.root);
        ASSERT_TRUE(recoverClean(f.s).ok());
        EXPECT_EQ(treeHash(f.s.root), before);
        EXPECT_TRUE(leftoversInUpdate(f.s.root).empty());
    }
    {
        Forgery f("recover-no-journal");
        writeText(f.s.root / "bin/lib.dll", "lib 0.1.0\n");
        const std::string before = treeHash(f.s.root);
        ASSERT_TRUE(recoverClean(f.s).ok());
        EXPECT_EQ(treeHash(f.s.root), before);
        EXPECT_TRUE(leftoversInUpdate(f.s.root).empty());
    }
}

// ---- rollback, called directly -------------------------------------------------------------------------------------------------

TEST(Rollback, ARollbackOfAHalfSwappedTreeIsExactAndRunningItAgainChangesNothing)
{
    Loop loop("rollback-direct", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.renames[p.renames.size() / 2]);
    // The transaction's id: its folder.
    std::string tx;
    for (const auto& entry : fs::directory_iterator(s.root / ".update")) {
        if (entry.is_directory()) {
            tx = entry.path().filename().string();
        }
    }
    ASSERT_FALSE(tx.empty());

    StdFs disk;
    Rig rig(s, disk, "eeeeeeee");
    ASSERT_TRUE(rollback(s.root, tx, rig.services()).ok());
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    EXPECT_FALSE(isPresent(s.root / ".update" / tx));

    // Again: the folder is gone, so there is nothing to do, and that is success.
    FaultFs counting(disk);
    Rig again(s, counting, "eeeeeeee");
    EXPECT_TRUE(rollback(s.root, tx, again.services()).ok());
    EXPECT_EQ(counting.mutatingOperations(), 0u);
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
}

TEST(Rollback, ATransactionThatIsHealthyIsNeverRolledBack)
{
    Loop loop("rollback-healthy", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.healthy + 2);
    std::string tx;
    for (const auto& entry : fs::directory_iterator(s.root / ".update")) {
        if (entry.is_directory()) {
            tx = entry.path().filename().string();
        }
    }
    ASSERT_FALSE(tx.empty());
    StdFs disk;
    Rig rig(s, disk, "eeeeeeee");
    EXPECT_TRUE(rollback(s.root, tx, rig.services()).ok());
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << "the new version proved it starts";
}

TEST(Rollback, ARunningNewProgramIsMovedOutOfTheWayNotDeleted)
{
    // A health-check rollback happens while the new version may still be running: a running
    // program cannot be deleted, so rollback renames it. A file system that refuses removeFile
    // (as Windows does for a running image) must therefore not stop the rollback.
    Loop loop("rollback-running", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.pending); // every file is in place; the pending record is not written
    StdFs disk;
    class NoDelete final : public ForwardingFs {
      public:
        using ForwardingFs::ForwardingFs;
        core::Status removeFile(const Path& path) override
        {
            // Only the files of the install are "running": the clean-up of the folder is free.
            if (path.string().find(".update") == std::string::npos) {
                return osError(core::ErrorCode::FileExportFailure, "access denied", path, 5);
            }
            return inner_.removeFile(path);
        }
    } refusing(disk);
    Rig rig(s, refusing, "eeeeeeee");
    const core::Status status = recoverInProcess(s.root, rig.services());
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
}

// ---- an update started over an interrupted one --------------------------------------------------------------------------------

namespace {

std::string onlyTransaction(const Scenario& s)
{
    std::string tx;
    for (const auto& entry : fs::directory_iterator(s.root / ".update")) {
        if (entry.is_directory()) {
            tx = entry.path().filename().string();
        }
    }
    return tx;
}

} // namespace

TEST(RecoverBeforeApply, AnUpdateStartedOverAnInterruptedOneFirstPutsTheOldVersionBack)
{
    // tx1 died in the middle of its commit and nothing has run recovery yet. A second update
    // planned over that half-swapped tree would succeed, and the next start would roll the
    // OLD files of tx1 over the new version: a mix, with the wrong installed.json.
    Loop loop("apply-over-interrupted", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.renames[p.renames.size() / 2]);
    const std::string crashed = treeHash(s.root);
    ASSERT_NE(crashed, s.v1Hash);
    ASSERT_NE(crashed, s.v2Hash) << "half swapped";

    StdFs disk;
    Rig second(s, disk, "bbbbbbbb");
    const ApplyResult result = second.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty()) << "and the interrupted transaction's folder is gone";
}

TEST(RecoverBeforeApply, AnInterruptedUpdateThatCannotBePutBackStopsTheNewOneBeforeItTouchesAnything)
{
    Loop loop("apply-over-stuck", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.renames[p.renames.size() / 2]);
    const std::string crashed = treeHash(s.root);

    StdFs disk;
    HookFs hooked(disk);
    hooked.beforeRename = [](const Path& from, const Path&) -> std::optional<core::Status> {
        return injected(32, from); // a file that stays locked
    };
    Rig second(s, hooked, "bbbbbbbb");
    const ApplyResult result = second.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(result.reason, "interrupted_update") << result.detail;
    EXPECT_NE(result.detail.find("katana_updater --recover"), std::string::npos) << result.detail;
    EXPECT_EQ(treeHash(s.root), crashed) << "nothing of the new update was done";
    EXPECT_FALSE(isPresent(s.root / ".update" / "bbbbbbbb")) << "and it made no folder of its own";
    EXPECT_TRUE(second.registry.runOnce.empty());

    // The old files are still in the first transaction's folder, and the next try finishes it.
    ASSERT_TRUE(recoverClean(s).ok());
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
}

// ---- a pending folder whose helper died, met by the next update ------------------------------------------------------------
//
// A swap settles, in its first step, an earlier update that is in place and was never said to
// start (apply.hpp, step 1; the rule is recover_internal.hpp's settlePending): healthy when the
// marker of that update is there, and otherwise nothing new is started over it. So such a
// folder never outlives the next update, and there is one at most.

namespace {

// v1 installed and v2 staged, from small specifications; `expected` is v2's tree.
void buildTinyLoop(Scenario& s)
{
    s.v1 = tiny("0.1.0", 100);
    s.v2 = tiny("0.2.0", 200);
    writeInstall(s.root, s.v1);
    writeStage(s.stage, s.v2);
    writeInstall(s.expected, s.v2);
    s.v1Hash = treeHash(s.root);
    s.v1Snapshot = snapshot(s.root);
    s.v2Hash = treeHash(s.expected);
}

// Every entry of the install folder, its working folder .update INCLUDED (snapshot() leaves that
// one out): what "the install is exactly as it was" compares, journal and old files too. Only
// the lock file is left out: asking for the lock makes it, and an empty lock file is no state.
std::map<std::string, std::string> wholeInstall(const Scenario& s)
{
    std::map<std::string, std::string> entries = snapshot(s.root.parent_path());
    const std::string prefix = s.root.filename().generic_string() + "/";
    for (auto it = entries.begin(); it != entries.end();) {
        const bool inside = it->first.rfind(prefix, 0) == 0;
        const bool lock = it->first == prefix + ".update/lock";
        it = (!inside || lock) ? entries.erase(it) : std::next(it);
    }
    return entries;
}

} // namespace

TEST(RecoverSuperseded, AnUpdateThatProvesHealthyDeletesThePendingFolderAnEarlierHelperLeft)
{
    REQUIRE_CAN_SIGN();
    Loop loop("superseded", buildTinyLoop);
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);

    // tx1 committed and passed its health check, and its helper died before it wrote HEALTHY.
    crashAt(loop, p.healthy);
    ASSERT_EQ(journalStateOf(s.root), JournalState::CommittedPendingHealth);
    ASSERT_EQ(treeHash(s.root), s.v2Hash);
    const std::string first = onlyTransaction(s);
    ASSERT_EQ(first, "a1b2c3d4");
    writeText(s.state / ("run-" + first) / "katana_updater.exe", "the dead helper's copy");
    ASSERT_TRUE(isPresent(healthMarkerPath(s.state, first))) << "written when it was started";

    // A later release, installed by a window that was running tx1's version.
    const ReleaseSpec v3 = tiny("0.3.0", 300);
    fs::remove_all(s.stage);
    writeStage(s.stage, v3);
    const Path expected3 = s.scratch.path() / "expected3";
    writeInstall(expected3, v3);

    StdFs disk;
    Rig second(s, disk, "bbbbbbbb");
    const ApplyResult result = second.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), treeHash(expected3));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty())
        << "the earlier folder, with the old files of the release before, is gone with its own";
    EXPECT_FALSE(isPresent(s.state / ("run-" + first))) << "and so is its helper copy";
    EXPECT_FALSE(isPresent(healthMarkerPath(s.state, first))) << "and its marker";
}

TEST(RecoverSuperseded, AnEarlierUpdateWhoseMarkerIsThereIsSettledFirstWhateverBecomesOfTheNextOne)
{
    // Until 2026-10-14 this case was named AnUpdateThatDoesNotProveHealthyLeavesThePendingFolder
    // WhereItWas and expected the earlier folder to stay: a later update swept it only when the
    // later one proved healthy. The rule changed, not the code's output: an earlier update with
    // its marker is settled healthy in the next swap's first step, on that marker alone, and
    // what the later release then does - here it does not start - is another matter.
    REQUIRE_CAN_SIGN();
    Loop loop("superseded-not", buildTinyLoop);
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.healthy);
    ASSERT_EQ(journalStateOf(s.root), JournalState::CommittedPendingHealth);
    const std::string first = onlyTransaction(s);
    writeText(s.state / ("run-" + first) / "katana_updater.exe", "the dead helper's copy");
    ASSERT_TRUE(isPresent(healthMarkerPath(s.state, first))) << "the premise: its marker is there";

    fs::remove_all(s.stage);
    writeStage(s.stage, tiny("0.3.0", 300));
    StdFs disk;
    Rig second(s, disk, "bbbbbbbb");
    second.launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
        SpawnResult bad;
        bad.exitCode = 1; // the third release does not start
        return bad;
    };
    const ApplyResult result = second.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::RolledBack) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << "back to the second release";
    EXPECT_TRUE(leftoversInUpdate(s.root).empty())
        << "the second release had said that it starts: its folder went before the third was planned";
    EXPECT_FALSE(isPresent(s.state / ("run-" + first))) << "and with it its dead helper's copy";
    EXPECT_FALSE(isPresent(healthMarkerPath(s.state, first))) << "and its marker";
}

TEST(RecoverSuperseded, AnEarlierUpdateThatWasNeverSaidToStartStopsTheNextOneBeforeItTouchesAnything)
{
    REQUIRE_CAN_SIGN();
    Loop loop("superseded-unproved", buildTinyLoop);
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.healthy);
    ASSERT_EQ(journalStateOf(s.root), JournalState::CommittedPendingHealth);
    ASSERT_EQ(treeHash(s.root), s.v2Hash);
    const std::string first = onlyTransaction(s);
    writeText(s.state / ("run-" + first) / "katana_updater.exe", "the dead helper's copy");
    // Nothing says that the second release ever started: its marker is not there.
    fs::remove(healthMarkerPath(s.state, first));

    const ReleaseSpec v3 = tiny("0.3.0", 300);
    fs::remove_all(s.stage);
    writeStage(s.stage, v3);
    const std::map<std::string, std::string> installBefore = wholeInstall(s);
    const std::map<std::string, std::string> stageBefore = snapshot(s.stage);

    StdFs disk;
    Rig second(s, disk, "bbbbbbbb");
    const ApplyResult result = second.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(result.reason, "interrupted_update") << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << "the second release is still what is installed";
    EXPECT_FALSE(isPresent(s.root / ".update" / "bbbbbbbb")) << "the new update made no folder of its own";
    // Nothing touched: the earlier update's folder, journal and old files are as they were - it
    // was not put back, which a swap never does to a release somebody may be running ...
    EXPECT_EQ(difference(installBefore, wholeInstall(s)), "");
    EXPECT_EQ(journalStateOf(s.root), JournalState::CommittedPendingHealth);
    // ... nothing was started to find out, no RunOnce entry was registered, and what its dead
    // helper left in U is still there for whoever settles it.
    EXPECT_TRUE(second.launcher.ran.empty());
    EXPECT_TRUE(second.registry.runOnceHistory.empty());
    EXPECT_TRUE(isPresent(s.state / ("run-" + first) / "katana_updater.exe"));
    EXPECT_EQ(difference(stageBefore, snapshot(s.stage)), "") << "and the download is kept";

    // Once the second release has said that it starts - its window writes the marker - the very
    // same update goes through.
    writeText(healthMarkerPath(s.state, first), "ok\n");
    const Path expected3 = s.scratch.path() / "expected3";
    writeInstall(expected3, v3);
    Rig third(s, disk, "cccccccc");
    const ApplyResult again = third.run();
    ASSERT_EQ(again.outcome, ApplyOutcome::Committed) << again.reason << ": " << again.detail;
    EXPECT_EQ(treeHash(s.root), treeHash(expected3));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

// ---- an update that is in place and was never said to start ------------------------------------------------------------------
//
// A helper that dies between its COMMITTED_PENDING_HEALTH record and its HEALTHY one leaves a
// whole new version that nothing has settled (recover.hpp, "AN UPDATE THAT IS INSTALLED AND WAS
// NEVER SAID TO START"). What follows holds the rule that settles it (recover_internal.hpp,
// settlePending) and two of its callers - settleUnproved, which is `katana_updater --recover`,
// and confirmStartedWith, the window - to the specification, worked out by hand:
//   - WHICH TREE: the new version's, built from the release in a folder of its own (s.v2Hash),
//     or the old one's, the snapshot taken before the update (s.v1Hash). Never what the code
//     left behind.
//   - "NOTHING WRITTEN" is zero changes counted by a FaultFs under the code, and the install
//     (its .update included) and the per-user folder compared entry by entry, byte by byte.
//   - THE ORDER of the registry against the disk is read off a registry that notes, at each
//     call, how many changes to the disk had been made by then.
//   - THE RUNONCE COMMAND is written out as apply.hpp's step 4 gives it.
// The state itself is not written by hand: the fixture update is applied on a FaultFs that dies
// before the operation a clean run's log names as the HEALTHY append.

namespace {

struct Unproved {
    explicit Unproved(const std::string& label)
        : loop(label, [](Scenario& s) { buildFixtureScenario(s); }), points(measureCleanApply(loop))
    {
        again();
    }

    // That state once more, from the pristine install (a Loop puts back the install and the
    // stage, and empties the per-user folder).
    void again()
    {
        crashAt(loop, points.healthy);
        transaction = onlyTransaction(loop.scenario());
        marker = healthMarkerPath(loop.scenario().state, transaction);
        helper = loop.scenario().state / ("run-" + transaction) / "katana_updater.exe";
        // What the dead helper ran from. It is only ever looked for and deleted here, never
        // started: a test starts no file that is not a program.
        writeText(helper, "the dead helper's copy");
    }

    // What every case starts from; one that did not would prove nothing.
    void expectPremise()
    {
        ASSERT_EQ(journalStateOf(s().root), JournalState::CommittedPendingHealth);
        ASSERT_EQ(treeHash(s().root), s().v2Hash) << "every file is the new version's";
        ASSERT_TRUE(isPresent(marker)) << "the health run wrote its marker before the helper died";
        ASSERT_EQ(leftoversInUpdate(s().root), std::vector<std::string>{transaction});
    }

    Scenario& s() { return loop.scenario(); }
    [[nodiscard]] Path work() { return s().root / ".update" / transaction; }
    // `"<helper>" --recover <R>`: the helper in quotes, and the install folder quoted when it
    // needs it, both with the separators of the system - it is the shell that reads the command
    // at the next logon (apply.hpp, step 4; TheRunOnceEntryIsRegisteredBeforeTheSwap... in
    // test_apply.cpp holds the swap to the same text).
    [[nodiscard]] std::string command()
    {
        Path program = helper;
        Path folder = s().root;
        return "\"" + core::pathToUtf8(program.make_preferred()) + "\" --recover " +
               detail::quoteArgument(core::pathToUtf8(folder.make_preferred()));
    }

    Loop loop;
    Points points;
    std::string transaction;
    Path marker;
    Path helper;
};

// settleUnproved as `katana_updater --recover` calls it: the helper is the copy the swap ran
// from, and the marker is looked for in the per-user folder the swap used.
Settled settle(Unproved& u, const ApplyServices& services)
{
    return settleUnproved(u.s().root, u.helper, u.s().state, services);
}

// The same, on `fs`, until it ends or "dies".
std::optional<SimulatedCrash> settleUntilCrash(Unproved& u, FsOps& fs, Settled* settled = nullptr)
{
    Rig rig(u.s(), fs, "eeeeeeee");
    try {
        const Settled ended = settle(u, rig.services());
        if (settled != nullptr) {
            *settled = ended;
        }
    } catch (const SimulatedCrash& crash) {
        return crash;
    }
    return std::nullopt;
}

// What a machine does after a program died while it was settling: the next
// `katana_updater --recover`, which is a recovery and then a settlement (src/katana_updater/
// main.cpp, runRecover), both on a file system that works.
void recoverThenSettle(Unproved& u)
{
    ASSERT_TRUE(recoverClean(u.s()).ok());
    StdFs clean;
    ASSERT_FALSE(settleUntilCrash(u, clean).has_value());
}

// A registry that also writes down, for each call, how many changes to the disk had been made
// by then (the length of a LogFs's log): the order of the registry against the file system.
class TimedRegistry final : public RegistryOps {
  public:
    explicit TimedRegistry(const LogFs& log) : log_(log) {}

    struct Call {
        std::string what;
        std::string name;
        std::string value;
        std::size_t changesBefore = 0;
    };
    std::optional<Path> location;
    std::vector<Call> calls;

    core::Result<std::optional<Path>> installerLocation() const override { return location; }
    core::Status setRunOnce(std::string_view name, std::string_view command) override
    {
        calls.push_back({"setRunOnce", std::string(name), std::string(command), log_.log.size()});
        return {};
    }
    core::Status clearRunOnce(std::string_view name) override
    {
        calls.push_back({"clearRunOnce", std::string(name), std::string(), log_.log.size()});
        return {};
    }
    core::Status setInstalledVersion(std::string_view version, std::string_view displayName) override
    {
        calls.push_back({"setInstalledVersion", std::string(displayName), std::string(version),
                         log_.log.size()});
        return {};
    }

  private:
    const LogFs& log_;
};

bool endsWith(const std::string& text, std::string_view tail)
{
    return text.size() >= tail.size() && text.compare(text.size() - tail.size(), tail.size(), tail) == 0;
}

// The path of a LogFs entry that is `<operation> <path>` and nothing more.
Path pathOf(const std::string& entry, std::string_view operation)
{
    return core::pathFromUtf8(entry.substr(operation.size() + 1));
}

// The RunOnce value as the registry double holds it when the logon net is in place.
std::map<std::string, std::string> armed(Unproved& u)
{
    return {{std::string(kRunOnceName), u.command()}};
}

} // namespace

TEST(SettleUnproved, AnUpdateWhoseMarkerIsThereIsSettledHealthyAndThePerUserFolderIsOnlyRead)
{
    Unproved u("settle-healthy");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    const std::map<std::string, std::string> userBefore = snapshot(s.state);
    const Path journal = u.work() / "journal";
    // The one record a settlement appends: HEALTHY, numbered after the records that are there.
    const std::string healthyRecord =
        encodeRecord(parseJournal(readText(journal)).records.size() + 1, {JournalOp::Healthy, {}});

    StdFs disk;
    LogFs log(disk);
    Rig rig(s, log, "eeeeeeee");
    EXPECT_EQ(settle(u, rig.services()), Settled::Healthy);

    EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty()) << "its folder is gone";
    // `--recover` is started by headless programs too: U is read, for the marker, and that is
    // all. The marker itself and the dead helper's copy are still there.
    EXPECT_EQ(difference(userBefore, snapshot(s.state)), "") << "the per-user folder was changed";
    EXPECT_TRUE(isPresent(u.marker));
    // The logon net: put back before anything else, and taken away once no folder is left.
    EXPECT_EQ(rig.registry.runOnceHistory, std::vector<std::string>{u.command()});
    EXPECT_TRUE(rig.registry.runOnce.empty());
    EXPECT_EQ(rig.registry.runOnceCleared, 1);
    EXPECT_TRUE(rig.registry.versions.empty()) << "an archive install has no entry in Settings > Apps";
    EXPECT_TRUE(rig.launcher.ran.empty()) << "no settler starts a program";
    EXPECT_TRUE(rig.launcher.started.empty());

    // On the disk: one flushed record first, then the folder, its journal the last file to go.
    ASSERT_FALSE(log.log.empty());
    const std::string& first = log.log.front();
    ASSERT_TRUE(first.rfind("appendDurable ", 0) == 0 && endsWith(first, " " + healthyRecord)) << first;
    EXPECT_EQ(core::pathFromUtf8(first.substr(14, first.size() - 14 - 1 - healthyRecord.size())), journal);
    const std::vector<std::uint64_t> removals = log.all("removeFile");
    ASSERT_FALSE(removals.empty());
    EXPECT_EQ(pathOf(log.log[removals.back() - 1], "removeFile"), journal)
        << "a crash part way leaves a folder that still says what it is";
    EXPECT_EQ(pathOf(log.log.back(), "removeEmptyDirectory"), u.work());

    // Again: there is nothing to settle, so nothing is asked for and nothing written.
    WatchedFs watched(disk);
    FaultFs counting(watched);
    Rig again(s, counting, "eeeeeeee");
    EXPECT_EQ(settle(u, again.services()), Settled::Nothing);
    EXPECT_EQ(counting.mutatingOperations(), 0u);
    EXPECT_EQ(watched.locks, 0) << "not even the lock";
    EXPECT_TRUE(again.registry.runOnceHistory.empty());
    EXPECT_EQ(again.registry.runOnceCleared, 0);
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
}

TEST(SettleUnproved, TheLogonNetIsPutBackFirstAndTheVersionIsRewrittenBeforeTheRecordIsWritten)
{
    Unproved u("settle-order");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    StdFs disk;
    LogFs log(disk);
    Rig rig(s, log, "eeeeeeee");
    TimedRegistry registry(log);
    registry.location = s.root; // HKCU Software\Jarada\Katana equals R: an installer's install
    const ApplyServices services{log, rig.processes, rig.launcher, registry, testTrust(), rig.timings};
    ASSERT_EQ(settle(u, services), Settled::Healthy);
    ASSERT_FALSE(log.log.empty());
    ASSERT_NE(log.log.front().find(" HEALTHY "), std::string::npos) << log.log.front();

    ASSERT_EQ(registry.calls.size(), 3u);
    // FIRST the RunOnce entry, with nothing on the disk changed yet: when Windows started this
    // run from the entry it had already taken the entry away.
    EXPECT_EQ(registry.calls[0].what, "setRunOnce");
    EXPECT_EQ(registry.calls[0].name, std::string(kRunOnceName));
    EXPECT_EQ(registry.calls[0].value, u.command());
    EXPECT_EQ(registry.calls[0].changesBefore, 0u);
    // Then the version Settings > Apps shows - the fixtures' v2 is 0.10.0 - before HEALTHY is
    // written (the first change to the disk, above): the swap itself rewrites it only once the
    // proof is there, and it may have died before it did.
    EXPECT_EQ(registry.calls[1].what, "setInstalledVersion");
    EXPECT_EQ(registry.calls[1].value, "0.10.0");
    EXPECT_EQ(registry.calls[1].name, "Katana 0.10.0");
    EXPECT_EQ(registry.calls[1].changesBefore, 0u);
    // And the entry is taken away last, when the folder is gone.
    EXPECT_EQ(registry.calls[2].what, "clearRunOnce");
    EXPECT_EQ(registry.calls[2].name, std::string(kRunOnceName));
    EXPECT_EQ(registry.calls[2].changesBefore, log.log.size());
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

TEST(SettleUnproved, AnUpdateWithNoMarkerAndNobodyRunningIsPutBackAndNothingPerUserSaysSo)
{
    Unproved u("settle-rollback");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    fs::remove(u.marker); // the new version never said that it starts
    // The tree's seed is the person's (v1's), not the one the release lists: a seed is written
    // only when absent. The tree is that release all the same - or nothing here would be put back.
    const std::string seed = readText(s.root / "etc/katana/settings.ini");
    ASSERT_EQ(seed, readText(fixtureFolder() / "install/v1/etc/katana/settings.ini"));
    ASSERT_NE(seed, readText(fixtureFolder() / "install/v2/etc/katana/settings.ini"));
    const std::map<std::string, std::string> userBefore = snapshot(s.state);

    StdFs disk;
    LogFs log(disk);
    Rig rig(s, log, "eeeeeeee");
    EXPECT_EQ(settle(u, rig.services()), Settled::RolledBack);

    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty()) << "its folder is gone";
    EXPECT_EQ(rig.processes.scans.load(), 2) << "asked before the files were hashed, and again as the last thing";
    // The rollback is the one every interrupted update gets: its two records, the first of them
    // before anything moves.
    EXPECT_EQ(log.find("appendDurable", "ROLLING_BACK"), 1u);
    EXPECT_GT(log.find("appendDurable", "ROLLED_BACK"), 1u);
    EXPECT_EQ(rig.registry.runOnceHistory, std::vector<std::string>{u.command()});
    EXPECT_TRUE(rig.registry.runOnce.empty());
    EXPECT_EQ(rig.registry.runOnceCleared, 1);
    EXPECT_TRUE(rig.registry.versions.empty());
    EXPECT_TRUE(rig.launcher.ran.empty()) << "the old version is not started either: no settler starts a program";
    EXPECT_TRUE(rig.launcher.started.empty());
    // What such a run cannot record is not recorded: no last.json, and the release is not added
    // to `blocked` (no state.json is made). U is as it was.
    EXPECT_EQ(difference(userBefore, snapshot(s.state)), "") << "the per-user folder was changed";
    EXPECT_FALSE(isPresent(s.state / "last.json"));
    EXPECT_FALSE(isPresent(s.state / "state.json"));

    FaultFs counting(disk);
    Rig again(s, counting, "eeeeeeee");
    EXPECT_EQ(settle(u, again.services()), Settled::Nothing);
    EXPECT_EQ(counting.mutatingOperations(), 0u);
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
}

TEST(SettleUnproved, NothingIsPutBackUnderARunningProgramOrOverFilesThatAreNoLongerThatRelease)
{
    Unproved u("settle-left");
    Scenario& s = u.s();
    static const Trust noKeys{};

    struct Row {
        const char* name;
        std::function<void()> arrange; // on top of the marker being gone
        std::vector<ScriptedProcesses::Answer> answers;
        const Trust* trust = nullptr; // nullptr: the keys the release is signed with
        bool evidence = true;         // false: no per-user folder is named at all
    };
    const ScriptedProcesses::Answer cli = somebody(4412, "C:/k/bin/katana_cli.exe");
    const std::vector<Row> rows = {
        {"a program of the install is running at the first scan", {}, {cli}},
        {"one was started while the files were hashed: at the second scan only", {}, {nobody(), cli}},
        {"the first scan fails, which is not 'nobody'", {}, {noAnswer()}},
        {"the second scan fails", {}, {nobody(), noAnswer()}},
        {"one file of the release was overwritten",
         [&] {
             // Deleted and written anew, never written in place: a Loop's install shares the
             // bytes of the files an update keeps with its pristine copy (replaceTree).
             fs::remove(s.root / "share/katana/readme.txt");
             writeText(s.root / "share/katana/readme.txt", "changed by hand\n");
         },
         {}},
        {"one file of the release is gone", [&] { fs::remove(s.root / "bin/libnew-2.dll"); }, {}},
        {"the journal's PLAN names another release than the manifest in the tree",
         [&] {
             // The same journal with another `toSequence` (the fixtures' v2 is 1792312800), written
             // record by record so that every number and check is right.
             const Path journal = u.work() / "journal";
             ParsedJournal parsed = parseJournal(readText(journal));
             ASSERT_FALSE(parsed.records.empty());
             ASSERT_EQ(parsed.records.front().args.size(), 3u);
             ASSERT_EQ(parsed.records.front().args[2], "1792312800");
             parsed.records.front().args[2] = "999";
             std::string text;
             std::uint64_t n = 1;
             for (const JournalRecord& record : parsed.records) {
                 text += encodeRecord(n++, record);
             }
             writeText(journal, text);
         },
         {}},
        {"the manifest in the tree is signed by a key this helper does not trust", {}, {}, &noKeys},
        {"something that is not a file has the marker's name", [&] { fs::create_directories(u.marker); }, {}},
        {"no per-user folder is named, so there is nowhere to look for a marker", {}, {}, nullptr, false},
    };
    for (const Row& row : rows) {
        SCOPED_TRACE(row.name);
        u.again();
        ASSERT_NO_FATAL_FAILURE(u.expectPremise());
        fs::remove(u.marker);
        if (row.arrange) {
            ASSERT_NO_FATAL_FAILURE(row.arrange());
        }
        ASSERT_EQ(journalStateOf(s.root), JournalState::CommittedPendingHealth);
        const std::map<std::string, std::string> installBefore = wholeInstall(s);
        const std::map<std::string, std::string> userBefore = snapshot(s.state);

        StdFs disk;
        FaultFs counting(disk);
        Rig rig(s, counting, "eeeeeeee");
        ScriptedProcesses processes(row.answers);
        const ApplyServices services{counting,     processes,
                                     rig.launcher, rig.registry,
                                     row.trust != nullptr ? *row.trust : testTrust(),
                                     rig.timings};
        EXPECT_EQ(settleUnproved(s.root, u.helper, row.evidence ? s.state : Path(), services), Settled::Left);

        EXPECT_EQ(counting.mutatingOperations(), 0u) << "nothing is written for an update that is left";
        EXPECT_EQ(difference(installBefore, wholeInstall(s)), "") << "the install was changed";
        EXPECT_EQ(journalStateOf(s.root), JournalState::CommittedPendingHealth);
        EXPECT_EQ(difference(userBefore, snapshot(s.state)), "") << "the per-user folder was changed";
        // The logon net is in place, and stays: the next logon comes back to decide.
        EXPECT_EQ(rig.registry.runOnce, armed(u));
        EXPECT_EQ(rig.registry.runOnceCleared, 0);
        EXPECT_TRUE(rig.launcher.ran.empty());
        EXPECT_TRUE(rig.launcher.started.empty());
    }
}

TEST(SettleUnproved, AHeldLockIsALiveHelperAndNothingIsWrittenNotEvenTheRunOnceEntry)
{
    Unproved u("settle-busy");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    // With no marker and nobody running this update would be put back - were the lock free. It
    // is held: its helper is alive, and in the middle of the very health run that would write it.
    fs::remove(u.marker);
    const std::map<std::string, std::string> installBefore = wholeInstall(s);
    const std::map<std::string, std::string> userBefore = snapshot(s.state);

    StdFs disk;
    BusyLockFs busy(disk, -1);
    FaultFs counting(busy);
    Rig rig(s, counting, "eeeeeeee");
    rig.timings.waitForHolders = std::chrono::milliseconds(300);
    EXPECT_EQ(settle(u, rig.services()), Settled::Busy);

    EXPECT_GT(busy.attempts, 1) << "the lock is polled, as a recovery polls it: one refusal is not believed";
    EXPECT_EQ(counting.mutatingOperations(), 0u);
    EXPECT_EQ(difference(installBefore, wholeInstall(s)), "") << "the install was changed";
    EXPECT_EQ(difference(userBefore, snapshot(s.state)), "") << "the per-user folder was changed";
    EXPECT_TRUE(rig.registry.runOnceHistory.empty()) << "the registry too is the live helper's";
    EXPECT_EQ(rig.registry.runOnceCleared, 0);
    EXPECT_EQ(rig.processes.scans.load(), 0) << "nobody is even asked who is running";
}

TEST(SettleUnproved, ALockThatIsHeldOnlyForAMomentIsWaitedForAndTheUpdateIsThenSettled)
{
    Unproved u("settle-lock-blip");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    StdFs disk;
    BusyLockFs busy(disk, 3); // a scanner that has the lock file open for a moment
    Rig rig(s, busy, "eeeeeeee");
    rig.timings.waitForHolders = std::chrono::seconds(5);
    EXPECT_EQ(settle(u, rig.services()), Settled::Healthy);
    EXPECT_EQ(busy.attempts, 4) << "three refusals, then the lock";
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

TEST(SettleUnproved, APuttingBackThatCannotFinishSaysSoAndTheNextRecoveryFinishesIt)
{
    Unproved u("settle-incomplete");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    fs::remove(u.marker);

    StdFs disk;
    HookFs hooked(disk);
    hooked.beforeRename = [](const Path& from, const Path&) -> std::optional<core::Status> {
        return injected(32, from); // a file that stays locked
    };
    Rig rig(s, hooked, "eeeeeeee");
    EXPECT_EQ(settle(u, rig.services()), Settled::RollbackIncomplete);
    // The journal says what every later start finishes (journal.hpp, needsRecovery), the folder
    // with the old files is still there, and the logon net stays.
    EXPECT_EQ(journalStateOf(s.root), JournalState::RollingBack);
    EXPECT_EQ(leftoversInUpdate(s.root), std::vector<std::string>{u.transaction});
    EXPECT_EQ(rig.registry.runOnce, armed(u));
    EXPECT_EQ(rig.registry.runOnceCleared, 0);

    // The next start of anything, with the file free: an ordinary recovery, which needs no marker
    // and asks nobody who is running.
    ASSERT_TRUE(recoverClean(s).ok());
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

TEST(SettleUnproved, TheProcessThatSettlesIsNotAProgramItGivesWayTo)
{
    // `katana_updater --recover` may be the install's own bin/katana_updater: recover() falls back
    // to it when the copy in U is gone, and it is what a person types. The scan then names this
    // very process, at both of its questions, and nobody else: that is "nobody is running".
    Unproved u("settle-self");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    fs::remove(u.marker);
    StdFs disk;
    Rig rig(s, disk, "eeeeeeee");
    rig.processes.pid = 999;
    rig.processes.holders = {RunningProgram{999, core::pathFromUtf8("C:/k/bin/katana_updater.exe")}};
    rig.processes.holdersFor = -1; // whenever it is asked
    EXPECT_EQ(settle(u, rig.services()), Settled::RolledBack);
    EXPECT_EQ(rig.processes.scans.load(), 2);
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

TEST(SettleUnproved, AFolderThatWouldNotGoKeepsItsJournalAndTheLogonEntryAndNothingIsQueuedPerUser)
{
    Unproved u("settle-held-file");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    const std::map<std::string, std::string> userBefore = snapshot(s.state);

    StdFs disk;
    // A scanner is reading one of the old files in the transaction's folder: it cannot be deleted.
    class Holding final : public ForwardingFs {
      public:
        using ForwardingFs::ForwardingFs;
        core::Status removeFile(const Path& path) override
        {
            if (path.filename() == "katana.exe" && path.string().find(".update") != std::string::npos) {
                return osError(core::ErrorCode::FileExportFailure, "sharing violation", path, 32);
            }
            return inner_.removeFile(path);
        }
    } holding(disk);
    Rig rig(s, holding, "eeeeeeee");
    EXPECT_EQ(settle(u, rig.services()), Settled::Healthy)
        << "the update is settled: a folder that will not go is not an update that is not";
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
    // The journal is the last thing to go, and something stayed: the folder still says what it is.
    EXPECT_EQ(leftoversInUpdate(s.root), std::vector<std::string>{u.transaction});
    EXPECT_EQ(journalStateOf(s.root), JournalState::Healthy);
    // A transaction folder is left, so the logon entry is: that logon sweeps it. And what would
    // not go is NOT queued in U/cleanup.list - this run only reads the per-user folder.
    EXPECT_EQ(rig.registry.runOnce, armed(u));
    EXPECT_EQ(rig.registry.runOnceCleared, 0);
    EXPECT_EQ(difference(userBefore, snapshot(s.state)), "") << "the per-user folder was changed";
    EXPECT_FALSE(isPresent(s.state / "cleanup.list"));

    // The next start of anything, with the file free, sweeps what is left.
    ASSERT_TRUE(recoverClean(s).ok());
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
}

TEST(SettleUnproved, WithNoSuchUpdateItAsksForNoLockAndWritesNothing)
{
    {
        // An install that never updated.
        Scenario s("settle-nothing");
        s.v1 = tiny("0.1.0", 1);
        writeInstall(s.root, s.v1);
        StdFs disk;
        WatchedFs watched(disk);
        FaultFs counting(watched);
        Rig rig(s, counting, "eeeeeeee");
        EXPECT_EQ(settleUnproved(s.root, rig.request.helper, s.state, rig.services()), Settled::Nothing);
        EXPECT_EQ(counting.mutatingOperations(), 0u);
        EXPECT_EQ(watched.locks, 0);
        EXPECT_TRUE(rig.registry.runOnceHistory.empty());
        EXPECT_EQ(rig.registry.runOnceCleared, 0);
        EXPECT_FALSE(isPresent(s.root / ".update")) << "it does not make the folder";
        EXPECT_EQ(settleUnproved(Path(), rig.request.helper, s.state, rig.services()), Settled::Nothing)
            << "no install folder is not the .update of the current folder";
        EXPECT_EQ(watched.locks, 0);
    }
    {
        // An update that a crash cut off in the middle of its commit is recovery's to put back,
        // not this function's to judge: it is as it was.
        Loop loop("settle-not-mine", [](Scenario& s) { buildFixtureScenario(s); });
        Scenario& s = loop.scenario();
        const Points p = measureCleanApply(loop);
        crashAt(loop, p.renames[p.renames.size() / 2]);
        ASSERT_EQ(journalStateOf(s.root), JournalState::Committing);
        const std::map<std::string, std::string> installBefore = wholeInstall(s);
        StdFs disk;
        WatchedFs watched(disk);
        FaultFs counting(watched);
        Rig rig(s, counting, "eeeeeeee");
        EXPECT_EQ(settleUnproved(s.root, rig.request.helper, s.state, rig.services()), Settled::Nothing);
        EXPECT_EQ(counting.mutatingOperations(), 0u);
        EXPECT_EQ(watched.locks, 0);
        EXPECT_TRUE(rig.registry.runOnceHistory.empty());
        EXPECT_EQ(difference(installBefore, wholeInstall(s)), "");
    }
}

// ---- the same, cut off anywhere ------------------------------------------------------------------------------------------------
//
// Every step is safe against a crash or a power loss at any point. Each loop dies before EVERY
// change to the disk of one settlement, then does what the machine does next - a recovery and a
// settlement, on a file system that works - and requires the whole tree to be one version's:
//   a healthy settlement   always the new one (before HEALTHY is on disk the marker decides
//                          again; after it any start sweeps);
//   a putting back         always the old one (before ROLLING_BACK it is decided again, with
//                          the same answer; after it every start finishes the rollback);
//   a window's confirming  the old one while its marker is not yet written - nothing says the
//                          release starts, and nobody is running - and the new one from then on.
// A loop that died nowhere would pass, so each requires a death at every k, at operation k.

TEST(SettleLoops, DyingBeforeEveryChangeOfAHealthySettlementAlwaysEndsAtTheNewTree)
{
    Unproved u("settle-loop-healthy");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    std::uint64_t total = 0;
    {
        StdFs disk;
        FaultFs counting(disk);
        Settled settled = Settled::Nothing;
        ASSERT_FALSE(settleUntilCrash(u, counting, &settled).has_value());
        ASSERT_EQ(settled, Settled::Healthy);
        total = counting.mutatingOperations();
    }
    ASSERT_GE(total, 3u) << "a record, at least one file of the folder, the folder";

    for (std::uint64_t k = 1; k <= total; ++k) {
        SCOPED_TRACE("dying before change " + std::to_string(k) + " of " + std::to_string(total));
        u.again();
        ASSERT_NO_FATAL_FAILURE(u.expectPremise());
        const std::map<std::string, std::string> userBefore = snapshot(s.state);
        StdFs disk;
        FaultFs faults(disk);
        faults.crashBefore(k);
        const std::optional<SimulatedCrash> crash = settleUntilCrash(u, faults);
        ASSERT_TRUE(crash.has_value()) << "it did not die";
        ASSERT_EQ(crash->operation, k);

        ASSERT_NO_FATAL_FAILURE(recoverThenSettle(u));
        ASSERT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
        ASSERT_TRUE(leftoversInUpdate(s.root).empty());
        ASSERT_EQ(difference(userBefore, snapshot(s.state)), "") << "the per-user folder was changed";
    }
    // Past the last change there is nothing to die before.
    u.again();
    StdFs disk;
    FaultFs faults(disk);
    faults.crashBefore(total + 1);
    Settled settled = Settled::Nothing;
    EXPECT_FALSE(settleUntilCrash(u, faults, &settled).has_value());
    EXPECT_EQ(settled, Settled::Healthy);
    EXPECT_EQ(faults.mutatingOperations(), total);
}

TEST(SettleLoops, DyingBeforeEveryChangeOfAPuttingBackAlwaysEndsAtTheOldTree)
{
    Unproved u("settle-loop-rollback");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    std::uint64_t total = 0;
    {
        fs::remove(u.marker);
        StdFs disk;
        FaultFs counting(disk);
        Settled settled = Settled::Nothing;
        ASSERT_FALSE(settleUntilCrash(u, counting, &settled).has_value());
        ASSERT_EQ(settled, Settled::RolledBack);
        ASSERT_EQ(treeHash(s.root), s.v1Hash);
        total = counting.mutatingOperations();
    }
    ASSERT_GE(total, 10u) << "two records, two renames for each file that changed, the folder";

    for (std::uint64_t k = 1; k <= total; ++k) {
        SCOPED_TRACE("dying before change " + std::to_string(k) + " of " + std::to_string(total));
        u.again();
        ASSERT_NO_FATAL_FAILURE(u.expectPremise());
        fs::remove(u.marker);
        const std::map<std::string, std::string> userBefore = snapshot(s.state);
        StdFs disk;
        FaultFs faults(disk);
        faults.crashBefore(k);
        const std::optional<SimulatedCrash> crash = settleUntilCrash(u, faults);
        ASSERT_TRUE(crash.has_value()) << "it did not die";
        ASSERT_EQ(crash->operation, k);

        ASSERT_NO_FATAL_FAILURE(recoverThenSettle(u));
        ASSERT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
        ASSERT_TRUE(leftoversInUpdate(s.root).empty());
        ASSERT_EQ(difference(userBefore, snapshot(s.state)), "") << "the per-user folder was changed";
    }
}

TEST(SettleLoops, DyingBeforeEveryChangeOfAWindowsConfirmingEndsAtTheOldTreeBeforeItsMarkerAndTheNewOneAfter)
{
    Unproved u("settle-loop-confirm");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    std::uint64_t total = 0;
    std::uint64_t markerWritten = 0; // the change that puts the window's marker on disk
    {
        fs::remove(u.marker);
        StdFs disk;
        FaultFs counting(disk);
        LogFs log(counting);
        FakeRegistry registry;
        ASSERT_EQ(detail::confirmStartedWith(s.root, s.state, log, registry), Confirmed::Done);
        total = counting.mutatingOperations();
        ASSERT_EQ(log.log.size(), total);
        markerWritten = log.find("writeDurable", "healthy-");
        ASSERT_GT(markerWritten, 0u);
        ASSERT_LT(markerWritten, total);
    }

    std::uint64_t old = 0;
    std::uint64_t fresh = 0;
    for (std::uint64_t k = 1; k <= total; ++k) {
        SCOPED_TRACE("dying before change " + std::to_string(k) + " of " + std::to_string(total));
        u.again();
        ASSERT_NO_FATAL_FAILURE(u.expectPremise());
        fs::remove(u.marker);
        StdFs disk;
        FaultFs faults(disk);
        faults.crashBefore(k);
        FakeRegistry registry;
        std::optional<SimulatedCrash> crash;
        try {
            (void)detail::confirmStartedWith(s.root, s.state, faults, registry);
        } catch (const SimulatedCrash& died) {
            crash = died;
        }
        ASSERT_TRUE(crash.has_value()) << "it did not die";
        ASSERT_EQ(crash->operation, k);

        ASSERT_NO_FATAL_FAILURE(recoverThenSettle(u));
        // The changes before k were made: the marker's write is one of them, or it is not.
        const bool said = k > markerWritten;
        ASSERT_EQ(treeHash(s.root), said ? s.v2Hash : s.v1Hash)
            << "versus the old: " << difference(s.v1Snapshot, snapshot(s.root))
            << "versus the new: " << difference(snapshot(s.expected), snapshot(s.root));
        ASSERT_TRUE(leftoversInUpdate(s.root).empty());
        ++(said ? fresh : old);
    }
    // The loop reached both.
    EXPECT_EQ(old, markerWritten);
    EXPECT_EQ(fresh, total - markerWritten);
    EXPECT_GT(old, 0u);
    EXPECT_GT(fresh, 0u);
}

// ---- the window of the new version --------------------------------------------------------------------------------------------

TEST(ConfirmStarted, TheWindowOfTheNewVersionSettlesTheUpdateHealthyAndClearsWhatTheDeadHelperLeft)
{
    for (const bool markerThere : {true, false}) {
        SCOPED_TRACE(markerThere ? "the health run had written the marker" : "no marker: the window writes it");
        Unproved u(std::string("confirm-done-") + (markerThere ? "marker" : "none"));
        ASSERT_NO_FATAL_FAILURE(u.expectPremise());
        Scenario& s = u.s();
        if (!markerThere) {
            fs::remove(u.marker);
        }
        StdFs disk;
        LogFs log(disk);
        FakeRegistry registry;
        registry.runOnce[std::string(kRunOnceName)] = u.command(); // as the swap that died left it

        EXPECT_EQ(detail::confirmStartedWith(s.root, s.state, log, registry), Confirmed::Done);
        EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
        EXPECT_TRUE(leftoversInUpdate(s.root).empty()) << "its folder is gone";
        EXPECT_FALSE(isPresent(u.marker)) << "the marker has done its work";
        EXPECT_FALSE(isPresent(s.state / ("run-" + u.transaction))) << "nothing starts the dead helper's copy again";
        EXPECT_FALSE(isPresent(s.state / "cleanup.list")) << "nothing stayed, so nothing is listed";
        EXPECT_TRUE(registry.runOnce.empty()) << "the entry named the copy that is gone";
        EXPECT_EQ(registry.runOnceCleared, 1);
        EXPECT_TRUE(registry.runOnceHistory.empty()) << "a window registers nothing";

        // The proof is on disk before anything is settled by it.
        const std::uint64_t healthy = log.find("appendDurable", " HEALTHY ");
        const std::uint64_t written = log.find("writeDurable", "healthy-");
        ASSERT_GT(healthy, 0u);
        if (markerThere) {
            EXPECT_EQ(written, 0u) << "a marker that is there is not written again";
        } else {
            ASSERT_GT(written, 0u);
            EXPECT_LT(written, healthy);
            EXPECT_TRUE(endsWith(log.log[written - 1], " ok\n")) << "what --update-health writes";
        }
    }
}

TEST(ConfirmStarted, AHeldLockIsALiveHelperTheWindowAsksOnceAndChangesNothing)
{
    Unproved u("confirm-busy");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    fs::remove(u.marker);
    const std::map<std::string, std::string> installBefore = wholeInstall(s);
    const std::map<std::string, std::string> userBefore = snapshot(s.state);

    StdFs disk;
    BusyLockFs busy(disk, -1);
    FaultFs counting(busy);
    FakeRegistry registry;
    registry.runOnce[std::string(kRunOnceName)] = u.command();
    EXPECT_EQ(detail::confirmStartedWith(s.root, s.state, counting, registry), Confirmed::Busy);

    EXPECT_EQ(busy.attempts, 1) << "a window does not wait for the lock: it asks again later";
    EXPECT_EQ(counting.mutatingOperations(), 0u) << "not even its marker is written under a live helper";
    EXPECT_EQ(difference(installBefore, wholeInstall(s)), "") << "the install was changed";
    EXPECT_EQ(difference(userBefore, snapshot(s.state)), "") << "the per-user folder was changed";
    EXPECT_EQ(registry.runOnce, armed(u));
    EXPECT_EQ(registry.runOnceCleared, 0);
}

TEST(ConfirmStarted, WithNoSuchUpdateTheWindowAsksForNoLockAndWritesNothing)
{
    {
        // Every start but the one after a helper died: an install with nothing pending.
        Scenario s("confirm-nothing");
        s.v1 = tiny("0.1.0", 1);
        writeInstall(s.root, s.v1);
        StdFs disk;
        WatchedFs watched(disk);
        FaultFs counting(watched);
        FakeRegistry registry;
        registry.runOnce[std::string(kRunOnceName)] = "somebody's";
        EXPECT_EQ(detail::confirmStartedWith(s.root, s.state, counting, registry), Confirmed::Nothing);
        EXPECT_EQ(counting.mutatingOperations(), 0u);
        EXPECT_EQ(watched.locks, 0);
        EXPECT_EQ(registry.runOnceCleared, 0);
        EXPECT_EQ(registry.runOnce.size(), 1u);
        EXPECT_FALSE(isPresent(s.root / ".update"));
        EXPECT_TRUE(snapshot(s.state).empty()) << "nothing is made in the per-user folder";
    }
    {
        // An update cut off in the middle of its commit is not a version that starts: the window
        // confirms nothing of it (its own recover() has it put back).
        Loop loop("confirm-not-mine", [](Scenario& s) { buildFixtureScenario(s); });
        Scenario& s = loop.scenario();
        const Points p = measureCleanApply(loop);
        crashAt(loop, p.renames[p.renames.size() / 2]);
        ASSERT_EQ(journalStateOf(s.root), JournalState::Committing);
        const std::map<std::string, std::string> installBefore = wholeInstall(s);
        StdFs disk;
        WatchedFs watched(disk);
        FaultFs counting(watched);
        FakeRegistry registry;
        EXPECT_EQ(detail::confirmStartedWith(s.root, s.state, counting, registry), Confirmed::Nothing);
        EXPECT_EQ(counting.mutatingOperations(), 0u);
        EXPECT_EQ(watched.locks, 0);
        EXPECT_EQ(registry.runOnceCleared, 0);
        EXPECT_EQ(difference(installBefore, wholeInstall(s)), "");
    }
}

TEST(ConfirmStarted, WhatWouldNotGoOfTheDeadHelpersCopyIsListedForTheNextStart)
{
    Unproved u("confirm-leftover");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    StdFs disk;
    // The copy is held - a scanner is reading it - so it cannot be deleted, and neither can the
    // folder it is in.
    class Holding final : public ForwardingFs {
      public:
        using ForwardingFs::ForwardingFs;
        core::Status removeFile(const Path& path) override
        {
            if (path.filename() == "katana_updater.exe") {
                return osError(core::ErrorCode::FileExportFailure, "sharing violation", path, 32);
            }
            return inner_.removeFile(path);
        }
    } holding(disk);
    FakeRegistry registry;
    EXPECT_EQ(detail::confirmStartedWith(s.root, s.state, holding, registry), Confirmed::Done)
        << "the update is settled all the same";
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
    EXPECT_FALSE(isPresent(u.marker));
    EXPECT_TRUE(isPresent(u.helper)) << "the premise: it was held";

    // U/cleanup.list, one path to a line (state.hpp): the file that would not go, and its folder.
    std::set<Path> listed;
    std::istringstream lines(readText(s.state / "cleanup.list"));
    for (std::string line; std::getline(lines, line);) {
        if (!line.empty()) {
            listed.insert(core::pathFromUtf8(line));
        }
    }
    EXPECT_EQ(listed, (std::set<Path>{u.helper, u.helper.parent_path()}));
}

TEST(ConfirmStarted, AWindowThatCannotWriteItsMarkerSettlesNothingAndNeverPutsAnythingBack)
{
    Unproved u("confirm-no-marker");
    ASSERT_NO_FATAL_FAILURE(u.expectPremise());
    Scenario& s = u.s();
    fs::remove(u.marker);
    const std::map<std::string, std::string> installBefore = wholeInstall(s);

    StdFs disk;
    class NoMarker final : public ForwardingFs {
      public:
        using ForwardingFs::ForwardingFs;
        core::Status writeDurable(const Path& path, std::string_view bytes) override
        {
            if (path.filename().string().rfind("healthy-", 0) == 0) {
                return osError(core::ErrorCode::FileExportFailure, "access denied", path, 5);
            }
            return inner_.writeDurable(path, bytes);
        }
    } refusing(disk);
    FakeRegistry registry;
    registry.runOnce[std::string(kRunOnceName)] = u.command();
    // With no marker the rule has nothing to call it healthy by, and a window is never the one
    // to put an update back: it is left, and the window is told to ask again.
    EXPECT_EQ(detail::confirmStartedWith(s.root, s.state, refusing, registry), Confirmed::Busy);
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << "nothing was put back";
    EXPECT_EQ(difference(installBefore, wholeInstall(s)), "") << "the install was changed";
    EXPECT_EQ(journalStateOf(s.root), JournalState::CommittedPendingHealth);
    EXPECT_EQ(registry.runOnce, armed(u)) << "the logon net stays for an update that is not settled";
    EXPECT_EQ(registry.runOnceCleared, 0);
    EXPECT_TRUE(isPresent(u.helper)) << "and so does the helper the entry names";
}

// ---- a journal that is not whole ----------------------------------------------------------------------------------------------

namespace {

void tearJournal(const Path& journal, std::size_t bytes)
{
    std::string text = readText(journal);
    ASSERT_GT(text.size(), bytes);
    text.resize(text.size() - bytes);
    writeText(journal, text);
}

} // namespace

TEST(RecoverTorn, AJournalCutOffInTheMiddleOfAnyRecordOfTheCommitIsPutBackExactly)
{
    Loop loop("recover-torn-commit", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    std::size_t cases = 0;
    for (const std::size_t at : {p.renames.size() / 4, p.renames.size() / 2, 3 * p.renames.size() / 4,
                                 p.renames.size() - 1}) {
        for (const std::size_t cut : {1u, 2u, 5u, 12u, 40u}) {
            SCOPED_TRACE("crash before rename " + std::to_string(at) + ", last " + std::to_string(cut) +
                         " bytes of the journal lost");
            crashAt(loop, p.renames[at]);
            tearJournal(s.root / ".update" / onlyTransaction(s) / "journal", cut);
            ASSERT_TRUE(recoverClean(s).ok());
            EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
            EXPECT_TRUE(leftoversInUpdate(s.root).empty());
            ++cases;
        }
    }
    EXPECT_EQ(cases, 20u);
}

TEST(RecoverTorn, AJournalLostBeyondItsFirstRecordStillPutsEveryOldFileBack)
{
    // Damage in the middle of the file (the disk, not a torn write) leaves a journal that says
    // PLAN and nothing else, over a tree whose files have all moved. The journal cannot say
    // what to put back, but the folder can: every file under old/ was an old file of this
    // install, and deleting the folder without it would lose the file for good. Files the
    // release ADDED cannot be known from nothing, so they stay: the tree is the old version's
    // files and some new ones, never the old version's files missing.
    Loop loop("recover-damaged-plan", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.pending); // every file has moved, the pending record is not written
    const Path journal = s.root / ".update" / onlyTransaction(s) / "journal";
    const std::string text = readText(journal);
    writeText(journal, text.substr(0, text.find('\n') + 1));
    ASSERT_EQ(parseJournal(readText(journal)).state, JournalState::Planned);

    ASSERT_TRUE(recoverClean(s).ok());
    const std::map<std::string, std::string> after = snapshot(s.root);
    for (const auto& [name, what] : s.v1Snapshot) {
        const auto found = after.find(name);
        ASSERT_NE(found, after.end()) << "lost: " << name;
        EXPECT_EQ(found->second, what) << "not the old bytes: " << name;
    }
}

TEST(RecoverTorn, WhatIsUnderOldAndNoUpdateWouldPutThereIsNotRestoredAndNotAnError)
{
    Forgery f("recover-old-junk");
    // Names no update writes: a letter outside ASCII (the path rules are ASCII only), and a
    // first folder that is not bin, share, etc or lib.
    writeText(f.work / "old" / core::pathFromUtf8("bin/caf\xC3\xA9.dll"), "a name no update writes");
    writeText(f.work / "old/docs/readme.txt", "a folder no update has");
    f.journal({
        {JournalOp::Plan, {"deadbeef", "1", "2"}},
        {JournalOp::Replace, {"bin/lib.dll"}},
        {JournalOp::Committing, {}},
    });
    StdFs disk;
    Rig rig(f.s, disk, "eeeeeeee");
    EXPECT_TRUE(recoverInProcess(f.s.root, rig.services()).ok());
    EXPECT_EQ(readText(f.s.root / "bin/lib.dll"), "lib 0.1.0\n") << "the honest one was restored";
    EXPECT_FALSE(isPresent(f.s.root / core::pathFromUtf8("bin/caf\xC3\xA9.dll")));
    EXPECT_FALSE(isPresent(f.s.root / "docs"));
    const std::string log = readText(f.s.root / ".update" / "recovery.log");
    EXPECT_NE(log.find("caf"), std::string::npos) << log;
    EXPECT_NE(log.find("readme.txt"), std::string::npos) << log;
}

TEST(RecoverTorn, ADoubleDotInsideANameIsAnOrdinaryNameToRollback)
{
    Forgery f("recover-dots-in-name");
    writeText(f.s.root / "share/katana/a..b.dat", "the new a..b\n");
    writeText(f.work / "old/share/katana/a..b.dat", "the old a..b\n");
    f.journal({
        {JournalOp::Plan, {"deadbeef", "1", "2"}},
        {JournalOp::Replace, {"bin/lib.dll"}},
        {JournalOp::Replace, {"share/katana/a..b.dat"}},
        {JournalOp::Committing, {}},
    });
    StdFs disk;
    Rig rig(f.s, disk, "eeeeeeee");
    EXPECT_TRUE(recoverInProcess(f.s.root, rig.services()).ok());
    EXPECT_EQ(readText(f.s.root / "share/katana/a..b.dat"), "the old a..b\n");
    EXPECT_FALSE(isPresent(f.s.root / ".update" / "recovery.log")) << "no record was refused";
}

TEST(RecoverTorn, ABareInstallFolderIsAFolderTheTransactionMayHaveMadeButNeverAFileToRestore)
{
    Forgery f("recover-bare-folder");
    fs::create_directories(f.s.root / "etc");
    f.journal({
        {JournalOp::Plan, {"deadbeef", "1", "2"}},
        {JournalOp::MakeDirectory, {"etc"}},
        {JournalOp::Replace, {"bin/lib.dll"}},
        {JournalOp::Remove, {"lib"}}, // a forged "delete the whole folder"
        {JournalOp::Committing, {}},
    });
    StdFs disk;
    Rig rig(f.s, disk, "eeeeeeee");
    EXPECT_TRUE(recoverInProcess(f.s.root, rig.services()).ok());
    EXPECT_FALSE(isPresent(f.s.root / "etc")) << "the empty folder the transaction made is gone";
    const std::string log = readText(f.s.root / ".update" / "recovery.log");
    EXPECT_NE(log.find("REMOVE lib"), std::string::npos) << log;
    EXPECT_EQ(log.find("MKDIR etc"), std::string::npos) << "and the folder was not refused: " << log;
}

TEST(RecoverTorn, TheInstallFolderMayBeGivenWithATrailingSeparator)
{
    Loop loop("recover-trailing-separator", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    const Path spelled(core::pathToUtf8(s.root) + "/");
    StdFs disk;
    {
        crashAt(loop, p.renames[p.renames.size() / 2]);
        Rig rig(s, disk, "eeeeeeee");
        ASSERT_TRUE(recoverInProcess(spelled, rig.services()).ok());
        EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    }
    {
        crashAt(loop, p.renames[p.renames.size() / 2]);
        Rig rig(s, disk, "eeeeeeee");
        ASSERT_TRUE(rollback(spelled, onlyTransaction(s), rig.services()).ok());
        EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    }
}

// ---- recover(): which helper it runs, with what, and what it sweeps ------------------------------------------------------------

namespace {

struct Interrupted {
    explicit Interrupted(const std::string& label)
        : loop(label, [](Scenario& s) { buildFixtureScenario(s); })
    {
        const Points p = measureCleanApply(loop);
        crashAt(loop, p.renames[p.renames.size() / 2]);
        transaction = onlyTransaction(loop.scenario());
        helperName = installedProgram(Path(), "katana_updater").filename();
    }
    Scenario& scenario() { return loop.scenario(); }
    Path copy() { return scenario().state / ("run-" + transaction) / helperName; }
    Path own() { return scenario().root / "bin" / helperName; }

    Loop loop;
    std::string transaction;
    Path helperName;
};

constexpr std::chrono::milliseconds kShortWait{100};

} // namespace

TEST(RecoverWith, TheHelperCopyIsRunWithRecoverAndTheRootFromItsOwnFolderAndTheStateFolderIsLeftAsItWas)
{
    Interrupted x("recoverwith-args");
    Scenario& s = x.scenario();
    writeText(x.copy(), "a stand-in for the helper copy");
    // The per-user folder, entry by entry and byte by byte, before the call.
    const std::map<std::string, std::string> before = snapshot(s.state);
    ASSERT_TRUE(before.contains("run-" + x.transaction)) << "the premise: the copy's folder is in U";
    StdFs disk;
    FakeLauncher launcher;
    const core::Status status = detail::recoverWith(s.root, disk, launcher, s.state, kShortWait);
    ASSERT_TRUE(status.ok()) << status.error().describe();

    ASSERT_EQ(launcher.ran.size(), 1u);
    EXPECT_EQ(launcher.ran[0].executable, x.copy());
    EXPECT_EQ(launcher.ran[0].arguments,
              (std::vector<std::string>{"--recover", core::pathToUtf8(s.root)}));
    EXPECT_EQ(launcher.ran[0].workingDirectory, x.copy().parent_path());
    EXPECT_GT(launcher.ran[0].timeout, std::chrono::seconds(30)) << "a bound, and not a short one";
    EXPECT_TRUE(launcher.started.empty());
    // recover() is the first call of katana_cli and katana_mcp too, and a headless program writes
    // no per-user state (recover.hpp, "WHAT RECOVERY WRITES, AND WHERE"; docs/headless.md). The
    // state folder is read, to find the copy, and that is all: the copy that was run is still
    // there, no cleanup.list is begun, and nothing else in U is made, changed or removed.
    EXPECT_TRUE(isPresent(x.copy())) << "the helper copy is per-user state: it is not recover()'s to delete";
    EXPECT_FALSE(isPresent(s.state / "cleanup.list"));
    EXPECT_EQ(difference(before, snapshot(s.state)), "") << "recover() changed the per-user state folder";
}

TEST(RecoverWith, TheRootIsPassedWithoutATrailingSeparator)
{
    Interrupted x("recoverwith-trailing");
    Scenario& s = x.scenario();
    writeText(x.copy(), "stand-in");
    StdFs disk;
    FakeLauncher launcher;
    ASSERT_TRUE(detail::recoverWith(Path(core::pathToUtf8(s.root) + "/"), disk, launcher, s.state, kShortWait).ok());
    ASSERT_EQ(launcher.ran.size(), 1u);
    EXPECT_EQ(launcher.ran[0].arguments,
              (std::vector<std::string>{"--recover", core::pathToUtf8(s.root)}));
}

TEST(RecoverWith, TheCopyOutsideTheInstallIsPreferredAndTheInstallsOwnIsTheFallback)
{
    {
        Interrupted x("recoverwith-prefers-copy");
        Scenario& s = x.scenario();
        writeText(x.copy(), "the copy");
        writeText(x.own(), "the install's own");
        StdFs disk;
        FakeLauncher launcher;
        ASSERT_TRUE(detail::recoverWith(s.root, disk, launcher, s.state, kShortWait).ok());
        ASSERT_EQ(launcher.ran.size(), 1u);
        EXPECT_EQ(launcher.ran[0].executable, x.copy());
        EXPECT_TRUE(isPresent(x.own())) << "the install's own helper is not the copy's to delete";
    }
    {
        Interrupted x("recoverwith-falls-back");
        Scenario& s = x.scenario();
        writeText(x.own(), "the install's own");
        StdFs disk;
        FakeLauncher launcher;
        ASSERT_TRUE(detail::recoverWith(s.root, disk, launcher, s.state, kShortWait).ok());
        ASSERT_EQ(launcher.ran.size(), 1u);
        EXPECT_EQ(launcher.ran[0].executable, x.own());
        EXPECT_EQ(launcher.ran[0].workingDirectory, x.own().parent_path());
    }
}

TEST(RecoverWith, AHelperThatFailsLeavesItsCopyForTheRunOnceEntryAndSaysTheCommandByHand)
{
    Interrupted x("recoverwith-fails");
    Scenario& s = x.scenario();
    writeText(x.copy(), "stand-in");
    StdFs disk;
    FakeLauncher launcher;
    launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
        SpawnResult failed;
        failed.exitCode = 2;
        return failed;
    };
    const core::Status status = detail::recoverWith(s.root, disk, launcher, s.state, kShortWait);
    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().message.find("katana_updater --recover"), std::string::npos);
    EXPECT_NE(status.error().context.find("2"), std::string::npos) << status.error().describe();
    EXPECT_TRUE(isPresent(x.copy())) << "the RunOnce entry runs --recover from it at the next logon";

    FakeLauncher timedOutLauncher;
    timedOutLauncher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
        SpawnResult hung;
        hung.timedOut = true;
        return hung;
    };
    EXPECT_FALSE(detail::recoverWith(s.root, disk, timedOutLauncher, s.state, kShortWait).ok());
    EXPECT_TRUE(isPresent(x.copy()));
}

TEST(RecoverWith, WhatALockKeptFromBeingDeletedAfterARollbackIsSweptAtTheNextStartWithoutAHelper)
{
    Loop loop("recoverwith-sweep", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.pending); // every file is in place; the pending record is not written

    // The rollback moves the new katana.exe back into the transaction's folder; something holds
    // it, so the folder cannot be deleted, and nothing here can queue it for later.
    StdFs disk;
    class Holding final : public ForwardingFs {
      public:
        using ForwardingFs::ForwardingFs;
        core::Status removeFile(const Path& path) override
        {
            if (path.filename() == "katana.exe" && path.string().find(".update") != std::string::npos) {
                return osError(core::ErrorCode::FileExportFailure, "sharing violation", path, 32);
            }
            return inner_.removeFile(path);
        }
    } holding(disk);
    {
        Rig rig(s, holding, "eeeeeeee");
        ASSERT_TRUE(recoverInProcess(s.root, rig.services()).ok());
    }
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
    ASSERT_FALSE(leftoversInUpdate(s.root).empty()) << "the folder stays: a file in it is held";
    ASSERT_EQ(journalStateOf(s.root), JournalState::RolledBack);

    // The next start: the file is free. recover() sweeps it without starting anything.
    FakeLauncher launcher;
    EXPECT_TRUE(detail::recoverWith(s.root, disk, launcher, s.state, kShortWait).ok());
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
    EXPECT_TRUE(launcher.ran.empty());
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
}

TEST(RecoverWith, WhatIsStillHeldAtTheNextStartStaysUnderItsJournalAndIsNotQueuedInTheStateFolder)
{
    Loop loop("recoverwith-sweep-held", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.pending); // every file is in place; the pending record is not written

    // As in the test above: the rollback leaves a folder that cannot be deleted, because
    // something holds the katana.exe in it.
    StdFs disk;
    class StillHolding final : public ForwardingFs {
      public:
        using ForwardingFs::ForwardingFs;
        core::Status removeFile(const Path& path) override
        {
            if (path.filename() == "katana.exe" && path.string().find(".update") != std::string::npos) {
                return osError(core::ErrorCode::FileExportFailure, "sharing violation", path, 32);
            }
            return inner_.removeFile(path);
        }
    } holding(disk);
    {
        Rig rig(s, holding, "eeeeeeee");
        ASSERT_TRUE(recoverInProcess(s.root, rig.services()).ok());
    }
    ASSERT_FALSE(leftoversInUpdate(s.root).empty()) << "the premise: the folder stays";
    ASSERT_EQ(journalStateOf(s.root), JournalState::RolledBack);
    const std::map<std::string, std::string> before = snapshot(s.state);

    // The next start, and the file is STILL held. recover() may be katana_cli's or katana_mcp's
    // first call, and a headless program writes no per-user state (recover.hpp): what the sweep
    // cannot delete is not queued in U/cleanup.list, and U is as it was. Nothing is lost by that:
    // the journal is the last thing a sweep deletes (remove_tree.hpp), so the folder still says
    // that its transaction is over ...
    FakeLauncher launcher;
    EXPECT_TRUE(detail::recoverWith(s.root, holding, launcher, s.state, kShortWait).ok());
    EXPECT_TRUE(launcher.ran.empty());
    EXPECT_FALSE(leftoversInUpdate(s.root).empty()) << "a file in it is held";
    EXPECT_EQ(journalStateOf(s.root), JournalState::RolledBack);
    EXPECT_FALSE(isPresent(s.state / "cleanup.list"));
    EXPECT_EQ(difference(before, snapshot(s.state)), "") << "recover() changed the per-user state folder";

    // ... and the start after that, with the file free, finds it and sweeps it.
    EXPECT_TRUE(detail::recoverWith(s.root, disk, launcher, s.state, kShortWait).ok());
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
    EXPECT_TRUE(launcher.ran.empty());
    EXPECT_EQ(difference(before, snapshot(s.state)), "");
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
}

TEST(RecoverWith, ASweepIsLeftToTheUpdateThatIsRunningIfThereIsOne)
{
    Loop loop("recoverwith-sweep-locked", [](Scenario& s) { buildFixtureScenario(s); });
    Scenario& s = loop.scenario();
    const Points p = measureCleanApply(loop);
    crashAt(loop, p.healthy + 1);
    ASSERT_EQ(journalStateOf(s.root), JournalState::Healthy);
    StdFs disk;
    core::Result<std::unique_ptr<FileLock>> taken = disk.tryLock(s.root / ".update" / "lock");
    ASSERT_TRUE(taken.ok());
    std::unique_ptr<FileLock> holder = std::move(taken).value();
    FakeLauncher launcher;
    EXPECT_TRUE(detail::recoverWith(s.root, disk, launcher, s.state, kShortWait).ok());
    EXPECT_FALSE(leftoversInUpdate(s.root).empty()) << "an update is running: it is cleaning up after itself";
    holder.reset();
    EXPECT_TRUE(detail::recoverWith(s.root, disk, launcher, s.state, kShortWait).ok());
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

TEST(RecoverWith, NothingToDoStartsNothingAndTakesNoLock)
{
    Scenario s("recoverwith-nothing");
    s.v1 = tiny("0.1.0", 1);
    writeInstall(s.root, s.v1);
    StdFs disk;
    WatchedFs watched(disk);
    FakeLauncher launcher;
    EXPECT_TRUE(detail::recoverWith(s.root, watched, launcher, s.state, kShortWait).ok());
    EXPECT_TRUE(launcher.ran.empty());
    EXPECT_EQ(watched.locks, 0);
    writeText(s.root / ".update" / "lock", "");
    EXPECT_TRUE(detail::recoverWith(s.root, watched, launcher, s.state, kShortWait).ok());
    EXPECT_EQ(watched.locks, 0);
}

TEST(RecoverWith, AnUpdateThatWasNeverSaidToStartIsLeftExactlyAsItIsWithNoLockAndNoHelper)
{
    // recover() is the first call of every program - the health run of a live helper among them,
    // and katana_cli and katana_mcp, which write no per-user state. Over an update that is in
    // place and not yet proved it must therefore do NOTHING: take no lock (the helper that owns
    // the update may be alive and waiting for this very program to start), run no helper (no
    // recursion: the helper would be starting what started it), and leave the install and the
    // per-user folder as they are - with the marker there and without it.
    for (const bool markerThere : {true, false}) {
        SCOPED_TRACE(markerThere ? "its marker is there" : "its marker is not there");
        Unproved u(std::string("recoverwith-unproved-") + (markerThere ? "marker" : "none"));
        ASSERT_NO_FATAL_FAILURE(u.expectPremise());
        Scenario& s = u.s();
        if (!markerThere) {
            fs::remove(u.marker);
        }
        ASSERT_TRUE(isPresent(u.helper)) << "the premise: there is a helper copy it could run";
        const std::map<std::string, std::string> installBefore = wholeInstall(s);
        const std::map<std::string, std::string> userBefore = snapshot(s.state);

        StdFs disk;
        WatchedFs watched(disk);
        FaultFs counting(watched);
        FakeLauncher launcher;
        EXPECT_TRUE(detail::recoverWith(s.root, counting, launcher, s.state, kShortWait).ok());
        EXPECT_EQ(watched.locks, 0);
        EXPECT_TRUE(launcher.ran.empty());
        EXPECT_TRUE(launcher.started.empty());
        EXPECT_EQ(counting.mutatingOperations(), 0u);
        EXPECT_EQ(difference(installBefore, wholeInstall(s)), "") << "the install was changed";
        EXPECT_EQ(difference(userBefore, snapshot(s.state)), "") << "the per-user folder was changed";

        // And the recovery the helper itself runs first under `--recover` is the same: it is what
        // comes AFTER it there, settleUnproved, that settles such an update.
        Rig rig(s, counting, "eeeeeeee");
        EXPECT_TRUE(recoverInProcess(s.root, rig.services()).ok());
        EXPECT_EQ(watched.locks, 0);
        EXPECT_EQ(counting.mutatingOperations(), 0u);
        EXPECT_TRUE(rig.registry.runOnceHistory.empty());
        EXPECT_EQ(rig.registry.runOnceCleared, 0);
        EXPECT_EQ(difference(installBefore, wholeInstall(s)), "") << "the install was changed";
        EXPECT_EQ(difference(userBefore, snapshot(s.state)), "") << "the per-user folder was changed";
    }
}

TEST(RecoverEmptyRoot, NoInstallFolderIsNotTheDotUpdateOfTheCurrentFolder)
{
    // updateWorkDirectory("") is ".update" under whatever folder the program was started in.
    Scenario s("recover-empty-root");
    StdFs disk;
    class Watching final : public ForwardingFs {
      public:
        using ForwardingFs::ForwardingFs;
        core::Result<EntryInfo> stat(const Path& p) const override
        {
            ++calls;
            return inner_.stat(p);
        }
        core::Result<std::vector<Path>> listDirectory(const Path& p) const override
        {
            ++calls;
            return inner_.listDirectory(p);
        }
        core::Result<std::unique_ptr<FileLock>> tryLock(const Path& p) override
        {
            ++calls;
            return inner_.tryLock(p);
        }
        mutable int calls = 0;
    } watching(disk);
    FakeLauncher launcher;
    Rig rig(s, watching);

    // recover(): nothing to recover where there is no install.
    EXPECT_TRUE(detail::recoverWith(Path(), watching, launcher, s.state, kShortWait).ok());
    EXPECT_TRUE(launcher.ran.empty());
    // The helper's --recover and a rollback are told a folder explicitly: an empty one is a
    // mistake, and an error.
    const core::Status inProcess = recoverInProcess(Path(), rig.services());
    ASSERT_FALSE(inProcess.ok());
    EXPECT_EQ(inProcess.error().code, core::ErrorCode::InvalidArgument);
    const core::Status rolled = rollback(Path(), "deadbeef", rig.services());
    ASSERT_FALSE(rolled.ok());
    EXPECT_EQ(rolled.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_EQ(watching.calls, 0) << "not one question was asked of the disk";
}

// ---- recover(), the system seams, on Windows -----------------------------------------------------------------------------------------

#ifdef _WIN32

namespace {

// Sets the environment variable for the life of the object (the state folder of state.hpp).
class EnvironmentVariable {
  public:
    EnvironmentVariable(const char* name, const std::string& value) : name_(name)
    {
        const std::string previous = core::environmentVariable(name);
        had_ = !previous.empty();
        previous_ = previous;
        _putenv_s(name, value.c_str());
    }
    ~EnvironmentVariable() { _putenv_s(name_, had_ ? previous_.c_str() : ""); }

  private:
    const char* name_;
    bool had_ = false;
    std::string previous_;
};

Path systemProgram(const char* name)
{
    return Path(core::environmentVariable("SystemRoot")) / "System32" / name;
}

// A crashed update on the real disk, and a stand-in for the helper that exits as told:
// cmd.exe (reads nothing from its null input, exits 0) or where.exe (refuses the unknown
// option, exits 2).
struct SystemRecovery {
    explicit SystemRecovery(const std::string& label) : loop(label, [](Scenario& s) { buildFixtureScenario(s); })
    {
        const Points p = measureCleanApply(loop);
        crashAt(loop, p.renames[p.renames.size() / 2]);
        transaction.clear();
        for (const auto& entry : fs::directory_iterator(loop.scenario().root / ".update")) {
            if (entry.is_directory()) {
                transaction = entry.path().filename().string();
            }
        }
    }
    void helperIn(const Path& folder, const char* program)
    {
        fs::create_directories(folder);
        fs::copy_file(systemProgram(program), folder / "katana_updater.exe", fs::copy_options::overwrite_existing);
    }
    Loop loop;
    std::string transaction;
};

} // namespace

TEST(RecoverOnWindows, NothingInterruptedIsOkWithoutRunningAnything)
{
    Scenario s("recover-sys-nothing");
    s.v1 = tiny("0.1.0", 1);
    writeInstall(s.root, s.v1);
    EXPECT_TRUE(recover(s.root).ok()) << "no .update";
    writeText(s.root / ".update" / "lock", "");
    EXPECT_TRUE(recover(s.root).ok()) << "a lock and nothing else";
    EXPECT_TRUE(recover(s.scratch.path() / "no" / "such" / "root").ok()) << "not even an install";
}

TEST(RecoverOnWindows, AnInterruptedUpdateRunsTheHelperCopyWithRecoverAndTheRoot)
{
    SystemRecovery r("recover-sys-helper");
    Scenario& s = r.loop.scenario();
    EnvironmentVariable state("KATANA_UPDATE_STATE_DIR", core::pathToUtf8(s.state));
    r.helperIn(s.state / ("run-" + r.transaction), "cmd.exe"); // exits 0
    EXPECT_TRUE(recover(s.root).ok());
}

TEST(RecoverOnWindows, TheHelperCopyOutsideTheInstallIsPreferredToTheInstallsOwn)
{
    {
        SystemRecovery r("recover-sys-prefers-copy");
        Scenario& s = r.loop.scenario();
        EnvironmentVariable state("KATANA_UPDATE_STATE_DIR", core::pathToUtf8(s.state));
        r.helperIn(s.state / ("run-" + r.transaction), "cmd.exe");        // exits 0
        r.helperIn(s.root / "bin", "where.exe");                          // exits 2
        EXPECT_TRUE(recover(s.root).ok()) << "the copy ran, not bin/katana_updater";
    }
    {
        SystemRecovery r("recover-sys-falls-back");
        Scenario& s = r.loop.scenario();
        EnvironmentVariable state("KATANA_UPDATE_STATE_DIR", core::pathToUtf8(s.state));
        r.helperIn(s.root / "bin", "where.exe");
        const core::Status status = recover(s.root);
        ASSERT_FALSE(status.ok()) << "with no copy the install's own helper ran, and it failed";
        EXPECT_NE(status.error().message.find("katana_updater --recover"), std::string::npos);
        EXPECT_NE(status.error().message.find(core::pathToUtf8(s.root)), std::string::npos);
    }
}

TEST(RecoverOnWindows, AHelperThatFailsOrIsMissingIsAnErrorThatNamesTheCommandToRunByHand)
{
    {
        SystemRecovery r("recover-sys-fails");
        Scenario& s = r.loop.scenario();
        EnvironmentVariable state("KATANA_UPDATE_STATE_DIR", core::pathToUtf8(s.state));
        r.helperIn(s.state / ("run-" + r.transaction), "where.exe");
        const core::Status status = recover(s.root);
        ASSERT_FALSE(status.ok());
        EXPECT_NE(status.error().message.find("katana_updater --recover \""), std::string::npos)
            << status.error().describe();
    }
    {
        SystemRecovery r("recover-sys-missing");
        Scenario& s = r.loop.scenario();
        EnvironmentVariable state("KATANA_UPDATE_STATE_DIR", core::pathToUtf8(s.state));
        const core::Status status = recover(s.root);
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(status.error().code, core::ErrorCode::NotFound);
        EXPECT_NE(status.error().message.find("katana_updater --recover"), std::string::npos);
    }
}

TEST(RecoverOnWindows, ALiveHolderOfTheLockIsWaitedForBeforeTheHelperIsRun)
{
    SystemRecovery r("recover-sys-lock");
    Scenario& s = r.loop.scenario();
    EnvironmentVariable state("KATANA_UPDATE_STATE_DIR", core::pathToUtf8(s.state));
    r.helperIn(s.state / ("run-" + r.transaction), "cmd.exe");
    const auto disk = makeSystemFsOps();
    auto holder = std::make_unique<core::Result<std::unique_ptr<FileLock>>>(disk->tryLock(s.root / ".update" / "lock"));
    ASSERT_TRUE(holder->ok());
    std::thread releasing([&] {
        std::this_thread::sleep_for(std::chrono::milliseconds(400));
        holder.reset();
    });
    const auto start = std::chrono::steady_clock::now();
    const core::Status status = recover(s.root);
    const auto took = std::chrono::steady_clock::now() - start;
    releasing.join();
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    EXPECT_GE(took, std::chrono::milliseconds(350)) << "it waited for the holder first";
}

#endif // _WIN32
