#include "katana/cad/definition_edit.hpp"

#include <cstddef>
#include <span>
#include <utility>
#include <vector>

#include "katana/cad/annotation/command_words.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/model.hpp"

namespace katana::cad {

namespace {

using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::Result;
using katana::entity::LineStyle;

// One line of the text that holds something: its number in the text, from 1,
// and what it holds without the comma that ends a stroke's line in a file.
struct StrokeLine {
    int number = 0;
    std::string text{};
};

std::vector<StrokeLine> strokeLines(std::string_view text)
{
    std::vector<StrokeLine> lines;
    int number = 0;
    for (const std::string_view raw : katana::core::splitLines(text)) {
        ++number;
        std::string_view line = katana::core::trimmed(raw);
        if (line.empty()) {
            continue;
        }
        if (line.back() == ',') {
            // A line that is nothing but a comma is kept as it is: it is not
            // a stroke, and passed over it would not be said to be one.
            const std::string_view stroke =
                katana::core::trimmed(line.substr(0, line.size() - 1));
            if (!stroke.empty()) {
                line = stroke;
            }
        }
        lines.push_back(StrokeLine{number, std::string(line)});
    }
    return lines;
}

// `header` - a definition with no strokes, as the format writes one: a single
// line, {"name": "A", "length": 12} - with `lines` as its strokes, one a line
// as the format lays them out.
std::string withStrokes(std::string_view header, std::span<const StrokeLine> lines)
{
    if (lines.empty() || header.empty() || header.back() != '}') {
        return std::string(header);
    }
    std::string out(header.substr(0, header.size() - 1));
    out += ", \"strokes\": [";
    for (std::size_t index = 0; index < lines.size(); ++index) {
        out += index == 0 ? "\n" : ",\n";
        out += lines[index].text;
    }
    out += "\n]}";
    return out;
}

bool allDigits(std::string_view text)
{
    if (text.empty()) {
        return false;
    }
    for (const char c : text) {
        if (c < '0' || c > '9') {
            return false;
        }
    }
    return true;
}

// `context` without the place the reader ends a refusal with - "1e999 at line
// 2, column 14" - which counts through the text it was handed: the lines
// joined here under a first line nobody typed. The line is named by
// readStrokeText instead.
std::string withoutPlace(std::string_view context)
{
    constexpr std::string_view kAt = " at line ";
    constexpr std::string_view kColumn = ", column ";
    const std::size_t at = context.rfind(kAt);
    if (at == std::string_view::npos) {
        return std::string(context);
    }
    const std::string_view place = context.substr(at + kAt.size());
    const std::size_t column = place.find(kColumn);
    if (column == std::string_view::npos || !allDigits(place.substr(0, column)) ||
        !allDigits(place.substr(column + kColumn.size()))) {
        return std::string(context);
    }
    return std::string(context.substr(0, at));
}

// A refusal as a person is told it: what was refused, then what it was
// refused of. Not Error::describe(), which leads with the code's name.
std::string worded(const Error& error)
{
    std::string out = error.message;
    const std::string context = withoutPlace(error.context);
    if (!context.empty()) {
        out += " - ";
        out += context;
    }
    return out;
}

// Text that is not JSON is refused in the JSON library's words, as the
// context: "parse error at line 2, column 9: syntax error while parsing array
// - ...". What follows the place, when `error` is one of those.
std::optional<std::string> syntaxReason(const Error& error)
{
    constexpr std::string_view kPlace = "parse error at ";
    if (!error.context.starts_with(kPlace)) {
        return std::nullopt;
    }
    const std::size_t colon = error.context.find(": ");
    return colon == std::string::npos ? error.context : error.context.substr(colon + 2);
}

std::string lineLabel(int line)
{
    std::string label = "Line ";
    label += std::to_string(line);
    return label;
}

} // namespace

Result<std::string> strokeText(const LineStyle& definition)
{
    const Result<std::string> written = katana::entity::definitionToJson(definition);
    if (!written) {
        return written.error();
    }
    // The format writes a definition's members on its first line, then its
    // strokes one a line, then "]}" (docs/customisation.md, "Layout", rule
    // 5); one with no strokes is that first line alone. A text is escaped
    // into its line, so no stroke spans two.
    const std::vector<std::string_view> lines = katana::core::splitLines(*written);
    std::string text;
    for (std::size_t index = 1; index + 1 < lines.size(); ++index) {
        if (index > 1) {
            text += '\n';
        }
        text += katana::core::trimmed(lines[index]);
    }
    return text;
}

StrokeTextRead readStrokeText(const LineStyle& members, std::string_view strokes)
{
    StrokeTextRead result;
    LineStyle header = members;
    header.strokes.clear();
    header.texts.clear();

    // The members as the format writes them, then the strokes as typed, read
    // back by the format's reader - which is also where entity::validate
    // refuses a length below 0 or a factor of 0.
    const Result<std::string> written = katana::entity::definitionToJson(header);
    if (!written) {
        result.error = written.error();
        result.problem = worded(written.error());
        return result;
    }
    const std::vector<StrokeLine> typed = strokeLines(strokes);
    Result<LineStyle> whole =
        katana::entity::definitionFromJson(withStrokes(*written, typed), header.symbol);
    if (whole) {
        // Only the strokes are taken from what was read; the members are the
        // caller's. A line that ended the list of strokes and went on to give
        // members of its own would otherwise make a definition `members` does
        // not describe.
        LineStyle fromMembers = header;
        fromMembers.strokes = whole->strokes;
        fromMembers.texts = whole->texts;
        if (!(fromMembers == *whole)) {
            result.problem = "The strokes hold more than strokes: a line ends the list and gives "
                           "the definition members of its own. Those are set in the fields "
                           "above.";
            result.error = katana::core::makeError(ErrorCode::InvalidArgument, result.problem);
            return result;
        }
        result.definition = std::move(fromMembers);
        return result;
    }
    result.error = whole.error();

    // Which line: each alone, under members of no consequence, until one
    // does not read. Asked only of text that already failed, so a definition
    // that reads costs one reading.
    constexpr std::string_view kAnyMembers = R"({"name": "a"})";
    for (std::size_t index = 0; index < typed.size(); ++index) {
        const Result<LineStyle> alone = katana::entity::definitionFromJson(
            withStrokes(kAnyMembers, std::span(typed).subspan(index, 1)), false);
        if (alone) {
            continue;
        }
        result.line = typed[index].number;
        if (syntaxReason(alone.error())) {
            // Not JSON. The reason is asked of the line by ITSELF: under the
            // members it would be told with the brackets and the brace of a
            // text nobody typed - a line missing its "]" as "unexpected '}'",
            // a word cut short as a string running on into them.
            const Result<LineStyle> bare =
                katana::entity::definitionFromJson(typed[index].text, false);
            std::optional<std::string> reason =
                bare ? std::nullopt : syntaxReason(bare.error());
            // By itself, a line holding MORE than one value reads only as
            // "something follows the first" - which is no fault here, where
            // the lines are joined with commas. Then it is asked as the list
            // of the values it holds, in one pair of brackets of this
            // function's making.
            constexpr std::string_view kMoreFollows = "expected end of input";
            if (!reason || reason->ends_with(kMoreFollows)) {
                std::string list = "[";
                list += typed[index].text;
                list += ']';
                const Result<LineStyle> asList =
                    katana::entity::definitionFromJson(list, false);
                if (!asList && syntaxReason(asList.error())) {
                    reason = syntaxReason(asList.error());
                }
            }
            result.problem = lineLabel(result.line);
            result.problem += " is not a stroke: ";
            result.problem += reason ? *reason : *syntaxReason(alone.error());
            return result;
        }
        // A stroke the reader refuses: its own words, from the strokes down
        // to this one, which name the stroke by its place among them and the
        // definition by its name.
        const Result<LineStyle> inPlace = katana::entity::definitionFromJson(
            withStrokes(*written, std::span(typed).first(index + 1)), header.symbol);
        result.problem = lineLabel(result.line);
        result.problem += ": ";
        result.problem += worded(inPlace ? alone.error() : inPlace.error());
        return result;
    }
    result.problem = worded(whole.error());
    return result;
}

std::string freeDefinitionName(const Document& document, std::string_view base)
{
    const auto taken = [&document](const std::string& name) {
        return document.styleLibrary().contains(name) ||
               document.model().linetypes.contains(name);
    };
    std::string name(base);
    for (int suffix = 2; taken(name); ++suffix) {
        name = std::string(base);
        name += ' ';
        name += std::to_string(suffix);
    }
    return name;
}

Result<std::string> removeDefinitionLine(std::string_view name, bool force)
{
    if (name.empty()) {
        return katana::core::makeError(ErrorCode::InvalidArgument,
                                       "a definition with no name cannot be named on a "
                                       "command line");
    }
    // The one rule every dialog writes a word by: refused for a double quote
    // or a line break.
    const Result<std::string> word = katana::cad::annotation::commandWord(name);
    if (!word) {
        return word.error();
    }
    // The tokenizer removes the quotes, so the verb cannot tell a name from
    // the words of its own grammar: `CUSTOMISE REMOVE <definition>... [FORCE]`
    // and `CUSTOMISE REMOVE CODE <key>...`.
    for (const std::string_view keyword : {std::string_view("CODE"), std::string_view("FORCE")}) {
        if (katana::core::equalsIgnoringCase(name, keyword)) {
            std::string message = "the name is a word of the CUSTOMISE REMOVE line itself (";
            message += keyword;
            message += "), which a command line cannot tell from the definition";
            return katana::core::makeError(ErrorCode::InvalidArgument, std::move(message),
                                           std::string(name));
        }
    }
    std::string line = "CUSTOMISE REMOVE \"";
    line += name;
    line += '"';
    if (force) {
        line += " FORCE";
    }
    return line;
}

} // namespace katana::cad
