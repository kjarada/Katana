// The rules a manifest file name must pass (include/katana/update/paths.hpp). One
// implementation, used by the engine when a manifest is verified and by the helper before it
// touches the install; tools/update_make_manifest.py and tools/update_verify_release.py
// hold the same rules in Python, and tests/update/test_paths.cpp holds all three to the table
// tests/update/fixtures/path_rules.txt.

#include "katana/update/paths.hpp"

#include <algorithm>
#include <cstddef>
#include <string_view>
#include <unordered_set>
#include <utility>
#include <vector>

namespace katana::update {

namespace {

// ASCII only, on purpose (paths.hpp): std::tolower is locale-dependent and std::isalpha
// would read a byte above 0x7F as a letter in some locales.
char lowerAscii(char c)
{
    return (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
}

std::string lowerAscii(std::string_view text)
{
    std::string out(text);
    std::transform(out.begin(), out.end(), out.begin(),
                   [](char c) { return lowerAscii(c); });
    return out;
}

bool equalsIgnoringAsciiCase(std::string_view a, std::string_view b)
{
    return a.size() == b.size() &&
           std::equal(a.begin(), a.end(), b.begin(),
                      [](char x, char y) { return lowerAscii(x) == lowerAscii(y); });
}

// A path as it appears in an error: bounded (a manifest can be megabytes) and with every
// byte that is not printable ASCII spelled \xNN, so a NUL, a newline or a byte of a UTF-8
// sequence cannot reshape a log line. An ordinary path is shown as it is.
std::string excerpt(std::string_view path)
{
    constexpr std::size_t kShown = 200;
    constexpr std::string_view kHex = "0123456789abcdef";
    std::string out;
    const std::size_t shown = std::min(path.size(), kShown);
    for (std::size_t i = 0; i < shown; ++i) {
        const auto c = static_cast<unsigned char>(path[i]);
        if (c >= 0x20 && c <= 0x7E) {
            out.push_back(static_cast<char>(c));
        } else {
            out += "\\x";
            out.push_back(kHex[c >> 4]);
            out.push_back(kHex[c & 0x0F]);
        }
    }
    if (path.size() > shown) {
        out += "...";
    }
    return out;
}

core::Error refuse(std::string rule, std::string_view path)
{
    return core::makeError(core::ErrorCode::InvalidArgument, std::move(rule), excerpt(path));
}

// "con", "prn", "aux", "nul", "com1".."com9", "lpt1".."lpt9" in any ASCII case. COM0 and
// LPT0 are NOT devices here: they are ordinary file names on the Windows 11 the producer's
// author tested, and tests/update/fixtures/path_rules.txt has no line that says otherwise.
bool isDeviceStem(std::string_view stem)
{
    if (stem.size() == 3) {
        for (const std::string_view name : {"con", "prn", "aux", "nul"}) {
            if (equalsIgnoringAsciiCase(stem, name)) {
                return true;
            }
        }
        return false;
    }
    if (stem.size() == 4 && stem[3] >= '1' && stem[3] <= '9') {
        return equalsIgnoringAsciiCase(stem.substr(0, 3), "com") ||
               equalsIgnoringAsciiCase(stem.substr(0, 3), "lpt");
    }
    return false;
}

// '~' followed by a digit, anywhere in the name: "PROGRA~1.DLL", "KATANA~12".
bool hasShortNameMarker(std::string_view name)
{
    for (std::size_t i = 0; i + 1 < name.size(); ++i) {
        if (name[i] == '~' && name[i + 1] >= '0' && name[i + 1] <= '9') {
            return true;
        }
    }
    return false;
}

constexpr std::array<std::string_view, 4> kReservedFileNames{
    "install.json", "installed.json", "installed.manifest.json", "installed.manifest.json.sig"};
constexpr std::string_view kWorkFolderName = ".update";

// Splits on '/' and on '\\' both: this is the "never removed" test as well as the manifest
// rule, and a backslash can only make a name reserved here, never less so.
template <typename Visitor> void forEachComponent(std::string_view path, Visitor&& visit)
{
    std::size_t start = 0;
    while (true) {
        const std::size_t end = path.find_first_of("/\\", start);
        const bool last = end == std::string_view::npos;
        visit(path.substr(start, last ? std::string_view::npos : end - start), last);
        if (last) {
            return;
        }
        start = end + 1;
    }
}

} // namespace

bool isReservedName(std::string_view path)
{
    bool reserved = false;
    forEachComponent(path, [&](std::string_view component, bool last) {
        if (equalsIgnoringAsciiCase(component, kWorkFolderName)) {
            reserved = true;
        }
        if (last) {
            for (const std::string_view name : kReservedFileNames) {
                if (equalsIgnoringAsciiCase(component, name)) {
                    reserved = true;
                }
            }
        }
    });
    return reserved;
}

core::Status checkManifestPath(std::string_view path)
{
    if (path.empty()) {
        return refuse("the path is empty", path);
    }

    // One byte at a time, before anything is split: a name with a byte outside 0x20 to 0x7E
    // (a non-ASCII letter, a control character, DEL, a NUL) never reaches the component
    // rules, whose comparisons assume ASCII.
    for (const char c : path) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte < 0x20 || byte > 0x7E) {
            return refuse("the path has a byte outside printable ASCII (0x20 to 0x7E): names "
                          "are ASCII only, with no control character or DEL",
                          path);
        }
    }
    constexpr std::string_view kForbidden = "\\:*?\"<>|";
    if (path.find_first_of(kForbidden) != std::string_view::npos) {
        return refuse("the path has a backslash, a colon (a drive or a stream) or one of "
                      "* ? \" < > |",
                      path);
    }
    if (path.front() == '/') {
        return refuse("the path is absolute", path);
    }

    std::vector<std::string_view> components;
    forEachComponent(path, [&](std::string_view component, bool) { components.push_back(component); });

    for (const std::string_view component : components) {
        if (component.empty()) {
            return refuse("the path has an empty component (a doubled or a trailing '/')", path);
        }
        if (component == "." || component == "..") {
            return refuse("the path has a '.' or '..' component", path);
        }
    }

    const bool rooted = std::find(kInstallRoots.begin(), kInstallRoots.end(), components.front()) !=
                        kInstallRoots.end();
    if (!rooted) {
        return refuse("the first component is not one of bin, share, etc, lib", path);
    }
    if (components.size() < 2) {
        return refuse("the path names an install folder, not a file below it", path);
    }

    for (const std::string_view component : components) {
        if (component.back() == '.' || component.back() == ' ') {
            return refuse("a component ends in '.' or ' ' (Windows drops it, so two names "
                          "become one)",
                          path);
        }
        // The device test is on what precedes the FIRST dot: "NUL.txt" and "com1.tar.gz"
        // open the device, not a file.
        if (isDeviceStem(component.substr(0, component.find('.')))) {
            return refuse("a component is a Windows device name (CON, PRN, AUX, NUL, "
                          "COM1-9, LPT1-9)",
                          path);
        }
        if (hasShortNameMarker(component)) {
            return refuse("a component is in 8.3 short form ('~' and a digit)", path);
        }
    }

    if (isReservedName(path)) {
        return refuse("the path names one of the updater's own records or its working "
                      "folder (install.json, installed.json, installed.manifest.json(.sig), "
                      ".update)",
                      path);
    }
    return {};
}

core::Status checkManifestPaths(std::span<const std::string> paths,
                                std::span<const std::uint64_t> sizes)
{
    if (paths.size() != sizes.size()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "checkManifestPaths: one size is needed for each path",
                               std::to_string(paths.size()) + " paths, " +
                                   std::to_string(sizes.size()) + " sizes");
    }
    // The count first: a hostile list is refused before any work is done per entry.
    if (paths.size() > kMaxFiles) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the manifest lists more than " + std::to_string(kMaxFiles) +
                                   " files",
                               std::to_string(paths.size()) + " files");
    }

    for (const std::string& path : paths) {
        if (core::Status status = checkManifestPath(path); !status) {
            return status;
        }
    }

    // A running total that is compared before it is added to: no sum of sizes can wrap.
    std::uint64_t total = 0;
    for (std::size_t i = 0; i < sizes.size(); ++i) {
        if (sizes[i] > kMaxInstallBytes - total) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the files add up to more than " +
                                       std::to_string(kMaxInstallBytes) + " bytes (2 GiB)",
                                   excerpt(paths[i]));
        }
        total += sizes[i];
    }

    // Two names that a file system folding case would make one, and a name that is a file
    // as well as the folder of another (bin/a and bin/A/b): no volume can hold either pair,
    // so an install that applied the set would fail half way. Both after folding ASCII case.
    std::unordered_set<std::string> folded;
    folded.reserve(paths.size());
    // The folded names in byte order, each with its place in the list: the set above owns
    // the strings and its nodes never move, so the views stay good while it is not changed.
    std::vector<std::pair<std::string_view, std::size_t>> ordered;
    ordered.reserve(paths.size());
    for (std::size_t i = 0; i < paths.size(); ++i) {
        const auto inserted = folded.insert(lowerAscii(paths[i]));
        if (!inserted.second) {
            return refuse("two paths are the same name once ASCII case is folded", paths[i]);
        }
        ordered.emplace_back(std::string_view(*inserted.first), i);
    }
    std::sort(ordered.begin(), ordered.end());

    // "Q is a folder of another path" is "some listed name starts with Q and a slash", and the
    // names that do are one run of the sorted list, which begins at the first name not below
    // "Q/". One binary search for each name, so the cost is the sum of the lengths times a
    // logarithm: it must not depend on how many slashes a name has (a signed manifest may hold
    // one path of megabytes, "bin/a/a/a/...", and a probe of every prefix of it is quadratic).
    for (const std::string& path : paths) {
        std::string folder = lowerAscii(path);
        folder.push_back('/');
        const auto below = std::lower_bound(
            ordered.begin(), ordered.end(), folder,
            [](const std::pair<std::string_view, std::size_t>& entry, const std::string& key) {
                return entry.first < key;
            });
        if (below != ordered.end() && below->first.starts_with(folder)) {
            return refuse("a path is inside another path that is listed as a file (case "
                          "folded)",
                          paths[below->second]);
        }
    }
    return {};
}

core::Status checkJournalDirectory(std::string_view path)
{
    // A release that first brings etc or lib makes that folder itself, and rollback removes
    // it again, so the journal names it bare. Only for a folder: a bare name in an ADD,
    // REPLACE or REMOVE record would let a forged line delete a whole install folder, and
    // checkJournalPath keeps refusing it.
    if (std::find(kInstallRoots.begin(), kInstallRoots.end(), path) != kInstallRoots.end()) {
        return {};
    }
    // The records are files; a folder spelled as one is as forged as any other.
    return checkManifestPath(path);
}

core::Status checkJournalPath(std::string_view path)
{
    // The helper's own three records: exactly as it writes them. The journal is not
    // authenticated (journal.hpp), and on a case-sensitive file system "Share/Katana/..." is
    // another place, outside bin, share, etc and lib, so a spelling in another case is not
    // accepted here even though checkManifestPath folds case for the reserved names.
    for (const std::string_view record : kRecordPaths) {
        if (path == record) {
            return {};
        }
    }
    return checkManifestPath(path);
}

} // namespace katana::update
