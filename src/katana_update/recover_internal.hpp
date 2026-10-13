#pragma once

// What apply.cpp and recover.cpp share, and what the tests reach: INTERNAL to the update library
// and never installed.

#include <chrono>
#include <cstdint>
#include <exception>
#include <filesystem>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "command_line.hpp"
#include "katana/core/error.hpp"
#include "katana/core/path_text.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/process.hpp"
#include "katana/update/recover.hpp"
#include "katana/update/registry.hpp"

namespace katana::update::detail {

// The install folder as every comparison in the swap wants it: lexically normal and without a
// trailing separator. `C:\x\R\` (a shell's tab completion adds the backslash) and `C:\x\R` name
// one folder, and a walk up from a file towards the root stops only at the second spelling.
// An empty path stays empty; a drive root keeps its separator.
inline std::filesystem::path normalizedRoot(const std::filesystem::path& root)
{
    std::filesystem::path normal = root.lexically_normal();
    if (normal.has_relative_path() && !normal.has_filename()) {
        normal = normal.parent_path();
    }
    return normal;
}

// The last component of `path` as UTF-8 text, or nothing when it is not text: a name with an
// unpaired surrogate, which the file systems of Windows allow, cannot be converted (the
// conversion throws). Every name read from a folder somebody else can write goes through this,
// so one odd name cannot stop a program from starting.
[[nodiscard]] inline std::optional<std::string> nameAsText(const std::filesystem::path& path)
{
    try {
        return core::pathToUtf8(path.filename());
    } catch (const std::exception&) {
        return std::nullopt;
    }
}

// `text` on one line: a control character (a line feed in a path that came from a journal)
// becomes %XX, so that a line written to a log cannot be made to look like two.
[[nodiscard]] inline std::string oneLine(std::string_view text)
{
    constexpr std::string_view hex = "0123456789ABCDEF";
    std::string out;
    out.reserve(text.size());
    for (const char c : text) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte < 0x20 || byte == 0x7F) {
            out += '%';
            out += hex[byte >> 4];
            out += hex[byte & 0x0F];
        } else {
            out += c;
        }
    }
    return out;
}

// The RunOnce command that runs recovery at the next logon: `"<helper>" --recover <R>`, both
// paths with the native separators, because it is the shell that parses it then. The ONE place
// it is written: the swap registers it when its journal says COMMITTING (apply.cpp), and
// settleUnproved registers it again before it settles anything (recover.cpp) - Windows takes
// the entry away before it runs the command, so the run that is to keep the logon net has to
// put the very same text back.
[[nodiscard]] inline std::string runOnceCommand(std::filesystem::path helper,
                                                std::filesystem::path root)
{
    return commandLine(core::pathToUtf8(helper.make_preferred()),
                       {"--recover", core::pathToUtf8(root.make_preferred())});
}

// A state.json that could not be read, and whether the helper may write the defaults over it
// (it records a release whose health check failed). One that is not JSON of ours (ParseFailure)
// is carried on from with the defaults, as state.hpp says: it is already lost. Any other failure
// to read it - a scanner holds the file for a moment, the disk hiccups - is no reason to write
// the defaults over the person's "automatic" and "skipped"; and a newer schema (Unsupported) is
// another version's file.
[[nodiscard]] inline bool mayReplaceUnreadableState(const core::Error& error)
{
    return error.code == core::ErrorCode::ParseFailure;
}

// Puts back every transaction folder under R/.update that a crash (or a rollback that could not
// finish) left unfinished, and sweeps the folders of the finished ones - except the one named
// `except` (the caller's own, which may not exist yet). The CALLER HOLDS THE UPDATE LOCK.
// `stateDirectory` (U, or empty) is where what a lock keeps from being deleted is queued.
// An error is the first rollback that failed; the others were still tried.
// A folder whose journal says COMMITTED_PENDING_HEALTH is neither: it is not looked at here
// (settlePending, below, is what decides one).
[[nodiscard]] core::Status recoverLocked(const std::filesystem::path& root,
                                         const ApplyServices& services, std::string_view except,
                                         const std::filesystem::path& stateDirectory);

// recover() (recover.hpp) with its seams given: the file system, the launcher that runs the
// helper copy, the state folder U, how long a live holder of the lock is waited for, and
// whether the calling program was started as administrator (process.hpp, processIsElevated:
// recover() hands its answer on, a test says which).
// `stateDirectory` (U, or empty) is only READ, to find the helper copy U/run-<tx>/: nothing in
// it is made, changed or deleted, whichever program calls (recover.hpp).
// With `elevated` the copy in U is not even looked for: U is a folder the same person's
// programs without administrator rights can write, and nothing is run with those rights from
// it. The install's own R/bin/katana_updater is run, or - where that is not there - the answer
// is NotFound with the command to run by hand.
[[nodiscard]] core::Status recoverWith(const std::filesystem::path& root, FsOps& fs,
                                       Launcher& launcher,
                                       const std::filesystem::path& stateDirectory,
                                       std::chrono::milliseconds lockWait, bool elevated);

// ---- an update that is installed and was never said to start ------------------------------------
//
// A helper that dies between its COMMITTED_PENDING_HEALTH record and its HEALTHY one (power, a
// shutdown, the process ended) leaves a tree that is a whole new version and a folder that
// nothing ever finished. recover() leaves both alone (recover.hpp). What settles them is ONE
// rule, settlePending, with three callers that differ only in what they may do:
// `katana_updater --recover` (settleUnproved, recover.hpp), the window of the new version
// (confirmStartedWith) and the next swap (Applier::begin, apply.cpp).

// What settlePending did, by transaction id, in the order of the names.
struct PendingOutcome {
    std::vector<std::string> healthy;    // proved: HEALTHY written, its folder deleted
    std::vector<std::string> rolledBack; // never proved and put back: the tree is the old version's
    std::vector<std::string> left;       // could not be decided now: it is as it was
    // The rollback did not finish, or could not begin: put back in part, or not at all.
    std::vector<std::string> incomplete;
    // What a lock kept from being deleted under R/.update (each such folder keeps its journal,
    // which says HEALTHY, so every later start sweeps it: remove_tree.hpp). Reported, not
    // queued: of the three callers one may not write the per-user folder.
    std::vector<std::filesystem::path> leftovers;
    // The pass could not look at every folder: R/.update could not be listed, a transaction's
    // folder could not be examined or its journal is there and would not be read (a scanner or
    // a backup has it open), or the pass was cut short. The lists above are then what it got
    // to, and an empty `left` is NOT "none is left": a caller that goes on only when everything
    // is settled must not go on.
    bool unread = false;
};

// THE RULE. For every folder under R/.update whose journal says COMMITTED_PENDING_HEALTH, but
// the one named `except` (the caller's own). THE CALLER HOLDS THE UPDATE LOCK, which is how it
// knows that no helper is alive and waiting on any of them.
//
//   HEALTHY      when U/healthy-<tx> is a file - the proof the swap itself accepts (apply.hpp,
//                step 7), whoever wrote it. For an installer-kind install the version Settings >
//                Apps shows is rewritten (the swap does that only once the proof is there, and
//                it may have died before); then HEALTHY is appended, one flushed record; then
//                the folder is deleted, its journal last.
//   ROLLED_BACK  when `mayRollBack`, and there is no marker, and the tree is still that release
//                (treeIsRelease, with the `toSequence` of the journal's PLAN record), and the
//                process scan - asked before the hashing, and again as the last thing - answers
//                and names nobody but this process: rollback() (apply.hpp), as it is.
//                Two more things are looked at first, because "no marker" and "put back" are
//                claims too. `stateDirectory` must be the folder the update was applied from -
//                it holds the dead helper's copy, U/run-<tx> - or a marker missing from it
//                says nothing (KATANA_UPDATE_STATE_DIR set for Katana and not for this run;
//                another account). And every file the PLAN's REPLACE and REMOVE records moved
//                aside must still be a file under <tx>/old, or the rollback would put back
//                some of the old version and call the mix whole.
//   left         in every other case - a marker that cannot be looked for, a scan that fails,
//                a tree somebody changed, a program of the install running - and then NOTHING
//                is written: not a journal record, not a file, not a registry value. (One
//                case writes a value: a marker that IS there and a HEALTHY record that cannot
//                be appended. The version Settings > Apps shows was rewritten before the
//                append, and it names what is installed either way.)
//
// A folder whose journal could not be READ is none of the three: what it holds is not known, so
// nothing is done to it, and the outcome says `unread` - it may be such an update, and no caller
// may take the pass for one that saw everything.
//
// `stateDirectory` (U, or empty: then there is no evidence, and nothing is decided) is only
// READ. Starts no program. Throws nothing but a FaultFs's SimulatedCrash.
[[nodiscard]] PendingOutcome settlePending(const std::filesystem::path& root,
                                           const ApplyServices& services, std::string_view except,
                                           const std::filesystem::path& stateDirectory,
                                           bool mayRollBack);

// What a caller that MAY write the per-user folder does once the rule has spoken: for each
// transaction settled HEALTHY its marker U/healthy-<tx> and the helper copy U/run-<tx> are
// deleted - the helper that ran from the copy is dead, and nothing starts it again - and what
// would not go, with `outcome.leftovers`, is put on U/cleanup.list for the window's next start.
void forgetSettled(FsOps& fs, const std::filesystem::path& stateDirectory,
                   const PendingOutcome& outcome);

// confirmStarted (recover.hpp) with its seams given.
[[nodiscard]] Confirmed confirmStartedWith(const std::filesystem::path& root,
                                           const std::filesystem::path& stateDirectory, FsOps& fs,
                                           RegistryOps& registry);

// Whether the tree under `root` is still the release whose manifest sequence is `sequence`:
// share/katana/installed.manifest.json verifies against its signature with `services.trust`
// (and names this install's product and target), its sequence is `sequence`, and every file it
// lists that is not a seed is an ordinary file whose SHA-256 is the listed one. A seed is left
// out because the swap never replaces one that is there: the person's edit of it is not a
// changed release. False for whatever cannot be read. Changes nothing; in apply.cpp, beside the
// hashing it shares with the plan.
[[nodiscard]] bool treeIsRelease(const std::filesystem::path& root, std::uint64_t sequence,
                                 const ApplyServices& services);

} // namespace katana::update::detail
