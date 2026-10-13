// A stand-in for the person's Katana, for the tests that drive the real helper program
// (tests/update/test_helper.cpp). Never installed, never shipped: tools/check_install_rules.cmake
// lists the programs that are.
//
// The helper starts `R/bin/katana.exe --update-health <tx>` to prove the new version starts, and
// starts `R/bin/katana.exe` again at the end. The test builds an install whose katana.exe IS this
// program (plus a tag appended to the file, so that v1 and v2 differ and each can tell which it
// is), and says by environment what it should do:
//
//   KATANA_STUB_LOG              file to append one line to for every start (what ran, with what)
//   KATANA_STUB_MODE             for --update-health: healthy (default: write the marker, exit 0),
//                                exit1 (exit 1 without the marker), delay_exit1 (the same after 1.5 s,
//                                time for a test to do something to the install), marker_then_exit3,
//                                silent (never write it and never return: the helper's timeout ends it)
//   KATANA_UPDATE_STATE_DIR      where the marker goes, <dir>\healthy-<tx> (state.hpp)
//   KATANA_STUB_REGISTRY_BASE    an HKCU subkey: the line says whether the RunOnce value of the
//                                helper is there at that moment (it must be, during the health
//                                check, and gone afterwards)
//
//   --sleep <ms>                 stay running that long: a process whose image is under the
//                                install, for the helper's scan to find
//
// Each log line is `<kind> tag=<tag> exe=<path> OPENSSL_CONF=<value or -> [runonce=<value or ->]`,
// UTF-8. The tag is read from the end of this very file: the text KATANA-STUB-TAG= and what follows
// it, the LAST occurrence (the first is this program's own string literal).

#ifndef _WIN32
#error "the stub is for Windows only: the helper's process scan and registry are"
#endif

#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <string>
#include <string_view>
#include <vector>

namespace {

std::string utf8(const std::wstring& wide)
{
    if (wide.empty()) {
        return {};
    }
    const int length = WideCharToMultiByte(CP_UTF8, 0, wide.data(), static_cast<int>(wide.size()),
                                           nullptr, 0, nullptr, nullptr);
    std::string out(static_cast<std::size_t>(length), '\0');
    WideCharToMultiByte(CP_UTF8, 0, wide.data(), static_cast<int>(wide.size()), out.data(), length,
                        nullptr, nullptr);
    return out;
}

std::wstring environment(const wchar_t* name)
{
    std::wstring value(512, L'\0');
    for (int attempt = 0; attempt < 3; ++attempt) {
        const DWORD size = GetEnvironmentVariableW(name, value.data(), static_cast<DWORD>(value.size()));
        if (size == 0) {
            return {};
        }
        if (size < value.size()) {
            value.resize(size);
            return value;
        }
        value.assign(size + 1, L'\0');
    }
    return {};
}

std::wstring ownPath()
{
    std::wstring path(1024, L'\0');
    const DWORD length = GetModuleFileNameW(nullptr, path.data(), static_cast<DWORD>(path.size()));
    path.resize(length);
    return path;
}

// The text after the last KATANA-STUB-TAG= in this program's own file, up to the line end.
std::string ownTag()
{
    const HANDLE file = CreateFileW(ownPath().c_str(), GENERIC_READ,
                                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                                    OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) {
        return "unreadable";
    }
    std::string bytes;
    std::vector<char> chunk(1 << 16);
    DWORD got = 0;
    while (ReadFile(file, chunk.data(), static_cast<DWORD>(chunk.size()), &got, nullptr) && got > 0) {
        bytes.append(chunk.data(), got);
    }
    CloseHandle(file);
    const std::string_view key = "KATANA-STUB-TAG=";
    const std::size_t at = bytes.rfind(key);
    if (at == std::string::npos) {
        return "none";
    }
    const std::size_t begin = at + key.size();
    const std::size_t end = bytes.find_first_of("\r\n", begin);
    return bytes.substr(begin, end == std::string::npos ? std::string::npos : end - begin);
}

void appendLine(const std::wstring& path, const std::string& line)
{
    if (path.empty()) {
        return;
    }
    const HANDLE file = CreateFileW(path.c_str(), FILE_APPEND_DATA,
                                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                                    OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) {
        return;
    }
    const std::string text = line + "\n";
    DWORD written = 0;
    WriteFile(file, text.data(), static_cast<DWORD>(text.size()), &written, nullptr);
    CloseHandle(file);
}

// The RunOnce value of the helper under HKCU\<base>, or "-" when it is not there.
std::string runOnceValue(const std::wstring& base)
{
    const std::wstring key = base + L"\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce";
    std::wstring value(1024, L'\0');
    DWORD bytes = static_cast<DWORD>(value.size() * sizeof(wchar_t));
    const LONG status = RegGetValueW(HKEY_CURRENT_USER, key.c_str(), L"KatanaUpdateRecover",
                                     RRF_RT_REG_SZ, nullptr, value.data(), &bytes);
    if (status != ERROR_SUCCESS) {
        return "-";
    }
    value.resize(bytes / sizeof(wchar_t));
    while (!value.empty() && value.back() == L'\0') {
        value.pop_back();
    }
    return utf8(value);
}

} // namespace

int main(int argc, char** argv)
{
    const std::wstring logPath = environment(L"KATANA_STUB_LOG");
    const std::string mode = utf8(environment(L"KATANA_STUB_MODE"));
    const std::wstring openssl = environment(L"OPENSSL_CONF");
    const std::wstring registryBase = environment(L"KATANA_STUB_REGISTRY_BASE");

    std::string line;
    const std::string first = argc > 1 ? argv[1] : "";
    line = (first == "--update-health" ? "health" : first == "--sleep" ? "sleep" : "start");
    line += " tag=" + ownTag() + " exe=" + utf8(ownPath()) + " OPENSSL_CONF=" +
            (openssl.empty() ? std::string("-") : utf8(openssl));
    if (!registryBase.empty()) {
        line += " runonce=" + runOnceValue(registryBase);
    }
    if (first == "--update-health" && argc > 2) {
        line += std::string(" tx=") + argv[2] + " mode=" + (mode.empty() ? "healthy" : mode);
    }
    appendLine(logPath, line);

    if (first == "--sleep" && argc > 2) {
        Sleep(static_cast<DWORD>(std::stoul(argv[2])));
        return 0;
    }
    if (first == "--update-health" && argc > 2) {
        if (mode == "exit1") {
            return 1;
        }
        if (mode == "delay_exit1") {
            Sleep(1500);
            return 1;
        }
        if (mode == "silent") {
            Sleep(120'000);
            return 0;
        }
        const std::wstring state = environment(L"KATANA_UPDATE_STATE_DIR");
        if (!state.empty()) {
            const std::wstring tx(argv[2], argv[2] + std::char_traits<char>::length(argv[2]));
            const std::wstring marker = state + L"\\healthy-" + tx;
            const HANDLE file = CreateFileW(marker.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr,
                                            CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
            if (file != INVALID_HANDLE_VALUE) {
                DWORD written = 0;
                WriteFile(file, "ok\n", 3, &written, nullptr);
                CloseHandle(file);
            }
        }
        return mode == "marker_then_exit3" ? 3 : 0;
    }
    return 0;
}
