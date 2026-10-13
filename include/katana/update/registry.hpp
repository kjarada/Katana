#pragma once

// The few registry values an update reads and writes, behind an interface so
// that apply() is driven in-process by tests with a fake and never touches
// the machine's registry. All of them are per-user (HKCU): the installer is
// per-user, so no update needs administrator rights.
//
//   installerLocation  HKCU\Software\Jarada\Katana, default value: the folder
//                      the per-user installer installed into. An install is of
//                      kind Installer when it equals the install root (and
//                      Archive when it is absent or another folder).
//   RunOnce            HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce:
//                      a command Windows runs once at the next logon. The
//                      helper registers `<run copy>\katana_updater --recover
//                      <R>` before it commits, so a machine that lost power
//                      mid-swap rolls back at its next logon even if the
//                      program cannot start (docs/update.md).
//   uninstall values   HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Katana:
//                      DisplayVersion and DisplayName, rewritten after a
//                      successful update so Settings > Apps names the version
//                      that is installed.
//
// Real implementation: Windows in src/katana_updater/registry_win.cpp, which
// the update library compiles (the program reads installerLocation too, to
// say `kind=installer`). Elsewhere makeSystemRegistryOps() is a registry that
// has nothing: installerLocation is empty and the writes succeed doing nothing.
//
// WHAT THE WINDOWS IMPLEMENTATION HOLDS ITSELF TO:
//   - HKCU only, so no call needs or asks for administrator rights.
//   - installerLocation reads the key's default value. An expandable string is
//     expanded; a missing key, an empty value and a value that is not a string
//     are all "no installer" (std::nullopt), not errors.
//   - setRunOnce writes a REG_SZ under the given name and nothing else in the
//     key, and has it flushed to disk before it returns (RegFlushKey): the
//     entry is there for a machine that loses power during the commit, and the
//     registry would otherwise write it some seconds later, or never.
//     It REFUSES (InvalidArgument, nothing written, an older value of the
//     name left standing) a command longer than 260 characters - the limit
//     Microsoft documents for the command line of a Run or RunOnce value -
//     rather than write an entry that would not run. apply() goes on without
//     the entry: it is the second safety net, behind recover() at the next start
//     of any program.
//   - The entry exists only while a swap could have been cut short. apply()
//     registers it once the journal says COMMITTING and before the first file
//     moves, and removes it when the transaction ends in any way but
//     RollbackIncomplete (then the next logon is wanted to try again);
//     recoverInProcess removes it after a recovery that put everything back.
//   - setInstalledVersion OPENS the Uninstall key and never creates it: only an
//     installer makes that key, so an archive install gains no entry in
//     Settings > Apps. Other values of the key are left as they are.
//   - A test never reaches these keys. The helper built for the tests
//     (katana_updater_test) uses the same code re-rooted under an HKCU subkey of
//     the test's own, or a registry that has nothing
//     (src/katana_updater/main.cpp, --registry-base).

#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::update {

// The RunOnce value name the helper uses; one, so a second transaction
// replaces the first rather than queue behind it.
inline constexpr std::string_view kRunOnceName = "KatanaUpdateRecover";

class RegistryOps {
  public:
    virtual ~RegistryOps() = default;

    // The installer's recorded folder, or std::nullopt when there is none.
    [[nodiscard]] virtual core::Result<std::optional<std::filesystem::path>>
    installerLocation() const = 0;

    // Registers `command` (a full command line, UTF-8) to run once at the next
    // logon under `name`; replaces a value of that name.
    [[nodiscard]] virtual core::Status setRunOnce(std::string_view name,
                                                  std::string_view command) = 0;

    // Removes it; success when it was not there.
    [[nodiscard]] virtual core::Status clearRunOnce(std::string_view name) = 0;

    // Rewrites DisplayVersion and DisplayName under the Uninstall key, when
    // that key exists (it exists only for an installer-kind install); success
    // doing nothing when it does not.
    [[nodiscard]] virtual core::Status setInstalledVersion(std::string_view displayVersion,
                                                           std::string_view displayName) = 0;
};

[[nodiscard]] std::unique_ptr<RegistryOps> makeSystemRegistryOps();

} // namespace katana::update
