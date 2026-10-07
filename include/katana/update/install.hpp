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
//        Absent on a fresh install: the installed pair is then
//        (programVersion(), sequence 0).
//   R/share/katana/installed.manifest.json   the signed manifest of the
//   R/share/katana/installed.manifest.json.sig   installed version, kept so
//        that a later update removes only files that version put there.
//        Absent on a fresh install: a first update deletes nothing.
//   R/.update/                               lock, one folder per transaction
//        (journal, new/, old/): the helper's working place, inside R so that
//        every rename stays on one volume.
//
// The names installed*.json, install.json and the folder .update are reserved
// (paths.hpp): a manifest can never carry them.

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

// The root of the running program: the parent of the directory that holds its
// executable, when that has bin/ beside share/. NotFound otherwise.
[[nodiscard]] core::Result<std::filesystem::path> installRootOfRunningProgram();

} // namespace katana::update
