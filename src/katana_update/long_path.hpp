#pragma once

// A path as the system's file calls want it: INTERNAL to the update library and never installed.
//
// On Windows a path of about 260 characters or more is refused by the ordinary calls unless it
// carries the \\?\ prefix, and one that ends in a dot or a space is silently shortened. The real
// file system (fsops_win.cpp) prefixes every path it hands to the system, so an install under a
// deep folder works. Code that reads a file by a way other than FsOps - the SHA-256 of a file,
// which streams it from the C++ library - has to give the same path or it fails exactly where the
// file system did not: a copy under R/.update/<tx>/new/ is about 46 characters deeper than the
// file it will replace.
//
// Nothing here touches the disk.

#include <filesystem>
#include <string>

namespace katana::update::detail {

#ifdef _WIN32

inline constexpr std::size_t kLongPathLength = 240; // below MAX_PATH (260) with room for a name

inline std::filesystem::path absoluteNormal(const std::filesystem::path& path)
{
    std::error_code ec;
    std::filesystem::path absolute = std::filesystem::absolute(path, ec);
    if (ec) {
        absolute = path;
    }
    absolute = absolute.lexically_normal();
    absolute.make_preferred();
    return absolute;
}

inline bool endsInDotOrSpace(const std::wstring& text)
{
    // A trailing separator is not part of the name.
    std::size_t end = text.size();
    while (end > 0 && text[end - 1] == L'\\') {
        --end;
    }
    return end > 0 && (text[end - 1] == L'.' || text[end - 1] == L' ');
}

// The path as the wide string the system calls take: absolute, with backslashes, and the
// long-path prefix (backslash backslash question-mark backslash) when it is long or ends in a
// dot or a space.
inline std::wstring widePath(const std::filesystem::path& path)
{
    const std::filesystem::path absolute = absoluteNormal(path);
    std::wstring text = absolute.native();
    if (text.rfind(L"\\\\?\\", 0) == 0 || text.rfind(L"\\\\.\\", 0) == 0) {
        return text;
    }
    if (text.size() < kLongPathLength && !endsInDotOrSpace(text)) {
        return text;
    }
    if (text.rfind(L"\\\\", 0) == 0) { // \\server\share\... -> \\?\UNC\server\share\...
        return L"\\\\?\\UNC\\" + text.substr(2);
    }
    return L"\\\\?\\" + text;
}

#endif

// The path to give code that opens a file with the C++ library (core::sha256FileHex): the same
// file, named the way fsops_win.cpp would name it. Unchanged where paths have no such limit.
inline std::filesystem::path openable(const std::filesystem::path& path)
{
#ifdef _WIN32
    return std::filesystem::path(widePath(path));
#else
    return path;
#endif
}

} // namespace katana::update::detail
