#pragma once

// What an install is, as the update engine and the helper see it, and the
// files that say so. Everything is under the install root R, the directory
// that holds bin/ and share/ (the folder of the unpacked archive, or the
// per-user installer's folder):
//
//   R/share/katana/install.json              what the package is:
//        {"schema":1,"target":"win64","version":"0.3.0","sequence":N}
//        Written ONLY by the CPack install (cmake/KatanaPackaging.cmake, the
//        pre-build script KatanaInstallRecord.cmake) - never by `cmake
//        --install`, the `bundle` target, or the build tree. Without it the
//        tree is a DEVELOPMENT tree: APPLY is refused and no automatic check
//        runs (tools/check_install_rules.cmake asserts this). The target comes
//        from here, never from the operating system: Windows on ARM running
//        the x64 zip stays win64.
//        `version` and `sequence` are OPTIONAL. They are THE COPY'S OWN BUILD,
//        apart from anything an update recorded here: the version the package was
//        built as, and the sequence of the release it belongs to - the number that
//        release's update manifests carry. A package built outside the release
//        workflow belongs to no release and names its version alone. A sequence
//        with no version to belong to does not read (ParseFailure). A record that
//        names neither - one made before the record carried them, and every test
//        tree that says its target and no more - leaves the program that asks to
//        stand for the copy: programVersion(), and no sequence.
//        Where the package names its version the asking program is NOT asked as
//        well. The program that reads an install need not be one of its programs:
//        the helper runs from a copy outside R and may be a later release's own
//        (stage.hpp, preflight), and its version is then the one it is about to
//        install, not the install's.
//   R/share/katana/installed.json            written by the helper at commit:
//        {"schema":1,"product":"katana","version":"0.3.0","sequence":N,"tag":"v0.3.0",
//         "target":"win64"}
//        Absent on a fresh install: the installed version is then the copy's own
//        (above), the sequence is 0 - whatever sequence the package names - and
//        `hasInstalledRecord` is false. A fresh install is NOT offered the release
//        it came from: isNewerThan (trust.hpp) ignores the sequence while there is
//        no record, and the copy counts as the release it was made from.
//        THE FLOOR AGAINST GOING BACK. Where the record is there it is not believed
//        below the copy's own build: the installed build is the HIGHER of the two,
//        in the order isNewerThan writes - by version, and within one version by
//        sequence. The record is the helper's, and nothing else keeps it true: a
//        newer zip unpacked by hand over a copy that had updated itself leaves it
//        naming the older build, and read alone it would let an earlier signed
//        release pass for a newer one and be installed over the programs that are
//        there. So where the copy's own version is the higher, it is the installed
//        version, and its sequence comes with it where it has one (a package of a
//        release); where the versions are equal, the higher sequence is the
//        installed one. A copy's own build with no sequence - a package that names
//        its version alone, or the asking program - raises the version alone and
//        the sequence stays the record's: such a copy is offered the release of its
//        own version once more, which changes no file that is that release's
//        already and writes the records that were stale.
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
    // The installed build: what a release must be above to be an update (isNewerThan).
    // `version` is the higher of installed.json's and the copy's own (install.json's, else
    // programVersion()); with no installed.json it is the copy's own, and in a development
    // tree programVersion().
    Version version;
    // installed.json's - or the PACKAGE's (install.json's), where the package's build is the
    // higher of the two and names one ("THE FLOOR" above). 0 with no installed.json, also
    // where the package names a sequence.
    std::uint64_t sequence = 0;
    // installed.json was present: `version` and `sequence` are what the helper recorded, each
    // raised to the copy's own build where that is the higher. It says that the helper's
    // record is there, whichever build is the higher. False on a fresh install, where
    // `sequence` is a placeholder (see above and isNewerThan).
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
