#pragma once

// What apply.cpp and recover.cpp share, and what the tests reach: INTERNAL to the update library
// and never installed.

#include <chrono>
#include <exception>
#include <filesystem>
#include <optional>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/core/path_text.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/process.hpp"

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
[[nodiscard]] core::Status recoverLocked(const std::filesystem::path& root,
                                         const ApplyServices& services, std::string_view except,
                                         const std::filesystem::path& stateDirectory);

// recover() (recover.hpp) with its seams given: the file system, the launcher that runs the
// helper copy, the state folder U, and how long a live holder of the lock is waited for.
// `stateDirectory` (U, or empty) is only READ, to find the helper copy U/run-<tx>/: nothing in
// it is made, changed or deleted, whichever program calls (recover.hpp).
[[nodiscard]] core::Status recoverWith(const std::filesystem::path& root, FsOps& fs,
                                       Launcher& launcher,
                                       const std::filesystem::path& stateDirectory,
                                       std::chrono::milliseconds lockWait);

} // namespace katana::update::detail
