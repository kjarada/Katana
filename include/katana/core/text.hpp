#pragma once

// Locale-independent text helpers for every reader of untrusted text: the 12d
// Archive, the XML reader, the survey field-data parsers.
//
// ONE set, here, because there had come to be three: `archive12d::detail`, a
// private copy in `xml.cpp` whose comment said it stayed private only because
// core had no text header, and a third written by the survey parsers because
// they may see neither of the other two (tools/check_layering.cmake). Three
// trims that differ in which bytes count as blank are three answers to what a
// file says.
//
// NOTHING HERE CONSULTS THE C LOCALE, and that is the reason these are not
// thin wrappers over <cctype> or strtod. `std::isspace` and `std::tolower` read
// the locale a program has set, and a GUI toolkit may set it from the
// environment at start-up. In a Latin-1 locale `isspace(0xA0)` is true, and 0xA0
// is also the second byte of "à" in UTF-8 - so trimming a name that ends in "à"
// cut the character in half and left text that is no longer valid UTF-8. The
// blank set below is exactly the C locale's, so in the "C" locale nothing
// changes; in any other nothing changes either, which is the point.
//
// Numbers go through std::from_chars, which is locale-independent by
// specification ([charconv.from.chars]); a decimal comma is never a decimal
// point here. A caller reading a file that uses one converts it explicitly.

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace katana::core {

// ' ', '\t', '\n', '\v', '\f' and '\r': the six the C locale's isspace accepts.
[[nodiscard]] constexpr bool isAsciiSpace(char c) noexcept
{
    return c == ' ' || c == '\t' || c == '\n' || c == '\v' || c == '\f' || c == '\r';
}

[[nodiscard]] constexpr char asciiLower(char c) noexcept
{
    return (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
}

// `text` without leading and trailing isAsciiSpace bytes. A view into `text`.
[[nodiscard]] std::string_view trimmed(std::string_view text) noexcept;

// ASCII letters folded to lower case; every other byte, including every byte of
// a multi-byte UTF-8 sequence, is left exactly as it was.
[[nodiscard]] std::string lowered(std::string_view text);
[[nodiscard]] bool equalsIgnoringCase(std::string_view a, std::string_view b) noexcept;

// A decimal integer occupying the WHOLE token, with at most one sign. nullopt for
// anything else - an empty token, a real ("2.5" must not become 2), trailing
// text, "+-1", or a value outside int64. The caller trims first if it wants to.
[[nodiscard]] std::optional<std::int64_t> parseInteger(std::string_view token) noexcept;

// A finite real occupying the WHOLE token, in the from_chars "general" form:
// "12", "-0.5", "1e-3", "6.02E+23", with at most one leading sign. nullopt for an
// empty token, trailing text, a decimal comma, hexadecimal, and for "inf" and
// "nan" - a coordinate that is not finite is not a coordinate, and every reader
// here would otherwise have to remember to check. Out-of-range magnitudes
// ("1e999") are nullopt rather than infinity for the same reason.
[[nodiscard]] std::optional<double> parseFiniteDouble(std::string_view token) noexcept;

// The lines of `text`, without their terminators. CR, LF and CRLF each end a
// line, because field files come off every operating system there is and a
// file edited on two of them mixes them. A terminator at the very end does not
// make an empty last line; an empty line in the middle is kept, because a
// record number is a line number and dropping one would shift every later one.
// Views into `text`.
[[nodiscard]] std::vector<std::string_view> splitLines(std::string_view text);

// The shortest decimal text that reads back as exactly `value` - what an
// exporter writes when the file is for another program rather than a person,
// so that export then import is the identity. "0.1", "502000.125", "1e+20".
// Non-finite values are written as from_chars would ("inf", "nan"), which
// parseFiniteDouble refuses; an exporter should not be handed one.
[[nodiscard]] std::string formatExactReal(double value);

// A text as typed on a command line, read back: "\n" is a line break - the
// command line has no other way to put one in a note - and "\\" is a
// backslash, so a text holding "\n" itself (a path, C:\new) can be typed as
// a reply writes it. Any other backslash is itself. The one reader for the
// verbs' text options (the annotation and the sheet verbs).
[[nodiscard]] std::string unescapeTyped(std::string_view text);

// ---- key=value replies ---------------------------------------------------------
//
// A verb's reply is records, one a line: leading words, then key=value fields
// ("line id=W1 type=water length=30.024"), so a script or an agent reads what
// happened (docs/cad.md). A value that is text goes through replyQuoted, and
// readReplyRecord reads a line back - one writer and one reader, here, for
// every verb whose reply is read by something other than a person.

// A text value in a reply: in double quotes, a line break written "\n", a
// backslash "\\" and a double quote "\"", so the reply stays one fact a
// line and reads back unambiguously.
[[nodiscard]] std::string replyQuoted(std::string_view text);

struct ReplyRecord {
    std::vector<std::string> words;                          // before the first key=value
    std::vector<std::pair<std::string, std::string>> fields; // in order, values unquoted

    // The value of `key`, the first if it is given twice; nullopt when absent.
    [[nodiscard]] std::optional<std::string> value(std::string_view key) const;
};

// One line of a reply read back: its leading words and its fields, a quoted
// value unescaped. nullopt for a line that is not a record - blank, a quote
// never closed, a word after the first field, a field with no key.
[[nodiscard]] std::optional<ReplyRecord> readReplyRecord(std::string_view line);

} // namespace katana::core
