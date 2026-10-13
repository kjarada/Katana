// FINISHED (not a stub): the command line as UTF-8, for every main() of the update system
// (process.hpp, utf8Arguments). Written in the skeleton because a review found that the
// helper's stub took argv, which on Windows is the ANSI code page's version of the command
// line: a --root under a profile folder with an accented letter would arrive mangled, and
// the helper is the one program that is always started with such paths.

#include "katana/update/process.hpp"

#ifdef _WIN32
#include <windows.h>
#include <shellapi.h>
#endif

namespace katana::update {

#ifdef _WIN32

std::string utf8FromWide(std::wstring_view wide)
{
    std::string out;
    out.reserve(wide.size());
    const auto put = [&out](char32_t code) {
        if (code < 0x80) {
            out.push_back(static_cast<char>(code));
        } else if (code < 0x800) {
            out.push_back(static_cast<char>(0xC0 | (code >> 6)));
            out.push_back(static_cast<char>(0x80 | (code & 0x3F)));
        } else if (code < 0x10000) {
            out.push_back(static_cast<char>(0xE0 | (code >> 12)));
            out.push_back(static_cast<char>(0x80 | ((code >> 6) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | (code & 0x3F)));
        } else {
            out.push_back(static_cast<char>(0xF0 | (code >> 18)));
            out.push_back(static_cast<char>(0x80 | ((code >> 12) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | ((code >> 6) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | (code & 0x3F)));
        }
    };
    constexpr char32_t kReplacement = 0xFFFD;
    for (std::size_t i = 0; i < wide.size(); ++i) {
        const char32_t unit = static_cast<char16_t>(wide[i]);
        if (unit >= 0xD800 && unit <= 0xDBFF) { // a high surrogate: needs a low one next
            if (i + 1 < wide.size()) {
                const char32_t next = static_cast<char16_t>(wide[i + 1]);
                if (next >= 0xDC00 && next <= 0xDFFF) {
                    put(0x10000 + ((unit - 0xD800) << 10) + (next - 0xDC00));
                    ++i;
                    continue;
                }
            }
            put(kReplacement);
        } else if (unit >= 0xDC00 && unit <= 0xDFFF) { // a low surrogate with no high one
            put(kReplacement);
        } else {
            put(unit);
        }
    }
    return out;
}

std::vector<std::string> utf8Arguments(int argc, char** argv)
{
    std::vector<std::string> arguments;
    int count = 0;
    wchar_t** wide = CommandLineToArgvW(GetCommandLineW(), &count);
    if (wide != nullptr) {
        arguments.reserve(static_cast<std::size_t>(count));
        for (int i = 0; i < count; ++i) {
            arguments.push_back(utf8FromWide(wide[i]));
        }
        LocalFree(wide);
        return arguments;
    }
    // Out of memory in the one call: fall back on what the C runtime parsed, which is
    // right for ASCII and the best there is otherwise.
    for (int i = 0; i < argc; ++i) {
        arguments.emplace_back(argv[i]);
    }
    return arguments;
}

#else

std::vector<std::string> utf8Arguments(int argc, char** argv)
{
    std::vector<std::string> arguments;
    arguments.reserve(argc > 0 ? static_cast<std::size_t>(argc) : 0);
    for (int i = 0; i < argc; ++i) {
        arguments.emplace_back(argv[i]);
    }
    return arguments;
}

#endif

} // namespace katana::update
