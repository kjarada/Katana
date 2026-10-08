// The apply's journal (include/katana/update/journal.hpp) and the real Windows file system
// under it (src/katana_update/fsops_win.cpp).
//
// Every expectation here is worked out outside the code under test:
//   - CRC-32 values are the published check values of the algorithm (the "check" value of
//     CRC-32/ISO-HDLC, 0xCBF43926 for "123456789") and Python's zlib.crc32 of each line body;
//   - encoded lines are written out by hand and their crc taken from zlib;
//   - the Windows error numbers are the documented ones (2 file not found, 32 sharing
//     violation, 80 file exists, 145 directory not empty, 183 already exists).

#include <gtest/gtest.h>

#include <thread>

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

// ---- the format -----------------------------------------------------------------------------------

TEST(JournalFormat, TheCrcIsTheCrc32OfZlibAndPng)
{
    // The check value every CRC-32/ISO-HDLC implementation publishes, and three more taken
    // from zlib.crc32.
    EXPECT_EQ(crc32(""), 0x00000000u);
    EXPECT_EQ(crc32("a"), 0xE8B7BE43u);
    EXPECT_EQ(crc32("123456789"), 0xCBF43926u);
    EXPECT_EQ(crc32("The quick brown fox jumps over the lazy dog"), 0x414FA339u);
}

TEST(JournalFormat, ARecordIsNumberOperationArgumentsAndTheCrcOfThoseBytes)
{
    EXPECT_EQ(encodeRecord(1, {JournalOp::Plan, {"t1", "5", "7"}}), "1 PLAN t1 5 7 a888bad4\n");
    EXPECT_EQ(encodeRecord(4, {JournalOp::Committing, {}}), "4 COMMITTING 51265d9e\n");
    EXPECT_EQ(encodeRecord(8, {JournalOp::CommittedPendingHealth, {}}),
              "8 COMMITTED_PENDING_HEALTH 5f36ea28\n");
    EXPECT_EQ(encodeRecord(9, {JournalOp::RollingBack, {}}), "9 ROLLING_BACK be72f1f2\n");
    EXPECT_EQ(encodeRecord(10, {JournalOp::RolledBack, {}}), "10 ROLLED_BACK b37b4c8a\n");
    EXPECT_EQ(encodeRecord(11, {JournalOp::Healthy, {}}), "11 HEALTHY 7905ab3b\n");
}

TEST(JournalFormat, EveryByteOutsideTheSafeSetIsPercentEncodedInUpperCaseHex)
{
    // The safe set is [A-Za-z0-9._/~-]; a space, a non-ASCII byte, a percent sign and a line
    // break are all escaped, so an argument never holds a space or a line break.
    EXPECT_EQ(encodeRecord(2, {JournalOp::Add, {"bin/a b.dll"}}), "2 ADD bin/a%20b.dll ac554854\n");
    EXPECT_EQ(encodeRecord(3, {JournalOp::Replace, {"share/katana/\xC3\xA9.txt"}}),
              "3 REPLACE share/katana/%C3%A9.txt f5b5dfa6\n");
    EXPECT_EQ(encodeRecord(6, {JournalOp::Remove, {"a%b\nc"}}), "6 REMOVE a%25b%0Ac 29770ce9\n");
    EXPECT_EQ(encodeRecord(7, {JournalOp::MakeDirectory, {"share/katana/more"}}),
              "7 MKDIR share/katana/more 4004c056\n");
}

TEST(JournalFormat, AnEmptyArgumentSurvivesTheRoundTrip)
{
    // Two spaces in a row: the empty token between them is the argument.
    EXPECT_EQ(encodeRecord(5, {JournalOp::MovedOld, {""}}), "5 MOVED_OLD  a5d06e93\n");

    JournalRecord record{JournalOp::MovedOld, {"", "x", ""}};
    const ParsedJournal again = parseJournal(encodeRecord(1, record));
    ASSERT_EQ(again.records.size(), 1u);
    EXPECT_EQ(again.records[0].args, (std::vector<std::string>{"", "x", ""}));
}

TEST(JournalFormat, ArgumentsWithSpacesNonAsciiBytesPercentAndLineBreaksComeBackAsTheyWent)
{
    const std::vector<std::string> awkward = {
        "bin/with space.dll", "caf\xC3\xA9/\xE6\x97\xA5\xE6\x9C\xAC.txt", "100%", "%41", "a\nb\r\nc",
        std::string("nul\0byte", 8), std::string(300, 'x'), "~tilde-and_dots.v1.2/x"};
    std::string text;
    std::uint64_t n = 1;
    for (const std::string& argument : awkward) {
        text += encodeRecord(n++, {JournalOp::Add, {argument}});
    }
    const ParsedJournal parsed = parseJournal(text);
    ASSERT_EQ(parsed.records.size(), awkward.size());
    EXPECT_FALSE(parsed.tornTail);
    for (std::size_t i = 0; i < awkward.size(); ++i) {
        ASSERT_EQ(parsed.records[i].args.size(), 1u);
        EXPECT_EQ(parsed.records[i].args[0], awkward[i]) << "record " << i;
    }
    // And no encoded line has a raw space inside an argument or a second line break.
    EXPECT_EQ(std::count(text.begin(), text.end(), '\n'), static_cast<std::ptrdiff_t>(awkward.size()));
}

TEST(JournalFormat, EveryOperationHasItsNameAndParsesBack)
{
    const std::vector<std::pair<JournalOp, std::string>> all = {
        {JournalOp::Plan, "PLAN"},
        {JournalOp::Add, "ADD"},
        {JournalOp::Replace, "REPLACE"},
        {JournalOp::Remove, "REMOVE"},
        {JournalOp::MakeDirectory, "MKDIR"},
        {JournalOp::Committing, "COMMITTING"},
        {JournalOp::MovedOld, "MOVED_OLD"},
        {JournalOp::MovedNew, "MOVED_NEW"},
        {JournalOp::CommittedPendingHealth, "COMMITTED_PENDING_HEALTH"},
        {JournalOp::RollingBack, "ROLLING_BACK"},
        {JournalOp::RolledBack, "ROLLED_BACK"},
        {JournalOp::Healthy, "HEALTHY"},
    };
    for (const auto& [op, name] : all) {
        EXPECT_EQ(toString(op), name);
        const core::Result<JournalOp> parsed = parseJournalOp(name);
        ASSERT_TRUE(parsed.ok()) << name;
        EXPECT_EQ(parsed.value(), op);
    }
    EXPECT_FALSE(parseJournalOp("plan").ok()) << "the names are upper case";
    EXPECT_FALSE(parseJournalOp("DELETE").ok());
    EXPECT_FALSE(parseJournalOp("").ok());
}

// ---- reading what a crash left ------------------------------------------------------------------------

TEST(JournalParse, AnEmptyTextIsAnEmptyJournalWithNothingTorn)
{
    const ParsedJournal parsed = parseJournal("");
    EXPECT_TRUE(parsed.records.empty());
    EXPECT_EQ(parsed.state, JournalState::Empty);
    EXPECT_FALSE(parsed.tornTail);
}

TEST(JournalParse, ALastLineWithoutItsLineFeedIsTornAndIgnored)
{
    const ParsedJournal parsed = parseJournal("1 ADD a 3d939a4d\n2 ADD b 9572d16a");
    ASSERT_EQ(parsed.records.size(), 1u);
    EXPECT_EQ(parsed.records[0].args[0], "a");
    EXPECT_TRUE(parsed.tornTail);
}

TEST(JournalParse, ALineCutShortAnywhereLeavesTheGoodPrefix)
{
    const std::string whole = "1 ADD a 3d939a4d\n2 ADD b 9572d16a\n3 ADD c 4402ea48\n";
    const std::string firstTwo = "1 ADD a 3d939a4d\n2 ADD b 9572d16a\n";
    for (std::size_t cut = 0; cut < whole.size(); ++cut) {
        const ParsedJournal parsed = parseJournal(std::string_view(whole).substr(0, cut));
        // The records that are complete in the first `cut` bytes: one per line feed.
        const auto expected = static_cast<std::size_t>(
            std::count(whole.begin(), whole.begin() + static_cast<std::ptrdiff_t>(cut), '\n'));
        EXPECT_EQ(parsed.records.size(), expected) << "cut at " << cut;
        // Torn exactly when bytes remain after the last line feed.
        const bool bytesAfter = cut > 0 && whole[cut - 1] != '\n';
        EXPECT_EQ(parsed.tornTail, bytesAfter) << "cut at " << cut;
    }
    EXPECT_EQ(parseJournal(firstTwo).records.size(), 2u);
}

TEST(JournalParse, AWrongCrcStopsTheReadAndEverythingAfterItIsIgnored)
{
    // The second line's crc is wrong by one digit; the third is fine but follows it.
    const ParsedJournal parsed =
        parseJournal("1 ADD a 3d939a4d\n2 ADD b 9572d16b\n3 ADD c 4402ea48\n");
    ASSERT_EQ(parsed.records.size(), 1u);
    EXPECT_TRUE(parsed.tornTail);
}

TEST(JournalParse, AnUpperCaseCrcIsNotTheFormatAndStopsTheRead)
{
    const ParsedJournal parsed = parseJournal("1 ADD a 3D939A4D\n");
    EXPECT_TRUE(parsed.records.empty());
    EXPECT_TRUE(parsed.tornTail);
}

TEST(JournalParse, AGapOrARepeatInTheNumbersStopsTheRead)
{
    // 1, 3: the second is not the previous plus one.
    EXPECT_EQ(parseJournal("1 ADD a 3d939a4d\n3 ADD c 4402ea48\n").records.size(), 1u);
    // 1, 1.
    EXPECT_EQ(parseJournal("1 ADD a 3d939a4d\n1 ADD a 3d939a4d\n").records.size(), 1u);
    // Starting at 2.
    EXPECT_TRUE(parseJournal("2 ADD b 9572d16a\n").records.empty());
    // A leading zero is not how a number is written.
    EXPECT_TRUE(parseJournal("01 ADD a 3d939a4d\n").records.empty());
}

TEST(JournalParse, AnUnknownOperationABadEscapeAndAMissingPartEndTheRead)
{
    const auto crcOf = [](const std::string& body) {
        char hex[9];
        std::snprintf(hex, sizeof(hex), "%08x", crc32(body));
        return body + " " + hex + "\n";
    };
    // Each line has a correct crc, so only its content is wrong.
    EXPECT_TRUE(parseJournal(crcOf("1 DELETE a")).records.empty());
    EXPECT_TRUE(parseJournal(crcOf("1 ADD a%2")).records.empty());
    EXPECT_TRUE(parseJournal(crcOf("1 ADD a%zz")).records.empty());
    EXPECT_TRUE(parseJournal(crcOf("1")).records.empty());
    EXPECT_TRUE(parseJournal(crcOf("ADD a")).records.empty());
    EXPECT_TRUE(parseJournal(crcOf("x ADD a")).records.empty());
    // And the same line right is read.
    EXPECT_EQ(parseJournal(crcOf("1 ADD a%2F")).records.size(), 1u);
    EXPECT_EQ(parseJournal(crcOf("1 ADD a%2F")).records[0].args[0], "a/");
}

TEST(JournalParse, ALineEndedWithCarriageReturnAndLineFeedIsNotOurs)
{
    // A text editor that rewrote the line ends changed the bytes the crc covers.
    const ParsedJournal parsed = parseJournal("1 ADD a 3d939a4d\r\n");
    EXPECT_TRUE(parsed.records.empty());
    EXPECT_TRUE(parsed.tornTail);
}

TEST(JournalParse, GarbageIsATornTailNotAFailure)
{
    for (const std::string& text : {std::string("\n"), std::string("\n\n\n"), std::string("garbage"),
                                   std::string(1000, '\xFF'), std::string("1 ADD a 3d939a4d\n\n")}) {
        const ParsedJournal parsed = parseJournal(text);
        EXPECT_LE(parsed.records.size(), 1u);
        EXPECT_TRUE(parsed.tornTail);
    }
}

TEST(JournalState, EachRecordLeadsToTheStateTheHeaderNames)
{
    const auto stateAfter = [](std::vector<JournalOp> ops) {
        std::string text;
        std::uint64_t n = 1;
        for (const JournalOp op : ops) {
            text += encodeRecord(n++, {op, {}});
        }
        return parseJournal(text).state;
    };
    EXPECT_EQ(stateAfter({}), JournalState::Empty);
    EXPECT_EQ(stateAfter({JournalOp::Plan}), JournalState::Planned);
    EXPECT_EQ(stateAfter({JournalOp::Plan, JournalOp::Add, JournalOp::Replace, JournalOp::Remove,
                          JournalOp::MakeDirectory}),
              JournalState::Planned)
        << "the operations of the PLAN group do not change the state";
    EXPECT_EQ(stateAfter({JournalOp::Plan, JournalOp::Committing}), JournalState::Committing);
    EXPECT_EQ(stateAfter({JournalOp::Plan, JournalOp::Committing, JournalOp::MovedOld,
                          JournalOp::MovedNew}),
              JournalState::Committing)
        << "progress records do not change the state";
    EXPECT_EQ(stateAfter({JournalOp::Plan, JournalOp::Committing, JournalOp::CommittedPendingHealth}),
              JournalState::CommittedPendingHealth);
    EXPECT_EQ(stateAfter({JournalOp::Plan, JournalOp::Committing, JournalOp::RollingBack}),
              JournalState::RollingBack);
    EXPECT_EQ(stateAfter({JournalOp::Plan, JournalOp::Committing, JournalOp::RollingBack,
                          JournalOp::RolledBack}),
              JournalState::RolledBack);
    EXPECT_EQ(stateAfter({JournalOp::Plan, JournalOp::Committing, JournalOp::CommittedPendingHealth,
                          JournalOp::Healthy}),
              JournalState::Healthy);
    // A rollback of a version that was pending health: the last state-bearing record wins.
    EXPECT_EQ(stateAfter({JournalOp::Plan, JournalOp::Committing, JournalOp::CommittedPendingHealth,
                          JournalOp::RollingBack}),
              JournalState::RollingBack);
}

TEST(JournalState, OnlyAJournalACrashLeftHalfDoneNeedsRecovery)
{
    EXPECT_FALSE(needsRecovery(JournalState::Empty));
    EXPECT_TRUE(needsRecovery(JournalState::Planned));
    EXPECT_TRUE(needsRecovery(JournalState::Committing));
    EXPECT_FALSE(needsRecovery(JournalState::CommittedPendingHealth));
    EXPECT_TRUE(needsRecovery(JournalState::RollingBack));
    EXPECT_FALSE(needsRecovery(JournalState::RolledBack));
    EXPECT_FALSE(needsRecovery(JournalState::Healthy));
}

// ---- the writer ----------------------------------------------------------------------------------------

TEST(JournalFile, AGroupIsOneAppendAndTheNumbersContinueAcrossGroupsAndObjects)
{
    Scratch scratch("journal-group");
    StdFs disk;
    FaultFs counting(disk);
    const Path path = scratch.path() / "tx" / "journal";
    fs::create_directories(path.parent_path());
    {
        Journal journal(counting, path);
        const std::vector<JournalRecord> first = {{JournalOp::Plan, {"t1", "1", "2"}},
                                                  {JournalOp::Add, {"bin/a.dll"}},
                                                  {JournalOp::Replace, {"bin/b.dll"}}};
        ASSERT_TRUE(journal.append(first).ok());
        EXPECT_EQ(counting.mutatingOperations(), 1u) << "one flush per group";
        const JournalRecord committing{JournalOp::Committing, {}};
        ASSERT_TRUE(journal.append(std::span(&committing, 1)).ok());
        EXPECT_EQ(counting.mutatingOperations(), 2u);
    }
    {
        // A new object over the same file learns where the numbers stand.
        Journal again(disk, path);
        const JournalRecord pending{JournalOp::CommittedPendingHealth, {}};
        ASSERT_TRUE(again.append(std::span(&pending, 1)).ok());
    }
    const core::Result<ParsedJournal> loaded = Journal(disk, path).load();
    ASSERT_TRUE(loaded.ok());
    ASSERT_EQ(loaded->records.size(), 5u);
    EXPECT_EQ(loaded->records[1].args, std::vector<std::string>{"bin/a.dll"});
    EXPECT_EQ(loaded->state, JournalState::CommittedPendingHealth);
    EXPECT_FALSE(loaded->tornTail);
    // The file holds exactly the numbered lines.
    const std::string text = readText(path);
    EXPECT_EQ(text.rfind("1 PLAN t1 1 2 ", 0), 0u);
    EXPECT_NE(text.find("\n4 COMMITTING 51265d9e\n"), std::string::npos);
    EXPECT_NE(text.find("\n5 COMMITTED_PENDING_HEALTH c796f7dd\n"), std::string::npos);
}

TEST(JournalFile, AnEmptyGroupWritesNothing)
{
    Scratch scratch("journal-empty-group");
    StdFs disk;
    FaultFs counting(disk);
    Journal journal(counting, scratch.path() / "journal");
    EXPECT_TRUE(journal.append({}).ok());
    EXPECT_EQ(counting.mutatingOperations(), 0u);
    EXPECT_FALSE(isPresent(scratch.path() / "journal"));
}

TEST(JournalFile, ALoadOfAMissingFileIsAnEmptyJournal)
{
    Scratch scratch("journal-missing");
    StdFs disk;
    const core::Result<ParsedJournal> loaded = Journal(disk, scratch.path() / "nope").load();
    ASSERT_TRUE(loaded.ok());
    EXPECT_EQ(loaded->state, JournalState::Empty);
    EXPECT_TRUE(loaded->records.empty());
}

TEST(JournalFile, RecordsAppendedAfterATornLineAreStillReadable)
{
    // A crash cut the third record in half. Appending ROLLING_BACK straight after those bytes
    // would glue it to the fragment and lose it; the journal is rewritten from its good prefix.
    Scratch scratch("journal-torn");
    StdFs disk;
    const Path path = scratch.path() / "journal";
    writeText(path, "1 ADD a 3d939a4d\n2 ADD b 9572d16a\n3 ADD c 44");
    Journal journal(disk, path);
    const JournalRecord rolling{JournalOp::RollingBack, {}};
    ASSERT_TRUE(journal.append(std::span(&rolling, 1)).ok());

    const core::Result<ParsedJournal> loaded = Journal(disk, path).load();
    ASSERT_TRUE(loaded.ok());
    ASSERT_EQ(loaded->records.size(), 3u);
    EXPECT_FALSE(loaded->tornTail);
    EXPECT_EQ(loaded->records[2].op, JournalOp::RollingBack);
    EXPECT_EQ(loaded->state, JournalState::RollingBack);
}

TEST(JournalFile, ACrashInsideAGroupWritesNothingOfIt)
{
    Scratch scratch("journal-crash");
    StdFs disk;
    FaultFs faults(disk);
    const Path path = scratch.path() / "journal";
    Journal journal(faults, path);
    const JournalRecord first{JournalOp::Plan, {"t1", "1", "2"}};
    ASSERT_TRUE(journal.append(std::span(&first, 1)).ok());
    faults.crashBefore(2);
    const JournalRecord second{JournalOp::Committing, {}};
    EXPECT_THROW((void)journal.append(std::span(&second, 1)), SimulatedCrash);
    const core::Result<ParsedJournal> loaded = Journal(disk, path).load();
    ASSERT_TRUE(loaded.ok());
    EXPECT_EQ(loaded->records.size(), 1u);
    EXPECT_EQ(loaded->state, JournalState::Planned);
}

// ---- the real file system, on Windows ---------------------------------------------------------------

#ifdef _WIN32

namespace {

// A fast retry policy: the tests that wait for it must not take 30 seconds.
std::unique_ptr<FsOps> systemFs(int intervalMs = 5, int totalMs = 120)
{
    return makeSystemFsOps(RetryPolicy{std::chrono::milliseconds(intervalMs),
                                       std::chrono::milliseconds(totalMs)});
}

std::string osErrorOf(const core::Error& error)
{
    // The context begins `os_error=<n>`.
    constexpr std::string_view prefix = "os_error=";
    if (error.context.rfind(prefix, 0) != 0) {
        return "(no os_error)";
    }
    const std::size_t end = error.context.find(' ', prefix.size());
    return error.context.substr(prefix.size(), end - prefix.size());
}

// Opens a file the way an antivirus scanner or a running program does, refusing every share.
class HeldOpen {
  public:
    explicit HeldOpen(const Path& path, DWORD share = 0)
        : handle_(CreateFileW(path.c_str(), GENERIC_READ, share, nullptr, OPEN_EXISTING,
                              FILE_ATTRIBUTE_NORMAL, nullptr))
    {
    }
    HeldOpen(const HeldOpen&) = delete;
    HeldOpen& operator=(const HeldOpen&) = delete;
    ~HeldOpen() { release(); }
    [[nodiscard]] bool ok() const { return handle_ != INVALID_HANDLE_VALUE; }
    void release()
    {
        if (handle_ != INVALID_HANDLE_VALUE) {
            CloseHandle(handle_);
            handle_ = INVALID_HANDLE_VALUE;
        }
    }

  private:
    HANDLE handle_;
};

bool makeJunction(const Path& link, const Path& target)
{
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    SpawnRequest request;
    request.executable = Path(core::environmentVariable("SystemRoot")) / "System32" / "cmd.exe";
    request.arguments = {"/c", "mklink", "/J", core::pathToUtf8(link), core::pathToUtf8(target)};
    const core::Result<SpawnResult> ran = launcher->run(request);
    return ran.ok() && ran->exitCode == 0;
}

} // namespace

TEST(WindowsFs, StatDescribesAFileADirectoryAndAnEntryThatIsNotThere)
{
    Scratch scratch("winfs-stat");
    const auto disk = systemFs();
    writeText(scratch.path() / "a.txt", "12345");
    fs::create_directories(scratch.path() / "dir");

    const core::Result<EntryInfo> file = disk->stat(scratch.path() / "a.txt");
    ASSERT_TRUE(file.ok());
    EXPECT_EQ(file->kind, EntryKind::File);
    EXPECT_EQ(file->size, 5u);
    EXPECT_FALSE(file->reparsePoint);

    const core::Result<EntryInfo> directory = disk->stat(scratch.path() / "dir");
    ASSERT_TRUE(directory.ok());
    EXPECT_EQ(directory->kind, EntryKind::Directory);
    EXPECT_FALSE(directory->reparsePoint);

    const core::Result<EntryInfo> missing = disk->stat(scratch.path() / "nope" / "deeper");
    ASSERT_TRUE(missing.ok()) << "missing is not an error";
    EXPECT_EQ(missing->kind, EntryKind::Missing);
}

TEST(WindowsFs, AJunctionIsAReparsePointAndResolveFollowsItWhereStatDoesNot)
{
    Scratch scratch("winfs-junction");
    const auto disk = systemFs();
    fs::create_directories(scratch.path() / "target" / "inner");
    writeText(scratch.path() / "target" / "inner" / "f.txt", "x");
    const Path link = scratch.path() / "link";
    if (!makeJunction(link, scratch.path() / "target")) {
        GTEST_SKIP() << "mklink /J is not available here";
    }
    const core::Result<EntryInfo> info = disk->stat(link);
    ASSERT_TRUE(info.ok());
    EXPECT_EQ(info->kind, EntryKind::Directory);
    EXPECT_TRUE(info->reparsePoint) << "a junction sends a path somewhere else";

    // Through the link the same folder is reached, and resolve says where it really is.
    const core::Result<Path> resolved = disk->resolve(link / "inner" / "not-yet.txt");
    ASSERT_TRUE(resolved.ok());
    const Path expected = fs::weakly_canonical(scratch.path() / "target") / "inner" / "not-yet.txt";
    EXPECT_EQ(core::pathToUtf8(resolved.value()).size(), core::pathToUtf8(expected).size());
    EXPECT_TRUE(_wcsicmp(resolved->c_str(), expected.c_str()) == 0)
        << resolved->string() << " vs " << expected.string();
    // And a folder inside the link is not itself one.
    const core::Result<EntryInfo> inner = disk->stat(link / "inner");
    ASSERT_TRUE(inner.ok());
    EXPECT_FALSE(inner->reparsePoint);
}

TEST(WindowsFs, ListDirectoryGivesFullPathsAndANotFoundForAMissingFolder)
{
    Scratch scratch("winfs-list");
    const auto disk = systemFs();
    writeText(scratch.path() / "one.txt", "1");
    fs::create_directories(scratch.path() / "two");
    const core::Result<std::vector<Path>> names = disk->listDirectory(scratch.path());
    ASSERT_TRUE(names.ok());
    std::set<std::string> found;
    for (const Path& name : names.value()) {
        found.insert(name.filename().string());
        EXPECT_EQ(name.parent_path(), scratch.path()) << "a full path, not a bare name";
    }
    EXPECT_EQ(found, (std::set<std::string>{"one.txt", "two"}));

    const core::Result<std::vector<Path>> none = disk->listDirectory(scratch.path() / "missing");
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, core::ErrorCode::NotFound);
    EXPECT_EQ(osErrorOf(none.error()), "3") << none.error().describe();
}

TEST(WindowsFs, ReadFileReturnsEveryByteAndAMissingFileIsErrorTwo)
{
    Scratch scratch("winfs-read");
    const auto disk = systemFs();
    std::string bytes;
    for (int i = 0; i < 70000; ++i) {
        bytes.push_back(static_cast<char>(i * 31)); // past one buffer, every byte value, NULs
    }
    writeText(scratch.path() / "b.bin", bytes);
    const core::Result<std::string> read = disk->readFile(scratch.path() / "b.bin");
    ASSERT_TRUE(read.ok());
    EXPECT_EQ(read.value(), bytes);

    writeText(scratch.path() / "empty", "");
    const core::Result<std::string> none = disk->readFile(scratch.path() / "empty");
    ASSERT_TRUE(none.ok());
    EXPECT_TRUE(none->empty());

    const core::Result<std::string> missing = disk->readFile(scratch.path() / "nope");
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, core::ErrorCode::NotFound);
    EXPECT_EQ(osErrorOf(missing.error()), "2");
}

TEST(WindowsFs, MakeDirectoriesMakesTheParentsAndIsDoneWhenItExists)
{
    Scratch scratch("winfs-mkdir");
    const auto disk = systemFs();
    const Path deep = scratch.path() / "a" / "b" / "c";
    ASSERT_TRUE(disk->makeDirectories(deep).ok());
    EXPECT_TRUE(fs::is_directory(deep));
    EXPECT_TRUE(disk->makeDirectories(deep).ok()) << "again is success";

    writeText(scratch.path() / "file", "x");
    const core::Status blocked = disk->makeDirectories(scratch.path() / "file" / "sub");
    ASSERT_FALSE(blocked.ok()) << "a file is where the folder must be";
    EXPECT_EQ(blocked.error().code, core::ErrorCode::InvalidState);
}

TEST(WindowsFs, CopyFileMakesANewFileWithTheSameBytesAndNeverOverwrites)
{
    Scratch scratch("winfs-copy");
    const auto disk = systemFs();
    std::string bytes(3 * 1024 * 1024 + 17, 'q'); // three buffers and a bit
    bytes[12345] = '\0';
    writeText(scratch.path() / "from.bin", bytes);
    ASSERT_TRUE(disk->copyFile(scratch.path() / "from.bin", scratch.path() / "to.bin").ok());
    EXPECT_EQ(readText(scratch.path() / "to.bin"), bytes);
    EXPECT_EQ(readText(scratch.path() / "from.bin"), bytes) << "the source is untouched";

    // The target exists: AlreadyExists, and it is not touched.
    writeText(scratch.path() / "taken", "mine");
    const core::Status refused = disk->copyFile(scratch.path() / "from.bin", scratch.path() / "taken");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, core::ErrorCode::AlreadyExists);
    EXPECT_EQ(osErrorOf(refused.error()), "80") << refused.error().describe(); // ERROR_FILE_EXISTS
    EXPECT_EQ(readText(scratch.path() / "taken"), "mine");

    // The source is missing: NotFound, and no half file.
    const core::Status missing = disk->copyFile(scratch.path() / "absent", scratch.path() / "never");
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, core::ErrorCode::NotFound);
    EXPECT_FALSE(isPresent(scratch.path() / "never"));

    // An empty file copies.
    writeText(scratch.path() / "zero", "");
    ASSERT_TRUE(disk->copyFile(scratch.path() / "zero", scratch.path() / "zero2").ok());
    EXPECT_EQ(fs::file_size(scratch.path() / "zero2"), 0u);
}

TEST(WindowsFs, ACopyOfAReadOnlyFileIsAFileTheUpdateCanReplaceLater)
{
    Scratch scratch("winfs-copy-readonly");
    const auto disk = systemFs();
    writeText(scratch.path() / "from.txt", "x");
    SetFileAttributesW((scratch.path() / "from.txt").c_str(), FILE_ATTRIBUTE_READONLY);
    ASSERT_TRUE(disk->copyFile(scratch.path() / "from.txt", scratch.path() / "to.txt").ok());
    const DWORD attributes = GetFileAttributesW((scratch.path() / "to.txt").c_str());
    EXPECT_EQ(attributes & FILE_ATTRIBUTE_READONLY, 0u)
        << "the copy takes the install's rules, not the stage's";
    SetFileAttributesW((scratch.path() / "from.txt").c_str(), FILE_ATTRIBUTE_NORMAL);
}

TEST(WindowsFs, RenameMovesAFileAndNeverReplacesOne)
{
    Scratch scratch("winfs-rename");
    const auto disk = systemFs();
    writeText(scratch.path() / "a", "AAA");
    ASSERT_TRUE(disk->renameFile(scratch.path() / "a", scratch.path() / "b").ok());
    EXPECT_FALSE(isPresent(scratch.path() / "a"));
    EXPECT_EQ(readText(scratch.path() / "b"), "AAA");

    writeText(scratch.path() / "c", "CCC");
    const core::Status refused = disk->renameFile(scratch.path() / "c", scratch.path() / "b");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, core::ErrorCode::AlreadyExists);
    EXPECT_EQ(osErrorOf(refused.error()), "183") << refused.error().describe();
    EXPECT_EQ(readText(scratch.path() / "b"), "AAA") << "the target is untouched";
    EXPECT_EQ(readText(scratch.path() / "c"), "CCC");

    const core::Status missing = disk->renameFile(scratch.path() / "absent", scratch.path() / "z");
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, core::ErrorCode::NotFound);
}

TEST(WindowsFs, ALockedFileIsRetriedForTheWholePolicyAndThenFailsWithErrorThirtyTwo)
{
    Scratch scratch("winfs-locked");
    const auto disk = systemFs(10, 150);
    writeText(scratch.path() / "busy.dll", "in use");
    HeldOpen held(scratch.path() / "busy.dll", 0); // share mode 0: nobody may move it
    ASSERT_TRUE(held.ok());

    const auto start = std::chrono::steady_clock::now();
    const core::Status status =
        disk->renameFile(scratch.path() / "busy.dll", scratch.path() / "moved.dll");
    const auto took = std::chrono::steady_clock::now() - start;
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(osErrorOf(status.error()), "32") << status.error().describe(); // ERROR_SHARING_VIOLATION
    EXPECT_GE(took, std::chrono::milliseconds(140)) << "it retried for the policy's total";
    EXPECT_LT(took, std::chrono::milliseconds(5000));
    EXPECT_TRUE(isPresent(scratch.path() / "busy.dll")) << "nothing moved";
    EXPECT_FALSE(isPresent(scratch.path() / "moved.dll"));
}

TEST(WindowsFs, ARenameWaitsOutAScannerThatLetsGoHalfWayThrough)
{
    Scratch scratch("winfs-released");
    const auto disk = systemFs(10, 3000);
    writeText(scratch.path() / "busy.dll", "in use");
    auto held = std::make_unique<HeldOpen>(scratch.path() / "busy.dll", 0);
    ASSERT_TRUE(held->ok());
    std::thread scanner([&held] {
        std::this_thread::sleep_for(std::chrono::milliseconds(120));
        held->release();
    });
    const core::Status status =
        disk->renameFile(scratch.path() / "busy.dll", scratch.path() / "moved.dll");
    scanner.join();
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    EXPECT_TRUE(isPresent(scratch.path() / "moved.dll"));
    EXPECT_FALSE(isPresent(scratch.path() / "busy.dll"));
}

TEST(WindowsFs, ANoRetryPolicyFailsOnTheFirstSharingViolation)
{
    Scratch scratch("winfs-noretry");
    const auto disk = systemFs(10, 0);
    writeText(scratch.path() / "busy.dll", "in use");
    HeldOpen held(scratch.path() / "busy.dll", 0);
    ASSERT_TRUE(held.ok());
    const auto start = std::chrono::steady_clock::now();
    const core::Status status =
        disk->renameFile(scratch.path() / "busy.dll", scratch.path() / "moved.dll");
    ASSERT_FALSE(status.ok());
    EXPECT_LT(std::chrono::steady_clock::now() - start, std::chrono::milliseconds(100));
}

TEST(WindowsFs, ARunningProgramCanBeMovedButNotDeleted)
{
    // Why rollback moves a new program out of the way instead of deleting it (apply.cpp): a
    // program that is running refuses deletion and allows a rename. cmd.exe, copied, is the
    // running program.
    Scratch scratch("winfs-running");
    const auto disk = systemFs(5, 50);
    const Path copy = scratch.path() / "prog.exe";
    fs::copy_file(Path(core::environmentVariable("SystemRoot")) / "System32" / "cmd.exe", copy);

    const std::wstring command = L"\"" + copy.wstring() + L"\" /c ping -n 4 127.0.0.1 >nul";
    std::vector<wchar_t> mutableCommand(command.begin(), command.end());
    mutableCommand.push_back(L'\0');
    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    PROCESS_INFORMATION process{};
    if (!CreateProcessW(copy.c_str(), mutableCommand.data(), nullptr, nullptr, FALSE,
                        CREATE_NO_WINDOW, nullptr, nullptr, &startup, &process)) {
        GTEST_SKIP() << "cannot start a program here";
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(200));

    EXPECT_FALSE(disk->removeFile(copy).ok()) << "a running image cannot be deleted";
    EXPECT_TRUE(disk->renameFile(copy, scratch.path() / "moved.exe").ok())
        << "but it can be renamed";
    TerminateProcess(process.hProcess, 0);
    WaitForSingleObject(process.hProcess, 5000);
    CloseHandle(process.hProcess);
    CloseHandle(process.hThread);
}

TEST(WindowsFs, RemoveFileDeletesIsDoneWhenGoneAndTakesAReadOnlyFile)
{
    Scratch scratch("winfs-remove");
    const auto disk = systemFs();
    writeText(scratch.path() / "a", "x");
    ASSERT_TRUE(disk->removeFile(scratch.path() / "a").ok());
    EXPECT_FALSE(isPresent(scratch.path() / "a"));
    EXPECT_TRUE(disk->removeFile(scratch.path() / "a").ok()) << "already gone is success";
    EXPECT_TRUE(disk->removeFile(scratch.path() / "no" / "such" / "file").ok());

    writeText(scratch.path() / "ro", "x");
    SetFileAttributesW((scratch.path() / "ro").c_str(), FILE_ATTRIBUTE_READONLY);
    EXPECT_TRUE(disk->removeFile(scratch.path() / "ro").ok());
    EXPECT_FALSE(isPresent(scratch.path() / "ro"));

    fs::create_directories(scratch.path() / "dir");
    EXPECT_FALSE(disk->removeFile(scratch.path() / "dir").ok()) << "a folder is not a file";
    EXPECT_TRUE(fs::is_directory(scratch.path() / "dir"));
}

TEST(WindowsFs, RemoveEmptyDirectoryRemovesOnlyAnEmptyOne)
{
    Scratch scratch("winfs-rmdir");
    const auto disk = systemFs();
    fs::create_directories(scratch.path() / "empty");
    ASSERT_TRUE(disk->removeEmptyDirectory(scratch.path() / "empty").ok());
    EXPECT_FALSE(isPresent(scratch.path() / "empty"));
    EXPECT_TRUE(disk->removeEmptyDirectory(scratch.path() / "empty").ok()) << "gone is success";

    writeText(scratch.path() / "full" / "f", "x");
    const core::Status refused = disk->removeEmptyDirectory(scratch.path() / "full");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidState);
    EXPECT_EQ(osErrorOf(refused.error()), "145"); // ERROR_DIR_NOT_EMPTY
    EXPECT_TRUE(isPresent(scratch.path() / "full" / "f"));
}

TEST(WindowsFs, AppendDurableAddsToTheEndAndCreatesTheFile)
{
    Scratch scratch("winfs-append");
    const auto disk = systemFs();
    const Path path = scratch.path() / "journal";
    ASSERT_TRUE(disk->appendDurable(path, "one\n").ok());
    ASSERT_TRUE(disk->appendDurable(path, "two\n").ok());
    ASSERT_TRUE(disk->appendDurable(path, "").ok());
    EXPECT_EQ(readText(path), "one\ntwo\n");

    // A reader that holds the file open for reading does not stop the append.
    HeldOpen reader(path, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    ASSERT_TRUE(reader.ok());
    EXPECT_TRUE(disk->appendDurable(path, "three\n").ok());
    EXPECT_EQ(readText(path), "one\ntwo\nthree\n");
}

TEST(WindowsFs, WriteDurableCreatesAndReplacesWholeAndLeavesNoTemporary)
{
    Scratch scratch("winfs-write");
    const auto disk = systemFs();
    const Path path = scratch.path() / "installed.json";
    ASSERT_TRUE(disk->writeDurable(path, "first version, rather long\n").ok());
    EXPECT_EQ(readText(path), "first version, rather long\n");
    ASSERT_TRUE(disk->writeDurable(path, "short").ok());
    EXPECT_EQ(readText(path), "short") << "replaced, not overwritten in place";
    ASSERT_TRUE(disk->writeDurable(path, "").ok());
    EXPECT_EQ(readText(path), "");
    std::size_t files = 0;
    for (const auto& entry : fs::directory_iterator(scratch.path())) {
        (void)entry;
        ++files;
    }
    EXPECT_EQ(files, 1u) << "no temporary file left beside it";
}

TEST(WindowsFs, TheLockIsExclusiveUntilItIsDestroyed)
{
    Scratch scratch("winfs-lock");
    const auto disk = systemFs();
    fs::create_directories(scratch.path() / ".update");
    const Path lockPath = scratch.path() / ".update" / "lock";

    core::Result<std::unique_ptr<FileLock>> first = disk->tryLock(lockPath);
    ASSERT_TRUE(first.ok());
    EXPECT_TRUE(isPresent(lockPath)) << "created when absent";

    // A second holder - the same process here, another one in life - is refused, "held".
    core::Result<std::unique_ptr<FileLock>> second = disk->tryLock(lockPath);
    ASSERT_FALSE(second.ok());
    EXPECT_EQ(second.error().code, core::ErrorCode::InvalidState);
    EXPECT_EQ(second.error().message, "held");

    // And so is another file system object over the same file.
    const auto other = systemFs();
    EXPECT_FALSE(other->tryLock(lockPath).ok());

    first = core::Result<std::unique_ptr<FileLock>>(core::makeError(core::ErrorCode::Internal, "x"));
    core::Result<std::unique_ptr<FileLock>> third = disk->tryLock(lockPath);
    EXPECT_TRUE(third.ok()) << "free once the holder is gone";

    // A lock in a folder that is not there is NotFound, not "held".
    const core::Result<std::unique_ptr<FileLock>> nowhere =
        disk->tryLock(scratch.path() / "no-such" / "lock");
    ASSERT_FALSE(nowhere.ok());
    EXPECT_NE(nowhere.error().message, "held");
}

TEST(WindowsFs, FreeSpaceIsTheVolumesAndWorksForAFolderThatDoesNotExistYet)
{
    Scratch scratch("winfs-space");
    const auto disk = systemFs();
    const core::Result<std::uint64_t> here = disk->freeSpace(scratch.path());
    const core::Result<std::uint64_t> later = disk->freeSpace(scratch.path() / "not" / "yet");
    ASSERT_TRUE(here.ok());
    ASSERT_TRUE(later.ok());
    // The standard library asks the same volume; the two differ only by what happened between.
    std::error_code ec;
    const std::uint64_t standard = fs::space(scratch.path(), ec).available;
    ASSERT_FALSE(ec);
    const std::uint64_t slack = 256ull * 1024 * 1024;
    EXPECT_LT(here.value() > standard ? here.value() - standard : standard - here.value(), slack);
    EXPECT_LT(later.value() > standard ? later.value() - standard : standard - later.value(), slack);
}

TEST(WindowsFs, SyncDirectoryIsSuccessOnAFolderThatExists)
{
    Scratch scratch("winfs-sync");
    const auto disk = systemFs();
    EXPECT_TRUE(disk->syncDirectory(scratch.path()).ok());
}

TEST(WindowsFs, APathPastTwoHundredAndSixtyCharactersWorksThroughTheLongPathPrefix)
{
    Scratch scratch("winfs-long");
    const auto disk = systemFs();
    Path deep = scratch.path();
    while (deep.native().size() < 320) {
        deep /= "a_folder_with_a_fairly_long_name";
    }
    ASSERT_TRUE(disk->makeDirectories(deep).ok());
    const Path file = deep / "file.dll";
    ASSERT_TRUE(disk->writeDurable(file, "long path bytes").ok());
    const core::Result<std::string> read = disk->readFile(file);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read.value(), "long path bytes");
    ASSERT_TRUE(disk->renameFile(file, deep / "moved.dll").ok());
    const core::Result<EntryInfo> info = disk->stat(deep / "moved.dll");
    ASSERT_TRUE(info.ok());
    EXPECT_EQ(info->kind, EntryKind::File);
    ASSERT_TRUE(disk->copyFile(deep / "moved.dll", deep / "copy.dll").ok());
    EXPECT_TRUE(disk->removeFile(deep / "moved.dll").ok());
    EXPECT_TRUE(disk->removeFile(deep / "copy.dll").ok());
    // The scratch folder's destructor cannot remove a path that long by the short form; the
    // system file system can.
    for (Path folder = deep; folder != scratch.path(); folder = folder.parent_path()) {
        ASSERT_TRUE(disk->removeEmptyDirectory(folder).ok()) << folder.string();
    }
}

TEST(WindowsFs, ANameWithANonAsciiLetterAndOneEndingInADotAreKeptAsTheyAre)
{
    Scratch scratch("winfs-names");
    const auto disk = systemFs();
    const Path accented = scratch.path() / core::pathFromUtf8("caf\xC3\xA9 \xE6\x97\xA5\xE6\x9C\xAC.txt");
    ASSERT_TRUE(disk->writeDurable(accented, "unicode").ok());
    const core::Result<std::string> read = disk->readFile(accented);
    ASSERT_TRUE(read.ok());
    EXPECT_EQ(read.value(), "unicode");

    // Without the \\?\ prefix Windows drops a trailing dot, so "trail." and "trail" would be
    // one file. The prefix keeps them two.
    ASSERT_TRUE(disk->writeDurable(scratch.path() / "trail.", "dotted").ok());
    ASSERT_TRUE(disk->writeDurable(scratch.path() / "trail", "plain").ok());
    EXPECT_EQ(disk->readFile(scratch.path() / "trail.").value(), "dotted");
    EXPECT_EQ(disk->readFile(scratch.path() / "trail").value(), "plain");
    ASSERT_TRUE(disk->removeFile(scratch.path() / "trail.").ok());
    EXPECT_TRUE(isPresent(scratch.path() / "trail"));
}

#endif // _WIN32
