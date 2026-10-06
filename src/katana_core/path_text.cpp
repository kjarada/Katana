#include "katana/core/path_text.hpp"

#include <cstdlib>
#include <fstream>
#include <iterator>
#include <system_error>

#include "katana/core/text_encoding.hpp"

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#endif

namespace katana::core {

namespace {

// The path a NARROW name names: what the C runtime opens when handed these
// bytes. On Windows that is the name read in the ANSI code page, and it has
// to be converted here: this standard library's std::filesystem::path does
// not do it - handed narrow bytes it converts them as UTF-8 and throws at
// the first that is not (path_text.hpp has what was probed), which is how a
// name typed in an ANSI console once ended a verb with an exception nothing
// caught. Elsewhere a narrow name is the native one, byte for byte.
[[nodiscard]] std::filesystem::path narrowPath(std::string_view text)
{
#if defined(_WIN32)
    if (text.empty()) {
        return {};
    }
    const int bytes = static_cast<int>(text.size());
    // No flags: a byte the code page has no character for becomes its
    // default character rather than a failure, as the runtime's own
    // conversion does. The name then names no file, and the read says so.
    const int length = MultiByteToWideChar(CP_ACP, 0, text.data(), bytes, nullptr, 0);
    if (length <= 0) {
        return {};
    }
    std::wstring wide(static_cast<std::size_t>(length), L'\0');
    MultiByteToWideChar(CP_ACP, 0, text.data(), bytes, wide.data(), length);
    return std::filesystem::path(wide);
#else
    return std::filesystem::path(std::string(text));
#endif
}

} // namespace

std::filesystem::path pathFromUtf8(std::string_view text)
{
    if (!isValidUtf8(text)) {
        return narrowPath(text);
    }
    // Through char8_t, which is what tells std::filesystem the bytes are
    // UTF-8 on every platform.
    std::u8string utf8;
    utf8.reserve(text.size());
    for (const char c : text) {
        utf8 += static_cast<char8_t>(c);
    }
    return std::filesystem::path(utf8);
}

std::string pathToUtf8(const std::filesystem::path& path)
{
    const std::u8string text = path.u8string();
    return std::string(text.begin(), text.end());
}

std::string environmentVariable(const char* name)
{
    if (name == nullptr || *name == '\0') {
        return {};
    }
#if defined(_WIN32)
    std::wstring wideName;
    for (const char* ch = name; *ch != '\0'; ++ch) {
        wideName += static_cast<wchar_t>(static_cast<unsigned char>(*ch));
    }
    // Asked of the system, which holds every value as it was set, rather
    // than of the C runtime's copies: the narrow one is the lossy one, and
    // the wide one (_wgetenv) is made on demand by rules of the runtime's
    // own, which this then need not depend on. What the runtime's _putenv_s
    // and _wputenv_s set, they set in the system's block too
    // (tests/core/test_path_text.cpp sets its variable that way).
    //
    // The size first, then the value: a value may be longer than any buffer
    // chosen beforehand (up to 32,767 characters). One read is the rule; a
    // second is for a value another thread made longer between the two
    // calls. After a few the variable is being rewritten as it is read, and
    // nothing is the answer rather than a wait.
    constexpr int kReads = 4;
    std::wstring value;
    bool fitted = false;
    DWORD size = GetEnvironmentVariableW(wideName.c_str(), nullptr, 0);
    for (int attempt = 0; size != 0 && attempt < kReads && !fitted; ++attempt) {
        value.assign(size, L'\0');
        const DWORD length = GetEnvironmentVariableW(wideName.c_str(), value.data(), size);
        if (length < size) {
            // It fits: `length` is the value's, without its terminator - 0
            // when the variable went between the two calls.
            value.resize(length);
            fitted = true;
        } else {
            // Too small now: `length` is the size the longer value needs.
            size = length;
        }
    }
    if (!fitted || value.empty()) {
        return {};
    }
    // No flags: half of a surrogate pair - not text, though a name on this
    // file system may hold one - becomes U+FFFD rather than a failure. The
    // name then names no file, and the read of it says so.
    const int characters = static_cast<int>(value.size());
    const int bytes =
        WideCharToMultiByte(CP_UTF8, 0, value.data(), characters, nullptr, 0, nullptr, nullptr);
    if (bytes <= 0) {
        return {};
    }
    std::string text(static_cast<std::size_t>(bytes), '\0');
    WideCharToMultiByte(CP_UTF8, 0, value.data(), characters, text.data(), bytes, nullptr,
                        nullptr);
    return text;
#else
    const char* value = std::getenv(name);
    return value == nullptr ? std::string() : std::string(value);
#endif
}

Result<std::string> readFileBytes(const std::filesystem::path& path)
{
    // A directory opens as a stream on some platforms and then reads as
    // nothing, which would pass for an empty file.
    std::error_code unknown;
    if (std::filesystem::is_directory(path, unknown)) {
        return makeError(ErrorCode::NotFound, "the file cannot be opened: it is a directory",
                         pathToUtf8(path));
    }
    std::ifstream in(path, std::ios::binary);
    if (!in) {
        return makeError(ErrorCode::NotFound, "the file cannot be opened", pathToUtf8(path));
    }
    std::string bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    if (in.bad()) {
        return makeError(ErrorCode::FileImportFailure, "the file cannot be read",
                         pathToUtf8(path));
    }
    return bytes;
}

} // namespace katana::core
