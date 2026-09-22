#include "text_utilities.hpp"

#include <algorithm>
#include <cctype>
#include <charconv>
#include <cmath>
#include <system_error>

#include "katana/archive12d/reader.hpp"

namespace katana::archive12d {

namespace detail {

std::string lowered(std::string_view text)
{
    std::string out(text);
    std::transform(out.begin(), out.end(), out.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return out;
}

bool equalsIgnoringCase(std::string_view a, std::string_view b)
{
    return a.size() == b.size() &&
           std::equal(a.begin(), a.end(), b.begin(), [](unsigned char x, unsigned char y) {
               return std::tolower(x) == std::tolower(y);
           });
}

std::string_view trimmed(std::string_view text)
{
    const auto isSpace = [](unsigned char ch) { return std::isspace(ch) != 0; };
    while (!text.empty() && isSpace(static_cast<unsigned char>(text.front()))) {
        text.remove_prefix(1);
    }
    while (!text.empty() && isSpace(static_cast<unsigned char>(text.back()))) {
        text.remove_suffix(1);
    }
    return text;
}

std::optional<std::int64_t> parseInteger(std::string_view token)
{
    if (!token.empty() && token.front() == '+') {
        token.remove_prefix(1);
    }
    if (token.empty()) {
        return std::nullopt;
    }
    std::int64_t value = 0;
    const auto result = std::from_chars(token.data(), token.data() + token.size(), value);
    if (result.ec != std::errc{} || result.ptr != token.data() + token.size()) {
        return std::nullopt;
    }
    return value;
}

std::optional<bool> parseBoolean(std::string_view token)
{
    if (token.empty()) {
        return std::nullopt;
    }
    if (const auto number = parseInteger(token)) {
        return *number != 0;
    }
    switch (token.front()) {
    case 'T':
    case 't':
    case 'Y':
    case 'y':
        return true;
    case 'F':
    case 'f':
    case 'N':
    case 'n':
        return false;
    default:
        return std::nullopt;
    }
}

std::string formatReal(double value, int decimalPlaces)
{
    // 1.8e308 in fixed notation is 309 digits before the point.
    char buffer[400];
    const auto result = std::to_chars(buffer, buffer + sizeof buffer, value,
                                      std::chars_format::fixed, std::clamp(decimalPlaces, 0, 17));
    std::string text(buffer, result.ptr);
    if (text.find('.') != std::string::npos) {
        while (text.back() == '0') {
            text.pop_back();
        }
        if (text.back() == '.') {
            text.pop_back();
        }
    }
    // -0.0004 at three places rounds to "-0", which is a zero with a sign no
    // reader wants.
    if (text == "-0") {
        text = "0";
    }
    return text;
}

std::string formatHexReal(double value)
{
    char buffer[64];
    const bool negative = std::signbit(value);
    const auto result = std::to_chars(buffer, buffer + sizeof buffer, std::fabs(value),
                                      std::chars_format::hex);
    std::string text = negative ? "-0x" : "0x";
    text.append(buffer, result.ptr);
    return text;
}

std::string formatExactReal(double value)
{
    char buffer[64];
    const auto result = std::to_chars(buffer, buffer + sizeof buffer, value);
    return std::string(buffer, result.ptr);
}

} // namespace detail

std::optional<double> parseReal(std::string_view token)
{
    // "502000.000, 6960000.000" is how a hand-made file separates a row.
    if (!token.empty() && token.back() == ',') {
        token.remove_suffix(1);
    }
    bool negative = false;
    if (!token.empty() && (token.front() == '+' || token.front() == '-')) {
        negative = token.front() == '-';
        token.remove_prefix(1);
    }
    if (token.empty()) {
        return std::nullopt;
    }
    auto format = std::chars_format::general;
    if (token.size() > 2 && token[0] == '0' && (token[1] == 'x' || token[1] == 'X')) {
        // from_chars takes hexadecimal floats WITHOUT the 0x that C99, and
        // 12d Model, write.
        format = std::chars_format::hex;
        token.remove_prefix(2);
    } else if (!(std::isdigit(static_cast<unsigned char>(token.front())) != 0 ||
                 token.front() == '.')) {
        // from_chars would accept "inf" and "nan"; neither is a number here.
        return std::nullopt;
    }
    double value = 0.0;
    const auto result = std::from_chars(token.data(), token.data() + token.size(), value, format);
    if (result.ec != std::errc{} || result.ptr != token.data() + token.size() ||
        !std::isfinite(value)) {
        return std::nullopt;
    }
    return negative ? -value : value;
}

} // namespace katana::archive12d
