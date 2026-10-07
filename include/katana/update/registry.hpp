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
