#include "katana/core/path_text.hpp"

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
