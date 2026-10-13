// Version text: MAJOR.MINOR[.PATCH[.BUILD]], digits only, compared as integers
// (include/katana/update/version.hpp). The text comes from a signed manifest and from this
// program's own build, so one parser holds both to the same rule.
//
// programVersionText is how the skeleton proves that project(Katana VERSION) reaches this
// library (test_skeleton.cpp), which katana_cli's own build never did.

#include "katana/update/version.hpp"

#include <algorithm>
#include <cstddef>

#ifndef KATANA_VERSION
#error "KATANA_VERSION is defined for katana_update by src/katana_update/CMakeLists.txt"
#endif

namespace katana::update {

namespace {

constexpr std::size_t kMinComponents = 2;
constexpr std::size_t kMaxComponents = 4;
// 999,999,999 is the largest nine-digit number and is below 2^32 (4,294,967,296), so a
// component of at most nine digits cannot overflow the std::uint32_t it is stored in, and
// there is no arithmetic to wrap: the length is the overflow check.
constexpr std::size_t kMaxDigits = 9;
// A valid version is at most 4 * 9 digits and 3 dots, 39 bytes; an error quotes no more
// than a little over that, so a hostile megabyte is not copied into a message.
constexpr std::size_t kExcerptBytes = 48;

// The text for an error context: bounded, and every byte that is not printable ASCII
// (a NUL, a newline, a byte of a UTF-8 sequence) shown as '?', so a log line stays one line.
std::string excerpt(std::string_view text)
{
    std::string out;
    const std::size_t shown = std::min(text.size(), kExcerptBytes);
    out.reserve(shown + 3);
    for (std::size_t i = 0; i < shown; ++i) {
        const auto c = static_cast<unsigned char>(text[i]);
        out.push_back(c >= 0x20 && c <= 0x7E ? static_cast<char>(c) : '?');
    }
    if (text.size() > shown) {
        out += "...";
    }
    return out;
}

core::Error refuse(std::string_view why, std::string_view text)
{
    return core::makeError(core::ErrorCode::ParseFailure,
                           "not a version (MAJOR.MINOR[.PATCH[.BUILD]], digits): " + std::string(why),
                           excerpt(text));
}

} // namespace

core::Result<Version> parseVersion(std::string_view text)
{
    if (text.empty()) {
        return refuse("empty", text);
    }

    Version version;
    std::size_t count = 0;
    std::size_t digits = 0;
    std::uint32_t value = 0;

    // One pass over the bytes; a component ends at a dot or at the end of the text. Only
    // ASCII digits and dots are accepted, so a sign, a space, a letter ("1.2-beta", "v1.2"),
    // a non-ASCII digit or a NUL is refused wherever it stands.
    for (std::size_t i = 0; i <= text.size(); ++i) {
        const bool atEnd = i == text.size();
        const char c = atEnd ? '.' : text[i];
        if (c >= '0' && c <= '9') {
            if (++digits > kMaxDigits) {
                return refuse("a component of more than nine digits", text);
            }
            value = value * 10 + static_cast<std::uint32_t>(c - '0');
        } else if (c == '.') {
            if (digits == 0) {
                return refuse("an empty component", text);
            }
            if (count == kMaxComponents) {
                return refuse("more than four components", text);
            }
            version.components[count++] = value;
            digits = 0;
            value = 0;
            if (atEnd) {
                break;
            }
        } else {
            return refuse("a character other than a digit or a dot", text);
        }
    }

    if (count < kMinComponents) {
        return refuse("fewer than two components", text);
    }
    version.written = static_cast<std::uint8_t>(count);
    return version;
}

std::string toString(const Version& version)
{
    // `written` is 2 to 4 for anything parseVersion made; a hand-built Version with another
    // number still gives text that parses, rather than an out-of-range read.
    const std::size_t count =
        std::clamp<std::size_t>(version.written, kMinComponents, kMaxComponents);
    std::string out;
    for (std::size_t i = 0; i < count; ++i) {
        if (i != 0) {
            out.push_back('.');
        }
        out += std::to_string(version.components[i]);
    }
    return out;
}

std::string_view programVersionText()
{
    return KATANA_VERSION;
}

namespace detail {

// programVersion for any text, so that the refusal can be tested without a broken build
// (tests/update/test_version.cpp declares it; it is in no public header).
core::Result<Version> programVersionOf(std::string_view text)
{
    core::Result<Version> parsed = parseVersion(text);
    if (parsed) {
        return parsed;
    }
    // The text is the project's own, not input: a refusal here is a broken build, not a bad
    // file, so it is Internal (version.hpp).
    return core::makeError(core::ErrorCode::Internal,
                           "the program's own version is not MAJOR.MINOR[.PATCH[.BUILD]]: " +
                               parsed.error().message,
                           excerpt(text));
}

} // namespace detail

core::Result<Version> programVersion()
{
    return detail::programVersionOf(programVersionText());
}

} // namespace katana::update
