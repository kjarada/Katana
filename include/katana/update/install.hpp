#pragma once

// What an install is, as the update engine and the helper see it, and the
// files that say so. Everything is under the install root R, the directory
// that holds bin/ and share/ (the folder of the unpacked archive, or the
// per-user installer's folder):
//
//   R/share/katana/install.json              {"schema":1,"target":"win64"}
//        Written ONLY by the CPack install (cmake/KatanaPackaging.cmake, the
//        pre-build script KatanaInstallRecord.cmake) - never by `cmake
//        --install`, the `bundle` target, or the build tree. Without it the
//        tree is a DEVELOPMENT tree: APPLY is refused and no automatic check
//        runs (tools/check_install_rules.cmake asserts this). The target comes
//        from here, never from the operating system: Windows on ARM running
//        the x64 zip stays win64.
//   R/share/katana/installed.json            written by the helper at commit:
//        {"schema":1,"product":"katana","version":"0.3.0","sequence":N,"tag":"v0.3.0",
//         "target":"win64"}
//        Absent on a fresh install: the installed version is then programVersion(),
//        the sequence is 0 and `hasInstalledRecord` is false. A fresh install is
//        NOT offered the release it came from: isNewerThan (trust.hpp) ignores the
//        sequence while there is no record, because the zip carries no sequence to
//        compare with the manifest's (one is written when the release is built, the
//        other when its manifest is). Only a record the helper wrote says "this is
//        the build with sequence N".
//   R/share/katana/installed.manifest.json   the signed manifest of the
//   R/share/katana/installed.manifest.json.sig   installed version, kept so
//        that a later update removes only files that version put there.
//        Absent on a fresh install: a first update deletes nothing.
//   R/.update/                               lock, one folder per transaction
//        (journal, new/, old/): the helper's working place, inside R so that
//        every rename stays on one volume.
//
// The four names install.json, installed.json, installed.manifest.json and
// installed.manifest.json.sig, and the folder .update, are reserved (paths.hpp): a
// manifest can never carry them. They are the literal names the helper writes, not a
// pattern: installed.json.bak is a file like any other.
//
// WHERE R IS, by platform:
//   Windows, Linux   R is the folder that holds bin/ and share/: the programs are
//                    in R/bin, the records in R/share/katana.
//   macOS            the bundle is Katana.app; its programs are in Contents/MacOS and
//                    everything that is not code - including share/katana/install.json,
//                    see KATANA_INSTALL_DATADIR in cmake/KatanaPackaging.cmake - in
//                    Contents/Resources. R is Katana.app/Contents/Resources: it holds
//                    share/ but not bin/. That is enough, because the macOS manifest is
//                    notify-only (manifest.hpp): R is read for install.json, which says
//                    what to ask the feed for, and nothing is ever applied there.
// installRootOfExecutable below is the one function that knows both layouts.

#include <cstdint>
#include <filesystem>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/update/registry.hpp"
#include "katana/update/version.hpp"

namespace katana::update {

enum class InstallKind {
    Installer,   // the per-user installer put it here (HKCU Software\Jarada\Katana equals R)
    Archive,     // an unpacked zip or tar.gz in a folder the user owns
    Development, // no install.json: a build tree, a plain `cmake --install`
};

[[nodiscard]] std::string_view toString(InstallKind kind); // "installer" "archive" "development"

struct Install {
    std::filesystem::path root;
    std::string target;          // "" for Development
    InstallKind kind = InstallKind::Development;
    std::string product = "katana";
    Version version;             // installed.json's, else programVersion()
    std::uint64_t sequence = 0;  // installed.json's, else 0
    // installed.json was present: `version` and `sequence` are what the helper recorded.
    // False on a fresh install, where `sequence` is a placeholder (see above and isNewerThan).
    bool hasInstalledRecord = false;
    bool hasInstalledManifest = false;
};

// The folder of R's own files and the names in it (see above).
[[nodiscard]] std::filesystem::path installRecordDirectory(const std::filesystem::path& root);
// R/.update
[[nodiscard]] std::filesystem::path updateWorkDirectory(const std::filesystem::path& root);

// Reads R's records. A missing install.json is not an error (Development); a
// malformed one is ParseFailure, an unknown schema Unsupported. `kind` asks
// the registry (RegistryOps) on Windows and is Archive elsewhere.
[[nodiscard]] core::Result<Install> readInstall(const std::filesystem::path& root,
                                                const RegistryOps& registry);

// The install root R for a program whose executable is at `executable` (an absolute
// path, as the operating system gives it). Looks at the disk, never writes:
//   - the executable's folder is named bin (any case) and the folder above it has a
//     directory share: R is the folder above (Windows, Linux, and a build tree, which
//     then has no install.json and so reads as Development);
//   - or the folder is Contents/MacOS of a bundle whose Contents/Resources has a
//     directory share: R is Contents/Resources (macOS, see above);
//   - otherwise NotFound.
// Matching of the names bin, MacOS, Contents and Resources ignores ASCII case, because
// a path typed or stored in another case still names the same folder on the file
// systems Windows and macOS use.
[[nodiscard]] core::Result<std::filesystem::path>
installRootOfExecutable(const std::filesystem::path& executable);

// installRootOfExecutable of the running program's own executable path
// (GetModuleFileNameW; /proc/self/exe; _NSGetExecutablePath on macOS).
[[nodiscard]] core::Result<std::filesystem::path> installRootOfRunningProgram();

} // namespace katana::update
