#include "katana/core/text.hpp"

#include <charconv>
#include <cmath>
#include <system_error>

namespace katana::core {

std::string_view trimmed(std::string_view text) noexcept
{
    while (!text.empty() && isAsciiSpace(text.front())) {
        text.remove_prefix(1);
    }
    while (!text.empty() && isAsciiSpace(text.back())) {
        text.remove_suffix(1);
    }
    return text;
}

std::string lowered(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        c = asciiLower(c);
    }
    return out;
}

bool equalsIgnoringCase(std::string_view a, std::string_view b) noexcept
{
    if (a.size() != b.size()) {
        return false;
    }
    for (std::size_t i = 0; i < a.size(); ++i) {
        if (asciiLower(a[i]) != asciiLower(b[i])) {
            return false;
        }
    }
    return true;
}

namespace {

// from_chars takes '-' but not '+'. A '+' is stepped over here, and a token that
// is then empty or signed again ("+", "+-1", "++1") is refused: one sign is a
// sign, two is a typo, and reading "+-1" as -1 would be picking one of them.
[[nodiscard]] std::optional<std::string_view> withoutPlus(std::string_view token) noexcept
{
    if (!token.empty() && token.front() == '+') {
        token.remove_prefix(1);
        if (token.empty() || token.front() == '+' || token.front() == '-') {
            return std::nullopt;
        }
    }
    if (token.empty()) {
        return std::nullopt;
    }
    return token;
}

} // namespace

std::optional<std::int64_t> parseInteger(std::string_view token) noexcept
{
    const std::optional<std::string_view> digits = withoutPlus(token);
    if (!digits) {
        return std::nullopt;
    }
    std::int64_t value = 0;
    const char* end = digits->data() + digits->size();
    const auto [ptr, ec] = std::from_chars(digits->data(), end, value);
    if (ec != std::errc{} || ptr != end) {
        return std::nullopt;
    }
    return value;
}

std::optional<double> parseFiniteDouble(std::string_view token) noexcept
{
    const std::optional<std::string_view> digits = withoutPlus(token);
    if (!digits) {
        return std::nullopt;
    }
    double value = 0.0;
    const char* end = digits->data() + digits->size();
    const auto [ptr, ec] =
        std::from_chars(digits->data(), end, value, std::chars_format::general);
    // result_out_of_range leaves `value` unmodified and so would otherwise read
    // as 0: "1e999" is refused rather than becoming a plausible zero.
    if (ec != std::errc{} || ptr != end || !std::isfinite(value)) {
        return std::nullopt;
    }
    return value;
}

std::vector<std::string_view> splitLines(std::string_view text)
{
    std::vector<std::string_view> lines;
    std::size_t start = 0;
    std::size_t i = 0;
    while (i < text.size()) {
        const char c = text[i];
        if (c == '\n' || c == '\r') {
            lines.push_back(text.substr(start, i - start));
            ++i;
            if (c == '\r' && i < text.size() && text[i] == '\n') {
                ++i;
            }
            start = i;
            continue;
        }
        ++i;
    }
    if (start < text.size()) {
        lines.push_back(text.substr(start));
    }
    return lines;
}

std::string unescapeTyped(std::string_view text)
{
    std::string out;
    out.reserve(text.size());
    for (std::size_t i = 0; i < text.size(); ++i) {
        const char next = i + 1 < text.size() ? text[i + 1] : '\0';
        if (text[i] == '\\' && next == 'n') {
            out += '\n';
            ++i;
        } else if (text[i] == '\\' && next == '\\') {
            out += '\\';
            ++i;
        } else {
            out += text[i];
        }
    }
    return out;
}

std::string formatExactReal(double value)
{
    // 24 is enough for the longest shortest-round-trip double,
    // "-2.2250738585072014e-308" (24 characters).
    char buffer[32];
    const auto result = std::to_chars(buffer, buffer + sizeof buffer, value);
    return std::string(buffer, result.ptr);
}

} // namespace katana::core
