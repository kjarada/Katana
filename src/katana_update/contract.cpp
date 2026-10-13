// FINISHED (not stubs): the small pure functions of the contract that the skeleton settles
// itself, because an independent review found each of them a defect in waiting if left to
// the task that would "just implement the header":
//
//   isNewerThan             a fresh install was offered the release it came from (trust.hpp)
//   installRootOfExecutable macOS could never find install.json (install.hpp)
//   checkIsDue              the 24 h + jitter rule had no home but Qt (state.hpp)
//   isValidTransactionId    a command line named a folder with nothing checking it (apply.hpp)
//
// Each has its tests in tests/update/test_contract.cpp, which no later task replaces. The
// tasks that own the headers (T5 trust, T6 install and state, T7 apply) keep these
// definitions and write the rest of their file around them.

#include <algorithm>
#include <cstddef>
#include <system_error>

#include "katana/core/path_text.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/install.hpp"
#include "katana/update/state.hpp"
#include "katana/update/trust.hpp"

namespace katana::update {

namespace {

char lowerAscii(char c)
{
    return (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
}

// True when the path's last component is `ascii` in any ASCII case. Compares the native
// string unit by unit, so no conversion from the wide Windows encoding can throw.
bool nameIs(const std::filesystem::path& path, std::string_view ascii)
{
    const std::filesystem::path name = path.filename();
    const auto& native = name.native();
    if (native.size() != ascii.size()) {
        return false;
    }
    for (std::size_t i = 0; i < ascii.size(); ++i) {
        const auto unit = static_cast<unsigned long>(native[i]);
        if (unit > 0x7F || lowerAscii(static_cast<char>(unit)) != lowerAscii(ascii[i])) {
            return false;
        }
    }
    return true;
}

bool isDirectory(const std::filesystem::path& path)
{
    std::error_code ec;
    return std::filesystem::is_directory(path, ec) && !ec;
}

} // namespace

bool isNewerThan(const Manifest& manifest, const Install& install)
{
    const std::strong_ordering order = manifest.version <=> install.version;
    if (order != std::strong_ordering::equal) {
        return order == std::strong_ordering::greater;
    }
    // The same version. Only a record the helper wrote says which build is installed; a
    // fresh install has none and counts as the release it was made from.
    if (!install.hasInstalledRecord) {
        return false;
    }
    return manifest.sequence > install.sequence;
}

core::Result<std::filesystem::path> installRootOfExecutable(const std::filesystem::path& executable)
{
    const std::filesystem::path directory = executable.parent_path();

    // Windows, Linux, a build tree: <R>/bin/<program>, with <R>/share beside bin.
    if (nameIs(directory, "bin")) {
        const std::filesystem::path root = directory.parent_path();
        if (!root.empty() && isDirectory(root / "share")) {
            return root;
        }
    }
    // macOS: Katana.app/Contents/MacOS/<program>, data in Contents/Resources/share.
    if (nameIs(directory, "MacOS")) {
        const std::filesystem::path contents = directory.parent_path();
        if (nameIs(contents, "Contents")) {
            const std::filesystem::path resources = contents / "Resources";
            if (isDirectory(resources / "share")) {
                return resources;
            }
        }
    }
    return core::makeError(core::ErrorCode::NotFound,
                           "this program is not inside an install (no bin/ beside share/, and "
                           "not in Contents/MacOS of an application bundle)",
                           core::pathToUtf8(executable));
}

bool checkIsDue(const UpdateState& state, std::int64_t nowEpoch, std::chrono::seconds jitter)
{
    if (!state.automatic) {
        return false;
    }
    if (state.lastCheckEpoch <= 0 || state.lastCheckEpoch > nowEpoch) {
        return true;
    }
    const std::chrono::seconds clamped =
        std::clamp(jitter, std::chrono::seconds{0}, kCheckJitterMax);
    const std::int64_t wait = (kCheckInterval + clamped).count();
    return nowEpoch - state.lastCheckEpoch >= wait;
}

bool isValidTransactionId(std::string_view text)
{
    if (text.empty() || text.size() > 64) {
        return false;
    }
    return std::all_of(text.begin(), text.end(), [](char c) {
        return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f');
    });
}

} // namespace katana::update
