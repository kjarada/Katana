// The planner of include/katana/update/plan.hpp: which manifest files differ from the
// installed ones, by hashing the installed files now.
//
// It hashes with core::Sha256 (sha256FileHex streams the file in blocks; nothing is held)
// and READS only: nothing under the root is created, changed or deleted here. What a
// release removes is decided by the helper alone, from the manifest it verified itself
// (apply.hpp), so there is no list of removals in the Plan.
//
// A link is never followed. A folder on the way to a manifest file that is a link (a
// Windows junction or symbolic link, a POSIX symlink) could lead a read - and, in the
// helper, a write - out of the install, so the plan refuses it by name; the helper refuses
// the same thing again (apply.hpp, step 3). The FILE itself being a link is not followed
// either: it cannot be hashed without reading what it points at, so it counts as changed,
// and the swap replaces the link, never what it points at (a seed that is a link is kept:
// a seed is never read or written once it exists).
//
// What is a link on Windows is decided by the reparse TAG, not by the attribute: a reparse
// point is also what OneDrive's placeholders, Windows' compressed and de-duplicated files and
// ProjFS use, and those stay where they are (a folder in OneDrive is `Directory, ReparsePoint`
// and holds ordinary files). Only a tag with the name-surrogate bit - symbolic link, junction
// and the like - sends a path somewhere else.

#include "katana/update/plan.hpp"

#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <string>
#include <string_view>
#include <system_error>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/core/sha256.hpp"
#include "katana/update/paths.hpp"

#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#endif

namespace katana::update {

// ---- declared here until a shared header carries it (plan.hpp and fsops.hpp are T1's, not T6's)

// Whether a Windows reparse tag is that of a LINK: a name surrogate (winnt.h:
// IsReparseTagNameSurrogate, bit 29), which makes a path stand for another path. Symbolic links
// (0xA000000C), junctions and mount points (0xA0000003), WSL symlinks (0xA000001D) are; OneDrive
// and other cloud placeholders (0x9000001A, 0x9000701A, ...), the de-duplication and compression
// filters (0x80000013, 0x80000017), app execution aliases (0x8000001B) and ProjFS (0x9000001C)
// are not. Pure, so it is tested on every platform. FsOps::stat (T7) and the helper's own check
// (T9) must apply the same rule.
[[nodiscard]] bool isLinkReparseTag(std::uint32_t tag);

bool isLinkReparseTag(std::uint32_t tag)
{
    return (tag & 0x20000000u) != 0;
}

namespace {

namespace fs = std::filesystem;

// kFreeSpaceFactor is applied in integer tenths, so the figure is exact: 2.6 is not a
// binary fraction, and ceil(2.6 * n) in doubles is one rounding away from the right answer
// at some n. Changing the factor must change this too.
static_assert(kFreeSpaceFactor == 2.6, "requiredFreeBytes below is written for a factor of 2.6");
constexpr std::uint64_t kFreeSpaceTenths = 26;

enum class Kind { Missing, File, Directory, Link, Other };

// What is at `path`, without following a link at the last component (lstat). A Windows
// reparse point that is a name surrogate (junction, symbolic link, mount point; see
// isLinkReparseTag) is a Link: std::filesystem on this toolchain reports none of them
// (libstdc++ for Windows has no symlink_status of its own, and answers for the target). Any
// other reparse point (a OneDrive placeholder) is the file or folder it appears to be.
core::Result<Kind> look(const fs::path& path)
{
#ifdef _WIN32
    const DWORD attributes = GetFileAttributesW(path.c_str());
    if (attributes == INVALID_FILE_ATTRIBUTES) {
        const DWORD error = GetLastError();
        if (error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND) {
            return Kind::Missing;
        }
        return core::makeError(core::ErrorCode::FileImportFailure, "cannot examine the file",
                               "os_error=" + std::to_string(error) + "; " + core::pathToUtf8(path));
    }
    if ((attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0) {
        // The tag is in the directory entry: dwReserved0 of what FindFirstFileW returns. The
        // path exists (it has attributes) and has no wildcard that FindFirstFileW would expand.
        WIN32_FIND_DATAW entry;
        const HANDLE find = FindFirstFileW(path.c_str(), &entry);
        if (find == INVALID_HANDLE_VALUE) {
            return core::makeError(core::ErrorCode::FileImportFailure, "cannot examine the file",
                                   "os_error=" + std::to_string(GetLastError()) + "; " +
                                       core::pathToUtf8(path));
        }
        FindClose(find);
        if ((entry.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0 &&
            isLinkReparseTag(entry.dwReserved0)) {
            return Kind::Link;
        }
    }
    return (attributes & FILE_ATTRIBUTE_DIRECTORY) != 0 ? Kind::Directory : Kind::File;
#else
    std::error_code error;
    const fs::file_status status = fs::symlink_status(path, error);
    if (status.type() == fs::file_type::not_found) {
        return Kind::Missing;
    }
    if (error) {
        return core::makeError(core::ErrorCode::FileImportFailure, "cannot examine the file",
                               error.message() + "; " + core::pathToUtf8(path));
    }
    switch (status.type()) {
    case fs::file_type::regular:
        return Kind::File;
    case fs::file_type::directory:
        return Kind::Directory;
    case fs::file_type::symlink:
        return Kind::Link;
    default:
        return Kind::Other;
    }
#endif
}

// The names a manifest path is made of, or nothing when it could leave the root. This is
// the containment rule only - relative, '/', no empty, "." or ".." name, no drive, stream or
// backslash - and not the naming rules of paths.hpp, which the manifest has passed already
// (verifyManifest): a manifest built any other way must still not make a read outside.
bool splitContained(std::string_view path, std::vector<std::string>& names)
{
    names.clear();
    std::size_t start = 0;
    while (start <= path.size()) {
        const std::size_t slash = path.find('/', start);
        const std::size_t length =
            slash == std::string_view::npos ? std::string_view::npos : slash - start;
        const std::string_view name = path.substr(start, length);
        if (name.empty() || name == "." || name == ".." ||
            name.find_first_of("\\:") != std::string_view::npos ||
            name.find('\0') != std::string_view::npos) {
            return false;
        }
        names.emplace_back(name);
        if (slash == std::string_view::npos) {
            break;
        }
        start = slash + 1;
    }
    return !names.empty();
}

enum class Found { Absent, File, Link };

core::Error refused(core::ErrorCode code, const std::string& manifestPath, std::string message)
{
    return core::makeError(code, std::move(message), manifestPath);
}

// Walks root/<names> one name at a time and says what the last one is. A name above the
// last must be a real folder: absent means the file is absent too; a link or a file is an
// error naming the manifest path.
core::Result<Found> locate(const fs::path& root, const std::vector<std::string>& names,
                           const std::string& manifestPath, fs::path& located)
{
    located = root;
    for (std::size_t i = 0; i < names.size(); ++i) {
        located /= core::pathFromUtf8(names[i]);
        const core::Result<Kind> kind = look(located);
        if (!kind) {
            return kind.error();
        }
        if (*kind == Kind::Missing) {
            return Found::Absent;
        }
        const bool last = i + 1 == names.size();
        if (!last) {
            if (*kind == Kind::Link) {
                return refused(core::ErrorCode::InvalidState, manifestPath,
                               "the installed file " + manifestPath +
                                   " is reached through a link (" + names[i] +
                                   "), which the update does not follow");
            }
            if (*kind != Kind::Directory) {
                return refused(core::ErrorCode::InvalidState, manifestPath,
                               "the installed file " + manifestPath +
                                   " cannot be there: " + names[i] + " is a file, not a folder");
            }
            continue;
        }
        switch (*kind) {
        case Kind::File:
            return Found::File;
        case Kind::Link:
            return Found::Link;
        default:
            return refused(core::ErrorCode::InvalidState, manifestPath,
                           "the installed file " + manifestPath + " is a " +
                               (*kind == Kind::Directory ? "folder" : "special file") +
                               ", not a file");
        }
    }
    return Found::Absent; // unreachable: names is not empty
}

bool addChecked(std::uint64_t& total, std::uint64_t amount)
{
    return !__builtin_add_overflow(total, amount, &total);
}

core::Error cancelled()
{
    return core::makeError(core::ErrorCode::InvalidState, "cancelled");
}

} // namespace

core::Result<Plan> plan(const Manifest& manifest, const Install& install, std::stop_token stop)
{
    std::error_code ignored;
    if (!fs::is_directory(install.root, ignored)) {
        return core::makeError(core::ErrorCode::NotFound, "the install folder does not exist",
                               core::pathToUtf8(install.root));
    }
    // The limits parseManifest enforces, again: a plan is only as cheap as the manifest is
    // small, and the totals below must not wrap.
    if (manifest.files.size() > kMaxFiles) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the manifest lists more files than an install may have",
                               std::to_string(manifest.files.size()));
    }
    std::uint64_t allBytes = 0;
    for (const ManifestFile& file : manifest.files) {
        if (!addChecked(allBytes, file.size) || allBytes > kMaxInstallBytes) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the manifest's files add up to more than an install may hold",
                                   std::to_string(kMaxInstallBytes) + " bytes at most");
        }
    }

    Plan result;
    result.totalFiles = manifest.files.size();
    result.files.reserve(manifest.files.size());
    std::vector<RangeMember> toFetch;
    std::vector<std::string> names;

    for (std::size_t index = 0; index < manifest.files.size(); ++index) {
        if (stop.stop_requested()) {
            return cancelled();
        }
        const ManifestFile& file = manifest.files[index];
        if (!splitContained(file.path, names)) {
            return refused(core::ErrorCode::InvalidArgument, file.path,
                           "the manifest names a file outside the install: " + file.path);
        }
        fs::path located;
        const core::Result<Found> found = locate(install.root, names, file.path, located);
        if (!found) {
            return found.error();
        }

        bool changed = false;
        const bool seed = file.fileClass == FileClass::Seed;
        switch (*found) {
        case Found::Absent:
            changed = true; // a seed is written when it is absent
            break;
        case Found::Link:
            changed = !seed; // cannot be hashed without following it
            break;
        case Found::File:
            if (!seed) { // a seed that exists is the person's: never read, never replaced
                const core::Result<std::string> hash = core::sha256FileHex(located);
                if (!hash) {
                    return core::makeError(
                        core::ErrorCode::FileImportFailure,
                        "cannot read the installed file " + file.path +
                            " (is it open in another program, or not readable by you?)",
                        file.path + "; " + hash.error().describe());
                }
                changed = *hash != file.sha256;
            }
            break;
        }

        result.files.push_back({index, changed});
        if (!changed) {
            continue;
        }
        ++result.changedFiles;
        result.changedBytes += file.size; // bounded by allBytes above
        if (!addChecked(result.downloadBytes, file.compressedSize)) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the manifest's compressed sizes add up to more than 64 bits");
        }
        toFetch.push_back({index, file.offset, file.compressedSize});
    }

    // ceil(2.6 * n) = (26 n + 9) / 10; n <= 2 GiB here, so 26 n cannot wrap.
    result.requiredFreeBytes = (result.changedBytes * kFreeSpaceTenths + 9) / 10;
    result.ranges = mergeRanges(std::move(toFetch));
    return result;
}

} // namespace katana::update
