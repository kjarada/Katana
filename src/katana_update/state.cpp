// The records the update system reads and writes beside the programs and in the per-user
// state folder: the install's own (install.hpp: install.json, installed.json and the signed
// manifest kept for removals) and the user's (state.hpp: state.json, last.json, the cleanup
// list). They share this file because they are all small JSON or text records read and
// written the same way.
//
// Already finished and tested elsewhere (contract.cpp): installRootOfExecutable and
// checkIsDue. installRootOfRunningProgram below only finds the executable's path and hands it
// to the first.

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <compare>
#include <cstdint>
#include <cstdio>
#include <fstream>
#include <functional>
#include <iterator>
#include <optional>
#include <string>
#include <string_view>
#include <system_error>
#include <thread>
#include <vector>

#include <nlohmann/json.hpp>

#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/update/install.hpp"
#include "katana/update/state.hpp"

#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <io.h>
#include <windows.h>
#else
#include <fcntl.h>
#include <unistd.h>
#if defined(__APPLE__)
#include <mach-o/dyld.h>
#endif
#endif

namespace katana::update {

namespace {

namespace stdfs = std::filesystem;
using Json = nlohmann::json;

// install.json, installed.json and last.json are a few lines; state.json lists the releases
// that failed their health check, one number each. Anything longer was not written by this
// program, and the files are user-writable, so they are not read whole into memory.
constexpr std::uint64_t kMaxRecordBytes = 64 * 1024;
constexpr std::uint64_t kMaxStateBytes = 1024 * 1024;

constexpr std::string_view kCleanupListName = "cleanup.list";
// A list of paths, a line each: at about 200 bytes a line this is some five thousand
// leftovers, where a healthy install has a handful.
constexpr std::uint64_t kMaxCleanupListBytes = 1024 * 1024;
// A folder is removed by walking it; one nested deeper than anything the updater creates (a
// transaction folder is about six levels) is not walked.
constexpr int kMaxCleanupDepth = 64;

core::Error parseFailure(std::string message, const stdfs::path& file)
{
    return core::makeError(core::ErrorCode::ParseFailure, std::move(message),
                           core::pathToUtf8(file));
}

// The whole file, read in blocks and stopped the moment it is longer than `maxBytes`: the files
// are user-writable, and what is read is bounded by this program, not by the file.
//
// Windows: opened with every share mode, DELETE included, so that this reader never stops
// another process from renaming or deleting the record. (A rename OVER a record that is open
// still waits for the reader to close it - Windows keeps the name until the last handle goes -
// which is why writeRecord retries.)
core::Result<std::string> readCapped(const stdfs::path& file, std::uint64_t maxBytes)
{
    std::string text;
    std::array<char, 16 * 1024> block;
    auto tooLong = [&file] {
        return parseFailure("the record is longer than any this program writes", file);
    };
#ifdef _WIN32
    constexpr DWORD kShareAll = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    const HANDLE handle = CreateFileW(file.c_str(), GENERIC_READ, kShareAll, nullptr, OPEN_EXISTING,
                                      FILE_ATTRIBUTE_NORMAL, nullptr);
    if (handle == INVALID_HANDLE_VALUE) {
        return core::makeError(core::ErrorCode::FileImportFailure, "the record cannot be opened",
                               "os_error=" + std::to_string(GetLastError()) + "; " +
                                   core::pathToUtf8(file));
    }
    for (;;) {
        DWORD got = 0;
        if (!ReadFile(handle, block.data(), static_cast<DWORD>(block.size()), &got, nullptr)) {
            const DWORD error = GetLastError();
            CloseHandle(handle);
            return core::makeError(core::ErrorCode::FileImportFailure, "the record cannot be read",
                                   "os_error=" + std::to_string(error) + "; " +
                                       core::pathToUtf8(file));
        }
        if (got == 0) {
            break;
        }
        text.append(block.data(), got);
        if (text.size() > maxBytes) {
            CloseHandle(handle);
            return tooLong();
        }
    }
    CloseHandle(handle);
#else
    std::ifstream in(file, std::ios::binary);
    if (!in) {
        return core::makeError(core::ErrorCode::FileImportFailure, "the record cannot be opened",
                               core::pathToUtf8(file));
    }
    while (in.read(block.data(), static_cast<std::streamsize>(block.size())) || in.gcount() > 0) {
        text.append(block.data(), static_cast<std::size_t>(in.gcount()));
        if (text.size() > maxBytes) {
            return tooLong();
        }
    }
    if (in.bad()) {
        return core::makeError(core::ErrorCode::FileImportFailure, "the record cannot be read",
                               core::pathToUtf8(file));
    }
#endif
    return text;
}

// The file's text; std::nullopt when there is no such file; an error when it is there and
// cannot be read or is longer than `maxBytes`.
core::Result<std::optional<std::string>> readOptionalText(const stdfs::path& file,
                                                          std::uint64_t maxBytes)
{
    std::error_code error;
    const stdfs::file_status status = stdfs::status(file, error);
    if (status.type() == stdfs::file_type::not_found) {
        return std::optional<std::string>{};
    }
    if (error) {
        return core::makeError(core::ErrorCode::FileImportFailure, "the record cannot be read",
                               error.message() + "; " + core::pathToUtf8(file));
    }
    if (status.type() != stdfs::file_type::regular) {
        return core::makeError(core::ErrorCode::FileImportFailure,
                               "the record cannot be read: it is not a file",
                               core::pathToUtf8(file));
    }
    core::Result<std::string> text = readCapped(file, maxBytes);
    if (!text) {
        return text.error();
    }
    return std::optional<std::string>(std::move(*text));
}

// A JSON object with an integer `schema` member: ParseFailure when the text is not that,
// Unsupported when the schema is not `supported`.
core::Result<Json> parseRecord(const std::string& text, const stdfs::path& file,
                               std::int64_t supported)
{
    Json json = Json::parse(text, nullptr, /*allow_exceptions=*/false);
    if (json.is_discarded() || !json.is_object()) {
        return parseFailure("the record is not a JSON object", file);
    }
    const auto schema = json.find("schema");
    if (schema == json.end() || !schema->is_number_integer()) {
        return parseFailure("the record has no integer \"schema\"", file);
    }
    const bool known = schema->is_number_unsigned()
                           ? schema->get<std::uint64_t>() == static_cast<std::uint64_t>(supported)
                           : schema->get<std::int64_t>() == supported;
    if (!known) {
        return core::makeError(core::ErrorCode::Unsupported,
                               "the record is of a schema this version does not know (it was "
                               "written by a newer or older Katana)",
                               core::pathToUtf8(file));
    }
    return json;
}

// A string member that must be there (`required`) or may be absent (then `fallback`).
core::Result<std::string> stringMember(const Json& json, const char* name, bool required,
                                       const stdfs::path& file, std::string fallback = {})
{
    const auto member = json.find(name);
    if (member == json.end()) {
        if (required) {
            return parseFailure(std::string("the record has no \"") + name + "\"", file);
        }
        return fallback;
    }
    if (!member->is_string()) {
        return parseFailure(std::string("\"") + name + "\" is not a string", file);
    }
    return member->get<std::string>();
}

// ---- writing a record whole or not at all -------------------------------------------------

struct Replaced {
    bool done = false;
    bool worthRetrying = false; // the cause may pass: another program has the record open
    std::string why;
};

// `from` renamed over `to`, and the rename itself on the disk before this returns: the data was
// flushed already, but a rename that a power failure can undo would take last.json's note that
// an update was rolled back with it. Windows: MoveFileExW with MOVEFILE_WRITE_THROUGH, so that
// the reason is also the system's own error number and the cases worth waiting for can be told
// from the rest; elsewhere the folder is fsynced after the rename, and a rename that fails
// fails for good.
Replaced replaceFile(const stdfs::path& from, const stdfs::path& to)
{
#ifdef _WIN32
    if (MoveFileExW(from.c_str(), to.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
        return {true, false, {}};
    }
    const DWORD error = GetLastError();
    // 32 sharing violation and 33 lock violation: somebody has the record open. 5 access
    // denied: the same when a scanner holds it, but also what a read-only record or a folder in
    // its place gives, which no wait cures.
    bool worthRetrying = error == ERROR_SHARING_VIOLATION || error == ERROR_LOCK_VIOLATION;
    if (error == ERROR_ACCESS_DENIED) {
        const DWORD attributes = GetFileAttributesW(to.c_str());
        worthRetrying = attributes == INVALID_FILE_ATTRIBUTES ||
                        (attributes & (FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_DIRECTORY)) == 0;
    }
    return {false, worthRetrying, "os_error=" + std::to_string(error)};
#else
    std::error_code error;
    stdfs::rename(from, to, error);
    if (error) {
        return {false, false, error.message()};
    }
#ifdef O_DIRECTORY
    // Best effort: the record is in place whatever this says, and a file system that cannot
    // fsync a folder (some network ones) is no reason to call the write failed.
    const int folder = ::open(to.parent_path().c_str(), O_RDONLY | O_DIRECTORY);
    if (folder >= 0) {
        (void)::fsync(folder);
        (void)::close(folder);
    }
#endif
    return {true, false, {}};
#endif
}

// `text` written to a new file `path` and pushed to stable storage before it is closed
// (_commit is FlushFileBuffers; fsync).
bool writeAndFlush(const stdfs::path& path, const std::string& text)
{
#ifdef _WIN32
    std::FILE* file = _wfopen(path.c_str(), L"wb");
#else
    std::FILE* file = std::fopen(path.c_str(), "wb");
#endif
    if (file == nullptr) {
        return false;
    }
    bool good = std::fwrite(text.data(), 1, text.size(), file) == text.size();
    good = std::fflush(file) == 0 && good;
#ifdef _WIN32
    good = _commit(_fileno(file)) == 0 && good;
#else
    good = ::fsync(::fileno(file)) == 0 && good;
#endif
    good = std::fclose(file) == 0 && good;
    return good;
}

// The text goes to a file of its own beside the record, is flushed to the disk, and is
// renamed over it: a reader sees the old record or the new one, never half of either, and a
// crash or a full disk leaves the old one in place. The flush comes BEFORE the rename: a
// rename that reaches the disk ahead of the data it names leaves a record of zero length, and
// last.json is the one place a person learns that an update was rolled back. (The swap's own
// files go through the FsOps seam, fsops.hpp; these two records are written here, with no
// FsOps to hand, so the fault loops do not see them.) A record that is unreadable anyway is
// ParseFailure to the reader, which carries on with the defaults and says so (state.hpp).
core::Status writeRecord(const stdfs::path& directory, const std::string& name,
                         const std::string& text)
{
    std::error_code error;
    stdfs::create_directories(directory, error);
    if (error) {
        return core::makeError(core::ErrorCode::FileExportFailure,
                               "the update state folder cannot be created",
                               error.message() + "; " + core::pathToUtf8(directory));
    }

    static std::atomic<std::uint64_t> counter{0};
    const std::uint64_t unique =
        static_cast<std::uint64_t>(std::chrono::steady_clock::now().time_since_epoch().count()) ^
        (counter.fetch_add(1) << 48) ^ std::hash<std::thread::id>{}(std::this_thread::get_id());
    const stdfs::path temporary = directory / (name + "." + std::to_string(unique) + ".tmp");
    const stdfs::path target = directory / name;

    if (!writeAndFlush(temporary, text)) {
        stdfs::remove(temporary, error);
        return core::makeError(core::ErrorCode::FileExportFailure,
                               "the record cannot be written", core::pathToUtf8(temporary));
    }

    // A reader that has the old record open, or a virus scanner that has just seen the new
    // one, can make the replacement fail for a while on Windows (sharing violation, lock
    // violation, access denied). last.json is the only way the person learns that an update was
    // rolled back, so it is worth waiting for: the tries back off from 2 ms to 100 ms and stop
    // after about two seconds. A failure that waiting cannot cure is reported at once.
    constexpr auto kReplaceFor = std::chrono::seconds(2);
    constexpr auto kLongestPause = std::chrono::milliseconds(100);
    const auto giveUp = std::chrono::steady_clock::now() + kReplaceFor;
    auto pause = std::chrono::milliseconds(2);
    std::string why;
    for (;;) {
        const Replaced replaced = replaceFile(temporary, target);
        if (replaced.done) {
            return {};
        }
        why = replaced.why;
        if (!replaced.worthRetrying || std::chrono::steady_clock::now() + pause > giveUp) {
            break;
        }
        std::this_thread::sleep_for(pause);
        pause = std::min(pause * 2, kLongestPause);
    }
    std::error_code ignored;
    stdfs::remove(temporary, ignored);
    return core::makeError(core::ErrorCode::FileExportFailure, "the record cannot be replaced",
                           why + "; " + core::pathToUtf8(target));
}

// stateDirectory() is an empty path on a machine with no home to put the state in, and an
// empty path joined with a name is a name in the WORKING folder: wherever the program was
// started from. Every function below that takes the state folder refuses an empty one, so that
// state.json, last.json and cleanup.list are never read, written or acted on there.
core::Status requireStateDirectory(const stdfs::path& stateDir)
{
    if (stateDir.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "there is no update state folder (no home folder to put it in)");
    }
    return {};
}

std::string dump(const Json& json)
{
    // `replace`: a byte that is not UTF-8 in a string (a version typed by hand) becomes
    // U+FFFD instead of making dump() throw.
    return json.dump(2, ' ', false, Json::error_handler_t::replace) + "\n";
}

// ---- install records ----------------------------------------------------------------------

constexpr std::string_view kInstallRecord = "install.json";
constexpr std::string_view kInstalledRecord = "installed.json";
constexpr std::string_view kInstalledManifest = "installed.manifest.json";
constexpr std::string_view kInstalledSignature = "installed.manifest.json.sig";

// The token in install.json becomes part of a URL (feed.hpp: Katana-update-<target>.json), so
// it is held to the characters the real ones use, without being a closed list that a new
// platform would have to change.
bool validTarget(const std::string& target)
{
    return !target.empty() && target.size() <= 32 &&
           std::all_of(target.begin(), target.end(), [](char c) {
               return (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-' || c == '_';
           });
}

bool isRegularFile(const stdfs::path& path)
{
    std::error_code error;
    return stdfs::is_regular_file(path, error);
}

// A build of Katana as one of the two records names it: its version, and the sequence of its
// release where the record has one.
struct RecordedBuild {
    Version version;
    std::optional<std::uint64_t> sequence;
};

// The record's "version": a string that is a version (version.hpp), or the record does not read.
core::Result<Version> versionMember(const Json& json, const stdfs::path& file)
{
    const core::Result<std::string> text = stringMember(json, "version", true, file);
    if (!text) {
        return text.error();
    }
    core::Result<Version> version = parseVersion(*text);
    if (!version) {
        return parseFailure("\"version\" is not a version: " + version.error().describe(), file);
    }
    return version;
}

// The record's "sequence": a whole number, not negative, that fits 64 bits.
core::Result<std::uint64_t> sequenceMember(const Json& json, const stdfs::path& file)
{
    const auto sequence = json.find("sequence");
    if (sequence == json.end() || !sequence->is_number_unsigned()) {
        return parseFailure("\"sequence\" is not a whole number", file);
    }
    return sequence->get<std::uint64_t>();
}

// What a package says of ITSELF in install.json, beside its target:
//
//   {"schema":1,"target":"win64","version":"0.5.0","sequence":1792312800}
//
// `version` is the version the package was built as, and `sequence` the sequence of the release
// it belongs to: the number the release workflow settles before it builds and writes into that
// release's update manifests. A package built outside the workflow has no release and names its
// version alone. One that names neither says nothing of itself (std::nullopt): the record as it
// was before it carried them, which every test tree that only says its target still is. A
// sequence with no version to belong to is no record of a build, and does not read.
core::Result<std::optional<RecordedBuild>> packageBuild(const Json& record,
                                                        const stdfs::path& file)
{
    const bool namesSequence = record.contains("sequence");
    if (!record.contains("version")) {
        if (namesSequence) {
            return parseFailure("the record has a \"sequence\" and no \"version\" it could "
                                "belong to",
                                file);
        }
        return std::optional<RecordedBuild>{};
    }
    const core::Result<Version> version = versionMember(record, file);
    if (!version) {
        return version.error();
    }
    RecordedBuild build;
    build.version = *version;
    if (namesSequence) {
        const core::Result<std::uint64_t> sequence = sequenceMember(record, file);
        if (!sequence) {
            return sequence.error();
        }
        build.sequence = *sequence;
    }
    return std::optional<RecordedBuild>(build);
}

// Whether the folder the installer recorded is the install root: the same folder when both
// exist (case, junctions and trailing separators then do not matter), else the same text.
bool sameFolder(const stdfs::path& recorded, const stdfs::path& root)
{
    std::error_code error;
    if (stdfs::equivalent(recorded, root, error) && !error) {
        return true;
    }
    auto bare = [](const stdfs::path& path) {
        stdfs::path normal = path.lexically_normal();
        return normal.has_filename() ? normal : normal.parent_path();
    };
    return bare(recorded) == bare(root);
}

} // namespace

std::string_view toString(InstallKind kind)
{
    switch (kind) {
    case InstallKind::Installer:
        return "installer";
    case InstallKind::Archive:
        return "archive";
    case InstallKind::Development:
        return "development";
    }
    return "development";
}

stdfs::path installRecordDirectory(const stdfs::path& root)
{
    return root / "share" / "katana";
}

stdfs::path updateWorkDirectory(const stdfs::path& root)
{
    return root / ".update";
}

namespace detail {

// readInstall (install.hpp) for a program of version `program`: all of it, with the version of
// the program that asks handed in instead of taken from this build. Where the package does not
// name its own version (packageBuild above) the installed version is the higher of the helper's
// record and the program's own, so what readInstall answers there depends on the version of the
// binary it is linked into. This is the same reading without that: the rule can be shown for a
// program below, at and above a record without a build of each, and a record that names a fixed
// version can be read by a build of any version.
// In no public header (tests/update/test_state.cpp declares it); readInstall below is its one
// caller in the library.
core::Result<Install> readInstallAs(const stdfs::path& root, const RegistryOps& registry,
                                    const Version& program)
{
    if (root.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument, "no install folder was given");
    }
    Install install;
    install.root = root;
    const stdfs::path records = installRecordDirectory(root);

    const stdfs::path installFile = records / kInstallRecord;
    const core::Result<std::optional<std::string>> installText =
        readOptionalText(installFile, kMaxRecordBytes);
    if (!installText) {
        return installText.error();
    }
    if (!installText->has_value()) {
        // A development tree: nothing is updated here. What was not installed by the helper
        // counts as the version it was built as.
        install.version = program;
        return install;
    }

    // ---- install.json: where this copy came from -------------------------------------------
    const core::Result<Json> record = parseRecord(**installText, installFile, 1);
    if (!record) {
        return record.error();
    }
    const core::Result<std::string> target = stringMember(*record, "target", true, installFile);
    if (!target) {
        return target.error();
    }
    if (!validTarget(*target)) {
        return parseFailure("\"target\" is not a platform token (lower-case letters, digits, "
                            "'-' and '_')",
                            installFile);
    }
    install.target = *target;

    // THE COPY'S OWN BUILD, apart from anything an update recorded here: what the package says
    // of itself. A package that says nothing (one made before install.json carried it) is
    // stood for by the program that asks - the version it was built as, and no sequence.
    //
    // Where the package does say, the asking program is NOT asked as well, and that is on
    // purpose. The program that reads an install need not be one of its programs: the helper
    // runs from a copy in the per-user folder, and the helper that installs a release may be
    // that release's own (stage.hpp, preflight: a manifest whose `applier` is above what the
    // installed helper speaks). Its version is then the version it is about to install, and
    // taken for the install's it would make every such release "not newer" - a `downgrade`
    // refused on a copy that never updated, and on one whose record carries a later sequence
    // than the release's (a later build of an older line).
    const core::Result<std::optional<RecordedBuild>> package = packageBuild(*record, installFile);
    if (!package) {
        return package.error();
    }
    RecordedBuild own;
    if (package->has_value()) {
        own = **package;
    } else {
        own.version = program;
    }

    // The registry decides installer or archive on Windows; elsewhere it has nothing, and a
    // registry that cannot be read is no reason to refuse to update: the difference is only
    // whether Settings > Apps shows the new version.
    const core::Result<std::optional<stdfs::path>> installed = registry.installerLocation();
    install.kind = installed && installed->has_value() && sameFolder(**installed, root)
                       ? InstallKind::Installer
                       : InstallKind::Archive;

    // ---- installed.json: the build the helper last put here --------------------------------
    const stdfs::path installedFile = records / kInstalledRecord;
    const core::Result<std::optional<std::string>> installedText =
        readOptionalText(installedFile, kMaxRecordBytes);
    if (!installedText) {
        return installedText.error();
    }
    if (installedText->has_value()) {
        const core::Result<Json> installedRecord = parseRecord(**installedText, installedFile, 1);
        if (!installedRecord) {
            return installedRecord.error();
        }
        const core::Result<Version> version = versionMember(*installedRecord, installedFile);
        if (!version) {
            return version.error();
        }
        const core::Result<std::uint64_t> sequence =
            sequenceMember(*installedRecord, installedFile);
        if (!sequence) {
            return sequence.error();
        }
        install.version = *version;
        install.sequence = *sequence;
        install.hasInstalledRecord = true;
        // THE FLOOR AGAINST GOING BACK is the higher of the build the helper recorded and the
        // copy's own (`own`, above), as isNewerThan (trust.hpp) orders builds: by version, and
        // within one version by sequence. The record is the helper's, written when an update
        // commits, and nothing else keeps it true: a newer zip unpacked by hand over a copy
        // that had updated itself leaves it saying the older build, and read alone it would
        // let an earlier signed release pass for a newer one and be installed over the
        // programs that are there. The zip brings its own install.json, and with it its floor.
        //
        // Both halves of the build are raised where the copy's own has both. A version alone
        // would leave the record's stale sequence under it, and a manifest of that same
        // version is judged by the sequences: the first of two builds of one version would be
        // "newer" than the second build unpacked over a copy of an older version.
        //
        // That is what remains where the copy's own build has no sequence - a package that
        // names its version alone, or none, so that the asking program stands for it: only
        // the version is raised, the sequence stays the record's, and such a copy is offered
        // the release of its own version once more, which changes no file where the files are
        // that release's already and writes the records that were stale. `hasInstalledRecord`
        // says that the helper's record is there, whichever build is the higher.
        const std::strong_ordering order = own.version <=> install.version;
        if (order == std::strong_ordering::greater) {
            install.version = own.version;
            if (own.sequence.has_value()) {
                install.sequence = *own.sequence;
            }
        } else if (order == std::strong_ordering::equal && own.sequence.has_value() &&
                   *own.sequence > install.sequence) {
            install.sequence = *own.sequence;
        }
    } else {
        // A fresh install: the copy's own version. The sequence stays 0 and there is no record,
        // so a release of this same version is not an update (isNewerThan), whatever sequence
        // the package names.
        install.version = own.version;
    }

    install.hasInstalledManifest =
        isRegularFile(records / kInstalledManifest) && isRegularFile(records / kInstalledSignature);
    return install;
}

} // namespace detail

core::Result<Install> readInstall(const stdfs::path& root, const RegistryOps& registry)
{
    // The program's own version is this build's (version.hpp). One that does not read is a
    // broken build, and is said before any file of the install is looked at.
    const core::Result<Version> program = programVersion();
    if (!program) {
        return program.error();
    }
    return detail::readInstallAs(root, registry, *program);
}

core::Result<stdfs::path> installRootOfRunningProgram()
{
    stdfs::path executable;
#if defined(_WIN32)
    std::wstring buffer(260, L'\0');
    for (;;) {
        const DWORD length =
            GetModuleFileNameW(nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
        if (length == 0) {
            return core::makeError(core::ErrorCode::NotFound,
                                   "the program's own path cannot be found",
                                   "os_error=" + std::to_string(GetLastError()));
        }
        if (length < buffer.size()) { // fits, with its terminating NUL
            buffer.resize(length);
            break;
        }
        if (buffer.size() >= 32768) { // the longest path Windows has
            return core::makeError(core::ErrorCode::NotFound, "the program's own path is too long");
        }
        buffer.resize(buffer.size() * 2);
    }
    executable = stdfs::path(buffer);
#elif defined(__APPLE__)
    std::uint32_t size = 0;
    _NSGetExecutablePath(nullptr, &size); // fails, and sets the length needed
    std::string buffer(size, '\0');
    if (_NSGetExecutablePath(buffer.data(), &size) != 0) {
        return core::makeError(core::ErrorCode::NotFound, "the program's own path cannot be found");
    }
    buffer.resize(std::char_traits<char>::length(buffer.c_str()));
    std::error_code error;
    executable = stdfs::canonical(core::pathFromUtf8(buffer), error);
    if (error) {
        return core::makeError(core::ErrorCode::NotFound,
                               "the program's own path cannot be resolved", error.message());
    }
#else
    std::error_code error;
    executable = stdfs::read_symlink("/proc/self/exe", error);
    if (error) {
        return core::makeError(core::ErrorCode::NotFound,
                               "the program's own path cannot be found (/proc/self/exe)",
                               error.message());
    }
#endif
    return installRootOfExecutable(executable);
}

// ---- the state folder ---------------------------------------------------------------------

stdfs::path stateDirectory()
{
    const std::string chosen = core::environmentVariable("KATANA_UPDATE_STATE_DIR");
    if (!chosen.empty()) {
        // Absolute now, so that a later change of the working folder cannot move it.
        const stdfs::path given = core::pathFromUtf8(chosen);
        std::error_code error;
        const stdfs::path absolute = stdfs::absolute(given, error);
        return error ? given : absolute;
    }
#ifdef _WIN32
    const std::string localAppData = core::environmentVariable("LOCALAPPDATA");
    if (!localAppData.empty()) {
        return core::pathFromUtf8(localAppData) / "Katana" / "update";
    }
    const std::string profile = core::environmentVariable("USERPROFILE");
    if (profile.empty()) {
        return {};
    }
    return core::pathFromUtf8(profile) / "AppData" / "Local" / "Katana" / "update";
#else
    // The XDG base directory specification: a value that is not an absolute path is invalid
    // and must be ignored.
    const std::string xdg = core::environmentVariable("XDG_STATE_HOME");
    if (!xdg.empty()) {
        const stdfs::path base = core::pathFromUtf8(xdg);
        if (base.is_absolute()) {
            return base / "katana" / "update";
        }
    }
    const std::string home = core::environmentVariable("HOME");
    if (home.empty()) {
        return {};
    }
    return core::pathFromUtf8(home) / ".local" / "state" / "katana" / "update";
#endif
}

// The three names below take their identifiers from the caller: `transaction` has been
// checked with isValidTransactionId where it came from a command line or a folder name
// (apply.hpp), and a sequence is a number.
stdfs::path stagePath(const stdfs::path& stateDir, std::uint64_t sequence)
{
    return stateDir / ("stage-" + std::to_string(sequence));
}

stdfs::path runPath(const stdfs::path& stateDir, const std::string& transaction)
{
    return stateDir / ("run-" + transaction);
}

stdfs::path healthMarkerPath(const stdfs::path& stateDir, const std::string& transaction)
{
    return stateDir / ("healthy-" + transaction);
}

// ---- state.json ---------------------------------------------------------------------------

core::Result<UpdateState> readState(const stdfs::path& stateDir)
{
    if (const core::Status usable = requireStateDirectory(stateDir); !usable) {
        return usable.error();
    }
    const stdfs::path file = stateDir / "state.json";
    const core::Result<std::optional<std::string>> text = readOptionalText(file, kMaxStateBytes);
    if (!text) {
        return text.error();
    }
    UpdateState state;
    if (!text->has_value()) {
        return state;
    }
    const core::Result<Json> json = parseRecord(**text, file, 1);
    if (!json) {
        return json.error();
    }

    if (const auto member = json->find("last_check"); member != json->end()) {
        const bool fits = member->is_number_integer() &&
                          (!member->is_number_unsigned() ||
                           member->get<std::uint64_t>() <= static_cast<std::uint64_t>(INT64_MAX));
        if (!fits) {
            return parseFailure("\"last_check\" is not a whole number of seconds", file);
        }
        state.lastCheckEpoch = member->get<std::int64_t>();
    }
    if (const auto member = json->find("auto"); member != json->end()) {
        if (!member->is_boolean()) {
            return parseFailure("\"auto\" is not true or false", file);
        }
        state.automatic = member->get<bool>();
    }
    const core::Result<std::string> skipped = stringMember(*json, "skipped", false, file);
    if (!skipped) {
        return skipped.error();
    }
    state.skippedVersion = *skipped;
    if (const auto member = json->find("blocked"); member != json->end()) {
        if (!member->is_array()) {
            return parseFailure("\"blocked\" is not a list", file);
        }
        for (const Json& entry : *member) {
            if (!entry.is_number_unsigned()) {
                return parseFailure("\"blocked\" holds something that is not a release sequence",
                                    file);
            }
            state.blockedSequences.push_back(entry.get<std::uint64_t>());
        }
    }
    return state;
}

core::Status writeState(const stdfs::path& stateDir, const UpdateState& state)
{
    if (const core::Status usable = requireStateDirectory(stateDir); !usable) {
        return usable;
    }
    Json json = Json::object();
    json["schema"] = 1;
    json["last_check"] = state.lastCheckEpoch;
    json["auto"] = state.automatic;
    json["skipped"] = state.skippedVersion;
    json["blocked"] = state.blockedSequences;
    return writeRecord(stateDir, "state.json", dump(json));
}

// ---- last.json ----------------------------------------------------------------------------

core::Result<std::optional<LastResult>> readLastResult(const stdfs::path& stateDir)
{
    if (const core::Status usable = requireStateDirectory(stateDir); !usable) {
        return usable.error();
    }
    const stdfs::path file = stateDir / "last.json";
    const core::Result<std::optional<std::string>> text = readOptionalText(file, kMaxRecordBytes);
    if (!text) {
        return text.error();
    }
    if (!text->has_value()) {
        return std::optional<LastResult>{};
    }
    const core::Result<Json> json = parseRecord(**text, file, 1);
    if (!json) {
        return json.error();
    }
    LastResult result;
    struct Field {
        const char* name;
        bool required;
        std::string* into;
    };
    for (const Field& field :
         {Field{"status", true, &result.status}, Field{"reason", false, &result.reason},
          Field{"holder", false, &result.holder}, Field{"version", false, &result.version},
          Field{"tx", false, &result.transaction}}) {
        const core::Result<std::string> value =
            stringMember(*json, field.name, field.required, file);
        if (!value) {
            return value.error();
        }
        *field.into = *value;
    }
    if (result.status.empty()) {
        return parseFailure("\"status\" is empty", file);
    }
    return std::optional<LastResult>(std::move(result));
}

core::Status writeLastResult(const stdfs::path& stateDir, const LastResult& result)
{
    if (const core::Status usable = requireStateDirectory(stateDir); !usable) {
        return usable;
    }
    Json json = Json::object();
    json["schema"] = 1;
    json["status"] = result.status;
    json["reason"] = result.reason;
    json["holder"] = result.holder;
    json["version"] = result.version;
    json["tx"] = result.transaction;
    return writeRecord(stateDir, "last.json", dump(json));
}

core::Status clearLastResult(const stdfs::path& stateDir)
{
    if (const core::Status usable = requireStateDirectory(stateDir); !usable) {
        return usable;
    }
    std::error_code error;
    stdfs::remove(stateDir / "last.json", error); // false, no error: it was not there
    if (error) {
        return core::makeError(core::ErrorCode::FileExportFailure, "last.json cannot be removed",
                               error.message() + "; " + core::pathToUtf8(stateDir / "last.json"));
    }
    return {};
}

// ---- U/cleanup.list -----------------------------------------------------------------------

core::Status appendCleanup(const stdfs::path& stateDir, const stdfs::path& leftover, FsOps& fs)
{
    if (const core::Status usable = requireStateDirectory(stateDir); !usable) {
        return usable;
    }
    const std::string text = core::pathToUtf8(leftover);
    // One path to a line: a line break inside a name would turn one leftover into two
    // entries, the second of them anywhere. (A relative path is refused for the same
    // reason it is dropped when read: it means something else in another working folder.)
    if (!leftover.is_absolute() || text.find_first_of("\r\n") != std::string::npos) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "a leftover is listed by its absolute path, on one line", text);
    }
    if (const core::Status made = fs.makeDirectories(stateDir); !made) {
        return made;
    }
    return fs.appendDurable(stateDir / kCleanupListName, text + "\n");
}

namespace {

#ifdef _WIN32
wchar_t foldAscii(wchar_t c)
{
    return c < 0x80 ? static_cast<wchar_t>(core::asciiLower(static_cast<char>(c))) : c;
}
#endif

// Two names or two whole paths (bare()), compared as the file system compares them. Windows
// folds case; an ASCII fold is what is done here, and a name whose case differs in a letter
// beyond ASCII is read as another name, which only ever makes the check refuse more.
bool sameName(const stdfs::path& a, const stdfs::path& b)
{
#ifdef _WIN32
    const auto& left = a.native();
    const auto& right = b.native();
    return left.size() == right.size() &&
           std::equal(left.begin(), left.end(), right.begin(),
                      [](wchar_t x, wchar_t y) { return foldAscii(x) == foldAscii(y); });
#else
    return a == b;
#endif
}

// The path without a trailing separator, normalised.
stdfs::path bare(const stdfs::path& path)
{
    stdfs::path normal = path.lexically_normal();
    if (!normal.has_filename()) {
        normal = normal.parent_path();
    }
    return normal;
}

// How many leading names `base` has, when `child` has all of them and at least one more;
// std::nullopt otherwise. Both are bare().
std::optional<std::size_t> namesBelow(const stdfs::path& child, const stdfs::path& base)
{
    if (base.empty()) {
        return std::nullopt; // no place at all is not every place
    }
    auto c = child.begin();
    std::size_t shared = 0;
    for (auto b = base.begin(); b != base.end(); ++b, ++c, ++shared) {
        if (c == child.end() || !sameName(*c, *b)) {
            return std::nullopt;
        }
    }
    if (c == child.end()) {
        return std::nullopt; // equal to the base is not below it
    }
    return shared;
}

#ifdef _WIN32
// Whether a name below the first `from` of `path` is one that Windows does not read as it is
// spelled: it drops the dots and spaces at the end of a name, so "..." is the folder it is in
// and ".. " may be the one above. Such a line would pass the check that it lies under the base
// and then act on the base itself, or on what is above it. The updater never makes such a
// name, so the line is not one it wrote.
bool namesSomethingElseThanItSays(const stdfs::path& path, std::size_t from)
{
    std::size_t index = 0;
    for (const stdfs::path& part : path) {
        if (index++ < from) {
            continue;
        }
        const std::wstring& name = part.native();
        if (!name.empty() && (name.back() == L'.' || name.back() == L' ')) {
            return true;
        }
    }
    return false;
}
#endif

core::Status removeTree(FsOps& fs, const stdfs::path& path, int depth)
{
    const core::Result<EntryInfo> info = fs.stat(path);
    if (!info) {
        return info.error();
    }
    if (info->kind == EntryKind::Missing) {
        return {};
    }
    if (info->reparsePoint) {
        // The link itself goes; what it points at is not touched, and never walked.
        return info->kind == EntryKind::Directory ? fs.removeEmptyDirectory(path)
                                                  : fs.removeFile(path);
    }
    if (info->kind != EntryKind::Directory) {
        return fs.removeFile(path);
    }
    if (depth >= kMaxCleanupDepth) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "a folder nested too deeply to clean up", core::pathToUtf8(path));
    }
    const core::Result<std::vector<stdfs::path>> children = fs.listDirectory(path);
    if (!children) {
        return children.error();
    }
    // Everything that can go goes; the first refusal is the one reported.
    std::optional<core::Error> first;
    for (const stdfs::path& child : *children) {
        if (const core::Status removed = removeTree(fs, child, depth + 1); !removed && !first) {
            first = removed.error();
        }
    }
    if (first) {
        return *first;
    }
    return fs.removeEmptyDirectory(path);
}

struct CleanupBase {
    stdfs::path lexical;                // as the caller spelled it
    std::optional<stdfs::path> resolved; // with every link resolved; none when that failed
};

enum class LineOutcome { Done, Retry };

// One line of the list. Done: removed, or nothing to remove, or not a place this list may
// name (never touched). Retry: it is ours and could not be removed (a locked file), or it is
// under a place that could not be examined just now: keep it.
LineOutcome cleanOne(FsOps& fs, const std::string& line, const std::vector<CleanupBase>& bases,
                     const std::vector<stdfs::path>& listFiles)
{
    const stdfs::path given = core::pathFromUtf8(line);
    if (!given.is_absolute()) {
        return LineOutcome::Done;
    }
    const stdfs::path path = bare(given);
    for (const stdfs::path& listFile : listFiles) {
        if (sameName(path, bare(listFile))) {
            return LineOutcome::Done; // the list does not delete itself
        }
    }

    for (const CleanupBase& base : bases) {
        const std::optional<std::size_t> shared = namesBelow(path, base.lexical);
        if (!shared) {
            continue;
        }
#ifdef _WIN32
        if (namesSomethingElseThanItSays(path, *shared)) {
            return LineOutcome::Done; // not a line this program wrote: never touched
        }
#endif
        if (!base.resolved) {
            // A place that could not be resolved (access denied, a sharing violation: a
            // missing place resolves fine) is not a place without leftovers. The line is by
            // its text one of ours, so it is kept for the next start, not dropped.
            return LineOutcome::Retry;
        }
        // Each folder between the base and the path, itself excluded, must be a real folder:
        // through a link the path would name something that is not under the base at all.
        stdfs::path walk;
        auto name = path.begin();
        for (std::size_t i = 0; i < *shared; ++i, ++name) {
            walk /= *name;
        }
        for (; std::next(name) != path.end(); ++name) {
            walk /= *name;
            const core::Result<EntryInfo> info = fs.stat(walk);
            if (!info) {
                return LineOutcome::Retry;
            }
            if (info->kind == EntryKind::Missing) {
                return LineOutcome::Done; // nothing there to remove
            }
            if (info->reparsePoint) {
                return LineOutcome::Done;
            }
        }
        // And the folder it sits in, with links resolved, must be the base or below it.
        const core::Result<stdfs::path> parent = fs.resolve(path.parent_path());
        if (!parent) {
            return LineOutcome::Retry;
        }
        if (!sameName(*parent, *base.resolved) && !namesBelow(bare(*parent), *base.resolved)) {
            return LineOutcome::Done;
        }
        const core::Status removed = removeTree(fs, path, 0);
        return removed ? LineOutcome::Done : LineOutcome::Retry;
    }
    return LineOutcome::Done; // not under any place the list may name
}

// The list a run is working on: cleanup.list renamed. The list is never read and rewritten in
// place, because the helper appends to it (appendCleanup) while the window may be processing
// it, and an append between the read and the rewrite would be lost with the old text. A rename
// is atomic and moves the file away from the appenders: from then on they start a new list,
// and what this run keeps is appended to that one.
constexpr std::string_view kCleanupWorkName = "cleanup.list.work";

// Acts on the lines of `work` and removes it, first appending the lines that must stay to
// `list`. Nothing is dropped unless it was acted on: a crash, a full disk or a refusal anywhere
// leaves `work` in place for the next start, and a line in both is acted on twice, which is
// harmless.
core::Status cleanTaken(const stdfs::path& stateDir, const stdfs::path& root, FsOps& fs,
                        const stdfs::path& list, const stdfs::path& work)
{
    const core::Result<EntryInfo> info = fs.stat(work);
    if (!info) {
        return info.error();
    }
    if (info->kind == EntryKind::Missing) {
        return {}; // another process finished it
    }
    if (info->kind != EntryKind::File) {
        return core::makeError(core::ErrorCode::InvalidState,
                               std::string(kCleanupWorkName) + " is not a file",
                               core::pathToUtf8(work));
    }
    if (info->size > kMaxCleanupListBytes) {
        // Not a list this program wrote. Dropped whole rather than walked line by line.
        if (const core::Status removed = fs.removeFile(work); !removed) {
            return removed;
        }
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "cleanup.list was too long to be one this program wrote; it was "
                               "discarded",
                               core::pathToUtf8(list));
    }
    const core::Result<std::string> text = fs.readFile(work);
    if (!text) {
        return text.error();
    }

    // The places a line may name: U, and R/.update. A place that cannot be resolved (it is not
    // "missing": that resolves; it is an error from the system, which may pass) is kept in the
    // list, and every line under it with it.
    std::vector<CleanupBase> bases;
    std::vector<stdfs::path> candidates{stateDir};
    if (!root.empty()) {
        candidates.push_back(updateWorkDirectory(root));
    }
    for (const stdfs::path& candidate : candidates) {
        const core::Result<stdfs::path> resolved = fs.resolve(candidate);
        bases.push_back({bare(candidate), resolved ? std::optional<stdfs::path>(bare(*resolved))
                                                   : std::nullopt});
    }
    const std::vector<stdfs::path> listFiles{list, work};

    std::vector<std::string> kept;
    std::size_t start = 0;
    while (start < text->size()) {
        std::size_t end = text->find('\n', start);
        if (end == std::string::npos) {
            end = text->size();
        }
        std::string line = text->substr(start, end - start);
        start = end + 1;
        if (!line.empty() && line.back() == '\r') {
            line.pop_back();
        }
        if (line.empty() || std::find(kept.begin(), kept.end(), line) != kept.end()) {
            continue;
        }
        if (cleanOne(fs, line, bases, listFiles) == LineOutcome::Retry) {
            kept.push_back(std::move(line));
        }
    }

    if (!kept.empty()) {
        std::string again;
        for (const std::string& line : kept) {
            again += line;
            again += '\n';
        }
        if (const core::Status appended = fs.appendDurable(list, again); !appended) {
            return appended; // `work` stays: the lines are not lost
        }
    }
    return fs.removeFile(work);
}

} // namespace

core::Status processCleanupList(const stdfs::path& stateDir, const stdfs::path& root, FsOps& fs)
{
    if (const core::Status usable = requireStateDirectory(stateDir); !usable) {
        return usable;
    }
    const stdfs::path list = stateDir / kCleanupListName;
    const stdfs::path work = stateDir / kCleanupWorkName;

    // A run that died after it took the list left it as `work`: that one is finished first, and
    // the list (which may hold what an appender added since) waits for the next start.
    const core::Result<EntryInfo> leftWork = fs.stat(work);
    if (!leftWork) {
        return leftWork.error();
    }
    if (leftWork->kind != EntryKind::Missing) {
        return cleanTaken(stateDir, root, fs, list, work);
    }

    const core::Result<EntryInfo> info = fs.stat(list);
    if (!info) {
        return info.error();
    }
    if (info->kind == EntryKind::Missing) {
        return {};
    }
    if (info->kind != EntryKind::File) {
        return core::makeError(core::ErrorCode::InvalidState, "cleanup.list is not a file",
                               core::pathToUtf8(list));
    }
    if (const core::Status taken = fs.renameFile(list, work); !taken) {
        return taken;
    }
    return cleanTaken(stateDir, root, fs, list, work);
}

} // namespace katana::update
