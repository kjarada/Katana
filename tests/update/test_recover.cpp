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

#include <thread>

#include "apply_harness.hpp"
#include "recover_internal.hpp"

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

TEST(RecoverWith, TheHelperCopyIsRunWithRecoverAndTheRootFromItsOwnFolderAndNoLongerNeededAfterwards)
{
    Interrupted x("recoverwith-args");
    Scenario& s = x.scenario();
    writeText(x.copy(), "a stand-in for the helper copy");
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
    // The helper has exited by now, so the copy of itself it could not delete is deleted.
    EXPECT_FALSE(isPresent(x.copy().parent_path()));
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
