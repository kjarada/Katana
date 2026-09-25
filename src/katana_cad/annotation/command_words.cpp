#include "katana/cad/annotation/command_words.hpp"

#include <algorithm>

#include "katana/core/text.hpp"

namespace katana::cad::annotation {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

Result<std::string> commandWord(std::string_view text)
{
    if (text.find('"') != std::string_view::npos) {
        return makeError(ErrorCode::InvalidArgument,
                         "a double quote cannot be written on a command line",
                         std::string(text));
    }
    if (text.find('\n') != std::string_view::npos || text.find('\r') != std::string_view::npos) {
        return makeError(ErrorCode::InvalidArgument,
                         "a line break cannot be written on a command line", std::string(text));
    }
    const bool blank = text.empty() || std::any_of(text.begin(), text.end(), [](char c) {
                           return katana::core::isAsciiSpace(c);
                       });
    return blank ? "\"" + std::string(text) + "\"" : std::string(text);
}

Result<std::string> annotationTextWord(std::string_view text)
{
    if (text.find("\\n") != std::string_view::npos) {
        return makeError(ErrorCode::InvalidArgument,
                         "a backslash before an n would be read as a line break",
                         std::string(text));
    }
    std::string escaped;
    escaped.reserve(text.size());
    for (const char c : text) {
        if (c == '\n') {
            escaped += "\\n";
        } else if (c != '\r') { // a Windows line end is one break, not two
            escaped += c;
        }
    }
    return commandWord(escaped);
}

std::string recordValue(std::string_view value)
{
    const bool needs = value.empty() || value.find_first_of(" \"\\\n=") != std::string_view::npos;
    if (!needs) {
        return std::string(value);
    }
    std::string out = "\"";
    for (const char c : value) {
        if (c == '"' || c == '\\') {
            out += '\\';
            out += c;
        } else if (c == '\n') {
            out += "\\n";
        } else {
            out += c;
        }
    }
    out += '"';
    return out;
}

} // namespace katana::cad::annotation
