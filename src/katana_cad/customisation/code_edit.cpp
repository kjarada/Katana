#include "katana/cad/code_edit.hpp"

#include <cstddef>

#include "katana/cad/code_table.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::entity::SurveyAttribute;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyRule;

[[nodiscard]] bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

void appendRecord(std::string& out, const std::vector<std::string>& fields)
{
    for (std::size_t i = 0; i < fields.size(); ++i) {
        if (i > 0) {
            out += ',';
        }
        out += csvField(fields[i]);
    }
    out += "\r\n";
}

} // namespace

std::string csvField(std::string_view text)
{
    if (text.find_first_of(",\"\r\n") == std::string_view::npos) {
        return std::string(text);
    }
    std::string quoted = "\"";
    for (const char c : text) {
        if (c == '"') {
            quoted += '"';
        }
        quoted += c;
    }
    quoted += '"';
    return quoted;
}

std::string codeListCsv(const katana::entity::SurveyMap& map)
{
    std::string out;
    appendRecord(out, {"code", "description", "group", "layer", "colour", "line/point",
                       "linestyle", "symbol", "size", "surface"});
    for (const CodeTableRow& row : codeTable(map)) {
        const SurveyRule& rule = row.combined;
        std::string breakline;
        if (rule.breakline) {
            breakline = *rule.breakline == SurveyBreakline::Line ? "line" : "point";
        }
        std::string symbol;
        std::string size;
        if (rule.symbol) {
            symbol = rule.symbol->style;
            // 0 is the map's "the definition's own size": no size of its own.
            if (rule.symbol->size != 0.0) {
                size = katana::core::formatExactReal(rule.symbol->size);
            }
        }
        std::string tinable;
        if (rule.tinable) {
            tinable = *rule.tinable ? "yes" : "no";
        }
        appendRecord(out, {row.key, rule.comment, rule.group, rule.model, rule.colour, breakline,
                           rule.linestyle, symbol, size, tinable});
    }
    return out;
}

std::string suggestedKey(std::string_view code)
{
    const std::string_view text = katana::core::trimmed(code);
    std::size_t end = text.size();
    while (end > 0 && isDigit(text[end - 1])) {
        --end;
    }
    // No trailing number, or nothing but a number: the code itself, exact.
    if (end == text.size() || end == 0) {
        return std::string(text);
    }
    // "KB 12" -> "KB*", not "KB *": the blank belongs to neither part, and a
    // key ending in a blank before its `*` would read as a typing slip.
    const std::string_view stem = katana::core::trimmed(text.substr(0, end));
    if (stem.empty()) {
        return std::string(text);
    }
    return std::string(stem) + "*";
}

std::string formatAttributeLines(const std::vector<SurveyAttribute>& attributes)
{
    std::string out;
    for (const SurveyAttribute& attribute : attributes) {
        out += attribute.type + " " + attribute.name + " = " + attribute.value + "\n";
    }
    return out;
}

katana::core::Result<std::vector<SurveyAttribute>> parseAttributeLines(std::string_view text)
{
    std::vector<SurveyAttribute> attributes;
    std::size_t lineNumber = 0;
    for (const std::string_view raw : katana::core::splitLines(text)) {
        ++lineNumber;
        const std::string_view line = katana::core::trimmed(raw);
        if (line.empty()) {
            continue;
        }
        const std::string where = "attribute line " + std::to_string(lineNumber);
        const std::size_t equals = line.find('=');
        if (equals == std::string_view::npos) {
            return makeError(ErrorCode::InvalidArgument,
                             "an attribute is \"<type> <name> = <value>\"; this has no '='",
                             where);
        }
        const std::string_view head = katana::core::trimmed(line.substr(0, equals));
        const std::size_t blank = head.find_first_of(" \t");
        const std::string_view type = head.substr(0, blank);
        const std::string_view name =
            blank == std::string_view::npos ? std::string_view{}
                                            : katana::core::trimmed(head.substr(blank));
        if (type != "text" && type != "integer") {
            return makeError(ErrorCode::InvalidArgument,
                             "an attribute's type is \"text\" or \"integer\", not \"" +
                                 std::string(type) + "\"",
                             where);
        }
        if (name.empty()) {
            return makeError(ErrorCode::InvalidArgument, "an attribute needs a name", where);
        }
        attributes.push_back(SurveyAttribute{
            std::string(type), std::string(name),
            std::string(katana::core::trimmed(line.substr(equals + 1)))});
    }
    return attributes;
}

} // namespace katana::cad
