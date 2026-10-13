#pragma once

// The apply's journal: R/.update/<tx>/journal, append-only text, one record
// per line, written so that a crash at any byte leaves something recover() can
// read.
//
//   <n> <OP> <arg>... <crc>\n
//
//   n    1, 2, 3 ... the record's place in the journal
//   OP   an operation name below
//   arg  zero or more arguments, each percent-encoded: every byte outside
//        [A-Za-z0-9._/~-] is %XX (upper-case hex), so an argument never
//        contains a space or a line break
//   crc  CRC-32 (IEEE 802.3, the zlib crc32) of the line's bytes before the
//        space that precedes it, as 8 lower-case hex digits
//
// A record is complete only with its LF. Parsing stops at the first line that
// is not complete, whose crc is wrong, or whose n is not the previous one plus
// one: the journal is append-only, so a bad line can only be the last (a torn
// write) and everything from it on is ignored (`tornTail`).
//
// Records are appended in GROUPS with one flush to stable storage per group
// (FsOps::appendDurable), so the PLAN - every operation the transaction will
// do - is on disk before the first file moves.
//
//   PLAN <tx> <fromSequence> <toSequence>        header; first record
//   ADD <path>          a file the new version adds (absent before): rollback deletes it
//   REPLACE <path>      a file present with other bytes: the old one moves to old/<path>
//   REMOVE <path>       a file of the old manifest, absent from the new, whose disk
//                       hash equals the old one's: it moves to old/<path>
//   MKDIR <path>        a directory this transaction creates: rollback removes it when empty
//   COMMITTING          the renames begin; from here the tree is mid-swap
//   MOVED_OLD <path>    informational progress (R/<path> moved to old/<path>)
//   MOVED_NEW <path>    informational progress (new/<path> moved to R/<path>)
//   COMMITTED_PENDING_HEALTH   every file is in place; the new version has not yet proved it starts
//   ROLLING_BACK        rollback begins (from a failure, a timeout, or recover())
//   ROLLED_BACK         rollback finished; the tree is the old version's
//   HEALTHY             the new version started and wrote its marker; the transaction is over
//
// Paths are root-relative with '/', as in the manifest.
//
// WHO WRITES WHAT FOLLOWS COMMITTED_PENDING_HEALTH. The helper that made the swap, while
// it lives: HEALTHY once its health run has passed, or ROLLING_BACK and ROLLED_BACK when
// it has not. A helper that dies in between leaves the journal at
// COMMITTED_PENDING_HEALTH, and no further kind of record exists for that: the SAME
// records are appended later, under the update lock, by whoever settles the transaction
// (recover.hpp) - HEALTHY by `katana_updater --recover`, by the window of the new version
// or by the next swap, each on the marker the health run writes; ROLLING_BACK and
// ROLLED_BACK by `katana_updater --recover` alone. So a journal still reads the same way
// whoever finished it. And it stays TRUE of the tree: while it says
// COMMITTED_PENDING_HEALTH the tree is the whole new version, because a rollback from that
// state moves no file before its ROLLING_BACK record is on disk, and the folder and the
// marker are deleted only after HEALTHY is (apply.cpp).
//
// THE JOURNAL IS NOT AUTHENTICATED. It sits in R/.update, which a process running as the
// same user can write, and its crc detects a torn write, not a forged line. So what rollback
// and recovery READ from it is never trusted as a place to act on: every path of an ADD,
// REPLACE or REMOVE record is checked with checkJournalPath (paths.hpp: the manifest
// rules, plus the helper's own three records), and that of an MKDIR record with
// checkJournalDirectory (the same, and a bare install folder such as "etc"), before any file
// is touched, and a record that fails is skipped and reported, not followed; the transaction
// id in the folder name is checked with isValidTransactionId (apply.hpp). A forged journal
// can therefore at worst make rollback do less, never reach outside R's bin, share, etc and
// lib.
//
// Rollback does NOT replay the journal backwards. It decides from the disk
// (apply.hpp), so it is idempotent after any crash, including a crash inside
// itself; the journal says which transaction state to resume (state) and
// which files and directories the transaction owns (the PLAN group).

#include <cstdint>
#include <filesystem>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/update/fsops.hpp"

namespace katana::update {

enum class JournalOp {
    Plan,
    Add,
    Replace,
    Remove,
    MakeDirectory,
    Committing,
    MovedOld,
    MovedNew,
    CommittedPendingHealth,
    RollingBack,
    RolledBack,
    Healthy,
};

[[nodiscard]] std::string_view toString(JournalOp op); // "PLAN" "ADD" ... "HEALTHY"
[[nodiscard]] core::Result<JournalOp> parseJournalOp(std::string_view text);

struct JournalRecord {
    JournalOp op = JournalOp::Plan;
    std::vector<std::string> args; // raw (decoded) arguments
};

// Where the transaction stands, from the last state-bearing record.
enum class JournalState {
    Empty,                  // no complete record
    Planned,                // PLAN written, COMMITTING not
    Committing,
    CommittedPendingHealth,
    RollingBack,
    RolledBack,
    Healthy,
};

// A journal in Planned, Committing or RollingBack is one a crash left behind:
// recover() must roll it back. CommittedPendingHealth is NOT one of them: its
// tree is a whole version and its helper may be alive and waiting, so recover()
// leaves it (recover.hpp says who settles one whose helper died, and how).
[[nodiscard]] bool needsRecovery(JournalState state);

struct ParsedJournal {
    std::vector<JournalRecord> records; // complete, valid, consecutive
    JournalState state = JournalState::Empty;
    bool tornTail = false;              // bytes after the last good record were ignored
};

// CRC-32 (IEEE 802.3), as zlib's crc32().
[[nodiscard]] std::uint32_t crc32(std::string_view bytes);

// "<n> <OP> <args> <crc>\n" for one record.
[[nodiscard]] std::string encodeRecord(std::uint64_t n, const JournalRecord& record);

// The journal text -> its good prefix. Never fails: whatever cannot be read is
// the torn tail.
[[nodiscard]] ParsedJournal parseJournal(std::string_view text);

// A journal file, appended through FsOps so the fault loops see every write.
class Journal {
  public:
    Journal(FsOps& fs, std::filesystem::path path);

    [[nodiscard]] const std::filesystem::path& path() const { return path_; }

    // Appends the records as one group, numbered from the journal's next
    // number, with one flush. The Journal reads the file once (on the first
    // append) to learn the next number.
    [[nodiscard]] core::Status append(std::span<const JournalRecord> group);

    // Reads and parses the file; an absent file is an Empty journal.
    [[nodiscard]] core::Result<ParsedJournal> load() const;

  private:
    FsOps& fs_;
    std::filesystem::path path_;
    std::uint64_t next_ = 0; // 0: not yet known
};

} // namespace katana::update
