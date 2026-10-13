// The Windows registry of include/katana/update/registry.hpp: the few HKCU values an update
// reads and writes. Per-user only, so nothing here asks for administrator rights (design, D1).
// Compiled into the update library, since the programs read installerLocation() to say
// `kind=installer`. CMake compiles this file on Windows only.
//
//   HKCU\Software\Jarada\Katana                        default value: the folder the installer used
//   HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce
//                                                      value kRunOnceName: the recovery command
//   HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Katana
//                                                      DisplayVersion and DisplayName, only if the
//                                                      key is there (the installer made it)
//
// RunOnce is Windows' own mechanism: at the next logon the shell runs each value once and deletes
// it first. The helper writes ours before it starts renaming files and removes it when the
// transaction ends, so the entry exists only while a swap could have been cut short. Microsoft
// documents a limit of 260 characters on the command line of a Run or RunOnce value; a longer one
// is refused here, with the reason, rather than written as an entry that would not run (apply()
// treats the entry as a second safety net behind recovery at the next start of any program, and
// goes on without it).
//
// THE TEST SEAM. makeRegistryOpsUnder(base) is the same code with every key path re-rooted under
// the HKCU subkey `base`, so that a test writes and deletes keys of its own and never the
// machine's RunOnce or Uninstall entries. Nothing in the production programs calls it; the helper
// built for the tests (katana_updater_test, --registry-base) and tests/update/test_helper.cpp do.
// It is declared where it is used, as testTrust() is, and not in registry.hpp, whose interface is
// the one the programs share.

#include "katana/update/registry.hpp"

#ifndef _WIN32
#error "registry_win.cpp is compiled on Windows only"
#endif

#include <algorithm>
#include <cwchar>
#include <string>
#include <vector>

#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

namespace katana::update {

namespace {

constexpr std::wstring_view kLocationKey = L"Software\\Jarada\\Katana";
constexpr std::wstring_view kRunOnceKey = L"Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce";
constexpr std::wstring_view kUninstallKey =
    L"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Katana";
// Microsoft, "Run and RunOnce Registry Keys": the command line of a value is limited to this.
constexpr std::size_t kRunOnceCommandLimit = 260;

std::wstring wideFromUtf8(std::string_view text, bool& valid)
{
    valid = true;
    if (text.empty()) {
        return {};
    }
    const int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text.data(),
                                           static_cast<int>(text.size()), nullptr, 0);
    if (length <= 0) {
        valid = false;
        return {};
    }
    std::wstring wide(static_cast<std::size_t>(length), L'\0');
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text.data(), static_cast<int>(text.size()),
                        wide.data(), length);
    return wide;
}

core::Error registryError(core::ErrorCode code, std::string message, LONG status,
                          std::string_view what)
{
    return core::makeError(code, std::move(message),
                           "os_error=" + std::to_string(status) + " " + std::string(what));
}

bool isMissing(LONG status)
{
    return status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND;
}

class Key {
  public:
    Key() = default;
    Key(const Key&) = delete;
    Key& operator=(const Key&) = delete;
    ~Key()
    {
        if (key_ != nullptr) {
            RegCloseKey(key_);
        }
    }
    [[nodiscard]] HKEY get() const { return key_; }
    [[nodiscard]] HKEY* out() { return &key_; }

  private:
    HKEY key_ = nullptr;
};

class WindowsRegistry final : public RegistryOps {
  public:
    // `base` is empty for the machine's real keys, else an HKCU subkey with no leading or
    // trailing backslash.
    explicit WindowsRegistry(std::wstring base) : base_(std::move(base)) {}

    [[nodiscard]] core::Result<std::optional<std::filesystem::path>>
    installerLocation() const override
    {
        const std::wstring path = under(kLocationKey);
        std::wstring value(MAX_PATH, L'\0');
        for (int attempt = 0; attempt < 8; ++attempt) {
            DWORD bytes = static_cast<DWORD>(value.size() * sizeof(wchar_t));
            // The default value (no name), a string; environment variables in an expandable one
            // are expanded, as an installer may have written %LOCALAPPDATA%. RRF_RT_REG_SZ alone
            // asks for exactly that: without RRF_NOEXPAND an expandable string is expanded and
            // handed back AS a REG_SZ, so it passes the restriction. (The flag for the expandable
            // type is for reading one unexpanded, with RRF_NOEXPAND; it is not wanted here.)
            const LONG status = RegGetValueW(HKEY_CURRENT_USER, path.c_str(), nullptr,
                                             RRF_RT_REG_SZ, nullptr, value.data(), &bytes);
            if (status == ERROR_SUCCESS) {
                // The text ends at the first NUL. For an expandable string the size reported is
                // that of the stored text, not of the expanded one, so what follows the NUL is
                // the stored text's leftover (measured: L"C:\\WINDOWS\\Katana\0a").
                value.resize(wcsnlen(value.c_str(), std::min<std::size_t>(
                                                        value.size(), bytes / sizeof(wchar_t))));
                if (value.empty()) {
                    return std::optional<std::filesystem::path>{};
                }
                return std::optional<std::filesystem::path>(std::filesystem::path(value));
            }
            if (status == ERROR_MORE_DATA) {
                // The size asked for can be the stored text's and not the expanded one's (the
                // same oddity), so the buffer also at least doubles: eight rounds reach the
                // longest path Windows has.
                value.assign(std::max<std::size_t>(bytes / sizeof(wchar_t) + 1, value.size() * 2),
                             L'\0');
                continue;
            }
            if (isMissing(status) || status == ERROR_UNSUPPORTED_TYPE) {
                // No installer key, or a value that is not a string: this is not an installer's.
                return std::optional<std::filesystem::path>{};
            }
            return registryError(core::ErrorCode::Internal, "the installer's folder cannot be read",
                                 status, "HKCU\\" + narrow(path));
        }
        return registryError(core::ErrorCode::Internal, "the installer's folder cannot be read",
                             ERROR_MORE_DATA, "HKCU\\" + narrow(path));
    }

    [[nodiscard]] core::Status setRunOnce(std::string_view name, std::string_view command) override
    {
        bool validName = false;
        bool validCommand = false;
        const std::wstring wideName = wideFromUtf8(name, validName);
        const std::wstring wideCommand = wideFromUtf8(command, validCommand);
        if (!validName || !validCommand || wideName.empty() || wideCommand.empty()) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the RunOnce name and command must be non-empty UTF-8");
        }
        if (wideCommand.size() > kRunOnceCommandLimit) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the command is longer than the 260 characters Windows runs "
                                   "from RunOnce",
                                   std::to_string(wideCommand.size()) + " characters");
        }
        const std::wstring path = under(kRunOnceKey);
        Key key;
        LONG status = RegCreateKeyExW(HKEY_CURRENT_USER, path.c_str(), 0, nullptr, 0,
                                      KEY_SET_VALUE, nullptr, key.out(), nullptr);
        if (status != ERROR_SUCCESS) {
            return registryError(core::ErrorCode::InvalidState, "the RunOnce key cannot be opened",
                                 status, "HKCU\\" + narrow(path));
        }
        status = RegSetValueExW(key.get(), wideName.c_str(), 0, REG_SZ,
                                reinterpret_cast<const BYTE*>(wideCommand.c_str()),
                                static_cast<DWORD>((wideCommand.size() + 1) * sizeof(wchar_t)));
        if (status != ERROR_SUCCESS) {
            return registryError(core::ErrorCode::InvalidState,
                                 "the RunOnce value cannot be written", status, std::string(name));
        }
        return {};
    }

    [[nodiscard]] core::Status clearRunOnce(std::string_view name) override
    {
        bool valid = false;
        const std::wstring wideName = wideFromUtf8(name, valid);
        if (!valid || wideName.empty()) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the RunOnce name must be non-empty UTF-8");
        }
        const std::wstring path = under(kRunOnceKey);
        const LONG status = RegDeleteKeyValueW(HKEY_CURRENT_USER, path.c_str(), wideName.c_str());
        if (status == ERROR_SUCCESS || isMissing(status)) {
            return {}; // already gone is gone
        }
        return registryError(core::ErrorCode::InvalidState, "the RunOnce value cannot be removed",
                             status, std::string(name));
    }

    [[nodiscard]] core::Status setInstalledVersion(std::string_view displayVersion,
                                                   std::string_view displayName) override
    {
        bool validVersion = false;
        bool validName = false;
        const std::wstring version = wideFromUtf8(displayVersion, validVersion);
        const std::wstring name = wideFromUtf8(displayName, validName);
        if (!validVersion || !validName) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the display version and name must be UTF-8");
        }
        const std::wstring path = under(kUninstallKey);
        Key key;
        // Opened, never created: only the installer makes this key, and an archive install has
        // none (Settings > Apps lists what an installer registered, nothing else).
        LONG status = RegOpenKeyExW(HKEY_CURRENT_USER, path.c_str(), 0, KEY_SET_VALUE, key.out());
        if (isMissing(status)) {
            return {};
        }
        if (status != ERROR_SUCCESS) {
            return registryError(core::ErrorCode::InvalidState,
                                 "the Uninstall key cannot be opened", status,
                                 "HKCU\\" + narrow(path));
        }
        const auto write = [&key](const wchar_t* valueName, const std::wstring& text) {
            return RegSetValueExW(key.get(), valueName, 0, REG_SZ,
                                  reinterpret_cast<const BYTE*>(text.c_str()),
                                  static_cast<DWORD>((text.size() + 1) * sizeof(wchar_t)));
        };
        status = write(L"DisplayVersion", version);
        if (status == ERROR_SUCCESS) {
            status = write(L"DisplayName", name);
        }
        if (status != ERROR_SUCCESS) {
            return registryError(core::ErrorCode::InvalidState,
                                 "the installed version cannot be recorded", status,
                                 "DisplayVersion");
        }
        return {};
    }

  private:
    [[nodiscard]] std::wstring under(std::wstring_view key) const
    {
        return base_.empty() ? std::wstring(key) : base_ + L"\\" + std::wstring(key);
    }

    // A key path for a message: the registry's names are UTF-16, a message is UTF-8.
    [[nodiscard]] static std::string narrow(const std::wstring& text)
    {
        if (text.empty()) {
            return {};
        }
        const int length = WideCharToMultiByte(CP_UTF8, 0, text.data(),
                                               static_cast<int>(text.size()), nullptr, 0,
                                               nullptr, nullptr);
        std::string out(static_cast<std::size_t>(length > 0 ? length : 0), '\0');
        if (length > 0) {
            WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), out.data(),
                                length, nullptr, nullptr);
        }
        return out;
    }

    std::wstring base_;
};

} // namespace

std::unique_ptr<RegistryOps> makeSystemRegistryOps()
{
    return std::make_unique<WindowsRegistry>(std::wstring());
}

// The test seam (see the top): the same keys under HKCU\<base>. nullptr for a base that is empty,
// starts or ends with a backslash, or is not UTF-8: it would not name a folder of keys of its own.
std::unique_ptr<RegistryOps> makeRegistryOpsUnder(std::string_view base)
{
    bool valid = false;
    std::wstring wide = wideFromUtf8(base, valid);
    if (!valid || wide.empty() || wide.front() == L'\\' || wide.back() == L'\\') {
        return nullptr;
    }
    return std::make_unique<WindowsRegistry>(std::move(wide));
}

} // namespace katana::update
