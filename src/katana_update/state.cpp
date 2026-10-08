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
#include <cstdint>
#include <fstream>
#include <functional>
#include <iterator>
#include <string>
#include <string_view>
#include <system_error>
#include <thread>
#include <vector>

#include <nlohmann/json.hpp>

#include "katana/core/path_text.hpp"
#include "katana/update/install.hpp"
#include "katana/update/state.hpp"

#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#elif defined(__APPLE__)
#include <mach-o/dyld.h>
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

// The text goes to a file of its own beside the record and is renamed over it: a reader sees
// the old record or the new one, never half of either, and a crash or a full disk leaves the
// old one in place. Not flushed to the disk before the rename: these records are advice
// (when to check next, what the last swap said), and one that a power cut turns into an
// unreadable file is read as absent and rebuilt - unlike the swap's own files, whose
// durability the FsOps seam provides (fsops.hpp).
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

    {
        std::ofstream out(temporary, std::ios::binary | std::ios::trunc);
        out.write(text.data(), static_cast<std::streamsize>(text.size()));
        out.flush();
        if (!out) {
            out.close();
            stdfs::remove(temporary, error);
            return core::makeError(core::ErrorCode::FileExportFailure,
                                   "the record cannot be written", core::pathToUtf8(temporary));
        }
    }

    // A reader that has the old record open can make the replacement fail for a moment on
    // Windows (a sharing violation); a few short tries cover it, and a record that is
    // read-only or locked for good fails all of them and is reported.
    constexpr int kTries = 5;
    for (int attempt = 1;; ++attempt) {
        stdfs::rename(temporary, target, error);
        if (!error) {
            return {};
        }
        if (attempt == kTries) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    std::error_code ignored;
    stdfs::remove(temporary, ignored);
    return core::makeError(core::ErrorCode::FileExportFailure, "the record cannot be replaced",
                           error.message() + "; " + core::pathToUtf8(target));
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

core::Result<Install> readInstall(const stdfs::path& root, const RegistryOps& registry)
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
    // What a build that was not installed by the helper counts as: the version it was built as.
    auto builtVersion = [&install]() -> core::Status {
        const core::Result<Version> program = programVersion();
        if (!program) {
            return program.error();
        }
        install.version = *program;
        return {};
    };
    if (!installText->has_value()) {
        // A development tree: nothing is updated here.
        if (const core::Status built = builtVersion(); !built) {
            return built.error();
        }
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
        const core::Result<std::string> versionText =
            stringMember(*installedRecord, "version", true, installedFile);
        if (!versionText) {
            return versionText.error();
        }
        const core::Result<Version> version = parseVersion(*versionText);
        if (!version) {
            return parseFailure("\"version\" is not a version: " + version.error().describe(),
                                installedFile);
        }
        const auto sequence = installedRecord->find("sequence");
        if (sequence == installedRecord->end() || !sequence->is_number_unsigned()) {
            return parseFailure("\"sequence\" is not a whole number", installedFile);
        }
        install.version = *version;
        install.sequence = sequence->get<std::uint64_t>();
        install.hasInstalledRecord = true;
    } else if (const core::Status built = builtVersion(); !built) {
        return built.error(); // a fresh install: the version it was built as, sequence 0
    }

    install.hasInstalledManifest =
        isRegularFile(records / kInstalledManifest) && isRegularFile(records / kInstalledSignature);
    return install;
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
    return (c >= L'A' && c <= L'Z') ? static_cast<wchar_t>(c + 32) : c;
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
    stdfs::path lexical;  // as the caller spelled it
    stdfs::path resolved; // with every link resolved
};

enum class LineOutcome { Done, Retry };

// One line of the list. Done: removed, or nothing to remove, or not a place this list may
// name (never touched). Retry: it is ours and could not be removed (a locked file): keep it.
LineOutcome cleanOne(FsOps& fs, const std::string& line, const std::vector<CleanupBase>& bases,
                     const stdfs::path& listFile)
{
    const stdfs::path given = core::pathFromUtf8(line);
    if (!given.is_absolute()) {
        return LineOutcome::Done;
    }
    const stdfs::path path = bare(given);
    if (sameName(path, bare(listFile))) {
        return LineOutcome::Done; // the list does not delete itself
    }

    for (const CleanupBase& base : bases) {
        const std::optional<std::size_t> shared = namesBelow(path, base.lexical);
        if (!shared) {
            continue;
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
        if (!sameName(*parent, base.resolved) && !namesBelow(bare(*parent), base.resolved)) {
            return LineOutcome::Done;
        }
        const core::Status removed = removeTree(fs, path, 0);
        return removed ? LineOutcome::Done : LineOutcome::Retry;
    }
    return LineOutcome::Done; // not under any place the list may name
}

} // namespace

core::Status processCleanupList(const stdfs::path& stateDir, const stdfs::path& root, FsOps& fs)
{
    const stdfs::path listFile = stateDir / kCleanupListName;
    const core::Result<EntryInfo> info = fs.stat(listFile);
    if (!info) {
        return info.error();
    }
    if (info->kind == EntryKind::Missing) {
        return {};
    }
    if (info->kind != EntryKind::File) {
        return core::makeError(core::ErrorCode::InvalidState, "cleanup.list is not a file",
                               core::pathToUtf8(listFile));
    }
    if (info->size > kMaxCleanupListBytes) {
        // Not a list this program wrote. Dropped whole rather than walked line by line.
        if (const core::Status removed = fs.removeFile(listFile); !removed) {
            return removed;
        }
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "cleanup.list was too long to be one this program wrote; it was "
                               "discarded",
                               core::pathToUtf8(listFile));
    }
    const core::Result<std::string> text = fs.readFile(listFile);
    if (!text) {
        return text.error();
    }

    // The places a line may name: U, and R/.update. A place that cannot be resolved at all
    // has nothing under it that could exist, so no line is taken to be under it.
    std::vector<CleanupBase> bases;
    std::vector<stdfs::path> candidates{stateDir};
    if (!root.empty()) {
        candidates.push_back(updateWorkDirectory(root));
    }
    for (const stdfs::path& candidate : candidates) {
        const core::Result<stdfs::path> resolved = fs.resolve(candidate);
        if (resolved) {
            bases.push_back({bare(candidate), bare(*resolved)});
        }
    }

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
        if (cleanOne(fs, line, bases, listFile) == LineOutcome::Retry) {
            kept.push_back(std::move(line));
        }
    }

    if (kept.empty()) {
        return fs.removeFile(listFile);
    }
    std::string rewritten;
    for (const std::string& line : kept) {
        rewritten += line;
        rewritten += '\n';
    }
    return fs.writeDurable(listFile, rewritten);
}

} // namespace katana::update
