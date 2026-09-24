#include "tokenizer.hpp"

#include <charconv>
#include <cmath>
#include <cstring>

namespace katana::dxf::detail {

namespace {

constexpr bool isBlank(char c) { return c == ' ' || c == '\t' || c == '\r'; }

} // namespace

std::string_view trimmedValue(std::string_view value)
{
    std::size_t begin = 0;
    std::size_t end = value.size();
    while (begin < end && isBlank(value[begin])) {
        ++begin;
    }
    while (end > begin && isBlank(value[end - 1])) {
        --end;
    }
    return value.substr(begin, end - begin);
}

std::optional<double> toReal(std::string_view value)
{
    std::string_view text = trimmedValue(value);
    if (!text.empty() && text.front() == '+') {
        text.remove_prefix(1);
    }
    double result = 0.0;
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), result);
    if (error != std::errc{} || end != text.data() + text.size() || !std::isfinite(result)) {
        return std::nullopt;
    }
    return result;
}

std::optional<std::int64_t> toInteger(std::string_view value)
{
    std::string_view text = trimmedValue(value);
    if (!text.empty() && text.front() == '+') {
        text.remove_prefix(1);
    }
    std::int64_t result = 0;
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), result);
    if (error != std::errc{} || end != text.data() + text.size()) {
        // Some writers put a real where an integer belongs ("1.0"); take it
        // when it is a whole number rather than lose the entity over it.
        if (const auto real = toReal(text); real && std::trunc(*real) == *real &&
                                            std::abs(*real) < 9.0e15) {
            return static_cast<std::int64_t>(*real);
        }
        return std::nullopt;
    }
    return result;
}

std::optional<std::uint64_t> toHandle(std::string_view value)
{
    const std::string_view text = trimmedValue(value);
    std::uint64_t result = 0;
    const auto [end, error] =
        std::from_chars(text.data(), text.data() + text.size(), result, 16);
    if (error != std::errc{} || end != text.data() + text.size()) {
        return std::nullopt;
    }
    return result;
}

bool Tokenizer::readLine(std::string_view& out)
{
    if (position_ >= text_.size()) {
        return false;
    }
    const char* begin = text_.data() + position_;
    const std::size_t remaining = text_.size() - position_;
    const void* found = std::memchr(begin, '\n', remaining);
    std::size_t length = found == nullptr ? remaining
                                          : static_cast<std::size_t>(
                                                static_cast<const char*>(found) - begin);
    position_ += found == nullptr ? remaining : length + 1;
    if (length != 0 && begin[length - 1] == '\r') {
        --length;
    }
    out = std::string_view(begin, length);
    ++lineNumber_;
    return true;
}

bool Tokenizer::next(Pair& out)
{
    if (hasPending_) {
        out = pending_;
        hasPending_ = false;
        return true;
    }
    if (!failure_.empty()) {
        return false;
    }
    for (;;) {
        std::string_view codeLine;
        if (!readLine(codeLine)) {
            return false;
        }
        const std::string_view codeText = trimmedValue(codeLine);
        if (codeText.empty() && position_ >= text_.size()) {
            return false; // a blank last line is only the file's final newline
        }
        int code = 0;
        const auto [end, error] =
            std::from_chars(codeText.data(), codeText.data() + codeText.size(), code);
        if (codeText.empty() || error != std::errc{} ||
            end != codeText.data() + codeText.size() || code < 0 || code > 1071) {
            std::string shown(codeLine.substr(0, 40));
            failure_ = "line " + std::to_string(lineNumber_) +
                       ": a group code was expected and \"" + shown + "\" was found";
            return false;
        }
        codeLine_ = lineNumber_;
        std::string_view value;
        if (!readLine(value)) {
            truncated_ = true;
            return false;
        }
        if (code == 999) {
            continue; // a comment
        }
        out.code = code;
        out.value = value;
        return true;
    }
}

} // namespace katana::dxf::detail
